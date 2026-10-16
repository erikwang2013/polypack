# Changelog

本仓库的显著变更记录于此。格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。
版本号自 v1.0.0 起按语义化版本递增（tag 与 GitHub Release 名称一致，见 [README](README.md#路线图)）。
Notable changes are recorded here; since v1.0.0, tags and releases follow `vX.Y.Z` semantic increments.

## [Unreleased]

## [1.5.0] — 2026-10-17

### Added
- **M3 鸿蒙与小程序交付**（2026-10-17）：RNOH 宿主模板 + NAPI 桥出口（`poly bridge ohos`——OHOS aarch64 真交叉编译自包含 `libpax.so` 6.2MB + node 真装载对拍同字面量）与 RNOH 承载分支（双宿主结构快照逐字节一致）；HAP 打包（hvigorw 真跑，增量三跑字节一致）与 hap-sign-tool 签名/校验（凭据全引用、口令经 argv 全链脱敏）；adapter-taro v0 + Taro 四端管线（weapp / alipay / tt 代码包 + 快应用真 build 出 `.rpk`；真编译 + 结构断言 + 金样 + 二跑缓存命中）；wasm 沙箱出口（`poly bridge wasm`：生成期/加载期两道白名单闸 + loader 适配层随包）与 wasm-opt 裁剪（−51%）+ `[budget]` 体积门禁（warn/fail 双态入 DAG）；三平台 publish 客户端（微信 B 协议 + 支付宝网关 v1 + 抖音，四步零分叉驱动 / 断点续走 / 回执永久归档）与单文件载具 + quickapp 渠道备位 + `poly publish` 提审薄层（真 rpk dry-run 幂等）；小程序契约测试（devtools CLI 校验腿）与 `miniprogram-contract` workflow（mock 每提交 / real 每夜 18:47 错峰）。**终版全量验证（fmt / clippy `-D warnings` / workspace 测试）全绿；六工作流三宿主全绿。**

*M3 HarmonyOS & mini-program delivery — RNOH host + NAPI outlet, HAP packager + hap-sign-tool chain, adapter-taro + four-target Taro pipeline (incl. real .rpk), wasm sandbox outlet with a two-gate allowlist + wasm-opt trimming (−51%) + the `[budget]` gate, three publish clients on a zero-fork driver with a single-file carrier, devtools contract tests + the sixth workflow. Full-battery verification and all six workflows green on three hosts.*

### Fixed
- **CI 逐环修复与如实更正**（2026-10-16/17）：devtools 契约登录态路径断言兼容 Windows Debug 转义（windows-2022 实证）；`cargo-deny` 与 `cargo-audit` 双配置豁免 RUSTSEC-2023-0071（rsa Marvin 计时侧信道——上游无修复版、仅本地签名无解密面；audit 腿此前被 deny 挡在前面掩盖）；`probes` 计时用例边距加固（重载假红实证）；`bridge-real` NDK 安装 retry 与留产物空转修复（RAII 先删 → `POLY_KEEP_TEMP` 通道；非空回执后翻 `error`）；清单统一（poly-publish / base64 进根 `workspace.dependencies`）。更正：v1.4.0 记「`schedule` 触发历史为 0」实为观察窗口过短——首夜即已触发（best-effort 延迟约 3 小时），mobile-contract 真实模式经 schedule 全绿。

*CI hardening ring by ring (Windows path assertion, dual-config RUSTSEC-2023-0071 exemption, probes timing margins, NDK retry + artifact keep-temp); manifest unification; and an honest correction — the `schedule` trigger does fire (fired ~3 h late on its first night).*

## [1.4.0] — 2026-10-16

### Added
- **M2 收口：设备走查闭环 + sign/verify 入 DAG**（2026-10-16）：RN 宿主设备走查真跑（emulator）——六走查 touch 6/6、none 6/6（焦点序列与计数逐条命中 `expectations.json`）+ 焦点环差分像素采样双态实证（T-M2-04/05 升 ✅；自带漂移哨兵与 adb 驱动口径）；`poly build` 内联 sign 阶段（凭据门控三级语义：未声明静默 / 环境缺→告警跳过 / 非法→硬失败）与 verify 校验器注册表入 DAG（T-M2-10「verify 阶段接入」落地）；真签实证（Secret Service 一次性密钥 → apksigner v2+v3 独立复核，钥匙库事后清空）；T-M2-11 双真环境闭环（真 Vault dev KV v2 读写往返——audit 日志 token 仅 hmac；真 keyring 隔离总线——Deepin 补丁版根因实证）；真生成器 × Apple 编译首证（真 uniffi 产物 + 真调用面过 `swiftc -typecheck`，macos-14 真模式）与 simulator 调用对拍（`simctl` 真跑 `paxAdd(-5,3)=-2` 与 i32 回绕）；windows msi / macos dmg 真检查器落地（MSI 真机执行首次全通：windows-2022 真跑 `candle`+`light`+`msiexec` 载荷字节核对）。**工作区 384 测试全绿（另 ui/ 42）；五工作流三宿主绿；真实模式经手动 dispatch 全绿实证（mobile-contract 双新腿 + bridge 全链）。**

*M2 closure — device walks proven on emulator (6/6 in both forms, plus ring pixel evidence), sign/verify stages wired into the DAG with real apksigner evidence, Vault/keyring real closures, first real proof of generator × Apple compile and simulator call parity, MSI/DMG real checkers with MSI real execution green on windows-2022. All workflows green; real mode evidenced via manual dispatch.*

### Fixed
- **首夜真跑根因三修**（2026-10-16）：msi 链接 LGHT0311（WixUI 本地化 wxl 钉死 DB 码页 1252 与中文描述冲突——UI 腿退出，`Product/@Codepage=65001` 保全任意 Unicode；`exec` 失败证据补 stdout 回落）；Swift 调用面文件须为 `main.swift`（多文件编译顶层表达式规则——两处同修）；`bridge.yml` 的 `setup-android` v3→v4（v3 默认包含已下架死包 `tools`——两次真派实证，对齐 mobile-contract 同款修复）。
- **如实发现**：`schedule` 事件历史触发数为 0——两个 workflow 的「每夜真跑」此前从未发生（原生 API 实证；仓库/YAML/账号侧均已排查，平台侧成因待查）；真实模式今后以手动 dispatch 达成，台账已注记。

*First-real-run root causes fixed (WiX code page LGHT0311, swiftc `main.swift` rule, setup-android v3→v4); noted honestly: the `schedule` trigger has never fired — real mode is now evidenced via manual dispatch.*

## [1.3.0] — 2026-10-15

### Added
- **M2 移动与桥生态交付**（2026-10-15）：uniffi 桥生成器（UDL 单源 → Kotlin/Swift 绑定、接口摘要入缓存键、`poly bridge`）；Kotlin/JNI AAR（双 ABI 真交叉编译，emulator 三层实证：loadLibrary + dlsym 真调用 + Kotlin→JNA→Rust 全链）；Swift XCFramework + iOS IPA 链（macos-14 真跑直出未签名 IPA 并留档）；Android 链（AAPT2 免 Gradle 直出 AAB/APK、bundletool 校验、emulator 冷启动冒烟、跨绝对目录逐字节可复现）；apksigner 签名（v2+v3 真验、临时密钥零残留）与凭据层（keyring/Vault/Env + 全通道脱敏）；远程农场协议 v0（发起地/构建地分离、共享对象存储跨构建点缓存复用、`E_FARM_0002` 准入语义）；secret-scan 双检（全历史 823 文件零命中 + CI 三宿主）；`poly verify --reproducible` / `poly retry --from sign`；adapter-native v0（RN 宿主 + 焦点基线）；移动契约 CI（mobile-contract：mock 每提交 + nightly 真列并留档）与 bridge 真跑 nightly；FARM 双语文档。**355 测试、五工作流三宿主全绿、真模式两腿全绿（IPA/AAB 留档）。**

*M2 mobile & bridge ecosystem delivered — uniffi bridge, AAR, Android AAPT2 chain, iOS IPA chain, signing & credentials, farm v0, secret-scan, verify/retry, adapter-native, mobile-contract CI. 355 tests; five workflows green on three hosts; real-mode dispatch green with archived unsigned IPA and AAB.*

### Fixed
- **CI 三宿主全绿收敛**（2026-10-15）：测试与宿主工具解耦（夹具入库 + 显式 SKIP 体系）、`.gitattributes` 强制 LF 检出、显式 linux 目标整文件门控、`EXE_SUFFIX`/路径分隔符/`getopt` 参数序等跨宿主修复——**windows 腿首次全绿**；随后真模式七轮 dispatch 逐层根因（SDK 死包 `tools`、`lipo` 输出目录执行序、上传基准 `$TMPDIR`、`exportArchive` 无 Team 语义、骨架绑定引用写死、取件竞态）全部修复并经真机判决，含三处"静默假绿"通道的点亮（aapt2 `warn:` 前缀、SKIP 掩盖失败、错误显示丢 stderr）。

*Three-host CI convergence (windows leg green for the first time) and real-mode hardening: every root cause fixed with local reproduction — including three silent-false-green channels lit up.*

## [1.2.0] — 2026-10-15

### Added
- **M1 桌面与 Web 交付**（2026-10-15）：`poly init` 示例应用脚手架（React 19 + vite 7 真构建）；`poly build` 多端构建（真 triple 探测、内容寻址缓存 **二跑 100% 命中**、DAG 双支线并行）与**真实打包**（Linux 真出 deb（含 Pax 派生六尺寸图标）/ AppDir / Web 静态包 + SRI 清单）；`poly dev` 整页重载热预览（Chrome 真浏览器证据）；`poly clean` 与保留策略（LRU）；五态 CLI 表情（Pax 情绪映射）；英文版图示合入 README.en。**200 测试全绿、clippy `-D warnings` 零告警。**

*M1 Desktop & Web delivered: poly init/build/dev/clean, the five-packager matrix, and four-target contracts — 200 tests green.*

## [1.1.0] — 2026-10-15

### Added
- **M0 地基交付**（2026-10-15）：9 crate Rust 工作区（依赖方向约束测试）；`poly doctor`（6 探测器 + L1 覆盖矩阵，人读与 `--json`）；`polypack.toml` schema（serde + JSON Schema 导出，严格模式）与 TargetSpec 注册表（L1 七目标）；诊断框架（稳定错误码 + BuildEvent 事件流）；三宿主 CI 工作流（远端执行待账单恢复）；供应链检查（cargo-deny 四项通过 + cargo-audit 0 漏洞）。**56 测试全绿、clippy `-D warnings` 零告警。**
- Pax 单形态动效：五种表情各自带专属动态（哭=泪滴下落、惊恐=发抖加汗滴、疑惑=问号浮动、眨眼=睁闭眼循环、笑=浮动），联系表逐格动画且相位错开
- Pax 全身化：联系表改为全身像（六腿 + 包裹 + 双钳）；`pax-mark.svg` 徽章补蟹腿；图示右下角角标同步更新
- 校验器前置交付：`tools/render-diagrams.mjs --check`（图示漂移）、`tools/check-docs.mjs`（结构级双语校验）与最小 CI（`.github/workflows/docs-check.yml`）
- 规划缺口审计落地：供应链检查（T-M0-11）、Pax 品牌落地任务（T-M1-14）、`--target all` 语义、版本策略（§4.6）、构建时限预算（§5.4）、清理与保留策略（§5.5 / T-M1-15）、扩展方向孵化池（§13，E1–E8）
- `LICENSE`（Apache-2.0）、`SECURITY.md`

*Per-form motion for Pax, full-body expression sheet, drift checkers + minimal CI, license and security policy.*

## [1.0.0] — 2026-10-14

### Added
- 双语 README（简介 / 50+ 平台矩阵 / UI 组件策略 / Pax 品牌约定）
- [`docs/ROADMAP.md`](docs/ROADMAP.md)：L1–L7 分级策略、49 平台逐项工程明细、M0–M8 验收标准、风险登记册
- [`docs/TASKS.md`](docs/TASKS.md)：113 任务 / 279.5 人日（角色 / 依赖 / 估算 / 交付物 / 验收）
- 四张架构类 SVG（架构 / 请求周期 / 功能 / 生命周期）+ Pax 六形态资产（动画版 / 五表情 / 图标 / 联系表）
- 两条零依赖流水线：`tools/render-diagrams.mjs`（JSON IR → SVG，含 Pax 角标）、`tools/build-pax.mjs`（Pax 全形态，`--check` 防漂移）

*Initial release — documentation & planning baseline.*
