# PolyPack 生态 GA 文档集

> 适用版本：M8 生态 GA（收口基线 `52968b4`）。English: [GA.en.md](GA.en.md)
> 配套阅读：[README](../README.md) · [ROADMAP](ROADMAP.md) · [TASKS](TASKS.md) · [FARM-DEPLOY](FARM-DEPLOY.md) · [EXPERIMENTAL-TARGETS](EXPERIMENTAL-TARGETS.md)

## 1. GA 总览与稳定性承诺

M8 生态 GA 交付四件事：WASM 插件 SDK（沙箱、SRI/签名、模板/脚手架）、远程农场 v1、商店发布流水线 GA，以及由 TargetSpec 生成的 49 目标现状表。插件接口按 `target-spec/v1` 版本化；本 GA 文档集与「1 天新增目标」演练记录一并发布。

### 1.1 交付范围

| M8 交付面 | 状态 | 证据 / 入口 |
|---|---|---|
| WASM 插件沙箱 | 已交付 | `poly-plugin`：默认零能力、显式授权、fuel/epoch/内存上限（T-M8-01） |
| 插件接口 `target-spec/v1` | 已交付 | WIT `polypack-target`；非 v1 → `E_PLUGIN_0005`（T-M8-02） |
| SRI 与签名链 | 已交付 | `blake3-<hex>` 先于验签、验签先于实例化（T-M8-03） |
| 模板与脚手架 | 已交付 | `poly plugin new` / `poly plugin check` / `poly plugin digest` + `templates/plugin-target`（T-M8-04） |
| 内建 / 插件统一接口 | 已交付 | `TargetProvider` + `ProviderRegistry`，无 builtin 特判（T-M8-05） |
| 远程农场 v1 | 已交付 | 协议 v1、并发 / worker 池 / 超时重试、v0 worker 兼容（T-M8-06..09） |
| 商店发布 GA | 已交付 | 指数退避、幂等键、SubmitJournal、回执 v2、`poly retry --from publish`（T-M8-10/11） |
| 49 目标现状表 | 已交付 | 生成式表格接入 docs-check（T-M8-12） |

### 1.2 版本策略与兼容承诺

- `target-spec/v1` 是当前唯一接受的插件接口版本。破坏性变更（字段增删、语义变化、WIT world 变化）必须以新版本号（如 `target-spec/v2`）发布，不得就地修改 v1。
- 宿主拒绝非 v1 插件并返回 `poly::E_PLUGIN_0005`，附迁移提示；当前宿主要求 `target-spec/v1`。
- `polypack.toml` 顶层为 `schema = 1`；`[[plugins]]` 等配置段 `deny_unknown_fields`，未知键直接拒绝，不静默丢弃。
- 插件制品的内容摘要（SRI）是 `blake3-<hex>`；SRI 与签名必须随制品同时更新。篡改制品后即使同步更新 SRI，验签仍会失败。

| 对象 | 策略 |
|---|---|
| 插件接口 | `target-spec/v1`；破坏性变更走新版本号 |
| 插件制品 | SRI（`blake3-<hex>`）+ 可选 GPG detached 签名 |
| 配置 | `schema = 1`；未知键拒绝 |
| 本仓库 | `vX.Y.Z` tag 语义化递增 |

## 2. 插件作者指南

### 2.1 前置条件

- Rust 1.85+（模板 `templates/plugin-target/README.md` 声明）与 `wasm32-unknown-unknown` 目标：

  ```bash
  rustup target add wasm32-unknown-unknown
  ```

- 与本仓同一提交构建的 `poly`（`poly-cli` 的 `publish = false`，尚未发布到 crates.io）。
- 构建农场 / 发布链路不是插件开发的必需项；本地 `poly build` 即可完成插件目标出包。

### 2.2 从脚手架到出包

| 步骤 | 命令 | 预期 |
|---|---|---|
| 1. 生成工程 | `poly plugin new pocket-notes` | 生成 `plugin/`、`componentize/`、`contract/`、`build.sh` 等模板文件 |
| 2. 构建 wasm | `cd pocket-notes && ./build.sh` | 产物 `dist/plugin.wasm` |
| 3. 工程检查 | `poly plugin check .` | 脚手架文件齐全、WIT 与宿主一致；`dist/plugin.wasm` 缺失只警告（提示先构建） |
| 4. 契约测试 | `cargo test -- --nocapture` | 加载组件并调用 `describe` / `package`；失败常见于契约断言未同步 |
| 5. 计算 SRI | `poly plugin digest dist/plugin.wasm` | 输出 `blake3-<hex>`（与宿主加载同口径） |
| 6. 注册插件 | 编辑应用 `polypack.toml` 的 `[[plugins]]` | 见 §2.3 |
| 7. 真实出包 | `poly build --config <app>/polypack.toml --target pocket-notes` | 由插件 provider 产出目标产物 |

说明：`poly plugin new` 只生成插件工程，不写入应用目录。构建后把 `dist/plugin.wasm` 复制到应用的 `plugins/` 目录，再按 `polypack.toml` 的 `path` 引用。

### 2.3 在应用中注册

```toml
[[plugins]]
id = "pocket-notes"
path = "plugins/pocket-notes.wasm"
sri = "blake3-<hex>"
# 可选：分离签名（相对工程根）与公钥信任根
# signature = "plugins/pocket-notes.wasm.sig"
# trust_root = { name = "pocket-notes-pub", source = { env = { variable = "POLYPACK_POCKET_NOTES_TRUST_ROOT" } } }
# 默认零能力；确需时按最小权限显式授权：
# fs_read = ["data"]
# fs_write = ["out"]
# net = ["api.example.com"]
```

- `sri` 设置后，宿主在加载前强制校验；不匹配返回 `poly::E_PLUGIN_0006`。
- `signature` 与 `trust_root` 必须成对配置；`trust_root` 只放公钥，载荷为 `base64(JSON) {"public_key":"<base64>"}`（keyring / Vault / env 均可承载）。验签失败返回 `poly::E_PLUGIN_0009`，信任根不可用返回 `poly::E_PLUGIN_0010`。
- 能力默认空：无文件系统、无网络。`fs_read` / `fs_write` 的路径相对工程根；`net` 写 `host` 或 `host:port`。授权清单进入 `JobSpec`，并以 `plugin_granted` 事件出现在 `poly build --json` 输出中。

### 2.4 最小宿主工程与产物复制

插件目标仍要求应用 `polypack.toml` 具备 `[ui]` 与 `[core]` 段（插件可以不使用它们，但 schema 必填）：

```toml
[ui]
framework = "react"
entry = "index.html"

[core]
crate = "core"
```

```bash
mkdir -p <app>/plugins
cp dist/plugin.wasm <app>/plugins/pocket-notes.wasm
poly build --config <app>/polypack.toml --target pocket-notes
```

> 单机验证提示：`poly plugin digest` 只输出摘要；`poly build --json` 才会给出结构化事件。`poly build` 的人类可读输出当前不显示插件 `describe` 的完整目标声明，需以 `--json` 事件或产物结果核对（见 §7 已知缺口）。

### 2.5 模板契约断言的单一来源

模板的契约测试在 `contract/src/lib.rs` 顶部只保留两个断言常量：`EXPECTED_FORMAT` 与 `EXPECTED_PACKAGE`。修改插件 `describe` / `package` 后同步这两个常量即可；忘记同步会让 `cargo test` 以退出码 101 失败（本次演练的实际卡点，见 §6.4）。模板 WIT 是宿主 `crates/poly-plugin/wit/polypack-target.wit` 的副本：仓库侧由 `tools/check-docs.mjs` 逐字比对门禁兜底，工程侧可用 `poly plugin check` 校验同步状态。

## 3. 安全模型

### 3.1 默认零能力与显式授权

| 能力 | 默认 | 授权方式 |
|---|---|---|
| 文件系统读 | 拒绝 | `fs_read = ["<相对目录>"]` |
| 文件系统写 | 拒绝 | `fs_write = ["<相对目录>"]` |
| 出网 | 拒绝 | `net = ["<host 或 host:port>"]` |

授权路径按工程根解析并规范化；越界、路径穿越、符号链接逃逸均返回 `poly::E_PLUGIN_0001`（写操作额外要求写权限）。出网只放行显式白名单；重定向一律不跟随（`max_redirects = 0`），非 2xx 响应返回 `E_PLUGIN_0007`。未授权调用不会获得宿主文件系统或网络句柄。

### 3.2 制品完整性与签名链

加载顺序固定为：**SRI 校验 → 验签 → 组件解析 → 实例化**。

1. `sri` 存在时，先计算 `blake3-<hex>` 并比对；不符返回 `E_PLUGIN_0006`，不进入验签。
2. `signature` 存在时，用 `trust_root` 解析出的公钥做 GPG detached 验签；签名缺失或不匹配返回 `E_PLUGIN_0009`，信任根不可用返回 `E_PLUGIN_0010`。验签在 `Component::from_binary` 之前完成。
3. 验签通过后才解析组件并实例化；失败返回 `E_PLUGIN_0007`。

GPG 操作用一次性 0700 `GNUPGHOME`；信任根只接受公钥，绝不内联私钥。签名与 SRI 必须同时更新；只更新 SRI 不能绕过验签。

### 3.3 资源上限

| 资源 | 默认 | 硬上限 |
|---|---|---|
| fuel | 1,000,000,000 | 10,000,000,000 |
| epoch | 500 tick（tick = 10ms，约 5s） | 60,000 tick（约 10 分钟） |
| 线性内存 | 64 MiB | 1 GiB |

配置只能收紧、不能放开：0 或超过硬上限一律拒绝（fuel / epoch → `E_PLUGIN_0003`，内存 → `E_PLUGIN_0004`）。超限中止插件调用，不影响宿主进程。

### 3.4 错误码与制品扫描

| 错误码 | 含义 |
|---|---|
| `E_PLUGIN_0001` | 能力拒绝：路径越出授权目录（穿越 / 符号链接逃逸 / 无写权限） |
| `E_PLUGIN_0002` | 出网域名未授权 |
| `E_PLUGIN_0003` | 资源上限耗尽（fuel / epoch 超时） |
| `E_PLUGIN_0004` | 内存超限（`memory.grow` 被宿主上限拒绝） |
| `E_PLUGIN_0005` | 插件接口版本不受支持（非 `target-spec/v1`） |
| `E_PLUGIN_0006` | 插件制品内容摘要（SRI）不匹配 |
| `E_PLUGIN_0007` | 插件执行失败（trap / 插件返回 error / 出网传输失败） |
| `E_PLUGIN_0008` | 授权清单非法（目录不存在 / 域名或端口格式非法） |
| `E_PLUGIN_0009` | 签名缺失或验签失败（签名与制品 / 信任根不匹配） |
| `E_PLUGIN_0010` | 签名信任根不可用（CredentialRef 解析失败 / 公钥导入失败 / 缺验签工具） |

`poly scan` 对 `.wasm` 制品额外施加宿主路径、环境变量痕迹与调试符号规则：`/home/`、`C:\Users\`、`C:/Users/`、`/var/folders/`、`/tmp/`（宿主路径）；`CARGO_MANIFEST_DIR`、`USERPROFILE=`、`TMPDIR=`、`$HOME`（环境痕迹）；`.debug_info`、`.debug_abbrev`（调试段）。命中只报告规则、位置与脱敏描述（偏移），不回显命中路径本身；修复提示为 release 构建并 strip 调试段。

## 4. 农场与发布 GA

### 4.1 农场 v1

农场协议 v1（`farm/v1`）在 v0 的「发起地 / 构建地」模型上增加并发调度、worker 池、任务超时与重试；v1 主端仍接受 `farm/v0` worker（版本协商成功即可接入）。命令与部署步骤见 [FARM-DEPLOY](FARM-DEPLOY.md) 与 README。

| 能力 | 行为 |
|---|---|
| 并发 | 主端按目标拆任务，池内空闲 worker 各取一条可构建目标并发执行 |
| worker 池 | 每个进程一个信箱；`--jobs` / `--idle` / `--target` 控制会话与接单范围 |
| 超时 / 失联 | `E_FARM_0004` / `E_FARM_0005` 带修复提示；按 `--retries` 换 worker 重试 |
| v0 兼容 | v1 主端接受 v0 worker；`JobSpec` 序列化向后兼容 |

### 4.2 隔离、凭据与共享缓存

- 每个任务在 worker 进程内执行；`--work-dir` 下每任务一个 0700 一次性目录，目录名只含 pid + 序号，结束即 `remove_dir_all`，worker 退出删除工作根；失败返回 `E_FARM_0006`。
- 凭据只经 `--credential <档案名>=env:<变量名>` 注入内存，任务结束随 `CredentialScope` 清空；参数与值不进 argv、日志或磁盘。磁盘 / 日志零明文由演练脚本断言（Round 3）。
- `--cache <共享目录>` 使用 `DirSharedStore` 共享目录 CAS：同一输入在不同 worker 上产生同一缓存键，二跑命中 100%；对象写入用同目录临时文件 + 原子 rename。共享目录不可用时告警并降级到工程本地 CAS，构建继续。
- 真实云对象存储（S3 / OSS）尚未实现；接入点是实现 `ObjectStore` trait 并替换 `CacheBackend` 后端（见 FARM-DEPLOY §6.2）。

### 4.3 发布流水线

| 机制 | 行为 |
|---|---|
| 指数退避 | 5xx / 超时 / 429 / 平台限流码按 `RetryPolicy` 重试 |
| 幂等键 | `Idempotency-Key` = 渠道 + appid + 版本 + 内容摘要的 blake3（跨进程稳定） |
| SubmitJournal | 发送前先落盘提交意图；受理后记 `Accepted`；崩溃续跑同键重发 |
| 平台去重 | 微信 85009（已有审核中版本）等原生去重码视为已受理，不产生二次提交 |
| 回执 | 回执 schema v2；含提交号与时间戳，可追溯 |
| 续跑 | `poly retry --from publish` 复用同一回执与提审去重状态 |

自动化止步于「提审」：不承诺自动过审，审核结果以轮询与回执归档为准。

## 5. 技术审查报告

### 5.1 依赖方向与无环性

| crate | 依赖的内部 crate |
|---|---|
| `poly-targets` | 无（依赖图叶层） |
| `poly-cache` | 无 |
| `poly-toolchain` | 无 |
| `poly-core` | `poly-cache`、`poly-targets`（可选 `poly-toolchain`） |
| `poly-sign` | `poly-core` |
| `poly-publish` | `poly-core` |
| `poly-bundle` | `poly-targets` |
| `poly-plugin` | `poly-core`、`poly-sign`、`poly-targets` |
| `poly-cli` | 上述全部 |

依赖方向单向：`poly-plugin` 依赖 core / sign / targets，而 core、targets、sign 均不依赖 `poly-plugin`；没有环。Cargo 本身拒绝循环依赖，本表由各 crate 的 `Cargo.toml` 逐项核对。

### 5.2 no_std 边界

- `poly-targets` 默认 `std`；`--no-default-features` 时 `#![no_std] + alloc`，`TargetSpec` 解析 / 校验与 `TargetProvider` 契约仍可用（`PackageInput` / `PackageArtifact` 均为 alloc 面类型，无 std 专属类型）。
- `poly-core` 同样在 `--no-default-features` 下保留引擎层与 `JobSpec`（`no_std + alloc`）；std 专属能力（工具链、缓存 std 面、桥、农场、流水线等）由 feature 隔离。
- `poly-plugin` 依赖 wasmtime，恒为 std；它不进入裸机目标，只作为宿主侧 provider。
- 内建 provider（`poly-bundle`）为 std；其委托的既有 packagers 源码未因 T-M8-05 改动。

### 5.3 wasmtime 版本与特性

- 版本钉死为 `=49.0.3`；组件模型 / C API 随小版本演进，沙箱边界不接受漂移。
- `default-features = false`，只启用 `component-model`、`cranelift`、`runtime`、`std`；关闭 async / GC / pooling / cache / profiling / WAT 等非必需特性，减少体积与编译面。
- 出网能力复用 `ureq`（与 `poly-sign` / `poly-publish` 同版本同特性），关闭默认 rustls provider，使用系统信任库；重定向由调用侧禁用，避免绕过域名白名单。

### 5.4 provider 统一

- `TargetProvider` 契约位于 `poly-targets`（no_std），`describe` 返回 `TargetSpec`，`package` 接收通用 `PackageInput` 并返回 `PackageArtifact`。
- 内建目标由 `poly-bundle::BuiltinProvider` 包装既有 packagers；第三方插件由 `poly-plugin::PluginProvider` 桥接 wasm，加载期固化 `describe`。
- `ProviderRegistry` 按（目标 id，产物格式）统一注册与查找；CLI 的 build / verify / package 走同一路径，无 `if builtin` 特判。
- 同一套契约测试同时跑内建 Tizen `.wgt` 与 wasm 参考插件；Tizen 统一入口产物与直接打包器字节一致，sha256 基线 `626f4a5988183c434aeeb32b1e28cbb9a300d5fe49eccf0b4bae07c1a14f1532`。

### 5.5 CI 门禁与审查结论

| 工作流 | 覆盖 |
|---|---|
| `rust-ci` | ubuntu / macos-14 / windows-2022 三宿主：fmt、clippy `-D warnings`、workspace 测试、UI 适配层测试 |
| `secret-scan` | 三宿主全历史扫描；命中即退出码 1 |
| `docs-check` | Pax 资产、图示漂移（archify 可用时）、双语结构、SVG XML |

审查结论：M8 交付面与 [TASKS](TASKS.md) M8 任务一致；插件边界（零能力、SRI、签名、资源上限）均有负向测试；provider 统一未改动 L1–L5 打包器源码；农场与发布链路的真跑证据来自本地 Linux 演练与注入测试，macOS IPA R3 与真实云对象存储仍为环境槽 / 后续（见 §7）。

## 6. 「1 天新增目标」演练记录

### 6.1 执行条件

- 日期：2026-10-18。
- 执行者：独立上下文开发者代理（非人类外部开发者）。
- 目标：`pocket-notes`（自定义插件目标）。
- 环境：Linux 宿主；Rust + `wasm32-unknown-unknown`；本仓 M8 收口窗口；首次构建包含冷编译。
- 总墙钟约 38 分钟：冷编译约 30 分钟，实际插件开发约 2 分钟。

### 6.2 时间线

| 阶段 | 结果 | 证据 |
|---|---|---|
| 脚手架 → 模板 wasm | 成功 | 3m20s |
| 脚手架 → 自定义契约 PASS | 成功 | 11m32s |
| 脚手架 → CLI 真实出包 | 成功 | 12m23s |
| 总墙钟 | 成功 | 约 38min（冷编译约 30min，开发约 2min） |

### 6.3 退出码证据

| 命令 | 退出码 | 耗时 / 产物 |
|---|---|---|
| CLI 构建（cargo build -p poly-cli） | 0 | 1203s |
| `poly plugin new pocket-notes` | 0 | — |
| `./build.sh` | 0 | 197s |
| `cargo test`（契约） | 0 | 379s |
| `poly plugin digest` | 0 | — |
| `poly build --target pocket-notes` | 0 | 3s，92B |
| SRI 负向（摘要不匹配） | 1 | `E_PLUGIN_0006` |

### 6.4 卡点与处置

| 卡点 | 处置 | 状态 |
|---|---|---|
| 模板契约硬编码 | 收敛为 `contract/src/lib.rs` 的 `EXPECTED_FORMAT` / `EXPECTED_PACKAGE` 单一常量来源，并写入模板 README 与本文档 | 本次已修 |
| 无 `poly plugin check` 命令 | 新增 `poly plugin check <dir>`：校验脚手架文件、WIT 与宿主同步、产物摘要（产物缺失仅警告） | 已修（本次） |
| 注册示例缺 `sri` | 已在模板 README 与 `poly plugin new` 输出补 SRI 示例 | 已修（`52968b4`） |
| 最小宿主工程未文档化 | 已写入模板 README 与本文档 §2.4 | 已修（`52968b4` / 本文） |
| digest 横幅 / 产物复制 / build 输出不显露 describe | 记为已知缺口；用 `poly build --json` 事件或产物结果核对 | 已知缺口 |

### 6.5 结论

演练全程 ≤ 1 天，满足 M8 出口条件。实际插件开发约 2 分钟，总耗时由冷编译主导（约 30 分钟）；脚手架 → 自定义契约 PASS 11m32s，脚手架 → CLI 真实出包 12m23s。该结论的边界：执行者是独立上下文代理而非人类外部开发者，未做人类可用性研究；SRI 负向仅验证摘要不匹配路径，签名 / 信任根链路由单元与集成测试覆盖，未在本次演练端到端走查。

## 7. 已知缺口与后续

### 7.1 缺口清单

| 缺口 | 影响 | 处置 / 后续 |
|---|---|---|
| 无独立 `poly plugin check` | 作者需手工串 build / test / digest / build | 已修：`poly plugin check <dir>` 一条命令校验脚手架文件 / WIT 同步 / 产物摘要（产物缺失仅警告） |
| 模板 WIT 与宿主 WIT 无自动比对 | 接口升级需手工同步模板副本 | 已修：`tools/check-docs.mjs` 新增模板 WIT ↔ 宿主 WIT 逐字比对门禁；`poly plugin check` 对工程 WIT 做同口径校验 |
| 签名 / 信任根未在演练端到端走查 | 演练只覆盖 SRI 负向 | 单元 / 集成测试已覆盖签名链；后续演练补端到端 |
| macOS 农场 IPA R3 为环境槽 SKIP | Linux 环境无 Xcode / codesign | 按 FARM-DEPLOY §9 保持开放，macOS 主机执行后替换 |
| 真实云对象存储（S3 / OSS）未实现 | 仅共享目录 CAS | `ObjectStore` trait 接入点已留；无对象存储时本地降级 |
| Garmin 无内建 iq 打包器 | `poly build --target garmin` package 阶段 SKIP-with-print | 已记录实验缺口；条件任务 |
| 13 个规划平台尚未声明 TargetSpec | 49 目标表中标注「规划中」 | 按 ROADMAP 推进，生成表随注册表更新 |

### 7.2 后续动作

| 动作 | 触发条件 | 验收 |
|---|---|---|
| 人类外部开发者试用 | GA 发布后 | 记录卡点并回写文档 |
| 签名信任根端到端演练 | 下一次插件演练 | 覆盖签名缺失 / 不匹配 / 信任根不可用 |
| macOS worker + IPA 真跑 | 有 macOS 主机 | FARM-DEPLOY §9 日志与产物归档 |
| 云对象存储后端 | 需要跨机大规模缓存 | `ObjectStore` 实现 + 二跑命中 |
| Garmin iq 打包器 | 条件任务触发 | 产物结构检查与真机实证 |

参考：[README](../README.md) · [ROADMAP](ROADMAP.md) · [TASKS](TASKS.md) · [FARM-DEPLOY](FARM-DEPLOY.md) · [EXPERIMENTAL-TARGETS](EXPERIMENTAL-TARGETS.md) · English: [GA.en.md](GA.en.md)
