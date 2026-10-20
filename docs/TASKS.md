# PolyPack 任务计划

> 派生自 [ROADMAP.md](ROADMAP.md) §9 的里程碑分解；由 4 人规划小组并行产出、交叉对账后合并：**116 个任务**，跨里程碑引用经「存在性 + 描述匹配」两轮校验零悬空，中英任务 ID 逐一对齐。
> 英文版：[TASKS.en.md](TASKS.en.md)。本文件受「文档同步约定」管辖（见 ROADMAP §11）：任务增删须双文件同一次提交更新，并由 T-M0-10 的结构级校验器在 CI 拦截漂移。
> 扩展方向（E1–E7）与规划平台 / 缺口项目的详细立项、方案与 55 个候选任务见 [INITIATIVES.md](INITIATIVES.md)；候选任务不占本任务表的 M0–M8 排期，也不计入 116 个已交付任务。
> 进度（2026-10-19）：**M0 11/11** · **M1 已交付 15/15**（15 项 ✅）· **M2 已交付 15/15**（15 项 ✅；T-M2-01/03/04/05/10/12/13 带宿主/阶段/设置注记，逐项见任务条目）· **CI 三宿主全绿**（rust-ci#38654820649）+ M2 新增四工作流（secret-scan 三宿主 / mobile-contract mock+夜跑真列 / bridge 夜跑真跑）· **M3 已交付 12/12**（7 项 ✅：T-M3-05/06/07/08/09/10/11；T-M3-01/02/03/04/12 带宿主/环境/联调注记，逐项见任务条目）+ M3 新增一工作流（miniprogram-contract mock+夜跑真列）· **M4 已交付 15/15**（T-M4-15 条件任务已交付：Roku 频道包打包器与契约测试落库（结构自检 / zip / 金样 / 确定性覆盖），真机侧载按环境槽 SKIP-with-print；T-M4-01/06/11/13 等带设备/宿主/联调注记，逐项见任务条目）+ M4 新增一工作流（tv-smoke mock+夜跑真列）· **M5 已交付 11/11**（T-M5-07 的 OpenBSD 安装冒烟与 T-M5-08 的 NetBSD `bmake` 真跑按环境槽 SKIP-with-print（镜像带宽 / 宿主缺席），由 bsd-ci 每夜裁决；T-M5-11 已补按 target 分凭据（重复 `--credential TARGET=REF`；裸 `REF` 仍为全局缺省；未知/重复映射报 `E_PLAN_0002`），多目标 dry-run 双事件验证）+ M5 新增一工作流（bsd-ci 夜跑 BSD 原生构建与安装冒烟）· **M6 已交付 13/13**（no_std 引擎层 + C ABI、QNX/AUTOSAR/VxWorks/Zephyr/FreeRTOS 五目标、AAOS、CI 矩阵与 QEMU 冒烟、状态表与预算/标注校验；QNX SDP / wr-cc / Xcode 类环境槽按 SKIP-with-print，模型与串口证据见各任务条目）+ M6 新增一工作流（embedded-ci：tier-6 矩阵从 TargetSpec 派生 + QEMU 冒烟，夜跑 19:27）· **M7 已交付 11/11**（adapter-wear + Wear OS 双冒烟 + watchOS 模板/打包链 + Fitbit/Zepp + Garmin spike + 实验目标三段式文档 + 穿戴契约；xcodebuild / codesign / Wear 设备类环境槽按 SKIP-with-print，garmin 的内建 iq 打包器已接线（monkeyc 环境槽 + 结构自检））+ M7 新增两工作流（wear-smoke 夜跑表单冒烟 / wear-release-smoke 发版门）。 · **M8 已交付 13/13**（插件沙箱 + WIT world、SRI/签名链、模板/脚手架、内建统一 provider、农场 v1、发布 GA、49 目标表、双语 GA 文档/技术审查/1 天演练（38min）；M8 未新增仓库工作流（模板 CI 位于 templates/），环境槽注记见各任务条目）。**2026-10-19 缺口收口**：M5-11 按目标凭据、Garmin 内建 iq 打包器、Vault 信任根载体、keyring/zbus 升级、GitHub Releases 二进制与文档一致性已落地；环境槽关闭台账见 [ENV-SLOTS](ENV-SLOTS.md)，未立项方向见 [INITIATIVES](INITIATIVES.md)。

## 团队编制与角色

| 角色 | 职责 |
|---|---|
| `core` | 核心与流水线：poly-core / poly-targets / poly-toolchain / poly-cache |
| `packagers` | 打包器矩阵：poly-bundle 各格式（apk / ipa / hap / msi / dmg / wgt / ipk / deb / rpm / …） |
| `ui-adapters` | UI 适配层：设计系统、平台渲染适配器、电视焦点系统 |
| `platforms` | 平台移植：SDK / 工具链对接、交叉编译、原生构建 |
| `release-sec` | 签名发布与安全：poly-sign / poly-publish / 密钥库 / 插件签名 |
| `infra` | CI 与基础设施：宿主矩阵、冒烟、远程构建农场 |
| `docs` | 文档与图示：双语同步、图示流水线、用法文档 |

## 总量、关键路径与排期备注

| 里程碑 | 任务数 | 人日 | 备注 |
|---|---|---|---|
| M0 地基 | 11 | 18 | 含文档同步校验器（已交付）与供应链检查 |
| M1 桌面与 Web | 15 | 31.5 | 无缓冲（31.5d / 30d 容量，105%）；产物在 M2 前未签名 |
| M2 移动 | 15 | 38 | 满载 101%；含远程农场与「发起地/构建地」文档 |
| M3 鸿蒙与小程序 | 12 | 31.5 | 满载 105%；含快应用打包与商店校验 |
| M4 TV 家族 | 15 | 36 | 含条件任务 T-M4-15（Roku 频道包打包器与契约测试已交付；真机侧载为环境槽 SKIP-with-print） |
| M5 发行版与 BSD | 11 | 25 | BSD 构建宿主方案在 T-M5-09 定型并回写 §8 |
| M6 嵌入式与车机 | 13 | 36 | 含 no_std feature 拆分（不新增 crate） |
| M7 穿戴 | 11 | 29 | Wear OS 为平台级支持（UI 运行时 beta）；watchOS / Garmin 为实验目标 |
| M8 生态 GA | 13 | 38 | 低于 45d 目标（受任务粒度上限约束），仍在 ±20% 内 |
| **合计** | **116** | **283** | 不含条件任务为 280d；@1.5 FTE ≈ 37.5 周，与 ROADMAP 排期一致 |

**全局最长依赖链（16.5d）**：T-M2-01 → T-M2-02 → T-M2-06 → T-M2-07 → T-M2-14 → T-M3-11（桥生成器 → Android 打包链 → 移动契约金样 → 小程序契约测试）。

**排期备注（规划小组对账产出，如实记录）**：

- M0–M3 均顶格或超容量（18/15、31.5/30、38/37.5、31.5/30 人日）——接受不压缩；关键路径 16.5d / 25 工作日，并行度充足，但任一任务滑期将直接挤压验收项（M2 的「农场出 IPA」、M3 的「三端预览」）。
- M1 产物在 M2 前未签名：macOS 演示会出现 Gatekeeper / SmartScreen 警告（符合 M2 才引入签名链的排期）。
- M4 在 Roku 条件任务触发时顶到 +20% 估算上限；T-M4-15 是 M4/M5 中唯一可调度让位项。
- 跨里程碑引用完成两轮对账：存在性（113 任务、零悬空）+ 描述匹配（第二轮修正了 fragment-m4m5 的 5 处猜号引用）。

## 用法

- **认领**：按 `角色` 筛选任务；执行顺序按 `依赖` 栏做拓扑排序；`T-<M#>-<NN>` 可直接作为 issue key 导入（如 GitHub Issues）。
- **勾选**：完成后在任务标题追加 `✅`，并同步更新该里程碑结尾的统计注释。
- **变更**：任务增删、估算调整必须同步 [TASKS.en.md](TASKS.en.md)，遵守 [ROADMAP.md](ROADMAP.md) §11 文档同步约定。
## M0 地基（2 周）

> 前置依赖: 无｜出口条件: `poly doctor` 正确报告本机缺失工具链；schema 覆盖 L1 全部字段；CI 三宿主（ubuntu-latest / macos-14 / windows-2022）全绿；文档同步校验器（图示 / Pax / 双语结构校验）在 CI 中生效。

### T-M0-01 · ✅ Cargo workspace 骨架与依赖方向约束

- 角色: core
- 依赖: —
- 估算: 2d
- 交付物: 含 9 个 crate 空壳的 Cargo workspace（poly-cli / poly-core / poly-targets / poly-toolchain / poly-bundle / poly-sign / poly-publish / poly-cache / poly-plugin）+ 依赖方向约束的检查脚本。
- 验收: `cargo check --workspace` 全绿；人为引入反向依赖（如 poly-core 依赖 poly-bundle）时检查失败并指出违规方向。

### T-M0-02 · ✅ 三宿主 CI 与 Rust 构建缓存（三宿主 rust-ci 全绿：ubuntu / macos / windows 近期运行均通过（2026-10-18）；「远端账单恢复」注记已过时）

- 角色: infra
- 依赖: T-M0-01
- 估算: 2d
- 交付物: ubuntu-latest / macos-14 / windows-2022 三宿主 CI 工作流（fmt + clippy + test），带 Rust 构建缓存（cargo registry 与 target 目录）。
- 验收: 三宿主全绿；二次运行恢复缓存命中（构建时延较冷启动明显下降）。

### T-M0-03 · ✅ polypack.toml schema（serde + JSON Schema 导出）

- 角色: core
- 依赖: T-M0-01
- 估算: 1.5d
- 交付物: `polypack.toml` 的 serde 模型（[package] / [ui] / [core] / [targets.*] / [publish]）、解析校验与 JSON Schema 导出。
- 验收: 覆盖 README 示例配置的全部键；非法配置（未知键、类型错误）报错带稳定错误码；导出的 JSON Schema 可校验仓库内示例配置；序列化往返单测通过。

### T-M0-04 · ✅ TargetSpec DSL 与目标注册表

- 角色: core
- 依赖: T-M0-03
- 估算: 1.5d
- 交付物: TargetSpec 结构（§4.3 全字段：id / tier / hosts / runtime / bridge / ui / package / signing / verify / publish / experimental / notes）与 TargetRegistry::resolve 接口。
- 验收: 全字段解析与序列化往返一致；非法 tier（超出 1–7）与未知 runtime 枚举报错带错误码；注册表解析未知 id 时返回诊断而非 panic。

### T-M0-05 · ✅ L1 七目标 TargetSpec 声明

- 角色: platforms
- 依赖: T-M0-04
- 估算: 1.5d
- 交付物: Android / iOS / HarmonyOS / Windows / macOS / Linux / Web 七个 L1 目标的完整声明（运行时、桥、UI 适配、打包格式、签名引用、校验器、发布渠道、备注），逐列对齐 §3.2。
- 验收: 七目标均可被注册表解析；声明与 §3.2 明细表逐列一致（比对脚本或测试）；每目标至少 1 个校验器与 1 个发布渠道，凭据只以引用出现。

### T-M0-06 · ✅ JobSpec 与 plan 阶段（DAG 骨架）

- 角色: core
- 依赖: T-M0-04
- 估算: 1.5d
- 交付物: JobSpec 结构（§4.2：profile / targets / stages / cache_policy / secrets 引用 / 产物预算）与 plan 阶段（装载 polypack.toml → JobSpec），含 plan → probe → compile → bundle → package 阶段图骨架。
- 验收: 未知目标、循环依赖、缺凭据引用三类错误均在 plan 阶段快速失败（不进入编译），各带稳定错误码与修复提示。

### T-M0-07 · ✅ 诊断框架（错误码体系 + --json 事件流）

- 角色: core
- 依赖: T-M0-06
- 估算: 2d
- 交付物: `poly::E_<模块>_<编号>` 错误码体系（错误码 + 原因 + 修复提示）、§4.2 BuildEvent 事件总线与序列化、`poly build --json` 全量事件流输出。
- 验收: 任意失败路径输出稳定错误码与 hint；`--json` 输出可被 JSON 解析器逐行消费；事件变体定义覆盖 §4.2 全部枚举（M0 至少可触发 Planned / Failed）；错误码以快照测试锁定。

### T-M0-08 · ✅ poly doctor（探测报告骨架）

- 角色: core
- 依赖: T-M0-01
- 估算: 1.5d
- 交付物: `poly doctor` 命令、Probe trait（locate / install）与探测报告渲染（终端人读 + 机器可读）。
- 验收: 未注册探测器的环境项报告「未探测」而非崩溃；报告对每项工具链给出「就绪 / 缺失 / 版本」三态；退出码区分「全部就绪」与「存在缺失」。

### T-M0-09 · ✅ L1 宿主工具链探测器

- 角色: platforms
- 依赖: T-M0-08
- 估算: 2d
- 交付物: L1 相关工具链探测器实现：Android SDK/NDK、Xcode、HarmonyOS SDK、emsdk、WiX/NSIS、WebView2 运行时、Linux 打包工具链（按宿主适用性标注）。
- 验收: `poly doctor` 在三宿主上正确报告缺失/已装工具链及版本（对照人工核对清单）；单项探测失败不中断其余项；全部缺失场景输出可操作建议。

### T-M0-10 · ✅ 文档同步校验器接入 CI（已交付）

- 角色: docs
- 依赖: T-M0-02
- 估算: 1.5d
- 交付物: 文档同步校验器并接入 CI：1) 为 `tools/render-diagrams.mjs` 增加 `--check` 模式（重渲染并与磁盘 SVG 比对，漂移即非零退出，语义对齐 `tools/build-pax.mjs --check`）；2) 运行 `tools/build-pax.mjs --check`（Pax 资产无漂移）；3) 新增结构级双文档校验，对 README、ROADMAP 与任务分解文档（TASKS）的中英文档对分别校验 H2/H3 标题数一致、任务 ID 集合逐一相同、Markdown 表格行数一致（不做全文翻译比对）。
- 验收: 手改 `docs/assets/*.svg` 而不重渲染时 CI 失败并指出漂移文件；单侧增删标题 / 任务 / 表格行时双文档校验失败并指出不一致项；既有文档对（README、ROADMAP）基线通过；全部检查通过时 CI 绿。

### T-M0-11 · ✅ 供应链检查（cargo-deny / cargo-audit / 锁文件约束）
- 角色: release-sec
- 依赖: T-M0-01, T-M0-02
- 估算: 1d
- 交付物: `cargo-deny`（许可 / 漏洞 / 重复依赖）与 `cargo-audit` 接入 CI；`Cargo.lock` 提交强制；首次全量扫描基线与报告归档。
- 验收: 引入违规依赖时 CI 失败；基线报告归档于构建产物；`cargo tree` 无重复版本告警。

## M1 桌面与 Web（4 周）

> 前置依赖: M0（T-M0-03、T-M0-04、T-M0-05、T-M0-06、T-M0-07、T-M0-08、T-M0-09、T-M0-10）｜出口条件: 示例应用在 Windows / macOS / Linux / Web 四端出包；同一输入二跑缓存命中率 ≥ 90%；英文版图示合入并经 CI 校验（`poly dev` 热更仅为演示项——出口条件不依赖核心热更注入的高级形态）。

### T-M1-01 · ✅ 流水线骨架：Stage trait 与 DAG 调度

- 角色: core
- 依赖: T-M0-05、T-M0-06、T-M0-07
- 估算: 3d
- 交付物: Stage trait 实现框架（plan / probe / compile / bundle / package）与 DAG 调度（petgraph），compile 与 bundle(ui) 双支线并行、汇聚于 package。
- 验收: 以 mock 目标跑通「双并行支线 → 汇聚」的完整事件序列；单支线失败时其余支线状态汇总上报；事件与错误码符合 T-M0-07 约定。

### T-M1-02 · ✅ Tauri 管线：triple 探测与核心编译（Linux 真跑；跨宿主编译已由三宿主 rust-ci 全绿覆盖）

- 角色: core
- 依赖: T-M0-09、T-M1-01
- 估算: 3d
- 交付物: 宿主 → target triple 探测与核心 Rust crate 的 release 编译阶段（产物落盘、事件上报、失败带诊断）。
- 验收: 三宿主探测的 triple 与 rustc 实际目标一致；示例核心编译成功产出可检视产物；缺 linker/SDK 时在 probe/compile 阶段报错带错误码与修复提示。

### T-M1-03 · ✅ Tauri 捆绑阶段与裸产物（Linux 含真实 Tauri 二进制验收；Win/Mac 布局契约级）

- 角色: platforms
- 依赖: T-M1-02
- 估算: 2d
- 交付物: 捆绑阶段：前端 dist 注入 WebView 资源，按宿主调用 Tauri 捆绑，产出未分格式裸产物（Windows 可执行 + 资源、macOS .app、Linux 可执行 / AppDir 输入）。
- 验收: 三宿主示例应用产出可启动裸产物（启动输出应用版本后正常退出；无 GUI 会话时以 --version 冒烟替代）；WebView2 等宿主运行时依赖写入产物说明。

### T-M1-04 · ✅ Web 管线（vite 驱动 + PWA manifest 生成）

- 角色: ui-adapters
- 依赖: T-M1-01
- 估算: 2.5d
- 交付物: vite 前端构建集成（调起构建、产出 dist）、react-dom 渲染基线对接与 PWA manifest 生成（名称 / 图标 / 主题色来自 polypack.toml；图标基于 Pax 品牌资产）。
- 验收: 示例 UI 构建出 dist；manifest 结构与图标尺寸断言通过；html-validate 对产物 HTML 通过；构建失败带错误码与定位信息。

### T-M1-05 · ✅ poly-bundle：Packager trait 与 Windows 打包器（MSI / NSIS）（MSI 真机执行已通——windows-2022 CI 真跑 candle+light+msiexec 载荷字节核对（首跑 LGHT0311 码页修复后）；NSIS 真编译 + 静默安装 → 启动 → 卸载冒烟已入 rust-ci Windows 腿——makensis 出非空 PE 安装器，rustc 现编译的夹具 PE 经 `/S` 装到测试临时目录（`/D=`）并启动输出标记，`uninstall.exe /S` 后安装目录移除；真实样例 Tauri 二进制与默认 Program Files 路径的启动仍为缺口，非 Windows 宿主显式 skip）

- 角色: packagers
- 依赖: T-M1-03
- 估算: 2d
- 交付物: Packager trait（id / package(inputs, spec) → Artifact）与 WiX MSI、NSIS 两个 Windows 打包器。
- 验收: windows-2022 上由裸产物生成 MSI 与 NSIS 安装器；MSI 表结构与安装器产物结构断言通过；每夜冒烟「安装 → 启动 → 卸载」通过。

### T-M1-06 · ✅ macOS DMG 打包器（实现+契约完备；macos-14 rust-ci 真跑 hdiutil create/verify/imageinfo + 挂载核对；Linux/Windows 腿显式 skip）

- 角色: packagers
- 依赖: T-M1-03、T-M1-05
- 估算: 1.5d
- 交付物: DMG 打包器（.app → 磁盘映像，含拖拽安装布局）。
- 验收: macos-14 上生成 DMG，`hdiutil verify` 与挂载/卸载冒烟通过；非 macOS 宿主以 mock 模式跑通契约断言。

### T-M1-07 · ✅ Linux 打包器（AppImage / deb）（AppImage 1.9.1 真产单文件并启动冒烟；lintian `--fail-on error` 零 error（Maintainer 邮箱 / changelog.gz / libc6 / 描述兜底）；`dpkg -i` → 启动 → `dpkg -r` 冒烟在 ubuntu-latest CI 真跑；本地无 sudo 时显式 skip）

- 角色: packagers
- 依赖: T-M1-03、T-M1-05
- 估算: 2d
- 交付物: AppImage 与 deb 打包器（desktop 文件、图标、依赖与元数据齐备）。
- 验收: ubuntu-latest 上 AppImage 可执行启动冒烟通过；deb 经 `dpkg -i` 安装与卸载冒烟通过；deb 通过 lintian（无 error 级问题）。

### T-M1-08 · ✅ Web 静态站点打包器

- 角色: packagers
- 依赖: T-M1-04、T-M1-05
- 估算: 1d
- 交付物: 静态站点打包器（dist + manifest → 可托管静态包，含 PWA 资产），与内建目标走同一 Packager 接口。
- 验收: 产物被静态服务器直接托管即访问可用；html-validate 与 manifest 断言通过；与 §3.2 Web 行（静态站点 + PWA manifest）一致。

### T-M1-09 · ✅ poly-cache（blake3 CAS + 本地后端 + 对象存储 trait）

- 角色: core
- 依赖: T-M1-01、T-M1-02
- 估算: 3d
- 交付物: §5.2 缓存键（源文件树摘要 ‖ 依赖锁文件 ‖ TargetSpec 版本 ‖ 工具链版本 ‖ 构建参数）与内容寻址存储（blake3 + CAS）、本地文件系统后端、对象存储后端 trait、命中统计事件。
- 验收: 缓存键对五类输入任一变化敏感（单测逐项验证）；二跑命中缓存（事件流出现 Cached 且 hit=true，构建时延明显下降）；内容与摘要不符的损坏条目被检出并视为未命中。

### T-M1-10 · ✅ poly dev 热预览 v0（Web 宿主）

- 角色: ui-adapters
- 依赖: T-M1-01、T-M1-04
- 估算: 2.5d
- 交付物: `poly dev` 命令：启动 Web 宿主 dev server；UI 变更热更；核心（Rust）变更触发重编译并刷新注入。降级预案：若注入方案超出估算，降级为整页重载，热更（注入形态）延后至 M2+。
- 验收: 编辑 UI 源文件后浏览器免手动刷新即更新；编辑核心源码后自动重编译且页面可见效果（整页重载为可接受的降级形态）；退出后无残留子进程。

### T-M1-11 · ✅ poly init 示例应用模板

- 角色: core
- 依赖: T-M0-03
- 估算: 2d
- 交付物: `poly init` 生成最小示例应用：Rust core crate + React UI 骨架（使用 @polypack/ui 最小 tokens）+ polypack.toml（windows / macos / linux / web 四个 L1 目标声明）+ 基于 Pax 的应用图标资产。
- 验收: 空目录执行 `poly init` 后配置直接通过 schema 校验，`poly doctor` 可正常给出工具链报告；signing 全部为引用、无明文凭据；README 快速开始步骤与生成物一致。

### T-M1-12 · ✅ 英文版图示（英文标签 JSON → docs/assets/en/*.svg）

- 角色: docs
- 依赖: T-M0-10
- 估算: 2d
- 交付物: 英文标签 JSON（与现有中文 JSON 同结构）+ render-diagrams.mjs 支持渲染 `docs/assets/en/*.svg` + README.en.md / ROADMAP.en.md 引用英文图示。
- 验收: 运行 `node tools/render-diagrams.mjs` 生成全部英文 SVG 且 `--check` 通过；中英图示节点结构一致（仅标签语言不同）；英文图示保留右下角 Pax 角标（`--no-brand` 可关）。

### T-M1-13 · ✅ 四端契约测试与 M1 验收

- 角色: infra
- 依赖: T-M1-03、T-M1-04、T-M1-05、T-M1-06、T-M1-07、T-M1-08、T-M1-09、T-M1-11
- 估算: 2.5d
- 交付物: 示例应用四端（Windows / macOS / Linux / Web）契约测试（打包 + 校验器 + 产物结构断言与金样比对；mock 模式每次提交、真实模式每夜）与二跑缓存命中率断言。
- 验收: CI 三宿主全绿；四端产物结构断言通过；同一输入二跑缓存命中率 ≥ 90%（构建报告输出统计数据）。

### T-M1-14 · ✅ Pax 品牌落地（CLI 状态表情 + 应用图标派生；deb 真资产 / MSI·DMG 契约级）
- 角色: ui-adapters
- 依赖: T-M1-05, T-M1-11
- 估算: 1.5d
- 交付物: ① CLI 启动横幅与构建状态表情映射（成功=欢乐 / 失败=哭 / 异常=惊恐 / 待确认=撒娇 / 空闲=笑；终端不支持图形时降级为文字与颜色）；② 图标派生管线：`pax-mark.svg` → PNG / ICNS / ICO（构建期生成，不入库）；③ poly-gui 应用图标与窗口图标。
- 验收: MSI / DMG / deb 产物图标为 Pax 派生资产且尺寸断言通过；CLI 成功与失败路径输出对应表情（降级模式输出文字）；品牌资产全部来自 `tools/build-pax.mjs` 单一源。

### T-M1-15 · ✅ `poly clean` 与保留策略
- 角色: core
- 依赖: T-M1-09
- 估算: 1d
- 交付物: `poly clean`（`--cache` / `--artifacts` / `--all`；默认 dry-run 列出可回收条目与空间，`--yes` 执行）；`polypack.toml` `[retention]` 配置（缓存天数 / 容量上限、产物保留次数）；超限 LRU 自动淘汰；清理报告（释放空间统计）。
- 验收: 超限时 LRU 自动淘汰且构建可正常复用剩余缓存；`poly clean --all --yes` 后 `poly doctor` 复检通过；dry-run 不产生任何写入。

<!-- 统计: M0=11任务/18d, M1=15任务/31.5d -->
## M2 移动（5 周）

> 前置依赖: T-M0-01（workspace）、T-M0-02（三宿主 CI）、T-M0-04/T-M0-05（TargetSpec DSL 与 L1 声明）、T-M0-06/T-M0-07（JobSpec 与诊断框架）、T-M0-09（工具链探测）、T-M1-05（Packager 接口）、T-M1-09（poly-cache）、T-M1-11（示例应用模板）｜出口条件: 示例 APK/IPA 可安装且签名可验证；密钥零入库（secret-scan 全绿）；Linux 宿主经远程农场出 IPA

### T-M2-01 · ✅ uniffi 桥生成器（UDL 单源 → Kotlin/Swift 绑定；摘要入缓存键、四出口登记；Kotlin 真编译 + 双 ABI .so 真跑；Swift 编译真证——真生成器 uniffi 产物 + 真调用面过 swiftc -typecheck（macos-14 真模式实证））

- 角色: core
- 依赖: T-M0-01、T-M1-11
- 估算: 3d
- 交付物: 工作区接口生成器：从单处 Rust 接口定义产出 Kotlin 与 Swift 绑定（uniffi），接口按 `bridge/v1` 版本化；生成器结构预留 napi / wasm-bindgen / cffi 出口（M3 消费），桥接代码不得手写四份。
- 验收: 对示例核心一键生成两端绑定代码且可被两端工具链编译；接口变更即触发库级缓存失效（缓存键含接口摘要）；生成产物与实际 Rust 导出签名一致（测试断言）。

### T-M2-02 · ✅ Kotlin/JNI 绑定产物（AAR）（双 ABI 真交叉编译 + AAR 499,948B 二跑字节一致；emulator 三层实证：loadLibrary + dlsym 真调用 add(2,3)=5 + Kotlin→JNA→Rust 全链；arm64 仅 ELF 符号层、app_process 边界如实注记）

- 角色: core
- 依赖: T-M2-01、T-M1-09
- 估算: 2.5d
- 交付物: 核心编译为各 ABI triple 的 JNI `.so` + 生成的 Kotlin 绑定，组装为 AAR（清单/资源最小化），作为 Android 打包阶段的输入。
- 验收: AAR 在 Android 契约测试中加载成功；Kotlin 侧调用示例接口结果与 Rust 单测一致；AAR 走库级缓存，二跑命中。

### T-M2-03 · ✅ Swift 绑定产物（XCFramework）（三 triple/lipo/结构与篡改校验完备；macos-14 真跑已覆盖 cargo×3 交叉编译 + lipo 合并 + `xcodebuild -create-xcframework` 真链接与 archive 链；simulator 调用对拍真证——simctl spawn 真跑 paxAdd(-5,3)=-2 与 i32 回绕（macos-14 真模式实证））

- 角色: platforms
- 依赖: T-M2-01
- 估算: 2.5d
- 交付物: 核心编译为 device + simulator 切片的 XCFramework（staticlib）+ 生成的 Swift 绑定与 modulemap。
- 验收: 最小宿主工程在 macOS 宿主用 xcodebuild 链接通过；Swift 侧调用示例接口与 Rust 输出一致；切片架构可枚举校验（`lipo -info`）。

### T-M2-04 · ✅ @polypack/adapter-native v0（RN 宿主 + capabilities 契约；42 测试绿；示例页真机渲染实证——emulator-5554 uiautomator 语义标记全中 + 截图留档；降级分支双形态走查并跑；宿主构建环境适配成文）

- 角色: ui-adapters
- 依赖: T-M1-04、T-M1-11
- 估算: 3d
- 交付物: `@polypack/adapter-native` v0：实现 §6 RendererAdapter 契约（`mount` / `platformStyle(tokens)` / `capabilities(): { focusNav, safeArea, gestures }`）的 RN 宿主装配层，原生模块注册位预留桥产物挂载点。
- 验收: 同一示例页面组件树在 Android RN 宿主渲染；`capabilities()` 输出与 TargetSpec/宿主能力一致；降级路径由 capabilities 声明驱动（无指针 → 焦点环分支可枚举，完整导航图留待 M4）。

### T-M2-05 · ✅ adapter-native 焦点/导航基线（遍历图 + Back 记忆栈 + 键路由；设备六走查双形态 6/6 + 6/6；环像素差分双态 4 点实证；真根因修复——首键吞噬与 CENTER 双计 → 焦点锚点）

- 角色: ui-adapters
- 依赖: T-M2-04
- 估算: 2.5d
- 交付物: 基线焦点模型：方向键遍历的最小遍历图、焦点环渲染、Back 键焦点记忆接口——作为共享内核供 M4 TV 焦点导航系统复用。
- 验收: 示例应用在 Android 宿主以方向键完成一个核心任务（外接键盘/调试模式）；焦点顺序稳定（快照断言）；Back 返回上一焦点，无指针环境不出现死路。

### T-M2-06 · ✅ Android Packager（AAPT2 免 Gradle 直出；真契约 3 连绿命中金样；零告警通道防假绿；SOURCE_DATE_EPOCH 二跑逐字节一致、跨绝对目录稳定）

- 角色: packagers
- 依赖: T-M2-02、T-M0-09、T-M1-05
- 估算: 3d
- 交付物: `poly-bundle` Android 打包器：AAPT2 compile/link 资源与清单（含 ABI 与语言裁剪、可复现参数），产出未签名 AAB/APK 中间产物（签名位由 T-M2-09 接入）；全程不依赖 Gradle。
- 验收: `poly build --target android` 直出 AAB 与 APK；AAPT2 链接零告警；注入 `SOURCE_DATE_EPOCH`，二跑产物除签名与时间戳外字节一致（`poly verify --reproducible`）。

### T-M2-07 · ✅ Android 产物组装与校验（bundletool）（validate 真跑；emulator-5554 安装冷启动 Status: ok；结构断言入事件流）

- 角色: packagers
- 依赖: T-M2-06
- 估算: 2.5d
- 交付物: bundletool 直出与校验链：`build-bundle` / `build-apks` 组装、`bundletool validate` 校验、设备安装冒烟脚本。
- 验收: `bundletool validate` 通过；样本 APK 在模拟器/真机安装并冷启动成功；产物结构断言（清单、ABI、资源表）以事件流输出。

### T-M2-08 · ✅ iOS Packager（archive + Payload 直压）（macos-14 真跑全链闭合：CLI → XCFramework 现建 → 骨架 → archive → 未签名 IPA（4.8KB/3 条目）→ 双产物命中金样 → 留档；exportArchive 语义留给有 Team 的签名流）

- 角色: packagers
- 依赖: T-M2-03、T-M2-04、T-M0-09、T-M1-05
- 估算: 3d
- 交付物: `poly-bundle` iOS 打包器：生成/复用 Xcode 工程骨架，`xcodebuild archive` + `exportArchive` 产出未签名 IPA（导出方式可配）；仅 macOS 宿主可执行，宿主判定接入 probe 阶段。
- 验收: 示例应用在 macOS 宿主直出 IPA；`plutil`/`xcrun` 检查 Info.plist 与架构正确；描述文件与证书只以凭据引用出现（无明文）。

### T-M2-09 · ✅ Android 签名（apksigner + keystore 引用）（真签 v2+v3 双方案 + DN 断言；plan fail-fast；临时密钥零残留、口令零泄露）

- 角色: release-sec
- 依赖: T-M2-07、T-M2-11
- 估算: 2d
- 交付物: `poly-sign` Android Signer：`CredentialRef` → keystore 解析 → apksigner 签名，密钥材料只落内存/临时目录；签名后自动 `apksigner verify`。
- 验收: 示例 APK `apksigner verify --print-certs` 通过；缺失/错误引用在 plan 阶段快速失败（稳定错误码 + 修复提示）；签名后断言无临时密钥残留。

### T-M2-10 · ✅ iOS 签名与公证（codesign + notarytool）（库级链路/凭据引用/回执槽就绪；sign/verify 已入 DAG——`poly build` 内联 sign 凭据门控三级语义、真签 apksigner 双方案独立复核；codesign 真执行与真机公证待 Apple 门控）

- 角色: release-sec
- 依赖: T-M2-08、T-M2-11
- 估算: 2d
- 交付物: `poly-sign` iOS 链路：codesign（证书/描述文件经凭据引用）+ `notarytool submit --wait` 公证 + stapler 装订；`codesign --verify` 与 `spctl` 校验接入 verify 阶段。
- 验收: 示例 IPA 公证成功并装订；`codesign --verify --deep` 与 `spctl -a -vv` 通过；公证回执归档入提审级缓存记录。

### T-M2-11 · ✅ 密钥库集成（keyring + Vault 后端 trait）（后端链 + 脱敏断言；retry --from sign 真签两轮重拉凭据；真 Vault dev KV v2 读写往返（audit 日志 token 仅 hmac）+ 真 keyring 隔离总线闭环——Deepin 补丁版根因实证）

- 角色: release-sec
- 依赖: T-M0-06
- 估算: 3d
- 交付物: 凭据解析层：`CredentialRef` → 后端链（本地 keyring + Vault 后端 trait + CI 内存注入），签名阶段按需拉取、用后即弃；仓库内明文凭据即构建失败。
- 验收: 同一 JobSpec 分别在 keyring 与 Vault 后端解析成功；事件流/日志中凭据值不可见（脱敏断言）；`poly retry --from sign` 独立重跑并重新拉取凭据。

### T-M2-12 · ✅ secret-scan（提交与构建双重检查）（构造样本全命中 + 全历史 823 文件真扫零命中 + CI 三宿主工作流；「必过检查项」branch protection 已设置——3 宿主 secret-scan 为 required checks（2026-10-16 核实）；enforce_admins=true（2026-10-20 核实），共 9 个 required contexts，admin 直推同样受约束）

- 角色: release-sec
- 依赖: T-M0-02
- 估算: 2.5d
- 交付物: 内置规则集（keystore/JKS、证书私钥、API key 模式）+ 提交钩子与 CI 作业双重检查，命中即失败并输出修复提示（错误码体系）。
- 验收: 各类规则的构造样本全部命中；仓库全历史扫描绿灯；CI 中 secret-scan 为必过检查项（三宿主）；branch protection `enforce_admins=true`（2026-10-20 核实）。

### T-M2-13 · ✅ 远程农场协议 v0（JobSpec 序列化 + worker 拉取执行）（帧/双信箱/共享对象存储缓存复用真跑；R3 准入语义 ✓；macOS worker 真出 IPA 待 nightly；master 陈旧帧留 v1）

- 角色: core
- 依赖: T-M2-08、T-M2-10、T-M1-09
- 估算: 3d
- 交付物: 农场协议 v0：JobSpec 序列化格式 + worker 拉取-执行-回传循环（worker 注册与能力声明），本地与远程共用同一 DAG 执行实现。
- 验收: Linux 宿主发起 iOS 目标构建 → macOS worker 出 IPA，签名校验通过（R3 验收路径打通）；worker 端失败时错误码与修复提示完整回传；缓存跨机复用经对象存储后端验证。

### T-M2-14 · ✅ 移动目标 CI 接入与契约测试金样（mock 列三宿主全绿；真模式两腿通过并留档——ios 9,062B / android 9,770B；契约 ios 腿走库形态夹具 + 双产物（ipa + xcframework）真比对、android 取件按名锚定 base.zip；金样漂移即红含 diff；windows msi / macos dmg 真检查器已落（本机全绿；真编译面待 windows/macos CI 腿首证）；schedule 首夜已实证（2026-10-16，best-effort 调度延迟约 3h）：mobile-contract 真模式两腿 ✓ 并留档；bridge-real 首跑因 NDK 下载瞬态损坏中断（同 SHA 重派 38849101630 ✓，已加 retry 加固）；其留产物步自落地起空转（RAII 先删临时工程）已修）

- 角色: infra
- 依赖: T-M2-07、T-M2-10、T-M1-13、T-M0-02
- 估算: 2.5d
- 交付物: CI 增加移动目标契约测试作业（Android 于 ubuntu-latest、iOS 于 macos-14；mock 模式每提交、真实模式每晚）+ 金样产物入库与比对脚本。
- 验收: CI 矩阵按 §8 运行且 mock 模式全绿；金样漂移即失败并给出 diff；nightly 真实模式通过并留产物。

### T-M2-15 · ✅ 远程农场与发起地说明文档（FARM 双语结构一致 8/0/16、内链全通；发起地/构建地定义与 Linux→macOS IPA 端到端示例）

- 角色: docs
- 依赖: T-M2-13
- 估算: 1d
- 交付物: 农场使用文档（R3 缓解）：明确区分「发起地 / 构建地」、农场部署与 worker 注册方式、凭据只在构建地按需拉取（同 T-M2-11 模型）、APK/IPA 签名验证指引；README 与 ROADMAP 中英双语镜像更新（§11）。
- 验收: 中英两版结构一致、链接可用；「发起地 / 构建地」在双版均有定义与端到端示例（Linux 发起 → macOS worker 出 IPA）；文档同步检查作业（T-M0-10）保持全绿。

## M3 鸿蒙与小程序（4 周）

> 前置依赖: T-M0-09（鸿蒙 SDK 探测）、T-M1-05（Packager 接口）、T-M2-01（桥生成器）、T-M2-04/T-M2-05（adapter-native 与焦点基线）、T-M2-11（密钥库）、T-M2-14（契约测试金样基建）｜出口条件: 三端小程序开发者工具预览通过；鸿蒙 HAP 签名安装；提审脚本化可重跑

### T-M3-01 · ✅ RNOH 宿主与 NAPI 桥接入（ui/adapters/native/host/harmony 宿主模板 42 文件 + `poly bridge ohos`：OHOS aarch64 真交叉编译自包含 libpax.so 6,241,136B + node 真装载对拍 greet/add 同字面量；poly doctor 鸿蒙双场景；workspace 426 测全绿；模板 HAP 构建止于 CompileArkTS——RNOH 0.84.3 har 需 SDK ≥21 vs 本机 19，环境侧阻塞，修复路径入 README）

- 角色: platforms
- 依赖: T-M2-01、T-M0-09
- 估算: 3d
- 交付物: HarmonyOS 宿主工程模板（RNOH）+ NAPI（C ABI）桥接入：`.so` 编译链（HarmonyOS triple）与 napi 绑定注册，桥产物由 T-M2-01 生成器统一产出。
- 验收: 示例核心以 NAPI 模块在 RNOH 宿主加载；示例接口调用结果与 Rust 单测一致；`poly doctor` 识别 HarmonyOS SDK 并对缺失给出修复提示。

### T-M3-02 · ✅ RNOH 承载的 adapter-native（能力映射）（component-map 按 target 剔宿主专属 prop——harmonyos 剔 hasTVPreferredFocus、页面树零改动；native-registry 分族 + 双宿主结构快照逐字节比对；63 测 62 绿 1 跳过（设备腿 SKIP 附走查手册）；兼修模板 target "harmony"→"harmonyos" 真缺陷 + 哨兵测试）

- 角色: ui-adapters
- 依赖: T-M2-04、T-M2-05
- 估算: 2.5d
- 交付物: adapter-native 的 RNOH 承载分支：组件映射、`capabilities()`/tokens 蒙皮的鸿蒙适配，页面组件树不改动。
- 验收: 同一示例页面在 Android RN 与 RNOH 两宿主渲染一致（结构快照比对）；焦点/导航基线在 RNOH 下可用（外接键盘/调试模式）。

### T-M3-03 · ✅ HAP 打包器（hvigor 调用链）（工程配置生成 + 宿主模板复用/最小骨架回退 + hvigorw assembleHap；真跑 12,839B 未签名 HAP、二跑 16 处 UP-TO-DATE、三跑字节一致；结构检查 8 条目入事件流；工程落 out_dir 外保增量缓存）

- 角色: packagers
- 依赖: T-M3-01、T-M1-05
- 估算: 2.5d
- 交付物: `poly-bundle` HAP 打包器：工程配置生成 + hvigor 构建调用 + 产物收集，产出未签名 HAP；构建日志接入事件流。
- 验收: `poly build --target harmonyos` 直出 HAP；HAP 结构检查（清单/模块/资源清单项）通过；二跑缓存命中。

### T-M3-04 · ✅ HAP 签名与校验（hap-sign-tool）（sign-app/verify-app 两相；凭据全引用——隔离 keyring 真跑；真签 48,063B（Signing Block v3 / profile debug / Digest true / Verify success）；verify 腿 hap-sign-verify + hap-structure-check 双 true；口令仅经 argv → 计划/诊断/事件流统一脱敏；设备腿 hdc 无目标=SKIP 注记；retry --from sign 未接 harmony 留注记）

- 角色: release-sec
- 依赖: T-M3-03、T-M2-11
- 估算: 2.5d
- 交付物: `poly-sign` 鸿蒙链路：hap-sign-tool 签名（证书/Profile 经凭据引用）+ 签名校验 + HAP 结构检查接入 verify 阶段。
- 验收: 示例 HAP 在 DevEco 模拟器/真机签名安装并启动；签名校验命令绿灯；凭据全程引用化（无明文入库）。

### T-M3-05 · ✅ @polypack/adapter-taro v0（mount 路由绑定制 + 能力矩阵降级 + API 白名单构建期拒绝；poly-targets 增 L2 小程序族四目标声明；宿主工程落 ui/adapters/taro/host + 结构测试；node --test 36 项 + poly-targets 11 项全绿；devtools 预览腿无工具链=SKIP-with-print 注记）

- 角色: ui-adapters
- 依赖: T-M2-04（capabilities 契约与降级模式）
- 估算: 3d
- 交付物: `@polypack/adapter-taro` v0：React 语法页面在 Taro 渲染器的 `mount` 契约、小程序族 capabilities 降级（无 DOM 直连、生命周期与路由映射）与 tokens 蒙皮。
- 验收: 同一页面组件树在 Taro（微信目标）开发者工具预览中渲染；降级路径有断言测试；越出宿主 API 白名单的调用在构建期被拒。

### T-M3-06 · ✅ Taro 构建管线接入（微信/支付宝/抖音/快应用目标）（poly-bundle 小程序族四打包器 + poly-cli package 臂；真跑 weapp 301,644B / alipay 250,110B / tt 244,864B；快应用工程过 hap-toolkit 2.1.1 真 build 出 .rpk；描述符归一 miniprogramRoot=./dist/ 且 tt 源 project.tt.json→产物 project.config.json；wasm 出口四文件随包（wechat 22 / alipay 19 / douyin 19 / quickapp 10 条目）；二跑缓存 hit 2/2；contract_m3 mock 5 + real 四端 146.33s；node 41/41 含 host 结构测试；devtools 预览腿无工具链=SKIP-with-print 注记）

- 角色: packagers
- 依赖: T-M3-05、T-M1-05
- 估算: 2.5d
- 交付物: `poly-bundle` 小程序打包器：Taro 编译四目标配置 + 代码包组装，体积/API 白名单预检钩子留给 T-M3-08 接入。
- 验收: Taro 四目标编译产物齐备（快应用仅到编译工程，`.rpk` 由平台工具链环节处理）；微信/支付宝/抖音代码包可被对应开发者工具打开预览；二跑缓存命中。

### T-M3-07 · ✅ wasm 核心进小程序沙箱（wasm-bindgen + API 面白名单）（`poly bridge wasm`：UDL → 白名单两道闸（生成期 E_BRIDGE_0004 / 加载期 loader 核函数面与导入模块）→ shim → cargo wasm32；真跑 56,445B wasm 在 Node 沙箱内 greet→"hello, world"、导入表仅胶水自身模块、负例 ×3、二跑缓存字节一致；wasm-bindgen 0.2.129 落 ~/.cache/polypack/tools；产物 <root>/bridge/wasm-bindgen/ 四文件随代码包分发）

- 角色: core
- 依赖: T-M2-01
- 估算: 3d
- 交付物: wasm-bindgen 编译产物接入小程序沙箱：API 面白名单（仅暴露沙箱可用接口子集）与加载/初始化适配层，随代码包分发。
- 验收: 示例核心以 wasm 在小程序沙箱内执行示例接口；白名单外接口在编译期/加载期即拒绝（负例测试）；产物无原生模块依赖。

### T-M3-08 · ✅ 体积预算（budget.size）门禁与 wasm-opt 裁剪（wasm-opt@133 真跑 56,445→27,179B（-51%）；裁剪时机锁死出口生成后/随包拷贝前——随包字节≡裁剪后逐字节（quickapp sha256 对拍）；[budget] 逐产物判定 warn（Budgeted 事件 + E_BUDGET_0001）/ fail（DAG 中止、sign/verify 不执行）双态真跑；体积报告入事件流与 manifest size 块；缓存命中经 read_size 回读一致）

- 角色: core
- 依赖: T-M3-06、T-M3-07
- 估算: 2.5d
- 交付物: `budget.size` 预算检查接入 DAG（超预算告警/失败策略可配）+ wasm-opt 优化与按目标裁剪（R10 对策）。
- 验收: 示例应用输出 wasm 与代码包体积报告（事件流 + 终端报告）；人为超预算触发告警且错误码可定位；wasm-opt 前后体积对比写进产物元数据。

### T-M3-09 · ✅ 微信小程序 publish 客户端（上传 + 提审 + 状态轮询）（B 协议容器（miniprogram-ci 2.1.31 口径）+ raw PKCS#1 v1.5 签名 + gzip 上传 / submit_audit / get_latest_auditstatus 轮询（85009 恢复路径）；四步零分叉驱动——容器无关 blake3 摘要 + 断点续走 + at-least-once 提审边界；回执 canonical JSON 原子落盘永久保留；契约 28 项全绿；真实账号腿无凭据=SKIP 如实；poly-core scan 增 appsecret-hex32 门控规则（32-hex 属高熵盲区））

- 角色: release-sec
- 依赖: T-M3-06、T-M2-11
- 估算: 3d
- 交付物: `poly-publish` 微信开放平台客户端：代码包上传 → 提审 → 审核状态轮询 + 回执归档（自动化止步提审，R4）。
- 验收: 测试账号下一次完整上传+提审跑通且脚本可重跑（幂等/去重）；轮询状态与回执归档格式固定；凭据经引用模型注入。

### T-M3-10 · ✅ 支付宝 / 抖音 publish 客户端（复用骨架）（支付宝网关 v1：RSA2 签名独立复算入测、version.upload → build.query 构建等待 → audit.apply 平铺提审 → detail.query 轮询、幂等续走；抖音：err_no 信封 + access-token 头、版本创建 → get_audit_hosts → audit v2 → 版本轮询、11303 幂等；单文件载具 Package::load_file（域分隔摘要、重命名不换回执键、目录摘要字节不变）→ Channel::QuickApp；三渠道零分叉同一四步驱动；边界注记：支付宝文件材料明拒 E_PUB_0004 / 响应签未校验（凭据契约未含公钥）/ minidev 备选不做 / 抖音 token 铸造归外部链路；真平台腿 SKIP×3 如实）

- 角色: release-sec
- 依赖: T-M3-09
- 估算: 2.5d
- 交付物: 支付宝开放平台与抖音开放平台 publish 客户端：复用微信骨架，平台差异收敛在客户端实现内。
- 验收: 两平台各自的上传+提审+轮询路径跑通；三平台共用同一 Publisher 接口（核心零分叉）；凭据经引用模型注入。

### T-M3-11 · ✅ 小程序契约测试（开发者工具 CLI + 金样比对）（contract_m3_devtools：注册表 verify 声明↔CLI 表一致（quickapp 的 hap-toolkit-lint 归 T-M3-12）+ 探测链 + 凭据注入 0600 临时文件用后即弃 + 全链脱敏断言；新 workflow miniprogram-contract.yml（mock ubuntu 每提交 / real macos-14 每夜 18:47 错峰）；真实档本机无 CLI=SKIP×3 如实；CI vars/secrets 清单留配置注记）

- 角色: infra
- 依赖: T-M3-06、T-M2-14
- 估算: 3d
- 交付物: 每目标契约测试：微信/支付宝/抖音开发者工具 CLI 校验 + 代码包结构断言 + 金样比对，复用 M2 契约测试与金样基建；devtools 登录态经凭据引用注入（同 T-M2-11 模型）。
- 验收: 三端契约测试全绿；金样漂移即失败；CI 按 §8 两档执行（每提交 mock、每晚真实 CLI）；真实档登录态不入日志（脱敏断言）。

### T-M3-12 · ✅ 快应用打包与商店校验（hap-toolkit 2.1.1 真 build 出 .rpk（9 条目、manifest 与源同源交叉）+ lint 入 verify（hap-toolkit-lint→QuickAppLint）；format/builder 翻 rpk/hap-toolkit（§3.3 对齐，五消费者连带）；poly publish 提审薄层：dry-run 真跑（真 11,740B rpk——载具摘要/回执键与四步驱动逐字符同源、重跑字节一致、凭据零泄漏）；缺端点/客户端 → E_PUBLISH_0001/0002 精确诊断；联盟商店无公开 CI 端点（2026-10 文档核实）+ 无真账号 → 真提审腿 SKIP 注记；四端契约真跑 649.91s 全绿（quickapp 11 条目））

- 角色: packagers
- 依赖: T-M3-06、T-M3-09
- 估算: 1.5d
- 交付物: 快应用链路收口：hap-toolkit 构建产出 `.rpk` + hap-toolkit lint 校验接入 verify 阶段；联盟商店提审脚本复用 T-M3-09 的 publish 客户端骨架。
- 验收: 示例应用直出 `.rpk` 且 lint 通过；联盟商店提审脚本可重跑（幂等，凭据经引用模型）；与 §3.3 快应用行一致（`.rpk` → 快应用联盟商店）。

<!-- 统计: M2=15任务/38d, M3=12任务/31.5d -->
## M4 TV 家族（4 周）

> 前置依赖: M1（打包器矩阵与内容寻址缓存）、M2（Android/iOS 打包链、uniffi 桥与密钥库）、M3（wasm 核心与适配器基线）｜出口条件: 焦点遍历无死路；wgt/ipk 校验器通过；Roku 能力边界形成结论（继续/降级）

### T-M4-01 · ✅ adapter-tv 骨架与 10-foot tokens（react-dom 挂载 + 蒙皮（tokens 单源派生）+ 安全区四边 5%（1080p 96/54px）+ 能力矩阵六枚 -tv id；25/25 node + tokens 金样；设备腿 SKIP 注记）

- 角色: ui-adapters
- 依赖: —
- 估算: 2d
- 交付物: 新增 `@polypack/adapter-tv` 包（`RendererAdapter` 契约实现：react-dom 挂载 + 10-foot 皮肤 tokens——远距字号、间距与安全区，tokens 由 `@polypack/ui` 单一源派生）。
- 验收: `capabilities()` 返回 `focusNav: true` 与电视安全区；同一示例页面组件树在 adapter-dom 与 adapter-tv 下均渲染成功，tokens 快照金样比对通过。

### T-M4-02 · ✅ 焦点导航遍历图与焦点环（几何推导三重裁定 + focusOrder 覆盖 + 连通性检查器（示例 0 孤立 0 陷阱）+ 焦点环四向走查；37/37 node）

- 角色: ui-adapters
- 依赖: T-M4-01
- 估算: 3d
- 交付物: 方向键遍历图（按几何位置推导焦点邻居，支持显式 `focusOrder` 覆盖）、焦点环渲染、遍历图连通性静态检查器。
- 验收: 连通性检查器对示例应用报告 0 孤立节点（无死路）；四向移动与焦点环可见性冒烟通过。

### T-M4-03 · ✅ 焦点记忆、Back 键语义与数字键快捷键（层=独立焦点域逐层结构恢复 + 记忆栈 + 退出裁决 + 数字键四族归一；50/50 node——M4 焦点内核冻结）

- 角色: ui-adapters
- 依赖: T-M4-02
- 估算: 2d
- 交付物: 焦点记忆栈（返回上一焦点）、分层 Back 键语义（先关弹层 → 再回焦点记忆 → 最后退出应用）、遥控器数字键快捷键映射。
- 验收: 子页/弹层逐层 Back 回退并恢复原焦点；数字键快捷键触发对应功能；契约测试覆盖三层 Back 语义与焦点记忆恢复。

### T-M4-04 · ✅ Android TV / Fire TV Leanback 清单与 APK 出包（复用 M2 AAPT2+bundletool+apksigner 同一条链零第二实现；Leanback 三行 + banner 无则如实省去不伪造；真腿两目标各 2 件产物命中金样）

- 角色: packagers
- 依赖: T-M2-07, T-M2-09（Android AAPT2 + bundletool + apksigner 链）, T-M4-01
- 估算: 3d
- 交付物: `poly-bundle` 新增 TV 变体 Packager：Leanback 清单（`android.software.leanback`、`android.hardware.touchscreen=false`、TV banner/图标）与 Android TV、Fire TV 两个目标声明（Fire TV 复用同一 APK 链，渠道指向亚马逊应用商店）。
- 验收: APK 通过 bundletool validate 与 apksigner verify；Leanback/TV 轨道清单断言在契约测试中通过；体积在 `budget.size` 预算内。

### T-M4-05 · ✅ Android TV / Fire TV 契约测试与焦点导航冒烟（mock 6/6 + real exit 0；设备冒烟 6/6 走查 + 「无指针完成核心任务」PASS + 焦点环像素实证（#0e7490 恰一处随 DPAD_DOWN 移动）；AVD x86（上游无 x86_64 TV 镜像实证，ABI 自设备 abilist 派生））

- 角色: infra
- 依赖: T-M4-04, T-M4-03, T-M2-14（契约测试与金样基建）
- 估算: 2d
- 交付物: 两目标的契约测试（mock 模式每次提交、真实模式每晚上模拟器）与焦点导航冒烟用例（启动 → 方向键遍历 → 进入详情 → Back 返回）。
- 验收: mock 契约测试在 CI 全绿；每晚模拟器冒烟产出产物结构断言与日志归档；冒烟中含「无指针完成核心任务」判定。

### T-M4-06 · ✅ RN-tvOS 宿主接入与焦点对接（useTVEventHandler→内核键表、menu=Back（enableTVMenuKey）、DOWN-only；10-foot 蒙皮按 target 绑定 + 皮肤对拍哨兵；组件树零改动；CI 形态 node 全绿）

- 角色: ui-adapters
- 依赖: T-M4-02, T-M2-04, T-M2-05（RN 宿主基线）
- 估算: 2d
- 交付物: `@polypack/adapter-native` 的 RN-tvOS 分支：tvOS 遥控器事件源接入焦点导航系统，10-foot tokens 在 RN-tvOS 下生效。
- 验收: 示例应用在 tvOS 模拟器（macOS 宿主）启动，方向键遍历与焦点环行为与 adapter-tv 一致；无指针完成核心任务。

### T-M4-07 · ✅ tvOS 打包链（复用 M2 Xcode 打包链）（iOS 链按 ApplePlatform 描述符参数化复用（iOS 行为逐字节不变）+ 单架构 sim 直拷无 lipo + farm R3 扩臂；**macos-14 真跑首证**：XCFramework→archive→IPA 7,065B/3 条目 + 契约 6 项全绿——CI 三元组已入 mobile-contract）

- 角色: platforms
- 依赖: T-M4-06, T-M2-08, T-M2-10（iOS xcodebuild archive + exportArchive 链）, T-M2-13（远程农场协议 v0）
- 估算: 2.5d
- 交付物: tvOS 目标声明与打包器：复用 M2 的 Xcode 打包链（tvOS SDK 切片），uniffi 原生 staticlib 与 RN-tvOS 包合并，codesign 凭据引用；Linux/Windows 宿主经 M2 远程农场出包。
- 验收: `poly build --target tvos` 在 macOS 宿主出 TV 包，`codesign --verify` 通过；同一 JobSpec 经远程农场在 Linux 宿主发起亦出包成功。

### T-M4-08 · ✅ Tizen wgt 打包器与 wgt-lint 校验（config.xml + zip 真造真验 6,565B + python zipfile 独立见证；wgt-lint 缺席 SKIP-with-print + 内建结构断言守底（拆件硬失败 E_BND_0006））

- 角色: packagers
- 依赖: T-M3-07（wasm 核心构建）
- 估算: 3d
- 交付物: Tizen 目标（web 运行时）：wasm 核心 + react-dom UI 打成 `.wgt`，Tizen 作者证书签名（凭据引用），`wgt-lint` 接入 verify 阶段。
- 验收: `.wgt` 通过 `wgt-lint`；签名产物在 Tizen 模拟器侧载安装成功；体积在 `budget.size` 预算内。

### T-M4-09 · ✅ webOS ipk 打包器与 ares-package 校验（确定性 ar+ustar+gzip 手写读写 3,076B + ar/tar 独立见证；ares-package 缺席 SKIP；与 Tizen 同源同摘要）

- 角色: packagers
- 依赖: T-M4-08
- 估算: 2.5d
- 交付物: webOS 目标：复用 T-M4-08 的 wasm + react-dom UI 产物打成 `.ipk`，`ares-package` 校验接入 verify；签名按平台侧（发布签名）标注。
- 验收: `ares-package` 校验通过并可在 webOS 模拟器安装；与 Tizen 同源产物摘要一致（一份 UI 产物多目标复用）。

### T-M4-10 · ✅ VIDAA / Vizio SmartCast 应用包与沙箱自检（两应用包 6,150B/6,108B；入口/清单/体积三规则入 verify + budget 硬失败实证；无签名无推送如实入 notes；四目标同一摘要 sha384 单值）

- 角色: packagers
- 依赖: T-M4-08
- 估算: 2d
- 交付物: Hisense VIDAA 与 Vizio SmartCast 两个 web 运行时目标声明与打包器（应用包），沙箱自检规则（入口文件、清单字段、体积）接入 verify。
- 验收: 两目标出包并通过沙箱自检；TargetSpec 的 `notes` 如实标注无签名、无推送等边界。

### T-M4-11 · ✅ Roku SceneGraph 子集受限渲染器 spike（构建期发射器 + 焦点表内核同源（逐键等价）+ 官方侧载协议逐字取证（rokudev makefile）+ 结构自检 19 项 + node 9/9；BrightScript 行为级待真机侧载（T-M4-15 触发前置））

- 角色: ui-adapters
- 依赖: T-M4-01
- 估算: 3d
- 交付物: SceneGraph 子集渲染器 spike（Roku 核心不参与，仅 UI 子集 + 原生逻辑）：可承载组件与交互子集清单、最小频道 demo（zip 侧载自检）、能力边界文档（支持项 / 缺口 / 替代方案）。
- 验收: 最小 demo 在 Roku 设备或模拟器侧载并完成核心任务子集；能力边界文档列明缺口与降级路径，可直接作为继续/降级决策输入。

### T-M4-12 · ✅ Roku 继续/降级决策与 L3 能力边界文档同步（决策：**继续受限档**——experimental 维持、真机侧载为 T-M4-15 前置；roku TargetSpec 落 builtin.rs（19 内建目标）+ verify 归属登记 + ROADMAP R2 双语决策句；图示 8 张 / Pax / 文档校验全 PASS）

- 角色: docs
- 依赖: T-M4-11
- 估算: 1.5d
- 交付物: 决策记录（继续投入打包器 or 按受限目标降级），并按 §11 同步 README.md / README.en.md、ROADMAP.md / ROADMAP.en.md 的 L3 明细与 Roku 的 `experimental` / `notes` 字段，图示 JSON 同步后重渲染。
- 验收: 双语矩阵与 TargetSpec 对 Roku 的等级、`experimental`、能力边界描述逐一一致；`node tools/render-diagrams.mjs --check` 通过。

### T-M4-13 · ✅ 遥控器冒烟测试套件（无指针完成核心任务）（三腿复合真跑：Android TV 6/6 走查 + web 替代腿（headless Chrome/CDP，如实标注 surrogate）两目标各 4/4——14 场景/69 键步 fail 0；tv-smoke.yml（mock 每提交 / 夜跑 18:57 错峰 + KVM/vars 双门设备腿）；chrome ozone 根因修复（DISPLAY 在场探 X11 致卡死 → --ozone-platform=headless））

- 角色: infra
- 依赖: T-M4-03, T-M4-04, T-M4-09（web 系目标）, T-M2-14（契约测试与金样基建）
- 估算: 3d
- 交付物: 可复用 TV 冒烟框架：仅用方向键 + OK + Back + 数字键驱动示例应用完成核心任务（浏览 → 详情 → 播放占位 → 返回），覆盖 Android TV 模拟器与 web 系（Tizen / webOS 模拟器），输出结构化结果与失败定位。
- 验收: 套件在每晚 CI 覆盖全部已落地 TV 目标分支并通过；任一目标出现焦点死路即失败并给出定位信息。

### T-M4-14 · ✅ 鸿蒙 TV：HAP 复用与焦点适配冒烟（deviceTypes+tv 按 SDK19 枚举真文件实证 + polypack.tv 开关 + tv-frame 安全区单源（禁手抄百分比）+ 键序内核推导对拍；hdc 无目标 SKIP 注记；打包/签名链零改动复用 M3）

- 角色: platforms
- 依赖: T-M3-01, T-M3-03, T-M3-04（RNOH + HAP 打包与签名链）, T-M4-01
- 估算: 1.5d
- 交付物: HarmonyOS TV 目标适配核对：打包链复用 M3 的 RNOH 宿主与 HAP 打包/签名（hap-sign-tool），本任务只补 TV 侧适配——TV 分辨率与安全区核对、接入 adapter-tv 焦点系统。
- 验收: HAP 在 HarmonyOS 模拟器或开发者工具中安装并启动；方向键焦点遍历与焦点环按 adapter-tv 行为通过冒烟；目标分辨率下 TV 安全区无遮挡。

### T-M4-15 · ✅ Roku 频道包打包器与校验器（条件任务已交付：打包器 crates/poly-bundle/src/packagers/roku/ + 契约测试 crates/poly-bundle/tests/packagers_roku.rs + 适配产物 ui/adapters/roku；结构自检 / zip / 金样 / 确定性已覆盖；真机侧载按环境槽 SKIP-with-print）

- 角色: packagers
- 依赖: T-M4-12
- 估算: 3d
- 交付物: SceneGraph 子集渲染器产物打成 Roku 频道包（zip）+ 侧载自检，受限能力说明书随产物一并发布；档期为 T-M4-12 决策通过后安排（M5–M8 窗口内）。
- 验收: 仅当 T-M4-12 的 spike 决策为「继续」时执行；触发时频道包在 Roku 设备/模拟器侧载自检通过，受限能力说明书随产物发布且与 TargetSpec `experimental` / `notes` 一致；未触发时本任务不占用排期、不计入里程碑进度。

<!-- 统计: M4=15任务/36d（T-M4-15 条件任务已交付）, M5=11任务/25d -->

## M5 桌面发行版与 BSD（3 周）

> 前置依赖: M1（Linux 桌面打包器首版、Web 管线与 PWA manifest）、M2（poly-sign 与密钥库引用模型）｜出口条件: 各格式在对应宿主/容器内安装成功；BSD 目标进 CI；`poly release --target fedora,freebsd` 端到端可重跑

### T-M5-01 · ✅ deb / rpm 打包器成熟化（deb control+维护者脚本+AppStream、rpm spec+Requires+%files；L5 ubuntu/fedora/mint + lintian/rpmlint verify 腿；apt/dnf 真装装卸 0、双跑字节一致）

- 角色: packagers
- 依赖: T-M1-07（Linux 打包器：AppImage/deb）
- 估算: 2.5d
- 交付物: deb（control/Depends/维护者脚本）与 rpm（spec）打包器成熟化：多发行版元数据（Ubuntu / Fedora / Linux Mint 目标声明）、lintian 与 rpmlint 门禁接入 verify 阶段。
- 验收: 三发行版目标出包并分别通过 lintian / rpmlint（0 error）；在对应容器内 `apt install` / `dnf install` 安装成功。

### T-M5-02 · ✅ Flatpak 打包器（manifest + AppStream + 最小 finish-args（无 home/network/system-talk-name）+ flatpak-lint 三目标真跑；flatpak-builder 缺席→ stage 降级注记，不伪造 .flatpak）

- 角色: packagers
- 依赖: T-M5-01
- 估算: 2d
- 交付物: Flatpak 打包器：manifest、AppStream 元数据、沙箱权限最小化声明，`.flatpak` 产物与校验接入 verify。
- 验收: 产物通过 `flatpak build` / lint 校验并在容器内安装运行；权限声明与 `notes` 一致（无冗余权限）。

### T-M5-03 · ✅ Snap 打包器（snapcraft.yaml core24/strict + 四 plugs + snap-lint 真跑；snapcraft 缺席→ stage 降级注记）

- 角色: packagers
- 依赖: T-M5-01
- 估算: 2d
- 交付物: Snap 打包器：`snapcraft.yaml`、confinement 与接口声明、打包契约测试（产物结构断言）。
- 验收: 产物在容器内 `snap install --dangerous` 安装运行；接口声明与 TargetSpec `notes` 一致。

### T-M5-04 · ✅ AppImage 打包器成熟化（AppStream/desktop/hicolor + SOURCE_DATE_EPOCH；appstreamcli validate 0、AppDir 容器执行 0、--reproducible match；appimagetool 缺席→AppDir 注记）

- 角色: packagers
- 依赖: T-M1-07（Linux 打包器：AppImage/deb）
- 估算: 2d
- 交付物: AppImage 打包器成熟化：AppStream 元数据、desktop 入口与图标、`SOURCE_DATE_EPOCH` 可复现构建。
- 验收: 在容器内直接执行运行；AppStream 元数据通过 `appstreamcli validate`；`poly verify --reproducible` 对 AppImage 产物通过（签名与时间戳除外）。

### T-M5-05 · ✅ 仓库元数据生成与 GPG 签名（deb Packages/Release/InRelease、rpm repodata/repomd.xml.asc；CredentialRef/secret::resolve + 隔离 GNUPGHOME；apt 真跑+篡改拒绝、dnf 容器真跑、双跑字节幂等；密钥零入库）

- 角色: release-sec
- 依赖: T-M5-01
- 估算: 2.5d
- 交付物: deb 仓库元数据（Packages / Release / InRelease）与 rpm 仓库元数据（repomd）生成器，GPG 签名走凭据引用（密钥零入库）；自托管仓库目录布局与发布说明。
- 验收: 生成仓库可被 `apt update` / `dnf makecache` 消费且签名校验通过；重复生成幂等（二次生成除签名时间戳外字节一致）；仓库内无明文密钥（secret-scan 全绿）。

### T-M5-06 · ✅ FreeBSD pkg 目标（pkg create 显式 plist 清单（VM 真跑抓出空包缺陷）+ pkg-check 入 verify；FreeBSD 14.5 VM 真跑 pkg 签名仓库 + pkg install/check -s + 二跑幂等；Host::Bsd 原生宿主）

- 角色: platforms
- 依赖: T-M5-05（仓库签名抽象）
- 估算: 2.5d
- 交付物: FreeBSD 目标声明与打包器：原生构建（Tauri IPC 核心即宿主）、`pkg` 产物、`pkg check` 校验接入 verify、自建 pkg 仓库签名（可选，复用 T-M5-05 的仓库签名抽象）。
- 验收: 在 FreeBSD VM 内 `pkg install` 成功并启动应用；`pkg check` 通过；签名仓库可被 `pkg update` 消费。

### T-M5-07 · ✅ OpenBSD ports 目标（ports 骨架 + signify + portcheck 入 verify；Makefile 补 `.include <bsd.port.mk>`；OpenBSD VM 安装冒烟在托管 runner（无 KVM / TCG）两次实证 SSH 起不来，按 SKIP-with-print 留痕）

- 角色: platforms
- 依赖: —
- 估算: 2.5d
- 交付物: OpenBSD 目标：ports 目录骨架（Makefile、PLIST、DESCR）生成器、signify 签名接入、portcheck 校验接入 verify。
- 验收: 在 OpenBSD VM 内 `make package` 成功并安装运行；portcheck 通过；signify 签名可验证。

### T-M5-08 · ✅ BSD 家族 pkgsrc 目标（NetBSD / DragonFly；Makefile+PLIST+DESCR 同源 + pkgsrc-check；含空格资源目录引号化修复；bmake/NetBSD VM 真跑缺环境 SKIP-with-print）

- 角色: packagers
- 依赖: T-M5-06
- 估算: 2d
- 交付物: pkgsrc 目标声明与打包器（NetBSD / DragonFly 等）：`Makefile` + `PLIST` 生成，仓库签名与自建仓库布局复用 T-M5-06。
- 验收: 在 NetBSD VM 内 `bmake package` 成功并按打包器自检规则通过；产物进入自建仓库并可安装。

### T-M5-09 · ✅ BSD CI 方案定型（bsd-ci.yml 新工作流：FreeBSD 14.5 / OpenBSD 7.9 QEMU 原生构建 + 安装冒烟，夜跑 20:07 UTC；actionlint 绿；§8 CI 矩阵与 README/ROADMAP 双语回写；FreeBSD 腿 QEMU 真跑全绿，OpenBSD 尽力腿 VM 起不来按 SKIP-with-print 留痕）

- 角色: infra
- 依赖: T-M5-06, T-M5-07
- 估算: 2d
- 交付物: BSD 目标 CI 方案定型（VM vs 交叉编译的评估结论 + 落地实现）：CI 内 VM/QEMU 执行原生构建与安装冒烟；同步更新 §8 CI 矩阵与 README / ROADMAP 双语描述。
- 验收: FreeBSD / OpenBSD 目标在 CI 每夜任务中构建 + 冒烟通过；构建日志与产物归档可追溯；双语文档与 CI 矩阵一致。

### T-M5-10 · ✅ ChromeOS 双轨（chromeos 复用 deb、chromeos-pwa 复用 Web 链；同 JobSpec 双轨 + SRI 同源；pwa-audit 三腿入 verify + Chrome 151 真跑离线/manifest/安装性；Docker dpkg 装卸；Crostini 实机 SKIP）

- 角色: platforms
- 依赖: T-M5-01, T-M1-04（Web 管线与 PWA manifest）, T-M1-08（Web 静态站点打包器）
- 估算: 2.5d
- 交付物: ChromeOS 双轨交付：Crostini 侧复用 deb 打包器（安装冒烟）、PWA 侧复用 Web 管线 + manifest（PWA 审计规则接入 verify）。
- 验收: deb 在 Crostini 容器内安装并启动；PWA 通过离线能力、manifest、安装性审计；两轨产物来源同一 JobSpec。

### T-M5-11 · ✅ `poly release --target fedora,freebsd` 端到端（fedora rpm + freebsd pkg 各自 Published/回执/签名仓库安装 + 二跑幂等；多目标 dry-run 双事件；单全局 `--credential` 无法同时满足 GPG/PEM → 已补按目标凭据（重复 `--credential TARGET=REF`；裸 `REF` 仍为全局缺省；未知/重复映射报 `E_PLAN_0002`）；异构多目标真跑仍需真实密钥环 / Vault 环境）

- 角色: core
- 依赖: T-M5-05, T-M5-06, T-M5-09
- 估算: 2.5d
- 交付物: 多目标发布编排：一条命令完成 fedora（rpm 仓库）与 freebsd（pkg 仓库）的产物上传、仓库元数据刷新与签名、回执归档；与 M1/M2 的 publish 阶段复用同一 DAG 与事件流。
- 验收: 端到端可重跑（二跑缓存命中，仓库元数据幂等）；`--json` 事件流含 Published 事件与回执路径；失败时输出稳定错误码与修复提示。

<!-- 统计: M4=15任务/36d（T-M4-15 条件任务已交付）, M5=11任务/25d -->
## M6 嵌入式与车机（5 周）
> 前置依赖: M0（TargetSpec DSL 与 crate 依赖方向约束）、M2（Android 打包链，供 Automotive 复用）｜出口条件: 全部无 UI 目标（QNX / AUTOSAR / VxWorks / Zephyr / FreeRTOS）进 CI 编译矩阵；QNX 冒烟通过

### T-M6-01 · ✅ 核心分层：拆出 no_std 引擎层（poly-core feature 拆分不新增 crate；thumbv7em-none-eabihf 构建通过 + 依赖树无 tokio/petgraph/reqwest；std/no_std 同缓存键 golden）
- 角色: core
- 依赖: —
- 估算: 3d
- 交付物: 把 JobSpec 校验、TargetSpec 解析与缓存键计算在 `poly-core` 内做 feature 拆分，引擎层可在禁用 std（`no_std + alloc`）下编译，宿主依赖（tokio / petgraph / reqwest）只留在 std feature 后；**不新增 crate**，形态结论回写 ROADMAP §4.1 以保持 9-crate 表有效。
- 验收: `cargo build -p poly-core --no-default-features --target thumbv7em-none-eabihf` 通过；该构建下 `cargo tree -p poly-core --no-default-features` 不含 tokio / petgraph / reqwest；M0 的依赖方向检查在 workspace 仍为 9 个 crate 时保持绿；缓存键对同一输入在 std 与 no_std 两种构建下取值一致。

### T-M6-02 · ✅ 引擎层 C ABI 与 cffi 头文件生成（poly_engine_* 六导出 + 单源头文件；gcc/clang -std=c11 -Wall -Werror 真编译、nm 无 mangling、头文件唯一性测试）
- 角色: core
- 依赖: T-M6-01
- 估算: 2.5d
- 交付物: 引擎层稳定 C ABI（`poly_engine_*` 前缀）+ 由接口定义一处生成、四端复用的 C 头文件与静态库产物（§4.4 cffi 桥）。
- 验收: 头文件在 `gcc -std=c11 -Wall -Werror` 与 `clang -std=c11` 下均编译通过；`nm` 导出符号无 Rust mangling 泄漏；仓库内不存在第二份手写头文件（生成器为唯一来源）。

### T-M6-03 · ✅ QNX 交叉编译目标与产物（staticlib/cffi image|library + SDP 探测入 poly-toolchain + doctor 提示；无 SDP 本机 SKIP-with-print，应用侧 staticlib 包装 crate 不伪造产物）
- 角色: platforms
- 依赖: T-M6-02
- 估算: 3d
- 交付物: QNX TargetSpec（`runtime = "staticlib"`、`bridge = "cffi"`、`package = { format = "image" | "library" }`）与 QNX SDP 探测接入 `poly-toolchain`。
- 验收: `poly build --target qnx` 产出静态库与镜像两类产物；`poly doctor` 能报告本机 QNX SDP 缺失并给出安装提示；交叉编译冒烟（经 T-M6-09 的 QEMU）通过。

### T-M6-04 · ✅ AUTOSAR Adaptive 目标（ARXML + 静态库）（TargetSpec 无 UI/自部署 + ARXML 与 poly_engine_* 一一对应；xmllint 真校验、宿主 staticlib 真构建、nm 对照）
- 角色: platforms
- 依赖: T-M6-02
- 估算: 3d
- 交付物: AUTOSAR（Adaptive）TargetSpec（`runtime = "staticlib"`、无 UI、`no_std` 可选）与 ARXML 清单 + 静态库打包器。
- 验收: CI 编译矩阵内 AUTOSAR 目标全绿；ARXML 通过 XML schema 校验；服务/端口声明与 C ABI 导出符号一一对应（脚本断言）；`notes` 字段声明「无 UI、自部署」。

### T-M6-05 · ✅ VxWorks 目标（库 / 镜像）（库 2,206,316 B + 镜像 zip 真跑；nm 未解析闭包白名单 + §5.3 无绝对路径；wr-cc 缺席链接腿 SKIP-with-print）
- 角色: platforms
- 依赖: T-M6-02
- 估算: 2.5d
- 交付物: VxWorks TargetSpec 与静态库/镜像打包器（无 UI，cffi）。
- 验收: CI 编译矩阵内 VxWorks 目标全绿；产物可作为可加载模块被 VxWorks 宿主工具链 `ld` 校验通过（无未解析符号）；产物中无绝对路径（§5.3）。

### T-M6-06 · ✅ Zephyr 目标（west + no_std）（west 模板 + no_std module 链入；qemu_x86 / qemu_cortex_m3 `west build` 真跑、cortex_m3 QEMU 输出引擎版本；最小 CONFIG 集随模板 v2 版本化）
- 角色: platforms
- 依赖: T-M6-02
- 估算: 3d
- 交付物: Zephyr TargetSpec（`runtime = "staticlib"`、`no_std`）与 west 工程模板（zephyr 模块 + 引擎层静态库接入）。
- 验收: `west build` 对 `qemu_x86` 与 `qemu_cortex_m3` 两板均成功；引擎层以模块形式链入且不引入 std；模板中的最小 `CONFIG_*` 集被 CI 记录并随模板版本化。

### T-M6-07 · ✅ FreeRTOS 目标（静态库 + QEMU 示例板）（mps2-an385 真跑：poly_engine 版本打印 + SYS_EXIT_EXTENDED 退出 0；内核 V11.1.0 按 tag+sha256 现拉不入库，缺工具链/网络 SKIP-with-print）
- 角色: platforms
- 依赖: T-M6-06
- 估算: 2.5d
- 交付物: FreeRTOS TargetSpec 与静态库打包器 + 可跑在 QEMU 的示例板工程（复用 T-M6-06 的 no_std 接入方式）。
- 验收: CI 编译矩阵内 FreeRTOS 目标全绿；示例板在 QEMU 中启动并成功调用一次引擎层 API（打印版本号）后以退出码 0 结束。

### T-M6-08 · ✅ 无 UI 目标 CI 编译矩阵（`poly targets --tier 6 --headless --json` 派生矩阵，新增声明自动入列；embedded-ci 钉扎缓存 + REAL/SKIP job summary；缺 SDP/wr-cc SKIP-with-print）
- 角色: infra
- 依赖: T-M6-03, T-M6-04, T-M6-05, T-M6-06, T-M6-07
- 估算: 3d
- 交付物: §8 的 CI 宿主 × 目标层矩阵扩展：ubuntu-latest 上 QNX / AUTOSAR / VxWorks / Zephyr / FreeRTOS 交叉编译全跑，矩阵条目从 TargetSpec 派生，含工具链镜像与探测缓存（对策 R9）。
- 验收: 五目标在每次提交全绿；新增目标声明后矩阵自动出现该条目（无需改 CI 文件）；矩阵冷启动 ≤ 15 min，命中缓存 ≤ 5 min。

### T-M6-09 · ✅ QEMU 冒烟：QNX 镜像（共享 smoke runner：启动→自检→退出码；Failed{stage,code,hint} 与 result.json/串口归档；无 SDP/mkifs QEMU 时 SKIP-with-print，CI 首夜裁决）
- 角色: infra
- 依赖: T-M6-03, T-M6-08
- 估算: 3d
- 交付物: QNX 镜像的 QEMU 冒烟脚本（启动 → 引擎层自检 → 退出码判定 → 串口日志归档），接入每晚流水线。
- 验收: 冒烟在每晚 CI 通过；失败时输出 `Failed{stage, code, hint}` 且串口日志可定位到具体自检项；`poly retry --from verify` 可独立重跑该阶段。

### T-M6-10 · ✅ QEMU 冒烟：Zephyr（`west build -t run` + 退出码/具名步骤断言；与 T-M6-09 共用同一解析器与归档格式；ARM 与 CI x86 容器均真跑通过——x86 引擎改 i586 + blake3 关 SIMD 后三步 PASS + SMOKE PASS，本机 QEMU ROM 限制已记录）
- 角色: infra
- 依赖: T-M6-06, T-M6-08
- 估算: 2.5d
- 交付物: Zephyr 冒烟脚本（`west build -t run` + 退出码断言），与 T-M6-09 共用日志归档格式。
- 验收: 冒烟在每晚 CI 通过；Zephyr 与 QNX 两条冒烟共用同一份结果解析代码（无第二套判定逻辑）；失败项可映射到自检脚本中的具名步骤。

### T-M6-11 · ✅ Android Automotive 目标（复用 Android 链 + 车厂渠道占位）（AAOS 清单 + aapt2/apksigner 真跑、无第二套 Android 实现；OEM 占位 Publisher 四步报 E_PUB_0006 且零回执不伪造）
- 角色: platforms
- 依赖: —
- 估算: 3d
- 交付物: Android Automotive TargetSpec（`runtime = "native"`、`bridge = "uniffi"`、AAOS 清单变体）与车厂 / OEM 渠道的占位 Publisher。
- 验收: 复用 M2 的 AAPT2 + bundletool + apksigner 链产出 AAOS APK 且 `bundletool validate` 通过（不新增第二套 Android 打包实现）；占位 Publisher 返回「需 OEM 凭据引用」的明确错误而非伪造提交；`notes` 声明「车厂渠道，无统一公开 API」。

### T-M6-12 · ✅ 嵌入式目标状态表（由 TargetSpec 派生）（tools/gen-target-table.mjs 生成四文档标记块 + --check 漂移断言接入 check-docs；仓库无第二份手写表）
- 角色: docs
- 依赖: T-M6-03, T-M6-04, T-M6-05, T-M6-06, T-M6-07, T-M6-11
- 估算: 2.5d
- 交付物: L6 嵌入式 / 车机目标的状态表（运行时、桥、产物、签名、校验、渠道、`experimental`、`notes`），由 TargetSpec 生成并同步进 README / ROADMAP 双语（§11）。
- 验收: 表中每个字段可由 TargetSpec 复现，仓库内无双份手写表；`node tools/render-diagrams.mjs --check` 与文档同步检查在 CI 绿。

### T-M6-13 · ✅ 产物体积预算与实验性标注校验（L6 无 UI 四形态预算真跑 warn/fail + E_BUDGET_0001；experimental 三处一致校验器 TargetSpec × 文档表 × CI 强度，冲突点名三方）
- 角色: core
- 依赖: T-M6-08
- 估算: 2.5d
- 交付物: 无 UI 目标的 `budget.size` 校验（超预算告警）+ `experimental` 字段校验器（保证 TargetSpec、CI 强度、生成的文档表三处一致）。
- 验收: 超预算构建输出告警且不静默通过；标注不一致时 CI 失败并指出冲突三方；对 L6 全部十目标生效。

## M7 穿戴（4 周）
> 前置依赖: M2（Android 打包链、Apple 签名链与「本地 or 远程农场」判定）｜出口条件: Wear OS 有设备冒烟通过；实验目标（watchOS / Garmin）文档标注完备（能力 / 缺口 / 替代方案）

### T-M7-01 · ✅ `@polypack/adapter-wear` 紧凑 tokens（圆 454/方 320 安全区盒模型逐盒断言 + 金样；platformStyle/capabilities；焦点环复用共享内核零新导航；31/31 node）
- 角色: ui-adapters
- 依赖: —
- 估算: 3d
- 交付物: `@polypack/adapter-wear`（紧凑 tokens + 圆形/方形两种安全区），实现 `platformStyle(tokens) -> SkinnedTokens` 与 `capabilities()`（`safeArea` / `gestures`）。
- 验收: 同一页面组件树在圆、方两种 Wear 尺寸下均不越出安全区（断言 + 金样比对）；`capabilities()` 声明缺失能力时交互按契约降级（无指针 → 焦点环路径已有实现则复用，不新写导航系统）。

### T-M7-02 · ✅ Wear OS 目标声明与 Wear 清单（wearos tier6 native/uniffi + `android.hardware.type.watch` 变体 minSdk ≥ 26；复用 Android 链，aapt2 真出 APK、bundletool缺席 SKIP；experimental=false + RN on Wear beta notes）
- 角色: platforms
- 依赖: T-M7-01
- 估算: 2d
- 交付物: Wear OS TargetSpec（`runtime = "native"`、`bridge = "uniffi"`、APK（Wear））与 Wear 清单变体（`android.hardware.type.watch`）；Wear OS 为平台级支持，`experimental = false`，设备侧边界写进 `notes`：「UI 运行时（RN on Wear）为 beta，打包与签名走 Android 基座」。
- 验收: 清单校验器断言 Wear 特性与最小 API 级别；复用 M2 Android 链产出 APK 且 `bundletool validate` 通过；`experimental = false` 且生成的文档表如实呈现该值；`notes` 中 beta 说明进入 README / ROADMAP 的穿戴条目（不再标 ※实验）。

### T-M7-03 · ✅ Wear OS 打包与表单冒烟套件（AndroidWear 复用 aapt2 链真出 APK；ui/smoke/wear input/swipe/back 用例 + 新 `wear-smoke` 工作流 mock/夜跑；缺 KVM/APK 时 SKIP-with-print 且 result.json 如实）
- 角色: platforms
- 依赖: T-M7-02
- 估算: 3d
- 交付物: Wear 打包器（复用 M2 Android 链，不新增 Android 打包实现）+ §8 冒烟层的「Wear 表单」用例集（输入、滑动、返回）。
- 验收: APK 可安装到 Wear 模拟器；表单用例在每晚 CI 全绿；失败时输出可定位到具体表单步骤的报告。

### T-M7-04 · ✅ Wear OS 有设备冒烟（install→launch→forms→uninstall 生命周期 + 截图/logcat/构建摘要归档；新 `wear-release-smoke` 发版门，缺设备/APK 阻断发版；本机无设备 SKIP-with-print，blessed runner 待首跑）
- 角色: infra
- 依赖: T-M7-03
- 估算: 2.5d
- 交付物: 有设备冒烟流水线（安装 → 启动 → 表单用例 → 卸载）与截图/日志归档，发版前必跑。
- 验收: 在真实 Wear 设备或官方模拟器上冒烟通过；归档物含时间戳与构建摘要可追溯；冒烟失败阻断发版（发版流水线依赖该 job）。

### T-M7-05 · ✅ watchOS 伴生模板生成器（SwiftUI，实验）（`poly init --target watchos` 10 文件模板 + 结构断言、凭据零明文；xcodebuild 本机缺席按环境槽 SKIP-with-print）
- 角色: platforms
- 依赖: —
- 估算: 3d
- 交付物: `poly init --target watchos` 生成 SwiftUI 伴生模板（含核心 API 调用桩与 WatchConnectivity 说明），按 §3.7 标注实验。
- 验收: 模板在 macOS 宿主经 `xcodebuild` 构建通过；模板内无明文凭据（secret-scan 绿）；`experimental = true` 进入生成的文档表。

### T-M7-06 · ✅ watchOS 打包与签名链（Watchos 打包器 archive→Payload .app + plutil；非 macOS 宿主 ToolMissing 点名 R3 农场；签名/校验复用 Apple 链，codesign 本机 SKIP）
- 角色: platforms
- 依赖: T-M7-05
- 估算: 2.5d
- 交付物: watch 包打包（Xcode archive）与 codesign 签名链，复用 M2 的 Apple 宿主 / 远程农场判定（R3 路径）。
- 验收: 产出 watch 包且 `codesign --verify` 通过；Linux 宿主经远程农场可出包；签名凭据全程以引用形式出现（仓库无明文）。

### T-M7-07 · ✅ Fitbit OS 打包器（JS SDK）（runtime=none + `.fba` SDK 6.1 结构三层自检 + 金样/确定性；fitbit-build 缺席 SKIP-with-print，notes 明示核心不直接运行）
- 角色: platforms
- 依赖: T-M7-01
- 估算: 2.5d
- 交付物: Fitbit OS TargetSpec（`runtime = "none"`，核心不在设备上运行）与 Fitbit JS SDK 应用包打包器。
- 验收: Fitbit CLI 自检通过；产物结构与金样一致；`notes` 明示「核心不直接运行、能力由 JS 侧实现」并进入文档表。

### T-M7-08 · ✅ Zepp OS 打包器（Zepp JS API）（`.zab` manifest + 内容 md5 命名 zpk + device/app-side zip；金样与二跑一致；zeus/模拟器缺席 SKIP-with-print）
- 角色: platforms
- 依赖: T-M7-01
- 估算: 2.5d
- 交付物: Zepp OS TargetSpec 与 Zepp 小程序打包器（Zepp JS API）。
- 验收: Zepp 开发者工具校验通过；产物可安装到模拟器；契约测试（打包 + 校验 + 结构断言）进 CI。

### T-M7-09 · ✅ Garmin Monkey C 转译 spike（实验）（子集/API 白名单 + 圆/方确定性发射，29/29 node；结论=继续受限档，experimental=true；内建 Rust `iq` 打包器已接线（`crates/poly-bundle/src/packagers/garmin.rs`，monkeyc 环境槽 + `check_garmin_iq_structure` 结构自检），Connect IQ SDK / devkey 缺席时 SKIP-with-print）
- 角色: platforms
- 依赖: T-M7-01
- 估算: 3d
- 交付物: Monkey C 子集转译 spike（限定语言子集 + API 白名单）与能力边界结论（继续 / 降级，处理方式对齐 M4 的 Roku 结论格式）。
- 验收: 子集内至少一个示例产出 `.iq` 包并通过 Connect IQ 编译器；结论写入实验目标文档（能力 / 缺口 / 替代方案）；`experimental = true`。

### T-M7-10 · ✅ 实验目标能力 / 缺口 / 替代方案文档（新增双语 EXPERIMENTAL-TARGETS 章：watchOS W1–W7 / Garmin G1–G10 三段式 + Wear beta 边界；ROADMAP 规划表清理；check-docs / check-experimental 绿）
- 角色: docs
- 依赖: T-M7-04, T-M7-05, T-M7-09
- 估算: 2.5d
- 交付物: watchOS / Garmin 三段式文档：能力、缺口、替代方案，双语同步（§11）；Wear OS 不作为实验目标，仅在同章记录「UI 运行时（RN on Wear）为 beta」的边界说明。
- 验收: 每个实验目标三段齐全且缺口项均有替代方案或明示「无替代」；`experimental` 字段与文档、CI 标注三处一致（Wear OS 记为 `false`，文档中无其 ※实验 标注）；文档同步检查在 CI 绿。

### T-M7-11 · ✅ 穿戴契约测试与金样（统一四段骨架每目标一份 + 复制模板；金样 schema v2 与显式 `POLYPACK_UPDATE_GOLDEN` 门；Wear aapt2 真跑、Fitbit/Zepp 真打包）
- 角色: infra
- 依赖: T-M7-02, T-M7-07, T-M7-08
- 估算: 2.5d
- 交付物: Wear / Fitbit / Zepp 每目标一份契约测试（打包 + 校验器 + 产物结构断言 + 金样比对），按 §8 以 mock 模式每次提交运行。
- 验收: 三目标契约测试每次提交全绿；金样更新必须显式 `--update-golden` 且在 diff 中可见；新增穿戴目标时契约测试模板可直接复制（同一套断言骨架）。

## M8 生态 GA（6 周）
> 前置依赖: M0（TargetSpec schema）、M2（远程农场协议 v0、Apple 签名链）、M6 / M7（L6 目标声明齐备）｜出口条件: 第三方开发者按文档在 1 天内新增一个自定义目标并出包

### T-M8-01 · ✅ `poly-plugin` 沙箱能力模型（wasmtime 默认拒绝 fs/net，目录/域名显式授权 + O_NOFOLLOW；fuel/epoch/内存三道上限；JobSpec.plugins + `plugin_granted` 事件；5 单元 + 16 集成与逃逸负向集全绿，3 个 wasm 夹具无宿主路径/环境/调试符号）
- 角色: core
- 依赖: —
- 估算: 3d
- 交付物: wasmtime 宿主的能力模型落地：默认无网络、无文件系统，读写目录与出网域名按需显式授权（§4.5 capability 模型）。
- 验收: 未授权的网络 / 文件系统访问被拒绝并返回稳定错误码；授权清单进入 JobSpec 且出现在 `poly build --json` 事件流中；沙箱逃逸负向测试集全绿。

### T-M8-02 · ✅ WIT world `polypack-target` 定稿与 `target-spec/v1`（import inputs / export describe / export %package + sandboxed world；非 v1 → E_PLUGIN_0005 + 迁移提示；check-roadmap.sh 与 §4.5 逐行一致 OK；参考插件 describe/package 真跑）
- 角色: core
- 依赖: T-M8-01
- 估算: 3d
- 交付物: `world polypack-target`（`import inputs`、`export describe`、`export package`）定稿 + `target-spec/v1` 版本化方案（破坏性变更走新版本号）+ wit-bindgen 生成的 Rust 侧骨架。
- 验收: 参考插件用该 world 编译、实例化后成功完成 `describe` + `package`；非 v1 版本被拒绝并给出迁移提示；world 定义与 §4.5 逐行一致（脚本比对）。

### T-M8-03 · ✅ 插件 SRI 校验与签名制品（SRI 先行 + GPG 分离签名/一次性 0700 GNUPGHOME + CredentialRef 信任根，验签先于实例化；E_PLUGIN_0009/0010；poly scan 覆盖 wasm 宿主路径/env/调试段且脱敏；8 签名 + 11 scan 测试全绿；宿主 providers 已消费 sri/signature/trust_root）
- 角色: release-sec
- 依赖: T-M8-02
- 估算: 3d
- 交付物: 插件制品的内容摘要（SRI）校验 + 插件发布签名链（密钥以引用形式），secret-scan 规则覆盖插件制品。
- 验收: 摘要不符的插件拒绝加载；签名验证失败拒绝实例化；插件制品不含绝对路径、环境变量与调试符号（§7 脱敏检查通过）。

### T-M8-04 · ✅ 插件模板仓库与脚手架（templates/plugin-target 独立 [workspace] + WIT/componentize/契约测试/自带 CI；poly plugin new/digest；生成→build.sh→沙箱 SRI 加载→describe/package 真跑 14.4min ≤ 30min；模板契约 272s 绿；actionlint 绿；远端模板 CI SKIP-with-print）
- 角色: core
- 依赖: T-M8-02
- 估算: 3d
- 交付物: `poly plugin new <id>` 脚手架 + 独立模板仓库（Rust + WIT 起手式、`describe` / `package` 示例、自带 CI）。
- 验收: 从空仓库到「插件被 `poly` 加载并完成一次打包」≤ 30 min（不含依赖下载）；模板 CI 自带契约测试并在模板仓库内绿。

### T-M8-05 · ✅ 内建目标与插件走同一接口（no_std TargetProvider + ProviderRegistry；BuiltinProvider 委托未改动的 packagers，PluginProvider 桥接 wasm；CLI build/verify/package 统一路径、无 builtin 特判；同一契约测试两类全绿；Tizen 统一入口字节 == 直接打包器，sha256 基线 626f4a59…；L1–L5 打包器源码零改动）
- 角色: core
- 依赖: T-M8-02
- 估算: 3d
- 交付物: 内建目标统一经 `describe` / `package` 接口注册（内建即官方插件），注册表移除旁路分支。
- 验收: 内建目标与第三方插件走同一注册与调用路径（代码中无 `if builtin` 式分支）；同一套契约测试对两类目标全绿；既有 L1–L5 目标的打包结果与改造前字节一致（除签名与时间戳）。

### T-M8-06 · ✅ 远程农场 v1（协议 v1 + FarmPool 并发/worker 池/超时重试，v0 worker 兼容；E_FARM_0004/0005 带 hint）
- 角色: core
- 依赖: —
- 估算: 3d
- 交付物: 农场协议 v0 → v1（并发调度、worker 池、任务超时与重试），JobSpec 序列化保持对 v0 worker 的向后兼容。
- 验收: 多目标并发构建经农场完成；v0 worker 仍可接入（版本协商成功）；任务超时 / 失联时输出带 hint 的 `Failed` 事件。

### T-M8-07 · ✅ 农场隔离与凭据内存注入（0700 一次性工作目录、TaskScope/CredentialScope、结束即销毁；磁盘/日志零明文断言；E_FARM_0006）
- 角色: release-sec
- 依赖: T-M8-06
- 估算: 3d
- 交付物: worker 侧隔离（容器 / 进程边界 + 一次性工作目录）与凭据只进内存的下发通道（§7）。
- 验收: worker 磁盘与日志中不存在明文凭据（扫描断言）；任务结束即销毁凭据与工作目录；跨任务无残留（隔离负向测试集全绿）。

### T-M8-08 · ✅ 农场缓存共享（DirSharedStore 共享目录 CAS + FallbackStore 降级；跨 worker 同键、二跑命中 100%；无对象存储时本地降级告警不红）
- 角色: infra
- 依赖: T-M8-06
- 估算: 3d
- 交付物: `poly-cache` 对象存储后端在生产形态落地，多 worker 共享 CAS，实现 §5.2 的三级缓存跨机复用。
- 验收: 农场内二跑缓存命中 ≥ 90%（对齐 M1 验收口径）；同输入在不同 worker 上产生同一缓存键；后端仍为 trait 且本地文件系统默认可用（无对象存储时农场降级而非失败）。

### T-M8-09 · ✅ 远程农场部署指南（双语 FARM-DEPLOY + farm-drill.sh Linux 双 worker 演练；并发/共享缓存二跑命中/凭据内存注入/降级；macOS IPA R3 槽位 SKIP-with-print）
- 角色: docs
- 依赖: T-M8-06, T-M8-07, T-M8-08
- 估算: 2.5d
- 交付物: 远程农场部署指南（拓扑、worker 池、隔离参数、缓存后端、密钥库接入），双语。
- 验收: 照指南从零部署一套 macOS worker 池，并从 Linux 宿主完成一次 IPA 构建（R3 路径）；指南命令逐条可复制执行且全部出现在 CI 演练日志中。

### T-M8-10 · ✅ 商店发布流水线 GA（指数退避 + Idempotency-Key + SubmitJournal，85009=accepted；PublishFailure / Failed{stage,code,hint}；503/429/超时/accept-then-timeout 注入后最终成功且无重复提交）
- 角色: core
- 依赖: —
- 估算: 3d
- 交付物: `publish` 阶段成熟化：指数退避重试、幂等提交（重复提审不产生二次提交）、平台限流处理（对策 R4）。
- 验收: 注入 5xx / 超时 / 限流故障后提交最终成功且无重复提交记录；不可恢复失败输出 `Failed{stage, code, hint}` 且机器可读；自动化止步于「提审」，不承诺自动过审。

### T-M8-11 · ✅ 提审回执归档与轮询（receipt v2 + Published{submission} + poly retry --from publish；崩溃窗口恢复零重复提交；提交号规范化）
- 角色: release-sec
- 依赖: T-M8-10
- 估算: 2.5d
- 交付物: 提审回执归档（§5.2 提审级缓存）与审核状态轮询，落地 `Published{channel, submission}` 事件。
- 验收: 轮询到终态后回执落盘且含提交号与时间戳、可追溯；中断后 `poly retry --from publish` 续跑不产生重复提交。

### T-M8-12 · ✅ 49 目标现状表（gen-status-table 从 TargetSpec 生成 49 行，双语 README/ROADMAP 同步；L1–L7=7/9/9/4/7/10/3 与平台矩阵一致；--check 漂移门禁接入 docs-check）
- 角色: docs
- 依赖: —
- 估算: 3d
- 交付物: 由 TargetSpec 生成的 49 目标现状表（等级、运行时、桥、产物、签名、校验、渠道、`experimental`、`notes`），同步进 README / ROADMAP 双语。
- 验收: 表格与 TargetSpec 逐字段一致（生成式，无双份手写）；平台矩阵增删时 CI 依 §11 第 4 条提示待更新项；49 行完整且 L1–L7 分级计数与 README 相符。

### T-M8-13 · ✅ GA 文档、技术审查与「1 天新增目标」演练（双语 [GA 文档](GA.md) 7 节 + 技术审查；独立上下文开发者代理演练 pocket-notes：总墙钟 38min（冷编译 30min、开发 2min），脚手架→自定义出包 12m23s，`poly build --target` 真实加载+出包 92B，SRI 负向 E_PLUGIN_0006；卡点回写并修复模板契约单一来源/SRI 示例/最小宿主文档；docs-check 七对全绿）
- 角色: docs
- 依赖: T-M8-02, T-M8-04, T-M8-05, T-M8-09, T-M8-12
- 估算: 3d
- 交付物: GA 文档集（双语）+ 技术审查报告 + 由一名未参与本项目的开发者执行的「新增自定义目标并出包」演练记录。
- 验收: 演练全程 ≤ 1 天（含脚手架、`describe` / `package` 实现、出包与校验）；演练卡点全部回写文档或修复并复测；GA 文档同步检查绿，49 目标现状表与 TargetSpec 一致。

<!-- 统计: M6=13任务/36d, M7=11任务/29d, M8=13任务/38d -->

## M9 立项与交付状态

> 详细立项、方案、风险与逐任务验收见 [INITIATIVES.md](INITIATIVES.md)。55 个候选任务（T-M9-01..T-M9-55，粗估 226 人日）中，E1–E7、P1/P2/P3、G1/G5 等已交付或收口；T-M9-21 / T-M9-49 / T-M9-51 与 crates.io 发布等仍为外部项。

| 项目 | 内容 | 候选任务 |
|---|---|---|
| M9-E1 | SBOM / 构建溯源（已交付；SPDX / CycloneDX / in-toto + 双语 SBOM 文档 / 样例） | T-M9-01..03 |
| M9-E2 | monorepo / 多应用工作区（已交付；ADR 0008 / WORKSPACE / 示例脚本） | T-M9-04..06 |
| M9-E3 | 增量发布 `poly release --changed`（已交付；5 目标节时 73–81%） | T-M9-07..09 |
| M9-E4 | 生产热更 / OTA 通道（T-M9-10/11/12 已交付；run 39395255929 windows-updater-real 真实更新 / 回滚通过） | T-M9-10..12 |
| M9-E5 | 商店元数据管线（已交付；schema / 校验 / 确定性摘要 / 作者指南） | T-M9-13..15 |
| M9-E6 | GUI 目标矩阵视图 / 构建历史（已交付；`app/` Tauri v2；真实窗口环境门控） | T-M9-16..18 |
| M9-E7 | 发布前设备预览矩阵（T-M9-19/20 已交付；T-M9-21 外部） | T-M9-19..21 |
| M9-P1 | Google Play Instant（已停用 retired；保留行，ADR 0006） | T-M9-22..23 |
| M9-P2 | Snapchat Mini（已关闭；季度复审 2027-01-20，ADR 0006） | T-M9-24..25 |
| M9-P3 | LINE Mini App（已交付：部署模式 `poly deploy line-liff`，ADR 0009） | T-M9-26..27 |
| M9-P4 | Sailfish OS（条件支持，需赞助 / 设备承诺，ADR 0006） | T-M9-28..30 |
| M9-P5 | Ubuntu Touch（条件支持，需赞助 / 设备承诺，ADR 0006） | T-M9-31..33 |
| M9-P6 | PlayStation（NDA 门控，ADR 0006） | T-M9-34..35 |
| M9-P7 | Xbox（NDA 门控，ADR 0006） | T-M9-36..37 |
| M9-P8 | Nintendo Switch（NDA 门控，ADR 0006） | T-M9-38..39 |
| M9-D1 | 插件市场分发决策（已决策 ADR 0004；签名索引实现与生产密钥仪式外部） | T-M9-40..42 |
| M9-D2 | 远程农场托管决策（已决策 ADR 0005；自托管硬化清单待补） | T-M9-43..44 |
| M9-G1 | CLI 分发（GitHub Releases 已交付；T-M9-45 审计已落地，crates.io 发布因名称占用 / publish=false 关闭） | T-M9-45..47 |
| M9-G2 | 人类外部开发者试用协议（协议已交付；T-M9-49 真人试用外部） | T-M9-48..49 |
| M9-G3 | 环境槽收口计划（owner / validator 已交付；T-M9-51 其余真实槽外部） | T-M9-50..52 |
| M9-G4 | MSI 真样本启动测试（已交付；run 39308849058 windows-msi-real 通过） | T-M9-53..54 |
| M9-G5 | 分支保护管理员绕过（已交付；ENV-18 CLOSED） | T-M9-55 |

> **Wave 2 进度（2026-10-20）**：M9-E2 已交付（`[workspace]`、`--workspace` / `--app`、缓存 / 产物隔离、ADR 0008、WORKSPACE 双语文档与示例脚本）；M9-E4 已交付 T-M9-10/11（机制矩阵 + 签名清单 + `poly update`），T-M9-12 已交付演练装置（`examples/updater-app` + `scripts/drill.ps1`）与 rust-ci `windows-updater-real` 作业，windows-2022 run 39395255929 首跑通过。平台决策已收敛：M9-P1 标 retired、M9-P2 关闭并季度复审（2027-01-20）、M9-P3 交付 `poly deploy line-liff` 部署模式（ADR 0009）、M9-P4/P5 条件支持、M9-P6–P8 NDA 门控；M9-G5 已交付（ENV-18 `CLOSED`，`enforce_admins=true` + 9 个 required contexts，2026-10-20 核实）。
>
> **M9 收尾进度（2026-10-20）**：E3 T-M9-09（5 目标后端调用 5→1，墙钟 73–81%）、E4 T-M9-12 演练装置与 CI（`examples/updater-app`；windows-2022 run 39395255929 通过）、E6 GUI（`app/` Tauri v2 矩阵 / 详情）、E7 T-M9-19/20（设备农场矩阵 + `poly preview --dry-run`；ENV-20 真机外部）、G1 T-M9-45（workspace 1.13.1、元数据、`cargo package --list` 9/9；`poly-cli` / `poly-core` 名称占用与 `publish=false` 关闭发布）、`poly retry|verify --workspace/--app`、`publish.rs` 拆分为子模块、P3 LINE 部署模式、G5 已关闭、P1/P2/P4–P8 决策收敛。T-M9-21 / T-M9-49 / T-M9-51 与 crates.io 实际发布仍为外部项。
>
> **Wave 1 进度（2026-10-20）**：M9-E1（SBOM + `poly verify --sbom` + 回执 `sbom_digest`）、M9-E3（`--changed` + 5 目标实测，见 RELEASE-CHANGED）、M9-E5（store metadata）、M9-E6 历史与 GUI（T-M9-16/17 + T-M9-18 `app/`；真实窗口环境门控）、M9-G1（install script / 签名 / install-smoke；T-M9-45 审计已落地，crates.io 发布关闭）、M9-G2（协议；真人 T-M9-49 外部）、M9-G3（owner / validator；T-M9-51 外部）、M9-G4（样例 / CI；Windows 首跑已通过，ENV-01 CLOSED）、M9-D1/D2 决策已记录；平台决策与 G5 见 Wave 2 进度。外部项不标 ✅。
