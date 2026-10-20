# PolyPack 立项与规划：孵化项、规划平台与缺口项目

> 状态：孵化项大多已交付或收口，剩余为外部依赖与维护者决策；不构成排期承诺。本文只做立项与规划提案，不修改 `ROADMAP` / `TASKS`；后续集成点见 §8。
>
> 依据：`docs/ROADMAP.md` §13（孵化池 E1–E8）、§14（开放问题）；`docs/GA.md` §6（演练记录）、§7（已知缺口）；`README.md` 平台矩阵与 49 目标现状表。

## 0. 文档约定

- 任务 ID 与状态：`T-M9-xx` 与既有 `T-M0`–`T-M8` 不冲突，每个 ID 只出现一次；交付状态按项目记录，剩余项只有触发条件满足并经维护者评审后才进入正式里程碑。
- 人日为粗估（含实现、测试、文档），不含 NDA、审核、设备采购等外部等待。
- 证据边界：未验证的能力如实写「未验证 / 环境槽」，不把 SKIP 写成 PASS。
- 双语镜像：`docs/INITIATIVES.md` 与 `docs/INITIATIVES.en.md` 的 H2 / H3 / 表格行数逐项一致。

## 1. 总览与优先级

本文覆盖 22 个候选项：孵化项 7、规划平台 8、开放决策 2、GA 缺口 5；候选任务 55 个，粗估 226 人日。

| 批次 | 含义 | 项目 |
| --- | --- | --- |
| P0 | 现在可做：零代码或低歧义，直接补齐 GA 缺口 | M9-G5、M9-G1、M9-G2、M9-G3 |
| P1 | 有明确前置与价值，触发后一个迭代内可启动 | M9-E1、M9-E3、M9-E5、M9-G4 |
| P2 | 需设计决策、外部需求或成本验证 | M9-E2、M9-E4、M9-E6、M9-E7、M9-D1、M9-D2 |
| P3 | 关闭 / 条件 / NDA 门控 | M9-P1–M9-P8 |

## 2. 孵化项 E1–E7

> 来源：`docs/ROADMAP.md` §13。以下各项保留原「提升条件」，补全立项要素；不改变孵化池「不占 M0–M8 排期」的定位。

### M9-E1 · SBOM / 构建溯源

- 状态：候选 · 已交付（T-M9-01/02/03；SPDX / CycloneDX / in-toto、`poly verify --sbom`、回执 `sbom_digest`、双语 [SBOM](SBOM.md) 与真实样例）。
- 触发条件：首个合规或供应链审计需求出现；或目标渠道要求 SBOM / provenance。
- 背景与问题：`poly verify` 只校验产物结构与签名；`poly release` 回执只有 `manifest_digest`（`crates/poly-cli/src/release.rs`），没有依赖清单与构建来源；T-M0-11 只覆盖基础扫描，审计无法回答「这个产物由哪些输入、在什么工具链上产生」。
- 目标：每个产物旁生成 SPDX 2.3 JSON、CycloneDX 1.5 JSON 与 in-toto / SLSA 风格 provenance，绑定缓存键五输入（`CacheInputs`，`crates/poly-cache/src/digest.rs`）与 builder 身份；可离线校验。非目标：不追求 SLSA L3 认证、不接 Sigstore / Rekor、不生成 VEX。
- 方案概览：新增叶子 crate（暂名 `poly-sbom`）或在 `poly-core` 增加子模块，复用 `CacheInputs` 的 sources / lockfile / target_spec / toolchain / profile；CLI 在 `crates/poly-cli/src/verify.rs` 增加 `--sbom`，`poly release` 回执增加可选 `sbom_digest`；数据模型为 `<产物>.cdx.json`、`<产物>.spdx.json`、`<产物>.intoto.jsonl`；provenance predicate 记 builder、invocation、materials、subject。备选：外部 `cargo-cyclonedx` + `syft`（非 hermetic、额外安装）、只生成 SBOM 不生成 provenance。
- 交付物：设计说明；SBOM / provenance 生成器；`poly verify --sbom` 与回执字段；双语使用文档与 1 个真实产物样例。
- 风险与缓解：供应链工具格式漂移 → 固定 schema 版本并做金样；大产物清单膨胀 → 只记直接依赖并设体积上限；把「生成」误当「合规认证」→ 文档明示不等价。
- 决策项：优先 SPDX 还是 CycloneDX；是否引入 Sigstore；SBOM 是否随产物一起发布。
- 首步动作：在 `crates/poly-cache/src/digest.rs` 的 `CacheInputs` 上列出 materials 映射表，写 1 页设计草案。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-01 ✅ | SBOM 生成器（SPDX + CycloneDX） | coder | — | 4 | 同一输入二跑字节一致；真实产物的依赖集合与锁文件一致 |
| T-M9-02 ✅ | SLSA 风格 provenance | coder | T-M9-01 | 4 | subject 摘要等于产物摘要；materials 覆盖 CacheInputs 五输入；离线校验器通过 |
| T-M9-03 ✅ | verify / release / publish 接线 | coder + tester | T-M9-01, T-M9-02 | 3 | `poly verify --sbom` 产出三件套；回执含 `sbom_digest`；缺产物时明确报错 |

### M9-E2 · monorepo / 多应用工作区

- 状态：候选 · 已交付（T-M9-04/05/06；代码、测试、ADR、双语文档与示例脚本）。
- 触发条件：首个多应用仓库用户出现，且共享同一 Rust core 与发布流程。
- 背景与问题：`PolypackConfig`（`crates/poly-core/src/config.rs`）是单应用模型（`[package]` / `[ui]` / `[core]` / `[targets.*]` / `[retention]`）且 `deny_unknown_fields`；`Project`（`crates/poly-core/src/plan.rs`）只有一个根目录，没有 workspace 成员、应用选择与共享缓存语义。
- 目标：一份根配置声明多个应用成员，按应用解析 JobSpec 与产物落点，共享 core 源树缓存键。非目标：不做跨应用依赖图、不做 Nx / Turborepo 级任务编排、不改变单应用配置含义。
- 方案概览：已实现根 `[workspace] members`（成员目录各自持有 `polypack.toml`，id = 目录末段，唯一且不得逃逸根）；`JobSpec.app_id` 可省略，单应用序列化不变；`poly build --workspace` 按声明序构建全部成员，`--app <id>` 只构建一个；共享 core 的 compile 键不含 app id，UI bundle 键混入 app id；产物/快照落 `.polypack/packages/<app>/<target>/` 与 `.polypack/artifacts/<app>/<target>/`；新增 `E_WS_0001..0004`。集成点：`crates/poly-core/src/workspace.rs`、`plan.rs`、`job.rs`、`crates/poly-cli/src/build.rs`、`crates/poly-cache/src/digest.rs`。备选：外部脚本循环调用、把每个应用做成独立仓库（共享 core 成本高）。
- 交付物：`[workspace]` schema 与 [ADR 0008](../decisions/0008-monorepo-workspace.md)；成员发现与 JobSpec 复合语义；CLI 选择器；两应用示例（`crates/poly-cli/tests/workspace.rs` / `scripts/demo-workspace.sh`）与缓存命中测试；[WORKSPACE](WORKSPACE.md) 双语使用文档。
- 风险与缓解：schema 破坏单应用兼容 → 缺省无 `[workspace]` 时行为逐字节不变；成员间配置漂移 → 统一继承 + 显式覆盖表；缓存串键 → 键中强制包含 app_id 与成员路径。
- 决策项：单根配置还是每应用独立配置为事实源；成员是否允许不同 `core` crate。
- 首步动作：写 `[workspace]` schema 草案与两应用最小示例，先跑通配置解析再改流水线。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-04 | `[workspace]` schema 与 ADR | architect | — | 3 | 单应用配置零变化；两应用示例能通过解析；ADR 记录备选与影响 |
| T-M9-05 | 成员发现与 JobSpec 复合语义 | coder | T-M9-04 | 8 | 按 `--app` 只构建指定成员；共享 core 同输入缓存键一致；产物落点隔离 |
| T-M9-06 | CLI 选择器、示例与文档 | coder + tester | T-M9-05 | 5 | `poly build --workspace` 构建全部成员；两应用二跑缓存命中；双语文档可复现 |

### M9-E3 · 增量发布 `poly release --changed`

- 状态：候选 · 已交付（T-M9-07/08/09 于 2026-10-20 交付；5 目标实测见 [RELEASE-CHANGED](RELEASE-CHANGED.md)）。
- 触发条件：多目标 CI 发布耗时成为瓶颈，或目标数 ≥ 5 的仓库每周发布 ≥ 1 次。
- 背景与问题：`poly release` 只接受 target / repo / credential / dry-run（`crates/poly-cli/src/release.rs` 的 `Args`），没有变更判定；现有 no-op 只在单目标内比较回执 `manifest_digest`，CI 仍要构建全部目标。
- 目标：按缓存键或回执摘要跳过未变更目标，输出稳定的 skipped 事件与退出码，减少多端 CI 时间。非目标：不做增量编译、不做单目标内部分产物上传、不改变发布回执语义。
- 方案概览：已实现 `--changed`：在 `prepare` 后比较回执 `cache_key`（当前等于 `manifest_digest`）并回退 `manifest_digest`，同时校验 repo 根与仓库格式；命中时发独立 `release-skipped` 事件，不写仓库、不重写回执、不递增 `runs`，`key_source` 标明降级路径。`--since <ref>` 因 release 没有 per-target source/package 映射而返回 `E_PLAN_0002`，提示用 `--changed` 或预筛 `--target`。集成点：`crates/poly-cli/src/release.rs`、`release/report.rs`、`release/tests.rs`、`poly-cli/tests/release.rs`。备选：`git diff` 判定（跨仓库/生成文件脆弱）、mtime（不可靠）。
- 交付物：`--changed` 与 `--since`（已交付）；回执 `cache_key` 字段（已交付）；skipped JSON 契约（已交付）；CI 示例与节时实测（已交付：后端调用 5→1，墙钟节省 73–81%）。
- 风险与缓解：缓存键缺输入导致漏发 → 键构造单源 + 变更输入表测试；回执旧版本无 `cache_key` → 明确降级到摘要并告警；跳过被误认为成功发布 → skipped 与 published 事件严格区分。
- 决策项：判定主键用缓存键还是回执摘要；`--changed` 是否默认开启。
- 首步动作：在 `crates/poly-cli/src/release.rs` 写一个只读原型，打印每个目标的当前键与回执键差异。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-07 | 变更判定与 `--changed` | coder | — | 4 | 两目标中一个变更时只发一个；未变更目标输出 skipped 且退出码 0 |
| T-M9-08 | skipped 事件与回执字段 | coder + tester | T-M9-07 | 2 | JSON 契约稳定；旧回执降级有告警；published 与 skipped 不混淆 |
| T-M9-09 | CI 示例与节时实测 | tester + docs | T-M9-07, T-M9-08 | 2 | 5 目标仓库二次发布节时 ≥ 50%；双语文档可复现 |

### M9-E4 · 生产热更 / OTA 通道

- 状态：候选 · 已交付（T-M9-10/11/12 已交付；windows-2022 run 39395255929 的 `windows-updater-real` 真实更新 / 回滚通过）。
- 触发条件：首批用户提出线上更新诉求，且目标平台允许自管更新通道。
- 背景与问题：流水线止于 publish；发布后的版本迭代没有清单、签名、灰度或回滚语义。各端机制不同（Tauri updater、Play In-App Updates、App Store 分阶段发布、小程序版本管理），不能用一个包打天下。
- 目标：定义签名的更新清单与渠道 / 回滚模型，先覆盖自托管 Tauri 目标，再给出各平台机制矩阵。非目标：不做通用二进制 delta、不绕过商店审核、不承诺所有目标可自更新。
- 方案概览：已实现 `poly update manifest --channel <name>`：校验 channel/version/platform/base URL/pub_date，选定唯一产物并嵌入其 Tauri `.sig`，输出确定性的 Tauri v2 `latest.json`（排序键、两空格、尾换行），用 `GpgSigner` 生成 GPG 分离签名，同时保留 `<version>.json` / `.sig` 供回滚；`poly update check` 做 schema 校验与可选信任根验签。平台矩阵与来源日期见 [OTA](OTA.md)。集成点：`crates/poly-cli/src/update.rs`、`update/manifest.rs`、`crates/poly-sign`。备选：只写调研文档、完全交给各端工具链。
- 交付物：机制矩阵（已交付）；更新清单 schema 与签名（已交付）；Tauri 自托管通道真实安装 / 回滚演练装置与 CI 作业（已交付并通过首跑：run 39395255929，见 `examples/updater-app`）；回滚手册（已交付，见 [OTA](OTA.md)）。
- 风险与缓解：签名密钥泄露 → 复用引用模型与一次性签名环境；灰度失控 → 清单支持百分比/白名单但先只做单渠道；商店政策变化 → 矩阵标注来源日期，不写死。
- 决策项：GA 支持哪些更新渠道；更新签名是否复用发布密钥；是否提供托管清单服务。
- 首步动作：做一次逐端机制调研并写成矩阵，明确「可自更新 / 仅商店更新 / 不可更新」三类。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-10 | 各端更新机制矩阵 | researcher | — | 3 | 覆盖 Tauri / Play / App Store / 小程序 / 自托管；每条有官方来源与日期 |
| T-M9-11 | 清单 schema、签名与回滚设计 | architect | T-M9-10 | 4 | schema 版本化；签名与验签路径明确；回滚步骤可演练 |
| T-M9-12 ✅ | Tauri 自托管通道原型 | coder | T-M9-11 | 6 | 旧版本检测到新清单、验签、更新、回滚各 1 次真实演练通过（run 39395255929） |

### M9-E5 · 商店元数据管线

- 状态：候选 · 已实现（T-M9-13/14/15；作者指南见 [STORE-METADATA](STORE-METADATA.md)）。
- 触发条件：M2 / M3 首个商店发布跑通后，listing 文案或截图开始多语言维护。
- 背景与问题：`poly publish --materials` 只接受单个 JSON 文件（`crates/poly-cli/src/publish.rs`、`PublishRequest::with_materials`），没有版本化目录、字段长度校验、截图规格与多语言完整性检查；每次发版靠人工拼 JSON。
- 目标：仓库内版本化 metadata 目录，按平台 / 语言校验并确定性打包为提审材料。非目标：不自动生成截图、不调用商店 listing 更新 API、不替代人工审核。
- 方案概览：已实现 `store/metadata/<platform>/<locale>.toml`（`schema = 1`；微信 / 支付宝两套规则，CJK 计 2），校验返回全部违规并使用稳定 `metadata.*` 码；`--metadata-dir` / `--check-metadata` / `--metadata-locale` 接入 `poly publish`，`--materials` 与 `--metadata-dir` 互斥；材料按 key 排序并以 blake3(canonical JSON) 摘要写入回执 `materials_digest`。集成点：`crates/poly-publish/src/metadata.rs`、`flow.rs`、`archive.rs`、`crates/poly-cli/src/publish.rs`。备选：外部商店 CLI、每平台各自维护 JSON。
- 交付物：metadata schema 与校验器（已交付）；`poly publish` 接线（已交付）；多语言完整性报告（已交付）；双语作者指南（已交付，见 [STORE-METADATA](STORE-METADATA.md)）。
- 风险与缓解：平台字段规则变化 → 规则表版本化并标来源；截图资产膨胀 → 只存路径与摘要，不入库二进制；语言缺失 → 默认语言回退但显式告警。
- 决策项：metadata 事实源放仓库还是 CMS；截图是否允许外链。
- 首步动作：选 2 个已支持平台，列出其 listing 必填字段与长度限制，先落 schema。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-13 | metadata schema 与校验器 | architect + coder | — | 3 | 2 平台必填字段与长度校验；缺语言与超长均报稳定错误 |
| T-M9-14 | publish 接线与确定性打包 | coder | T-M9-13 | 4 | `--check-metadata` 失败不出包；同输入材料摘要一致；回执记摘要 |
| T-M9-15 | 多语言报告与作者文档 | tester + docs | T-M9-14 | 2 | 报告列出缺失语言/字段；双语指南含完整示例 |

### M9-E6 · GUI 目标矩阵视图 / 构建历史

- 状态：候选 · 已交付（T-M9-16/17 与 T-M9-18 均于 2026-10-20 交付；T-M9-18 GUI 已落 `app/`（Tauri v2 + 静态前端），满足 [ADR 0007](../decisions/0007-external-and-ui-decisions.md) 的 `app/` 跟踪条件；真实 Tauri 窗口启动为环境门控）。
- 触发条件：M1 Tauri 管线跑通且 `app/` 控制台骨架落地；或用户明确要求可视化构建历史。
- 背景与问题：README 把 `app/` 描述为 poly-gui 控制台目标形态，但此前仓库未跟踪该目录；`BuildEvent`（`crates/poly-core/src/events.rs`）只经 stdout / JSON Lines 输出，没有持久历史，无法回答「某目标最近一次成功构建是什么时候」。
- 目标：本地持久化构建事件，提供目标 × 最近构建的矩阵视图与单次构建详情。非目标：不做多用户服务端、不做云端同步、不做远程农场调度 UI。
- 方案概览：已实现 `HistoryRecord`（schema_version、build_id、ts、target、event、status、duration、artifact 摘要、错误码）写入 `.polypack/history/events.jsonl`，按 5 MiB / 10000 条双上限轮转并保留最新记录；路径落盘前脱敏为项目相对路径。`poly history [--target ID] [--limit N] [--json]` 提供按目标聚合的查询；`app/` 的 Tauri command `history_records` 复用同一 schema 并在读取时二次脱敏，前端渲染目标矩阵、按目标详情与空状态引导。集成点：`crates/poly-core/src/events.rs`、`crates/poly-cli/src/build.rs`、`crates/poly-cli/src/history.rs`、`app/`。备选：解析 CI 日志（不可靠）、SQLite（查询强但首版过重）。
- 交付物：history 写入与轮转（已交付）；查询 / 聚合命令（已交付）；矩阵与详情视图（已交付，`app/`）；隐私说明（路径脱敏，已交付）。
- 风险与缓解：事件流含敏感路径 → 落盘前脱敏，只留相对路径；历史无限增长 → 条数与体积双上限；GUI 与 CLI 漂移 → 共用同一 schema 与金样。
- 决策项：JSONL 还是 SQLite；历史默认保留期；是否记录本机绝对路径。
- 首步动作：在 `crates/poly-core/src/events.rs` 定义可落盘的 `HistoryRecord` schema，并让 `poly build --json` 与文件写入共用。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-16 | 事件持久化与轮转 | coder | — | 3 | 每次构建追加一条可解析记录；超限轮转；路径脱敏断言 |
| T-M9-17 | 历史查询与矩阵聚合 | coder | T-M9-16 | 5 | 按目标返回最近 N 次状态/耗时/摘要；空历史不报错 |
| T-M9-18 | poly-gui 矩阵与详情视图 | frontend | T-M9-17 | 8 | 矩阵显示全部已构建目标；点击进入详情；无历史时给出引导 |

### M9-E7 · 发布前设备预览矩阵

- 状态：候选 · 部分交付（T-M9-19 调研矩阵与 T-M9-20 `DevicePool` / `DeviceLease` + `poly preview --dry-run` 已于 2026-10-20 交付，见 [DEVICE-PREVIEW](DEVICE-PREVIEW.md)；T-M9-21 真机演练为外部环境门控，不标 ✅；[ADR 0007](../decisions/0007-external-and-ui-decisions.md) 的「本阶段不写设备农场代码」由干跑协议取代，真实执行仍未接线）。
- 触发条件：M4 TV 或 M7 穿戴冒烟遇到设备瓶颈，或商店预览 / 真机回归成为发布阻塞。
- 背景与问题：`poly smoke` 与各平台结构检查能在无设备时 SKIP，但没有设备池、租约、安装启动截图与日志归档；发布前真机矩阵完全靠人工。
- 目标：定义设备池与预览任务模型，在真机 / 模拟器上执行安装、启动、截图、日志采集并归档证据。非目标：不做完整设备农场产品、不做无人值守审核、不承诺所有目标有设备。
- 方案概览：已实现 `DevicePool` / `DeviceLease` / `CleanupPolicy` 与 `poly preview --dry-run`（确定性计划、租约超时、清理语义、`preview/v1` + `farm/v1` 词汇），不触网、不接设备；vendor 矩阵与 build-vs-buy 见 [DEVICE-PREVIEW](DEVICE-PREVIEW.md)。provider 可为 Firebase Test Lab、BrowserStack、AWS Device Farm 或自建；产物落 `.polypack/preview/<target>/<device>/`。真实设备执行仍为 T-M9-21 外部项。
- 交付物：设备池模型与协议扩展（已交付）；预览任务 CLI（已交付，dry-run）；2 个目标的真实预览演练（外部）；成本与保留策略（已交付矩阵 / 策略）。
- 风险与缓解：设备租用成本 → 先按需单次租用，不做常驻池；平台工具链差异 → provider 适配层隔离；截图含用户数据 → 只跑夹具应用并自动清理。
- 决策项：自建还是购买云设备农场；证据保留期与访问权限。
- 首步动作：调研 3 家云设备农场的 API、价格与覆盖矩阵，给出 build vs buy 结论。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-19 ✅ | 设备农场调研与成本模型 | researcher | — | 3 | 3 家供应商 API/价格/覆盖对比；含自建 TCO |
| T-M9-20 ✅ | DevicePool / DeviceLease 协议扩展 | architect | T-M9-19 | 4 | 租约、超时、回收语义明确；与农场协议 v1 兼容 |
| T-M9-21 | 双目标预览演练 | coder | T-M9-20 | 6 | Android + TV 各 1 台设备安装启动截图；失败有日志与稳定错误码 |

## 3. 规划平台（8 个未声明 TargetSpec）

> 来源：`README.md` 49 目标现状表中 8 个「规划中」行，清单单一事实源为 `tools/gen-status-table.mjs` 的 `PLATFORMS` 列表中 `planned` 行。注意：`docs/GA.md` §7.1 写「13 个规划平台」，与 README / 生成器的 8 个不一致，集成时先对账。
>
> 结论口径：**关闭（close）** = 从规划清单移除或标为 retired；**条件（conditional）** = 仅在明确触发条件下启动；**NDA 门控（NDA-gated）** = 无授权不写代码、SDK 不入库。

### M9-P1 · Google Play Instant

- 状态：决策已落地（2026-10-20）：关闭（retired，保留一行），生成器与四份文档已同步；见 [ADR 0006](../decisions/0006-platform-decisions.md)。
- 触发条件：Google 官方恢复 Instant Apps 受理与 Instant API（当前无迹象）。
- 现状：Google 自 2025-12 起停止受理 Instant Apps 发布，Play services Instant API 停用；README 与 `tools/gen-status-table.mjs` 已标注「已停用（retired）」，无 TargetSpec。
- 可行性：不可行；没有可提交的渠道。
- 阻塞与前置：服务停服；无开发者后台入口；无审核路径。
- 建议：关闭。保留历史链接，不再投入打包器。
- 目标：保持矩阵诚实并清理失效规划；非目标：不为已停服平台实现 `instant-aab`。
- 方案概览：不改 `crates/poly-targets/src/builtin.rs`；从 `tools/gen-status-table.mjs` 的规划清单移除或改标 retired，并同步 README 49 行计数与分级计数（ROADMAP §11 第 4 条）。备选：保留「规划中」但不实现（当前状态）、迁移到 PWA / Web 演示。
- 交付物：关闭决策记录；生成器与 README 计数同步变更；CHANGELOG 条目。
- 风险与缓解：删除行导致 49 目标表与 docs-check 全面漂移 → 一次性按生成器重生成并跑 `node tools/check-docs.mjs`；用户仍搜索该平台 → 保留一行 retired 说明。
- 决策项：从 49 表移除还是标 retired（影响 TOTAL 与分级计数）。
- 首步动作：引用 Google 官方停服公告归档证据，开一条 ADR 决定「移除 vs retired」。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-22 | 停服证据归档与关闭 ADR | researcher + maintainer | — | 1 | 官方公告可追溯；ADR 给出移除或 retired 结论 |
| T-M9-23 | 条件任务：恢复受理后的 instant-aab | coder | T-M9-22 | 6 | 仅在官方恢复后启动；AAB 含 instant 清单且通过 bundletool 校验 |

### M9-P2 · Snapchat Mini

- 状态：决策已落地（2026-10-20）：关闭 + 季度复审（下次 2027-01-20），生成器已标注关闭；见 [ADR 0006](../decisions/0006-platform-decisions.md)。
- 触发条件：Snap 官方重新发布可核验的 Minis 文档、SDK 与上传契约。
- 现状：官方文档入口 404，developers.snap.com 无 Minis 文档；npm `@snapchat/minis-sdk` 3.0.5（2022）标注 deprecating minis；无 zip / manifest / 打包契约可核验。
- 可行性：当前不可行；契约缺失使任何打包器都是猜测。
- 阻塞与前置：官方文档、开发者账号、上传接口、审核流程，四者缺一不可。
- 建议：关闭；每季度复查一次官方入口，出现完整契约再重开。
- 目标：避免伪造平台产物；非目标：不实现基于社区猜测的 zip 格式。
- 方案概览：保持 `tools/gen-status-table.mjs` 的 planned 行并加「季度复审」；若契约恢复，复用 HTML5 静态包路径（参考 `facebook-instant-games` / `tiktok-minis` 打包器）再加 Snap manifest。备选：只输出通用 HTML5 包供自托管（不声称 Snap 兼容）。
- 交付物：季度复审记录；关闭或重开的决策记录；条件任务的触发证据清单。
- 风险与缓解：把 404 当永久结论 → 设定复审日期；社区包误导 → 文档明示不可核验；开发者账号不可得 → 条件任务不启动。
- 决策项：是否保留在 49 表（建议保留但标注关闭候选）。
- 首步动作：建立季度复查清单（文档 URL、SDK 版本、上传入口、审核条款），本次记录 404 证据。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-24 | 官方契约复查与证据归档 | researcher | — | 1 | 文档/SDK/上传/审核四项均有结论与日期 |
| T-M9-25 | 条件任务：HTML5 zip + manifest 打包器 | coder | T-M9-24 | 5 | 仅在四项契约可核验后启动；结构自检 + 确定性 zip |

### M9-P3 · LINE Mini App

- 状态：仓库内部分已交付（T-M9-26/27，2026-10-20）：条件支持（仅部署模式，不声明 TargetSpec）；实现见 [ADR 0009](../decisions/0009-line-deploy.md) 与 [LINE-DEPLOY](LINE-DEPLOY.md)，真实 Console 注册与托管演练仍为外部步骤。
- 触发条件：出现明确使用 LINE 渠道的用户，且接受「只托管 URL、无本地上传包」。
- 现状：LIFF 应用由 LINE Developers Console 以托管 URL 注册（LINE Login 渠道 → LIFF；Endpoint URL 必须 HTTPS 且不得含 fragment）；官方无本地上传包格式或 LIFF 配置文件。
- 可行性：作为「部署 + 配置校验」可行；作为平台产物打包不可行。
- 阻塞与前置：LINE Login 渠道、LIFF 应用、HTTPS 静态托管；无官方 CI 上传 API。
- 建议：条件支持 `poly deploy line-liff --dir <dir> --config line-liff.toml`，只做校验与部署指引，不生成「平台包」。
- 目标：让 LINE 用户复用 Web 构建与配置校验；非目标：不伪造 LIFF 配置文件、不声明商店受理。
- 方案概览：已实现 `line-liff.toml`（`schema`、`liff_id`、`endpoint`、`channel_id`、`scopes`）与 `poly deploy line-liff --dir <dir> --config line-liff.toml`；校验 HTTPS / 无 fragment / 必填字段，输出 Console 人工步骤；命令始终 dry-run，不触网、不接上传 API。集成点：`crates/poly-cli/src/deploy.rs`、`crates/poly-cli/src/main.rs`；使用说明见 [LINE-DEPLOY](LINE-DEPLOY.md)。备选：纯文档指引、通用静态托管。
- 交付物：部署模式设计；配置校验器；端到端托管演练（人工 Console 步骤）；文档。
- 风险与缓解：把部署模式误当 TargetSpec → 文档与命令命名明确区分；Endpoint 规则变化 → 校验规则版本化；无上传 API → 明确 SKIP 人工步骤。
- 决策项：是否接受「非 TargetSpec 的部署模式」进入官方支持面。
- 首步动作：用现有 Web 构建产物手工注册一个 LIFF 应用，记录全部 Console 步骤与失败点。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-26 | line-liff 部署模式设计 | architect | — | 2 | 明确不声明 TargetSpec；配置字段与人工步骤完整 |
| T-M9-27 | 校验器与部署 dry-run | coder | T-M9-26 | 4 | 非 HTTPS / 含 fragment / 缺字段均失败；dry-run 不触网且输出步骤 |

### M9-P4 · Sailfish OS

- 状态：候选 · 条件支持（需赞助 / 设备承诺）；未排期，决策见 [ADR 0006](../decisions/0006-platform-decisions.md)。
- 触发条件：有 Sailfish 用户或赞助方承诺测试设备与 Jolla 账号。
- 现状：现有 `linux-rpm` 链只出桌面 Tauri RPM（`crates/poly-bundle/src/packagers/linux.rs`）；缺 Qt / Silica QML 宿主、Sailfish SDK 交叉工具链（armv7hl / aarch64）与 harbour 打包 / 签名。
- 可行性：技术上中等偏高（可复用 RPM 与 Linux 交叉经验）；市场上小众。
- 阻塞与前置：Sailfish SDK、QML / Silica 宿主模板、harbour 账号与签名、真机或模拟器。
- 建议：条件支持；无赞助则不做。
- 目标：产出可安装的 harbour RPM 并通过结构校验；非目标：不复用 Tauri 桌面壳冒充 Sailfish 应用。
- 方案概览：新增 `ui/adapters/native/host/sailfish` QML / Silica 壳；`crates/poly-bundle` 增加 `sailfish-harbour` 打包器，复用 RPM 写入但使用 harbour spec 与 desktop 文件；`poly-toolchain` 探测 Sailfish SDK；TargetSpec 在打包与校验通过后再声明。备选：Qt 独立应用（不接 PolyPack）、纯 Web。
- 交付物：工具链探测；QML 宿主模板；harbour RPM 打包器；契约测试与 TargetSpec 草案。
- 风险与缓解：SDK 获取与许可 → 只在本地探测，不入库；设备稀缺 → 条件任务要求提供设备；维护成本 → experimental 标注直到稳定。
- 决策项：是否接受 experimental 级别的社区维护目标。
- 首步动作：在 Linux 上安装 Sailfish SDK，交叉编译一个最小 QML 应用并记录工具链路径。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-28 | Sailfish SDK / harbour 工具链探测 | researcher | — | 3 | SDK 路径、交叉 triple、harbour 命令与许可结论可复现 |
| T-M9-29 | QML / Silica 宿主与 RPM 打包器 | coder | T-M9-28 | 8 | 最小应用出 harbour RPM；结构断言通过；缺 SDK 时 SKIP-with-print |
| T-M9-30 | 契约测试与 TargetSpec 草案 | tester + architect | T-M9-29 | 4 | 金样二跑一致；rpmlint 通过；TargetSpec 字段有证据支撑 |

### M9-P5 · Ubuntu Touch

- 状态：候选 · 条件支持（需赞助 / 设备承诺）；未排期，决策见 [ADR 0006](../decisions/0006-platform-decisions.md)。
- 触发条件：有 Ubuntu Touch 用户或赞助方承诺测试设备与 OpenStore 发布。
- 现状：现有 Linux deb 链不能产出 `.click`（格式、AppArmor、click 清单均不同）；缺 Clickable / Ubuntu Touch SDK 与 WebView 壳。
- 可行性：技术上中等；生态与设备规模小。
- 阻塞与前置：Clickable、UBports SDK、AppArmor profile、OpenStore 提交流程。
- 建议：条件支持；无赞助则不做。
- 目标：产出可安装的 `.click` 并通过结构校验；非目标：不把 deb 改后缀当 `.click`。
- 方案概览：新增 `ui/adapters/native/host/ubuntutouch` WebView 壳；`crates/poly-bundle` 增加 `clickable` 打包器（调用 Clickable 或生成 click 清单 + AppArmor）；`poly-toolchain` 探测 Clickable；TargetSpec 在真实安装通过后声明。备选：PWA、Flatpak、Ubuntu deb。
- 交付物：工具链探测；WebView 壳模板；`.click` 打包器；契约测试与 TargetSpec 草案。
- 风险与缓解：Clickable 版本漂移 → 固定版本并做金样；AppArmor 策略过宽 → 最小权限模板；设备缺失 → 条件任务要求提供设备。
- 决策项：是否接受 experimental 级别的社区维护目标。
- 首步动作：安装 Clickable，用官方模板构建一个空 `.click`，记录清单与 AppArmor 结构。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-31 | Clickable / UBports 工具链探测 | researcher | — | 3 | 能构建官方模板；清单、AppArmor、架构结论可复现 |
| T-M9-32 | WebView 壳与 `.click` 打包器 | coder | T-M9-31 | 8 | 最小应用出 `.click`；结构断言通过；缺工具时 SKIP-with-print |
| T-M9-33 | 契约测试与 TargetSpec 草案 | tester + architect | T-M9-32 | 4 | 真机安装启动 1 次；金样二跑一致；OpenStore 步骤成文 |

### M9-P6 · PlayStation

- 状态：候选 · NDA 门控（无授权不写代码）；未排期，决策见 [ADR 0006](../decisions/0006-platform-decisions.md)。
- 触发条件：获得 Sony 授权开发者身份并签署 NDA，且能本地安装官方 SDK。
- 现状：L7 规划目标；官方 SDK 需授权，SDK 不入库、只做本地路径探测；当前无 TargetSpec。
- 可行性：公开不可行；只能在授权环境内以插件形态实现。
- 阻塞与前置：NDA、devkit、SDK、认证流程；任何一项缺失都不启动。
- 建议：NDA 门控；公开仓库只保留插件接口与 mock，不提交 SDK 或平台包格式。
- 目标：授权后以 `target-spec/v1` 插件接入打包；非目标：不在公开仓库实现或分发厂商 SDK。
- 方案概览：插件实现 `describe` / `package`（`crates/poly-plugin/wit/polypack-target.wit`），宿主探测 `$SONY_SDK` 本地路径；无 SDK 时 SKIP-with-print；产物格式由授权方文档定义。备选：不做主机平台、只做 PC / Web 发行。
- 交付物：NDA 前置清单；条件插件骨架；mock 契约测试；本地路径探测。
- 风险与缓解：SDK 泄漏 → `.gitignore` 与 secret-scan 兜底；公开文档越界 → 只写已公开信息；维护者无授权 → 任务不启动。
- 决策项：是否投入 NDA 与 devkit 成本。
- 首步动作：整理 NDA / 开发者申请前置清单，明确「无授权不写代码」。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-34 | NDA 前置清单与 SDK 路径探测 | maintainer | — | 1 | 清单含授权、devkit、SDK、认证；仓库零 SDK 文件 |
| T-M9-35 | 条件任务：target-spec 插件骨架 | coder | T-M9-34 | 8 | 仅在授权后启动；mock 契约通过；无 SDK 时 SKIP-with-print |

### M9-P7 · Xbox

- 状态：候选 · NDA 门控（无授权不写代码）；未排期，决策见 [ADR 0006](../decisions/0006-platform-decisions.md)。
- 触发条件：获得微软授权开发者身份并签署 NDA，且能本地安装 GDK。
- 现状：L7 规划目标；GDK 需授权，SDK 不入库、只做本地路径探测；当前无 TargetSpec。
- 可行性：公开不可行；只能在授权环境内以插件形态实现。
- 阻塞与前置：NDA、devkit、GDK、认证流程。
- 建议：NDA 门控；公开仓库只保留插件接口与 mock。
- 目标：授权后以 `target-spec/v1` 插件接入打包；非目标：不在公开仓库实现或分发 GDK。
- 方案概览：插件实现 `describe` / `package`，宿主探测 `$GDK` 本地路径；无 GDK 时 SKIP-with-print；产物格式由授权文档定义。备选：不做主机平台、只做 PC / Web 发行。
- 交付物：NDA 前置清单；条件插件骨架；mock 契约测试；本地路径探测。
- 风险与缓解：SDK 泄漏 → secret-scan；公开文档越界 → 只写已公开信息；无授权 → 任务不启动。
- 决策项：是否投入 NDA 与 devkit 成本。
- 首步动作：整理微软授权 / NDA 前置清单，明确「无授权不写代码」。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-36 | NDA 前置清单与 GDK 路径探测 | maintainer | — | 1 | 清单含授权、devkit、GDK、认证；仓库零 SDK 文件 |
| T-M9-37 | 条件任务：GDK 插件骨架 | coder | T-M9-36 | 8 | 仅在授权后启动；mock 契约通过；无 GDK 时 SKIP-with-print |

### M9-P8 · Nintendo Switch

- 状态：候选 · NDA 门控（无授权不写代码）；未排期，决策见 [ADR 0006](../decisions/0006-platform-decisions.md)。
- 触发条件：获得任天堂授权开发者身份并签署 NDA，且能本地安装 NintendoSDK。
- 现状：L7 规划目标；NintendoSDK 需授权，SDK 不入库、只做本地路径探测；当前无 TargetSpec。
- 可行性：公开不可行；只能在授权环境内以插件形态实现。
- 阻塞与前置：NDA、devkit、NintendoSDK、认证流程。
- 建议：NDA 门控；公开仓库只保留插件接口与 mock。
- 目标：授权后以 `target-spec/v1` 插件接入打包；非目标：不在公开仓库实现或分发 NintendoSDK。
- 方案概览：插件实现 `describe` / `package`，宿主探测 `$NINTENDO_SDK` 本地路径；无 SDK 时 SKIP-with-print；产物格式由授权文档定义。备选：不做主机平台、只做 PC / Web 发行。
- 交付物：NDA 前置清单；条件插件骨架；mock 契约测试；本地路径探测。
- 风险与缓解：SDK 泄漏 → secret-scan；公开文档越界 → 只写已公开信息；无授权 → 任务不启动。
- 决策项：是否投入 NDA 与 devkit 成本。
- 首步动作：整理任天堂授权 / NDA 前置清单，明确「无授权不写代码」。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-38 | NDA 前置清单与 NintendoSDK 路径探测 | maintainer | — | 1 | 清单含授权、devkit、SDK、认证；仓库零 SDK 文件 |
| T-M9-39 | 条件任务：NintendoSDK 插件骨架 | coder | T-M9-38 | 8 | 仅在授权后启动；mock 契约通过；无 SDK 时 SKIP-with-print |

## 4. 开放决策

> 来源：`docs/ROADMAP.md` §14。两项都是决策项，不是已排期功能；先写 ADR，再决定是否投入实现。

### M9-D1 · 插件市场分发：官方注册表 vs 去中心化索引

- 状态：候选 · 部分落地；决策采用去中心化签名索引（[ADR 0004](../decisions/0004-plugin-marketplace.md)），T-M9-41 的签名索引与 `poly plugin add/search` 已交付；官方注册表 / 生产密钥仪式外部。
- 触发条件：第三方插件 ≥ 3 个，或出现第一个「如何发现 / 安装插件」的外部请求。
- 背景与问题：ADR 0003 已定 WASM 沙箱与 `target-spec/v1`，但插件只以文件形式分发（`crates/poly-plugin`、`crates/poly-cli/src/plugin_new.rs`），没有索引、版本解析、吊销与来源展示。
- 目标：选定分发模型并定义可验证的索引格式。非目标：不做付费市场、不做托管插件执行、不改变 SRI / 签名信任链。
- 方案概览：推荐「去中心化签名索引优先」：`index.json` 含 schema、plugins[]（id、version、wit_version、sri、signature、url、revoked），索引本身用 GPG/minisign 签名；`poly plugin search/add` 复用 `poly-sign` 验签；官方注册表作为可选策展镜像。备选：官方注册表（治理与可用性成本高）、Git 仓库索引（无签名语义）、OCI registry（生态重）。
- 交付物：ADR（已交付）；索引 schema（已交付）；`poly plugin add/search`（已交付，离线验签 / 吊销 / 回滚 / SRI）；吊销清单与信任根文档（ADR 已记录；生产公钥与仪式外部）。
- 风险与缓解：供应链投毒 → SRI + 签名 + 吊销三件套；索引不可用 → 本地缓存与多镜像；治理不清 → ADR 明确谁可吊销。
- 决策项：官方注册表还是去中心化索引；谁拥有吊销权；是否允许私有索引。
- 首步动作：写 ADR 0004 草案，比较两种模型的威胁与运维成本，先定签名与吊销语义。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-40 ✅ | 分发模型 ADR 与威胁模型 | architect | — | 3 | 两模型对比；签名、吊销、信任根与治理结论明确 |
| T-M9-41 ✅ | 签名索引与 `poly plugin add/search` | coder | T-M9-40 | 6 | 索引验签失败拒绝；安装复用 SRI + GPG；吊销条目拒绝加载 |
| T-M9-42 | 条件任务：官方注册表 / 镜像 | coder + maintainer | T-M9-41 | 8 | 仅在决定做官方服务后启动；可用性与吊销演练通过 |

### M9-D2 · 远程农场：仅自托管 vs 托管服务

- 状态：候选 · 部分落地；决策 GA 仅自托管（[ADR 0005](../decisions/0005-farm-hosting.md)），T-M9-44 的加固清单 + 预检 / 演练已交付；生产主机演练外部。
- 触发条件：外部用户缺少 macOS / Windows 构建机且明确要求托管；或自托管部署支持成本成为负担。
- 背景与问题：农场 v1 已有协议、隔离、共享缓存与部署指南（`crates/poly-cli/src/farm.rs`、`docs/FARM.md`、`docs/FARM-DEPLOY.md`），但 ROADMAP 原则写明不托管用户代码与密钥；托管与否会影响信任边界与商业模式。
- 目标：做出「仅自托管」或「托管服务」的明确决策与边界。非目标：本文不承诺实现托管服务、不改变现有自托管能力。
- 方案概览：推荐 GA 阶段仅自托管；托管作为独立服务另立项目，需租户隔离、配额、密钥托管、审计、数据驻留与 SLA。若做托管，复用农场协议 v1 与 worker 池，新增控制面；否则补强自托管参考部署。备选：合作伙伴托管、用户自带云（BYOC）。
- 交付物：ADR（已交付）；成本与威胁模型（已交付）；自托管生产加固清单 + 预检（已交付，见 [FARM-DEPLOY](FARM-DEPLOY.md) §11）；生产主机演练（外部）。
- 风险与缓解：托管违反现有原则 → 必须显式修订 ROADMAP 原则并评审；密钥托管风险 → 只存引用或用户自带 KMS；运维成本 → 先做 BYOC。
- 决策项：是否提供托管服务；若提供，谁承担密钥与合规责任。
- 首步动作：写 ADR 0005 草案，列出托管的最小可信边界与不做的理由。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-43 ✅ | 农场形态 ADR 与成本模型 | architect | — | 3 | 自托管 / 托管 / BYOC 对比；信任边界与成本可审计 |
| T-M9-44 ✅ | 条件任务：托管控制面原型或自托管加固 | coder | T-M9-43 | 8 | 决策为自托管：加固清单 + 预检 + Linux 演练已交付（生产主机演练外部） |

## 5. GA §7 缺口项目

> 来源：`docs/GA.md` §7.1 / §7.2。以下项目把「已知缺口」转成可验收的候选任务；不改变 GA 的完成声明。

### M9-G1 · CLI 分发到 crates.io / GitHub Releases

- 状态：候选 · 部分落地（GitHub Releases + install script + 签名 / install-smoke 已落地；T-M9-45 审计已落地：workspace 1.14.0、crate 元数据、`cargo package --list` 9/9；crates.io 实际发布因 `poly-cli` / `poly-core` 名称占用与 `publish = false` 关闭）。
- 触发条件：GA 发布；外部用户要求不装 Rust 工具链即可使用 `poly`。
- 背景与问题：根 `Cargo.toml` 版本为 `0.0.0`，`crates/poly-cli/Cargo.toml` 的 bin 名为 `poly`，仓库有 `repository` 元数据；此前没有发布工作流，用户只能源码构建。现已落 `.github/workflows/release.yml`（`v*` tag 三宿主构建 + `SHA256SUMS.txt`）、`scripts/install.sh`（强制 SHA-256 + 可选 minisign 严格验签）、双语 `docs/INSTALL`、secret-gated 签名与 install-smoke；`docs/CRATES-IO` 记录了发布顺序、元数据与阻塞：`poly-cli`（已被占用）与 `poly-core`（已被占用）需改名或取得所有权，且全部 crate 仍 `publish = false`。
- 目标：提供可验证的 crates.io 安装与 GitHub Releases 二进制，带校验和与签名。非目标：首版不做 Homebrew / Scoop / apt 仓库。
- 方案概览：先按依赖顺序 `cargo package` / `cargo publish --dry-run` 审计全部可发布 crate；GitHub Actions 构建 linux x86_64 / aarch64、macOS x86_64 / aarch64、Windows x86_64；发布 `SHA256SUMS` 与 GPG/minisign 签名；安装脚本验签后落盘。审计已完成（`cargo package --list` 9/9，`cargo publish --dry-run` 被 `publish = false` 拒绝）；GitHub Releases 腿已落地（三宿主构建 + `SHA256SUMS` + secret-gated minisign 签名 + install-smoke），安装脚本与双语文档已落地。crates.io 上传保持关闭，等待名称与 token 的维护者决策。集成点：根 `Cargo.toml`、`crates/poly-cli/Cargo.toml`、`.github/workflows/`、`docs/CRATES-IO`。备选：只发 GitHub Releases（已落地）、只发 crates.io（需 Rust，阻塞）。
- 交付物：发布清单与 crate 顺序（已交付，`docs/CRATES-IO`）；跨平台构建工作流（已交付）；校验和 / 签名（已交付，签名 secret-gated）；安装脚本与双语文档（已交付）；crates.io 实际发布（阻塞：名称占用、`publish = false`、无 token）。
- 风险与缓解：crate 名 / 二进制名 `poly` 冲突 → 已查占位并准备备用名（`polypack-cli` / `polypack-engine`）；版本号曾为 0.0.0 → 已提升到 1.14.0 并与 tag 对齐；供应链 → 签名 + 可复现构建说明。
- 决策项：crates.io 与 Releases 是否都做；二进制名冲突时是否改名。
- 首步动作：跑 `cargo package --list` 与 `cargo publish --dry-run`，列出阻塞发布的 crate 元数据。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-45 ✅ | crate 发布审计与版本策略 | release | — | 2 | 审计完成：元数据 9/9、`cargo package --list` 9/9、版本 1.14.0；实际发布因名称占用 / `publish = false` / 无 token 关闭 |
| T-M9-46 ✅ | 跨平台 Release 工作流与签名 | release + coder | T-M9-45 | 5 | 三宿主产物可下载；SHA256SUMS 与 secret-gated 签名验证通过 |
| T-M9-47 ✅ | 安装脚本与文档 | docs + tester | T-M9-45, T-M9-46 | 2 | install-smoke 跑通 `poly --version`；双语 INSTALL 可复现 |

### M9-G2 · 人类外部开发者试用协议

- 状态：候选 · 协议已落地（T-M9-48）；T-M9-49 的 ≥3 名真人试用是外部项。
- 触发条件：GA 发布后；首个外部开发者愿意参与。
- 背景与问题：GA §6 的「1 天新增目标」由独立上下文 AI 代理执行，明确未做人类可用性研究；没有招募、知情同意、任务脚本、观察记录与问题分级流程。协议与模板已落在 `docs/HUMAN-TRIAL.md` / `.en.md`；真人执行无法由本机完成。
- 目标：建立可重复的人类试用协议并完成 ≥ 3 名外部开发者试用。非目标：不做正式可用性实验室、不收集无关个人数据。
- 方案概览：协议含招募标准、知情同意、90 分钟任务脚本（脚手架 → 自定义目标 → 出包）、think-aloud 记录、问题分级（阻塞 / 摩擦 / 建议）、匿名化报告模板；问题回写 GA §7.2 与 issue。集成点：`docs/GA.md` §6 / §7.2、`.github/ISSUE_TEMPLATE`。备选：只做问卷、只做代理演练（现状）。
- 交付物：试用协议；同意书与记录模板；≥ 3 份匿名报告；修复清单。
- 风险与缓解：隐私 → 最小化采集与匿名化；样本偏差 → 覆盖不同 Rust / 移动背景；阻塞问题被淹没 → 按严重度设修复 SLA。
- 决策项：试用是否给报酬；报告是否公开。
- 首步动作：写 1 页协议与任务脚本，先内部彩排一次并修正。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-48 | 人类试用协议与模板 | docs | — | 2 | 含同意、脚本、记录、分级、匿名化；内部彩排通过 |
| T-M9-49 | 执行 ≥ 3 人试用并回写 | researcher + docs | T-M9-48 | 5 | 3 份匿名报告；阻塞问题有 issue；GA §7.2 更新建议成文 |

### M9-G3 · 环境槽收口计划

- 状态：候选 · 台账 / owner / validator 已落地（T-M9-50、T-M9-52）；T-M9-51 的真实逐槽关闭仍为外部项。
- 触发条件：现在；在任何对外声明「已验证」之前必须收口。
- 背景与问题：GA §7.1 与 FARM-DEPLOY §9 仍开放 macOS IPA、真实 S3 / OSS、keyring / Vault、Garmin iq 等环境槽；缺统一台账时 SKIP-with-print 容易被误读为通过。`docs/ENV-SLOTS.md` 已落地（ENV-01..ENV-19，含 Owner/Evidence 列），`tools/check-env-slots.mjs` 已接入 docs-check；真实逐槽关闭仍需外部环境与证据。
- 目标：每个槽有 owner、缺失能力、证据路径、关闭判据与状态；无 owner 的 SKIP 不得进入发布说明。非目标：不伪造环境、不把 mock 验证写成真实环境验证。
- 方案概览：台账已建（slot id、target、缺失能力、owner、证据路径、状态），槽位来源为 GA §7.1、FARM-DEPLOY §9、各目标 notes；关闭证据为真实日志 + 产物摘要；可加 `tools/check-env-slots.mjs` 校验台账与文档引用。集成点：`docs/GA.md` §7.1、`docs/FARM-DEPLOY.md` §9、`.github/workflows/`。备选：只在 issue 跟踪（不可审计）。
- 交付物：环境槽台账（已落 `docs/ENV-SLOTS.md`）；四类槽的关闭证据；无 SKIP 的发布检查清单。
- 风险与缓解：环境不可得 → 明确 owner 与替代方案，不硬造 PASS；台账与文档漂移 → 机器校验；成本 → 按影响排序，先 macOS IPA 与真实对象存储。
- 决策项：谁拥有各槽；是否接受长期开放槽并披露。
- 首步动作：从 GA §7.1 与 FARM-DEPLOY §9 摘出全部槽位，建一张 owner / 证据表。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-50 | 环境槽台账与 owner 分配 | maintainer | — | 1 | 每个槽有 owner、证据路径、关闭判据；无遗漏 |
| T-M9-51 | 逐槽关闭（macOS IPA / S3 / keyring / Garmin iq） | coder + maintainer | T-M9-50 | 10 | 每槽有真实日志与产物摘要；未关闭的明确披露 |
| T-M9-52 | 台账校验与发布门禁 | coder | T-M9-51 | 2 | 无 owner 的 SKIP 使检查失败；文档引用一致 |

### M9-G4 · MSI 真样本启动测试

- 状态：候选 · 已交付（2026-10-20 `windows-2022` 首跑通过：`windows-msi-real` 6m30s）。
- 进度（2026-10-20）：`examples/windows-msi-app/` 真实 Tauri v2 样例、`crates/poly-bundle/tests/windows_msi_real.rs` 门控真跑测试（`POLY_MSI_REAL=1`、`POLY_TAURI_CORE_BIN`、`POLY_MSI_REAL_OUT`，失败码 `POLY_MSI_REAL_0001..0006`）与 rust-ci `windows-msi-real` 作业已落；run 39308849058 真跑通过，MSI / 安装 / 启动 / 卸载 / 残留证据作为 CI 工件归档。ENV-01 已置 `CLOSED`。
- 触发条件：Windows L1 对外宣称可安装；或首次 Windows 用户报告安装问题。
- 背景与问题：`crates/poly-bundle/src/packagers/windows.rs` 生成 WiX `.wxs` 并调用 candle / light，注释给出 `msiexec /i` 冒烟提示，但没有真实样本的安装、启动、卸载与残留检查证据。
- 目标：用真实 Tauri 样例在干净 Windows 环境完成安装 → 启动 → 卸载 → 残留检查，并归档证据。非目标：不做 MSIX / Store 打包、不做多语言安装器矩阵。
- 方案概览：新增样例工程（建议 `tests/fixtures/windows-msi-app` 或 `examples/`），Windows runner 构建 MSI；`msiexec /i /qn` 安装，启动进程并检查窗口 / 退出码，`msiexec /x /qn` 卸载，检查安装目录与注册表残留；日志与截图归档。集成点：`crates/poly-bundle/src/packagers/windows.rs`、`.github/workflows/rust-ci.yml`。备选：只做脚本结构检查（现状）、手工测试。
- 交付物：真实样例工程；Windows CI 作业；安装 / 启动 / 卸载 / 残留证据；已知限制文档。
- 风险与缓解：CI runner 无桌面 → 用 Windows runner 的交互式会话或 VM；WiX 版本漂移 → 固定工具链版本；残留误判 → 明确白名单与快照差异。
- 决策项：样例放 `tests/fixtures` 还是 `examples/`；是否把该作业设为必过检查。
- 首步动作：在 Windows runner 用最小 Tauri 应用跑一次 `candle` / `light`，记录产物与安装日志。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-53 | 真实 Tauri 样例与 MSI 构建 | coder | — | 3 | Windows runner 出 MSI；产物摘要入库；构建日志可追溯 |
| T-M9-54 | 安装 / 启动 / 卸载 / 残留冒烟 | tester | T-M9-53 | 3 | 三步全部成功；残留检查有结论；失败路径有稳定错误码 |

### M9-G5 · 分支保护管理员绕过（外部流程）

- 状态：已交付（2026-10-20）：仓库 owner 已启用 `enforce_admins=true` 与 9 个 required contexts，ENV-18 已 `CLOSED`；证据见 T-M2-12（决策背景见 [ADR 0007](../decisions/0007-external-and-ui-decisions.md)）。
- 触发条件：已触发并完成（2026-10-20）；原记录为 T-M2-12 的 `enforce_admins=false`。
- 背景与问题：required checks 已设置，管理员直推曾可绕过 secret-scan 等必过检查，属于流程缺口而非代码缺口；仓库内无法修复。
- 目标：启用管理员同样受规则约束（或 org-level ruleset），并留下可核验证据。非目标：不改 CI 代码、不改变检查项集合。
- 方案概览：GitHub 仓库设置启用 ruleset：要求 PR、required checks、禁止 force push、`enforce_admins=true`；已由仓库 owner 执行并归档 API 证据。备选：仅靠流程约定、org-level policy。
- 交付物：GitHub branch protection API 证据（`enforce_admins.enabled=true` + 9 个 required contexts，2026-10-20）；T-M2-12 / ENV-18 状态更新。
- 风险与缓解：锁死紧急修复 → 保留 break-glass 流程并审计；权限不足 → owner 执行；规则误配 → 先在测试仓库演练。
- 决策项：是否允许紧急 break-glass；由谁持有豁免权。
- 首步动作：已完成：仓库 owner 启用管理员同等约束，API 输出归档到 T-M2-12 / ENV-18。

| 任务 | 标题 | 角色 | 依赖 | 人日 | 验收 |
| --- | --- | --- | --- | --- | --- |
| T-M9-55 | 启用管理员同等约束并归档证据 | maintainer | — | 1 | `enforce_admins=true` 与 9 个 required contexts（2026-10-20 API 核实）；管理员受 required checks 约束；证据链入 T-M2-12 / ENV-18 |

## 6. 推荐顺序

按「先补 GA 可信度，再做高杠杆工程，最后做外部依赖项」排序；E1–E7 / G5 已交付或收口，剩余为外部依赖项与维护者决策。

1. M9-G5 分支保护：已交付（2026-10-20，ENV-18 `CLOSED`）。
2. M9-G1 CLI 分发：没有可安装的 `poly`，外部试用与 GA 都缺入口。
3. M9-G2 人类试用：用真实用户校准后续优先级，避免凭假设开工。
4. M9-G3 环境槽收口：先建立「哪些已验证」的可信边界。
5. M9-E3 `release --changed`：复用既有缓存，CI 收益最快。
6. M9-G4 MSI 真样本：补齐 L1 Windows 的安装证据。
7. M9-E1 SBOM：合规触发明确，且可复用缓存键。
8. M9-E5 商店元数据：降低多语言发布人工成本。
9. M9-E2 monorepo：有首个用户再做，先写 schema / ADR。
10. M9-E6 GUI 历史：依赖 `app/` 骨架，先做事件持久化。
11. M9-E4 OTA：先调研与清单设计，有用户再做端到端。
12. M9-E7 设备预览：成本高，先做 build vs buy。
13. M9-D1 插件市场 ADR：在任何索引代码之前定治理。
14. M9-D2 农场形态 ADR：只做决策，不承诺托管。
15. M9-P3 LINE：出现 LINE 用户时以部署模式支持。
16. M9-P4 Sailfish：有赞助 / 设备承诺再做。
17. M9-P5 Ubuntu Touch：有赞助 / 设备承诺再做。
18. M9-P2 Snapchat：季度复查，无契约则关闭。
19. M9-P1 Google Play Instant：立即关闭。
20. M9-P6 PlayStation：NDA 门控，无授权不动。
21. M9-P7 Xbox：NDA 门控，无授权不动。
22. M9-P8 Nintendo Switch：NDA 门控，无授权不动。

## 7. 暂缓清单（Not now）

> 以下曾暂缓项已推进：E2、E3、E5、E6 已交付；E7 协议 / 干跑已交付（T-M9-21 真机外部）；E4 清单 / 签名已交付，T-M9-12 演练装置与 CI 已交付并通过 windows-2022 首跑（run 39395255929）。

- **M9-D1 官方注册表**：插件数量不足；去中心化索引也只在有需求时做。
- **M9-D2 托管农场**：与「不托管用户代码与密钥」原则冲突，需单独商业与合规评审。
- **M9-P1 Google Play Instant**：平台已停服，投入无回报。
- **M9-P2 Snapchat Mini**：官方契约不可核验，任何实现都是猜测。
- **M9-P4 / M9-P5 Sailfish / Ubuntu Touch**：生态小、工具链与设备成本高，需赞助触发。
- **M9-P6 / M9-P7 / M9-P8 主机平台**：NDA 与 devkit 门槛高；无授权不写代码。

## 8. 后续集成点（不修改本仓现有文件）

- `docs/ROADMAP.md` / `ROADMAP.en.md` §13：E1–E7 行增加「交付 / 外部项状态见 INITIATIVES §2」链接；保留原提升条件。
- `docs/ROADMAP.md` / `ROADMAP.en.md` §14：开放问题增加 M9-D1 / M9-D2 的 ADR 链接与推荐结论（不代替决策）。
- `docs/GA.md` / `GA.en.md` §7.1 / §7.2：把「13 个规划平台」与 README / `tools/gen-status-table.mjs` 的 8 个对账，逐平台行链接 M9-P1–M9-P8；人类试用链接 M9-G2，keyring / Vault、S3 / OSS、macOS IPA、Garmin iq 链接 M9-G3，MSI 链接 M9-G4。
- `README.md` / `README.en.md`：由 `tools/gen-status-table.mjs` 的 8 个 planned 行在 notes 增加本文链接后重新生成 49 目标现状表；平台矩阵计数不变。
- `docs/TASKS.md` / `TASKS.en.md`：在 T-M8-13 之后新增 M9 候选章节，收录 T-M9-01–T-M9-55；全部标「候选 / 未排期」，合计 226 人日。
- `tools/check-docs.mjs` 的 `PAIRS`：新增 `['docs/INITIATIVES.md', 'docs/INITIATIVES.en.md']`；M9-P1 已按 retired 落地（`PLATFORMS` 行保留、`TOTAL` / `TIER_COUNTS` 不变），P2 已标注关闭与复审日期。
- `docs/decisions/`：M9-D1 产出 ADR 0004，M9-D2 产出 ADR 0005，M9-G1 的发布策略可追加 ADR；`docs/ENV-SLOTS.md` 本文写作时不存在，若另一工作流落地，M9-G3 以其为单一事实源并互链，本文不创建该文件。
