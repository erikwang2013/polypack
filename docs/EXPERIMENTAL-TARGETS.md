# PolyPack 实验目标：能力 / 缺口 / 替代方案

> 适用目标：`watchos` / `garmin`（`experimental = true`）。Wear OS 为平台级目标（`experimental = false`），边界见 §4。
> 单一事实源：`crates/poly-targets/src/builtin.rs` 的 `TargetSpec.experimental`。生成的文档状态表（README ×2 / ROADMAP ×2）与 CI 强度由 `node tools/check-experimental.mjs` 校验三处一致；本文件与英文版的结构（标题数 / 表格行数）由 `node tools/check-docs.mjs` 对账。
> 决策：两个实验目标均**继续（受限档）**——watchOS 以 SwiftUI 伴生模板提供打包 / 签名链；Garmin 以构建期 Monkey C 子集发射器提供受限出包面。触发前置（编译器 / 真机闭环）见各节。

## 1. 判定口径与决策

| 目标 | experimental | 决策 | 触发前置 |
|---|---|---|---|
| `watchos` | `true` | 继续（受限档）：伴生模板 + Apple 打包 / 签名链 | `xcodebuild` 构建 + 设备走查（macOS 宿主） |
| `garmin` | `true` | 继续（受限档）：Monkey C 子集转译 spike | Connect IQ SDK 编译 + 真机 / 模拟器走查 |

## 2. watchOS（伴生模板）

### 2.1 能力

| 能力 | 说明 | 证据 |
|---|---|---|
| 模板生成 | `poly init --target watchos` 生成最小可构建 SwiftUI 手表 App：`@main` 入口、ContentView、CoreBridge 桩、WatchConnectivity 骨架、Info.plist（WKApplication）、Xcode 工程 + 共享 scheme | `crates/poly-cli/templates/watchos/`；TargetSpec `watchos` |
| 打包 | `watchos-xcodebuild`：`xcodebuild archive` → `ditto` → `export/Payload/PolyPackWatch.app`；`plan()` 为纯组装，任意宿主可跑 | `crates/poly-bundle/src/packagers/watchos.rs` |
| 签名 / 校验 | 复用 M2 Apple 链（`apple-codesign`，codesign 硬化运行时 + 时间戳）；`codesign --verify --deep --strict` + `spctl --assess`；凭据只以 CredentialRef 出现 | TargetSpec verify；模板 README |
| 伴生通信 | WatchConnectivity 三通道骨架（`sendMessage` / `transferUserInfo` / `updateApplicationContext`）；核心调用经 iOS 伴生 App 代理 | `WatchConnectivity.swift`；`CoreBridge.swift` |
| 跨宿主发起 | Linux / Windows 上 package 阶段如实 `ToolMissing`（`poly::E_BND_0001`），走 `poly farm submit --target watchos`（R3 macOS worker） | `packagers/watchos.rs`；模板 README |

### 2.2 缺口与替代方案

| # | 缺口 | 影响 | 替代方案（或「无替代」） |
|---|---|---|---|
| W1 | 手表端 `runtime = none`，核心不在手表上运行 | 不能直接调用 Rust 核心；手表 UI 逻辑无法复用 RN | 经 iOS 伴生 App + WatchConnectivity 代理；「手表直跑核心」无替代 |
| W2 | 模板的 CoreBridge 为桩（返回占位状态），真实核心调用未接线 | 开箱端到端调用不完整 | 按 `WatchConnectivity.swift` 三通道说明接入 iOS 伴生 App；自动生成真实调用无替代（需应用提供 iOS 核心） |
| W3 | 构建 / 签名工具链仅随 Xcode（macOS） | Linux / Windows 无法本地出包 | 远程农场 macOS worker（R3）；本地非 macOS 出包无替代 |
| W4 | 模板为 watch-only，不含 iOS 伴生 App 工程 | 配对需手工接入 iOS 工程 | 移除 `WKWatchOnly` 并补 `WKCompanionAppBundleIdentifier`（模板 README）；iOS 工程由 M2 链生成后手工配对；自动生成伴生工程无替代 |
| W5 | 本仓 CI（Linux）无 `xcodebuild` / `codesign` 实证 | 打包 / 签名 / 设备行为未在本仓闭环 | macOS 宿主或远程农场执行；本仓内闭环无替代（需 macOS） |
| W6 | 模板 UI 为 SwiftUI；watchOS 无 React 原生运行时 | React 页面不能原样运行 | 原生 SwiftUI 子集 + 核心 API 代理；「React on watchOS」无替代 |
| W7 | 证书 / 描述文件需系统钥匙库 / Vault / CI 注入 | 无凭据无法签名 | CredentialRef 引用（`POLYPACK_IOS_IDENTITY` / `POLYPACK_IOS_TEAM_ID` 为非机密配置）；明文凭据无替代（禁止入库） |

### 2.3 结论

继续（受限档）。`experimental = true` 保持不变；能力边界随模板 README 与本节交付。触发前置：在 macOS 宿主（或远程农场）完成 `xcodebuild` 构建与一次设备走查；未闭环前不宣称「设备已验证」。

## 3. Garmin（Monkey C 子集转译）

### 3.1 能力

| 能力 | 说明 | 证据 |
|---|---|---|
| 构建期转译 | wear 页模型（tokens / 布局盒 / 焦点序）→ Monkey C 源文件；`runtime = none`，无 JS 运行时 | `ui/adapters/garmin/src/page.js`、`emit.js`；能力边界草稿 §1 |
| UI 子集 | View / 卡片 / 文本 / Pressable 复合（底 + 焦点环 + 文字）；颜色 / 间距 / 安全区与 adapter-wear tokens 同源；圆 454×454、方 320×320 参考视口 | 草稿 §2 表 A/C |
| 交互子集 | 三键 UP / DOWN / SELECT / BACK；焦点序 = 共享焦点内核 `engine.order` 线性循环；Back 历史同形 | 草稿 §2 表 B |
| Connect IQ 工程 | `manifest.xml` + `monkey.jungle` + `resources/strings/strings.xml` + `source/*.mc`；参考设备 `venu2` / `venusq`；应用 id = `sha256(identifier)` 前 32 hex | 草稿 §2 表 D |
| 校验 | 29/29 Node 测试 + Rust 内建 `.iq` 结构自检（包根 `manifest.xml` + ≥1 个按设备 `.prg`）；`poly verify` 的 `iq-structure-check` 已接线；`npm run build` 真跑结构自检 | 草稿 §5；`ui/adapters/garmin/test/`；`crates/poly-bundle/tests/packagers_garmin.rs` |
| 编译器腿（环境就绪时） | Rust 打包器 `crates/poly-bundle/src/packagers/garmin.rs` 解析 `<project>/garmin`（或宿主根）并调用 `monkeyc -e -w -r -o out.iq -y <devkey> -f monkey.jungle` → `.iq`（`manifest.xml` + 按设备 `.prg`）；devkey 仅取路径（`CIQ_DEVKEY`） | 草稿 §2 表 D、§5；`crates/poly-bundle/src/packagers/garmin.rs` |

### 3.2 缺口与替代方案

| # | 缺口 | 影响 | 替代方案（或「无替代」） |
|---|---|---|---|
| G1 | 无 JS 运行时：React 组件函数体不可转换 | 页面逻辑不能复用；`RendererAdapter` 契约不适用 | 构建期子集发射器 + Monkey C 重写；「原样运行 JS」无替代 |
| G2 | Monkey C 子集限制（无异常 / 枚举 / switch / for / while / module / 可见性修饰符） | 复杂逻辑需重构；生成器词汇封闭 | `src/subset.js` 白名单失败即拒；示例仅 if/else + 赋值 |
| G3 | 本机无 Connect IQ SDK / `monkeyc` / 开发者密钥 | `.iq` 真实编译与设备行为无法本机闭环 | 结构 / 子集 / 确定性真跑；编译器腿 SKIP-with-print（`CIQ_MONKEYC` / `CIQ_HOME` / `CIQ_DEVKEY` 就绪后打印手动命令）；「编译器实证」无替代（需 SDK + 密钥） |
| G4 | 无 flex / 文本测量，布局构建期固定 | 动态版式与文本自适应不可行 | 复用 wear `layoutPage` 构建期布局 + 运行时线性缩放；长文本固定框（溢出待真机观察） |
| G5 | `Dc` 无圆角原语；字体为内置位图字体 | 视觉与 wear 非逐像素一致 | 方角替代圆角；`FONT_SMALL` / `FONT_XTINY` 近似 16/14 px |
| G6 | 指针 / 手势缺失；三键设备无左右；2×2 网格右列不可达 | 交互语义与四向遍历不同 | `focusNav=true` + `engine.order` 线性循环（四按钮全可达）；`traversal` 快照留给未来四向设备；不新写导航 |
| G7 | 资产 / 图标 / 启动图缺位 | manifest 省略图标字段（开发侧载形态） | 资产管线后续接入；本 spike 只发射 `AppName` 字符串资源 |
| G8 | 每形态仅声明参考设备（`venu2` / `venusq`） | 未做多设备产品矩阵 | `iq:products` 按需扩展；缩放公式按 `dc.getWidth()/getHeight()` 通用 |
| G9 | 生命周期最小：无 `onStart` / `onStop` 持久化 | 退出 / 重启后状态归零 | 示例页为瞬时状态；`Application.Storage` 不在白名单，列为后续扩展 |
| G10 | 无真实 Connect IQ SDK / 设备证据：内建打包器只做结构自检 | `.iq` 真实编译与设备行为仍需环境槽；结构可由 Rust 校验器真跑 | 内建 `iq` 打包器 + `check_garmin_iq_structure` 已落地；设 `CIQ_MONKEYC` / `CIQ_HOME` / `CONNECTIQ_HOME` + `CIQ_DEVKEY` 后真跑编译；「编译器 / 真机实证」无替代（需 SDK + 密钥 + 设备） |

### 3.3 结论

继续（受限档）。`experimental = true` 保持不变。触发前置：Connect IQ SDK 编译一次 + 真机 / 模拟器走查（本机无 `monkeyc` / SDK / devkey，编译器腿 SKIP-with-print）。降级备选（若不愿承担）：保留 L6 TargetSpec + `experimental` + 能力边界 + 内建打包器 / 结构自检，但不承诺编译器 / 真机实证。

## 4. 非实验目标：Wear OS 的边界

- `wearos` 的 `TargetSpec.experimental = false`；生成的四份文档表均显示 `false`，文档中**不得**出现其 ※实验 标注（`check-experimental.mjs` 三处一致校验）。
- 平台级支持：`runtime = native`、`bridge = uniffi`、APK（`android-aapt2`）、`android-keystore` 签名、`apksigner-verify` + `bundletool-validate`；Wear 清单 `android.hardware.type.watch`，minSdkVersion ≥ 26。
- 边界说明：**UI 运行时（RN on Wear）为 beta**——beta 只影响 UI 运行时置信度，不改变打包与签名链；打包与签名走 Android 基座（复用 M2，不新增 Android 打包实现）。
- 发布与验收：`google-play-wear` 渠道；有设备冒烟（安装 → 启动 → 表单 → 卸载）为发版门（T-M7-04）；契约测试与金样见 T-M7-11（并行）。

## 5. 维护约定

| 变更 | 必做 |
|---|---|
| 修改 experimental / 目标能力 | 先改 TargetSpec（单一事实源）→ `node tools/gen-target-table.mjs` → 同步本文件中英双语 |
| 缺口 / 替代方案变化 | 同提交更新本文件与 README L6 说明；每个缺口必须保留「替代方案」或明示「无替代」 |
| CI 校验 | `node tools/gen-target-table.mjs --check`；`node tools/check-docs.mjs`；`node tools/check-experimental.mjs` |

素材：`ui/adapters/garmin/CAPABILITY-BOUNDARY-DRAFT.md`（T-M7-09）、`crates/poly-bundle/src/packagers/garmin.rs`、`crates/poly-cli/templates/watchos/README.md`、`crates/poly-bundle/src/packagers/watchos.rs`、TargetSpec notes。
