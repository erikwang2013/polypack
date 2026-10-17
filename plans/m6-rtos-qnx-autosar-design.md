# M6 RTOS 设计草稿：T-M6-03（QNX）/ T-M6-04（AUTOSAR Adaptive）

> 状态：WIP 侦察稿（worktree `worktree-agent-a59b21bd09e9ae017`，base main@5a0786f）。
> 依赖 T-M6-02（`poly-core` 引擎层 C ABI）。放行前只定契约，不放实现。
> 侦察日期：2026-10-17。

## 1. 侦察结论（本机实测）

| 探测面 | 结果 | 设计含义 |
|---|---|---|
| Rust QNX target | `rustc --print target-list` 含 `aarch64-unknown-nto-qnx710` / `x86_64-pc-nto-qnx710` / `aarch64-unknown-nto-qnx700`（均未安装） | 交叉 triple 从 rustc 清单取，默认 `aarch64-unknown-nto-qnx710`；未装时给 `rustup target add` 提示 |
| QNX SDP | `QNX_HOST` / `QNX_TARGET` 未设；`/opt` 无 qnx；`qcc` / `mkifs` 不在 PATH | 本机走 SKIP-with-print；结构/单元测试真跑 |
| xmllint | `/usr/bin/xmllint` 在场 | ARXML schema 校验本机真跑；缺席时结构断言 + SKIP 注记 |
| 既有依赖方向 | `poly-bundle` 允许依赖 `poly-core` / `poly-targets`，不得依赖 `poly-toolchain` | SDP 探测只在 `poly-toolchain` 一处；路径由 poly-cli 经 `BundleInputs` 递入打包器 |
| 配置覆盖 | `[targets.<id>].format` 可覆盖注册表 format（poly-core config.rs） | `format` 取 `image` 或 `library` 两值 + 配置覆盖表达，无需改 PackageSpec |
| 打包器契约 | `Packager::package` 返回单个 `Artifact`；`plan()` 纯函数 | 一次打包可落多件产物，主产物按声明的 format 选，副产物写 `notes` |

## 2. TargetSpec（新增两条，tier 6）

| 字段 | qnx | autosar |
|---|---|---|
| id | `qnx` | `autosar` |
| tier | 6 | 6 |
| hosts | `win / linux / macos`（SDP 三宿主均有） | `win / linux / macos`（POSIX 静态库，无交叉 SDK） |
| runtime | `staticlib` | `staticlib` |
| bridge | `cffi` | `cffi` |
| ui | `[]`（headless；可选 HMI 归平台侧，notes 注明） | `[]` |
| package.format | `image`（配置可覆盖为 `library`） | `arxml` |
| package.builder | `qnx-sdp` | `autosar-adaptive` |
| signing | `None` | `None` |
| verify | `["qnx-image-structure", "c-abi-symbols"]` | `["arxml-schema", "c-abi-symbols"]` |
| publish | `["self-hosted"]` | `["self-hosted"]` |
| experimental | `false` | `false` |
| notes | 镜像=部署 tar + SDP 在场时 mkifs `.ifs`；无 SDP 时交叉编译 SKIP-with-print，不伪造二进制；可选 HMI 归平台侧 | 「无 UI、自部署」逐字出现；no_std 可选（引擎层 `--no-default-features`）；ARXML 清单由 C 头文件派生 |

计数影响：内建 27 → 29；`allow_unreachable` 28 → 30；Linux 宿主可达 23 → 25
（`poly-targets/tests/builtin.rs`、`tests/registry.rs` 同步）。

## 3. QNX

### 3.1 SDP 探测（`poly-toolchain/src/probes/qnx_sdp.rs`）

- id `qnx-sdp`，`required_for = ["qnx"]`；纳入 `default_probes()`（7 → 8）。
- 候选（只读）：
  1. `POLYPACK_QNX_SDP` / `QNX_SDP_ROOT` / `QNX_SDP` 指向 SDP 根、`QNX_HOST` 或 `QNX_TARGET`；
  2. 标准 `qnxsdp-env.sh` 导出的 `QNX_HOST` + `QNX_TARGET` 对。
- 命中判据：`QNX_HOST/usr/bin` 下存在 `qcc` 或 `nto*-gcc`，且 `QNX_TARGET` 目录存在
  （与 HarmonyOS 探测「只有 IDE 外壳不算命中」同口径）。
- 版本：`QNX_VERSION` 优先；否则由 `QNX_TARGET` 目录名（`qnx7` / `qnx710`）派生。
- 缺失 note：安装 QNX SDP 7.1 后 `source qnxsdp-env.sh`（导出 QNX_HOST/QNX_TARGET），
  或设 `POLYPACK_QNX_SDP` 指向 SDP 根；`poly doctor` 原样渲染该 note（doctor 无需改代码，
  目标矩阵自动出现 qnx 行）。

### 3.2 产物形态（`poly-bundle/src/packagers/qnx.rs`）

- **库**：`cargo build --release --lib --target <triple>`，`CARGO_TARGET_DIR=<out>/target`，
  env 注入 `QNX_HOST` / `QNX_TARGET`；产物 `lib<core>.a` + T-M6-02 生成的 C 头文件副本。
  core crate 的 `[lib] crate-type` 必含 `staticlib`（T-M6-02 契约，缺则明确报错，同 napi 口径）。
- **镜像**：生成 `qnx/<kebab>.build`（mkifs buildfile，含引擎自检程序与静态库）与
  `qnx/manifest.json`（target / triple / library / header / buildfile 摘要）；SDP 在场时
  `qcc` 编译自检程序 + `mkifs` 出 `.ifs`，再打确定性部署 tar `<kebab>-image.tar.gz`
  （库 + 头文件 + buildfile + manifest）。`format = "image"` 主产物 = tar（SDP 缺席时不存在）；
  `format = "library"` 主产物 = `.a`。两 format 都产出两类文件，副产物在 `Artifact.notes` 点名。
- **SKIP 口径**：SDP 缺失 / rustup 未装 triple / 头文件缺失 → `BundleError::ToolMissing`
  （或 MissingInput）→ CLI `environment_slot` SKIP-with-print；暂存目录清空，**不产任何
  伪造 `.a` / `.ifs` / tar**。`plan()` 与 `check_qnx_structure()` 不依赖 SDP，单元测试真跑。
- 结构检查 `check_qnx_structure(dir)`：buildfile 必含 `[image=`、`[virtual=`、静态库引用、
  启动脚本；manifest 三键齐全；作为 verify 腿 `qnx-image-structure` 与打包器自检同一实现。
- 交叉 triple：`POLYPACK_QNX_TRIPLE` 覆盖；缺省 `aarch64-unknown-nto-qnx710`；`QNX_CPUVARDIR`
  （aarch64le / x86_64）可派生。
- 自检程序：从 C 头文件里选一个无参 `poly_engine_*` 函数生成最小 `engine_selfcheck.c`
  （只调用，不重写 ABI）；T-M6-09 的 QEMU 冒烟复用同一 buildfile / 自检程序。

## 4. AUTOSAR Adaptive

### 4.1 静态库与 ARXML（`poly-bundle/src/packagers/autosar.rs`）

- 库：同 QNX，但用宿主 triple（POSIX）；no_std 可选：`POLYPACK_ENGINE_NO_STD=1` 时
  cargo 加 `--no-default-features`（notes 如实标注）。
- ARXML：`autosar/<kebab>.arxml`，命名空间 `http://autosar.org/schema/r4.0`，内容：
  - 每个头文件声明的 `poly_engine_*` 函数 → 一个 `SERVICE-INTERFACE`（一个 `METHOD`，
    `SHORT-NAME` = 符号名）+ 一个 `PROVIDED-SERVICE-INSTANCE`（端口名 `<符号>Port`）；
  - **一一对应**：符号集合 == METHOD 集合 == 端口集合（生成器与检查器同源）。
- schema：仓库内嵌最小 XSD（只覆盖本清单子集，`include_str!`），随产物落盘；
  `xmllint --noout --schema <xsd> <arxml>` 在场真跑；缺席时结构断言（标签/命名空间/同名
  一一对应）+ SKIP 注记，绝不假绿。
- 头文件是唯一 ABI 源：`POLYPACK_ENGINE_HEADER` → `<core>/include/poly_engine.h` →
  `<root>/bridge/cffi/poly_engine.h` → `<core>/poly_engine.h` 顺序发现；**不手写第二份**。
  头文件缺失 = MissingInput → SKIP-with-print。

### 4.2 校验（`poly-cli/src/verify.rs` 只加分发臂，逻辑在新模块）

- `arxml-schema`：结构断言 + xmllint（见上）。
- `c-abi-symbols`（QNX / AUTOSAR 共用）：
  1. 解析头文件声明集合；
  2. 解析 ARXML METHOD / 端口集合（AUTOSAR）；
  3. 静态库在场且 `nm` 在场：`nm -g --defined-only lib.a`，过滤 `poly_engine_*`，
     **集合相等**才算过；库或 nm 缺席 → SKIP-with-print（结构两级比对仍真跑）。
- 无第二套实现：打包器、verify、测试共用 `poly-bundle` 里的
  `embedded::{parse_c_abi, check_symbols, check_qnx_structure, check_arxml}`。

## 5. 接入点与共享文件（最终实现会碰到的）

| 文件 | 改动 |
|---|---|
| `crates/poly-targets/src/builtin.rs` | +2 TargetSpec |
| `crates/poly-targets/tests/{builtin,registry}.rs` | 计数 27→29 / 28→30 / 23→25 + 新目标断言 |
| `crates/poly-toolchain/src/probes/{mod.rs,qnx_sdp.rs}`、`probe.rs` | 新探测器 + default_probes 8 |
| `crates/poly-toolchain/tests/probes.rs` | 7→8、required_for 新行 |
| `crates/poly-bundle/src/inputs.rs` | `EmbeddedInputs { core_crate, header, qnx_sdp, triple }` |
| `crates/poly-bundle/src/packagers/{mod.rs,qnx.rs,autosar.rs,embedded.rs}` | 新打包器 + 共享检查 |
| `crates/poly-bundle/src/{lib.rs,packager.rs}` | 导出 + builtin() 两实例 |
| `crates/poly-cli/src/package.rs` | format 臂 `image` / `library` / `arxml` 填 EmbeddedInputs |
| `crates/poly-cli/src/verify.rs` | verifier_for 两 id + 分发臂（逻辑在新 `verify_embedded.rs`） |
| `crates/poly-cli/tests/contract_m6_rtos.rs`、`crates/poly-bundle/tests/packagers_embedded.rs` | 新测试 |

Cargo 预期：**零改动**（不需要 XML/归档新依赖；头文件解析与 tar 用 std / 现有依赖）。
若 T-M6-02 头文件生成入口需要 poly-cli 调用，也只在 poly-cli 内调 `poly-core`，不改依赖图。

## 6. 待 T-M6-02 放行后确认的接口点

1. 生成头文件的落点与生成入口（函数名 / `poly bridge` 子命令）——当前按候选路径发现；
2. 引擎 crate 的 `[lib] crate-type` 是否已含 `staticlib`；
3. `poly_engine_*` 符号清单与自检可用的无参函数；
4. no_std feature 名（缺省假设 `--no-default-features`）。

## 7. 验证计划（放行后）

- 聚焦：`cargo -j 1 test -p poly-targets -p poly-toolchain -p poly-bundle -p poly-cli`（本机低并发）。
- QNX 真跑：`poly build --target qnx` → 本机无 SDP：package 阶段 SKIP-with-print；
  `poly doctor` 显示 `✗ qnx-sdp：…安装提示`；结构测试与 buildfile 断言真跑。
- AUTOSAR 真跑：`poly build --target autosar` → 生成 ARXML + `.a`（T-M6-02 合入后），
  `xmllint --schema` 本机真跑；`nm` 符号集合与 ARXML 端口一一对应。
- 全量交 CI；提交只落本 worktree，不 push。
