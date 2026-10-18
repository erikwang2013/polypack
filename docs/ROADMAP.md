# PolyPack 详细规划

> 本文件是 PolyPack 的工程规划事实源（README 是其摘要）：愿景、设计原则、逐平台工程明细、架构与数据模型、流水线与缓存、安全、测试与 CI、里程碑任务分解、风险登记册。
> 英文版见 [ROADMAP.en.md](ROADMAP.en.md)；平台总览与图示见 [README](../README.md#支持平台矩阵)。

- 项目状态：**规划阶段（文档基线 v1.0.0）**
- 规划日期：2026-10-14；修订：2026-10-14（逐平台明细；缺口审计与扩展立项）
- 命名：PolyPack（CLI 命令 `poly`）；项目宠物：邮差蟹 Pax
- 排期假设：1–2 名熟悉 Rust 的工程师（里程碑估算按 1.5 FTE 计）

---

## 1. 愿景与目标

**一句话**：把「多端发布」从 N 套工具链、N 个 IDE、N 份打包脚本，收敛为一条 `poly` 命令。

**成功的样子（可度量）**：

1. 一个团队用一份 Rust 核心 + 一份 React 界面，在一个 afternoon 内把应用同时发到 Android / iOS / Web / 微信小程序；
2. **新增目标平台 ≤ 2 人日**：1 个 TargetSpec（声明式）+ 1 个适配器插件（可选 WASM）；
3. **可复现**：同样输入 → 字节级一致产物（签名与时间戳除外），二跑缓存命中率 ≥ 90%；
4. **可诊断**：构建失败时输出「哪一步、为什么、怎么修」，机器可读（JSON）+ 人可读（终端报告）；
5. 支持矩阵的每目标契约测试在 CI 全绿（含金样产物比对）。

## 2. 产品边界与设计原则

**非目标（Non-goals）**：

- 不做应用框架（状态管理、路由、数据层由用户自选，PolyPack 只做打包与交付）；
- 不托管用户代码与密钥（构建可本地或自建农场，密钥只以引用形式出现）；
- 不支持未获厂商授权的侧载（主机平台尤其）；
- 不做应用商店本身，只做商店的自动化客户端。

**设计原则**：

1. **单源真值**——目标能力由 TargetSpec 声明，文档、CLI、校验器全部从它派生；
2. **密钥零入库**——仓库中只存在凭据引用；出现明文凭据即构建失败；
3. **目标即声明**——新增平台不允许改核心代码路径（只允许新增声明与插件）；
4. **预算式构建**——每个目标有产物预算（体积、启动时延），超预算告警；
5. **诚实分级**——不支持/实验性/受限，如实标注，不夸大能力边界；
6. **插件优先**——内建目标只是「官方插件」，第三方插件与内建走同一接口；
7. **统一形象**——Pax 是项目的唯一图标与吉祥物：凡出现「项目图标」之处（文档 favicon、生成图示的右下角角标、未来的 CLI 启动图 / GUI 应用图标 / 安装包图标）一律使用 Pax（`docs/assets/pax.svg` / `pax-mark.svg`），不引入通用占位图标；五种表情映射构建状态（成功 = 笑、失败 = 哭、异常 = 惊恐、待确认 = 疑惑、空闲 = 眨眼）。生成图片资产时默认在右下角盖 Pax 微缩角标（`--no-brand` 关闭）；未来产品侧同理——PolyPack 为商店预览图等生成图片时，在右下角标注**对应目标平台**的微缩图标（可开关，默认开启）。（落地任务：T-M1-14）

## 3. 支持矩阵与逐平台工程明细

### 3.1 分级总览

| 等级 | 平台数 | 范围 | 统一工程策略 |
|---|---|---|---|
| L1 一等公民 | 7 | Android / iOS / HarmonyOS / Windows / macOS / Linux / Web | 原生库或 wasm + RN / react-dom，全链路签名与发布 |
| L2 小程序与即时应用 | 9 | 微信 / 支付宝 / 抖音 / 快应用 / TikTok / Play Instant / FB / Snapchat / LINE | wasm 进宿主沙箱（或 HTML5），Taro / web 打包 |
| L3 电视与大屏 | 9 | Android TV / tvOS / 鸿蒙 TV / Tizen / webOS / Roku / Fire TV / Vizio / VIDAA | Android 系复用 RN；web 系打 wgt/ipk；Roku 受限渲染 |
| L4 移动系统变体 | 4 | GrapheneOS / KaiOS / Sailfish / Ubuntu Touch | Android 变体 / web 栈 / Qt 或 WebView 壳 |
| L5 桌面发行版与 BSD | 7 | Ubuntu / Fedora / Mint / ChromeOS / FreeBSD / OpenBSD / BSD 家族 | 发行版打包器矩阵 + 原生构建 |
| L6 穿戴 / 车机 / 嵌入式 | 10 | watchOS / Wear / Auto / QNX / AUTOSAR / VxWorks / RTOS / Fitbit / Garmin / Zepp | 多数为 headless 核心库（no_std），UI 可选 |
| L7 主机平台 | 3 | PlayStation / Xbox / Switch | 厂商授权 SDK 插件，SDK 不入库 |

### 3.1.1 49 目标现状表（生成）

下表是 49 个平台现状的单一事实源：36 行由 `TargetSpec` 逐字段生成（38 个已声明目标；RTOS、Chrome OS 行各合并两个），13 行规划平台显式标注「规划中」，字段来自 `tools/gen-status-table.mjs` 的声明清单。请勿手改：`node tools/gen-status-table.mjs` 生成；`--check` 在漂移时失败。平台矩阵增删按第 11 节第 4 条同步。

<!-- BEGIN GENERATED: status-49 -->
| 等级 | 平台 | 运行时 | 桥 | 打包器与产物 | 签名 | 产物校验 | 发布渠道 | experimental | notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| L1 | Android（`android`） | `native` | `uniffi` | `aab` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `google-play` | `false` | — |
| L1 | iOS（`ios`） | `native` | `uniffi` | `ipa` · `ios-xcodebuild` | `apple-codesign` | `codesign-verify`, `spctl` | `app-store-connect` | `false` | 需 macOS 宿主 |
| L1 | HarmonyOS（`harmonyos`） | `native` | `napi` | `hap` · `harmonyos-hvigor` | `harmonyos-hap-sign` | `hap-sign-verify`, `hap-structure-check` | `appgallery-connect` | `false` | RNOH 承载 UI |
| L1 | Windows（`windows`） | `native` | `tauri-ipc` | `msi` · `windows-wix` | `windows-authenticode` | `signtool-verify` | `ms-store` | `false` | WebView2 运行时 |
| L1 | macOS（`macos`） | `native` | `tauri-ipc` | `dmg` · `macos-dmg` | `apple-codesign` | `codesign-verify`, `stapler-validate` | `app-store`, `self-hosted` | `false` | 公证必须 |
| L1 | Linux（`linux`） | `native` | `tauri-ipc` | `appimage` · `linux-appimage` | `linux-repo-gpg` | `lintian`, `rpmlint`, `appimage-structure`, `appstream-validate` | `flathub`, `snapcraft`, `self-hosted` | `false` | — |
| L1 | Web（`web`） | `wasm` | `wasm-bindgen` | `static-site` · `web-static` | — | `html-validate`, `sri-check` | `static-hosting` | `false` | — |
| L2 | 微信小程序（`wechat`） | `wasm` | `wasm-bindgen` | `weapp` · `taro-weapp` | — | `wechat-devtools-cli` | `wechat-open-platform` | `false` | 小程序代码包（Taro 编译）；wasm 进宿主沙箱（T-M3-07） |
| L2 | 支付宝小程序（`alipay`） | `wasm` | `wasm-bindgen` | `alipay` · `taro-alipay` | — | `alipay-devtools-cli` | `alipay-open-platform` | `false` | 小程序代码包（Taro 编译）；wasm 进宿主沙箱（T-M3-07） |
| L2 | 抖音小程序（`douyin`） | `wasm` | `wasm-bindgen` | `tt` · `taro-tt` | — | `douyin-devtools-cli` | `douyin-open-platform` | `false` | 小程序代码包（Taro 编译，平台 `tt`）；wasm 进宿主沙箱（T-M3-07） |
| L2 | 快应用（`quickapp`） | `wasm` | `wasm-bindgen` | `rpk` · `hap-toolkit` | — | `hap-toolkit-lint` | `quickapp-union-store` | `false` | 快应用 `.rpk`（hap-toolkit 构建）；lint 入 verify、提审走联盟商店（T-M3-12） |
| L2 | TikTok Minis（规划中） | HTML5 + Minis 桥 | HTML5 | zip | 平台侧 | 平台沙箱自检 | TikTok 平台 | — | 规划中：尚未声明 TargetSpec；经 HTML5 + Minis 桥接入，zip 由平台侧审核。 |
| L2 | Google Play Instant（规划中） | native | uniffi | instant-aab · android-aapt2 | android-keystore | bundletool-validate | google-play | — | 规划中：复用 Android 基座（native · uniffi），产物为 Instant-enabled AAB；尚未声明 TargetSpec。 |
| L2 | Facebook Instant Games（规划中） | HTML5 | HTML5 | zip | 无 | Meta 沙箱自检 | Meta 平台 | — | 规划中：HTML5 运行时，Meta 沙箱自检；尚未声明 TargetSpec。 |
| L2 | Snapchat Mini（规划中） | HTML5 | HTML5 | zip | 无 | Snap 沙箱自检 | Snap 平台 | — | 规划中：HTML5 运行时，Snap 沙箱自检；尚未声明 TargetSpec。 |
| L2 | LINE Mini App（规划中） | LIFF（web） | HTTPS | 静态包 + LIFF 配置 | HTTPS / 平台 TLS | LIFF 配置检查 | LINE 平台 | — | 规划中：LIFF（web）承载，HTTPS / 平台 TLS；尚未声明 TargetSpec。 |
| L3 | Android TV（`android-tv`） | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `google-play-tv` | `false` | Leanback 清单（android.software.leanback） |
| L3 | Apple tvOS（`tvos`） | `native` | `uniffi` | `ipa` · `tvos-xcodebuild` | `apple-codesign` | `codesign-verify`, `spctl` | `app-store-connect` | `false` | RN-tvOS 承载 UI；需 macOS 宿主 |
| L3 | HarmonyOS TV（规划中） | native | napi | hap · harmonyos-hvigor | harmonyos-hap-sign | hap-sign-verify, hap-structure-check | 华为视频商店 | — | 规划中：复用 HarmonyOS 基座（native · napi / RNOH）；尚未声明 TargetSpec。 |
| L3 | Samsung Tizen（TV）（`tizen-tv`） | `wasm` | `wasm-bindgen` | `wgt` · `tizen-wgt` | `tizen-author` | `wgt-lint` | `tizen-seller-office` | `false` | web 运行时（Tizen TV）；作者证书签名（凭据引用 tizen-author）与侧载安装归三星工具链（本机无 tizen CLI，SDK 校验器缺席时如实跳过） |
| L3 | LG webOS（`webos-tv`） | `wasm` | `wasm-bindgen` | `ipk` · `webos-ares` | — | `ares-package` | `lg-content-store` | `false` | web 运行时（LG webOS）；签名按平台侧（LG Content Store 发布签名），本地不签名（本机无 ares-cli，SDK 校验器缺席时如实跳过） |
| L3 | Roku OS（`roku`） | `none` | `none` | `zip` · `roku-channel` | — | `sideload-structure-check` | `roku-channel-store` | `true` | 实验：SceneGraph 子集受限渲染——仅 UI 子集 + 原生逻辑（BrightScript 重写，无 JS 运行时）；侧载结构自检可用，真机实证待补（T-M4-15 触发前置；T-M4-12 决策：继续受限档） |
| L3 | Amazon Fire TV（`fire-tv`） | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `amazon-appstore` | `false` | 复用 Android TV 同一 APK 链，渠道指向亚马逊应用商店 |
| L3 | Vizio SmartCast（`smartcast-tv`） | `wasm` | `wasm-bindgen` | `web-app` · `smartcast-web-app` | — | `sandbox-selfcheck` | `vizio-platform` | `false` | web 运行时（Vizio SmartCast，远程托管）；无签名、无推送——只出沙箱自检包（入口 / 清单字段 / 体积），提报与安装归平台侧 |
| L3 | Hisense VIDAA（`vidaa-tv`） | `wasm` | `wasm-bindgen` | `web-app` · `vidaa-web-app` | — | `sandbox-selfcheck` | `vidaa-store` | `false` | web 运行时（海信 VIDAA）；无签名、无推送——只出沙箱自检包（入口 / 清单字段 / 体积），提审与安装归平台商店侧 |
| L4 | GrapheneOS（规划中） | native | uniffi | apk · android-aapt2 | apksigner / 社区密钥 | apksigner-verify + 可复现比对 | F-Droid / 自托管 | — | 规划中：Android 构建变体（无 GMS flavor），APK 自签名 / F-Droid；尚未声明 TargetSpec。 |
| L4 | KaiOS（规划中） | wasm | wasm-bindgen | zip + manifest | 平台侧 | 开发者工具自检 | KaiStore | — | 规划中：react-dom web 栈（wasm · wasm-bindgen），zip + manifest 投 KaiStore；尚未声明 TargetSpec。 |
| L4 | Sailfish OS（规划中） | native | cffi | rpm | RPM GPG | rpmlint | Jolla Store | — | 规划中：native .so + Qt / Silica 壳，RPM 投 Jolla Store；尚未声明 TargetSpec。 |
| L4 | Ubuntu Touch（规划中） | native | cffi | .click（Clickable） | Clickable 打包 | Clickable 构建检查 | OpenStore | — | 规划中：native .so + WebView 壳，.click（Clickable）投 OpenStore；尚未声明 TargetSpec。 |
| L5 | Ubuntu（`ubuntu`） | `native` | `tauri-ipc` | `deb` · `linux-deb` | `linux-repo-gpg` | `lintian`, `appstream-validate`, `flatpak-lint`, `snap-lint` | `flathub`, `snapcraft`, `self-hosted` | `false` | L5 发行版：deb 主档；Flatpak 权限恰为 [--share=ipc, --socket=wayland, --socket=fallback-x11, --device=dri]，Snap plugs 恰为 [desktop, wayland, x11, opengl]（与打包器常量同源，T-M5-02/03） |
| L5 | Fedora（`fedora`） | `native` | `tauri-ipc` | `rpm` · `linux-rpm` | `linux-repo-gpg` | `rpmlint`, `appstream-validate`, `flatpak-lint` | `flathub`, `self-hosted` | `false` | L5 发行版：rpm 主档；Flatpak 权限恰为 [--share=ipc, --socket=wayland, --socket=fallback-x11, --device=dri]（与打包器常量同源，T-M5-02） |
| L5 | Linux Mint（`mint`） | `native` | `tauri-ipc` | `deb` · `linux-deb` | `linux-repo-gpg` | `lintian`, `appstream-validate`, `flatpak-lint` | `flathub`, `self-hosted` | `false` | L5 发行版：deb 主档（Linux Mint 兼容 Ubuntu 仓库）；Flatpak 权限恰为 [--share=ipc, --socket=wayland, --socket=fallback-x11, --device=dri]（与打包器常量同源，T-M5-02） |
| L5 | Chrome OS（`chromeos` + `chromeos-pwa`） | `native / wasm` | `tauri-ipc / wasm-bindgen` | `deb` · `linux-deb`; `static-site` · `web-static` | — | `lintian`, `pwa-audit` | `self-hosted` | `false` | chromeos：ChromeOS 双轨之 Crostini：复用 Linux deb 链（control / 维护者脚本 / AppStream 随载荷，不另写 deb 逻辑）；安装冒烟在 Crostini 容器不可得时 SKIP-with-print，结构检查（deb 数据段 / control）仍真跑；chromeos-pwa：ChromeOS 双轨之 PWA：复用 Web 管线 + T-M1-04 manifest + T-M1-08 静态站点打包器；离线能力 / manifest / 安装性审计由 pwa-audit 接入 verify。离线能力要求应用自带 service worker 注册与缓存（模板不生成 SW）；真 Chrome 腿（Page.getInstallabilityErrors + 断网重载）缺席时 SKIP-with-print |
| L5 | FreeBSD（`freebsd`） | `native` | `tauri-ipc` | `pkg` · `freebsd-pkg` | `freebsd-repo-pkg` | `pkg-check` | `freebsd-repo` | `false` | 原生 pkg create 链（Tauri IPC 核心即宿主，FreeBSD 用户态）；Linux 只做交叉发起 + 暂存树 / manifest 结构自检，pkg create / pkg check / pkg install 真跑归 FreeBSD VM（T-M5-09）；仓库签名走 pkg repo rsa\|ecdsa\|eddsa:<keyfile>（catalogue 内签名，客户端 PUBKEY） |
| L5 | OpenBSD（`openbsd`） | `native` | `tauri-ipc` | `ports` · `openbsd-ports` | `openbsd-signify` | `portcheck` | `openbsd-ports` | `false` | ports 目录骨架（Makefile / pkg/PLIST / pkg/DESCR + files 载荷，无 distfile）；signify 包签名（pkg_sign，钥经 POLYPACK_OPENBSD_SIGNIFY_KEY）；make package 与 portcheck 真跑归 OpenBSD VM（T-M5-09），Linux 宿主只生成骨架并做结构自检 |
| L5 | BSD 家族（NetBSD / DragonFly 等）（`netbsd`） | `native` | `tauri-ipc` | `pkgsrc` · `pkgsrc` | `netbsd-repo-gpg` | `pkgsrc-check` | `netbsd-repo` | `false` | pkgsrc 骨架（Makefile / PLIST / DESCR + files 载荷，无 distfile；NetBSD / DragonFly 等同源）；Linux 交叉发起只生成骨架并做结构自检，bmake package / pkg_add 真跑归 NetBSD VM（T-M5-09）；仓库签名走 pkg_admin gpg-sign-package（逐包 GPG，仓库为扁平 .tgz + pkg_summary.gz，客户端 GPG_KEYRING_VERIFY） |
| L6 | Apple watchOS（`watchos`） | `none` | `none` | `watch` · `watchos-xcodebuild` | `apple-codesign` | `codesign-verify` | `app-store` | `true` | 实验：SwiftUI 伴生模板（核心 API 调用桩 + WatchConnectivity 说明），核心不直接运行（runtime = none）；模板经 `poly init --target watchos` 生成，macOS 宿主 `xcodebuild` 构建；Linux / Windows 发起走远程农场 macOS worker（R3）；签名复用 M2 Apple 链（codesign，凭据只以 apple-codesign 引用出现） |
| L6 | Wear OS（`wearos`） | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `google-play-wear` | `false` | Wear OS 清单（android.hardware.type.watch，minSdkVersion ≥ 26）；UI 运行时（RN on Wear）为 beta，打包与签名走 Android 基座 |
| L6 | Android Automotive（`android-automotive`） | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `oem-channel` | `false` | AAOS 清单（android.hardware.type.automotive）；车厂渠道，无统一公开 API——发布需 OEM 凭据引用（占位 Publisher，不伪造提交） |
| L6 | QNX（`qnx`） | `staticlib` | `cffi` | `image` · `qnx-sdp` | — | `qnx-image-structure`, `c-abi-symbols`, `qnx-qemu-smoke` | `self-hosted` | `false` | headless 核心（可选 HMI 归平台侧）；静态库 + 部署镜像（附 mkifs buildfile，可引导 .ifs / QEMU 归 T-M6-09）；交叉编译需 QNX SDP（$QNX_HOST / $QNX_TARGET）与 Rust QNX target，无 SDP 时按环境槽 SKIP-with-print，不伪造二进制；自部署 |
| L6 | AUTOSAR（Adaptive）（`autosar`） | `staticlib` | `cffi` | `arxml` · `autosar-adaptive` | — | `arxml-schema`, `c-abi-symbols` | `self-hosted` | `false` | 无 UI、自部署；AUTOSAR Adaptive 清单（ARXML）由 poly_engine_* C ABI 单一来源派生（服务 / 端口与导出符号一一对应）；no_std 可选（引擎层 --no-default-features，应用提供 global_allocator / panic_handler） |
| L6 | VxWorks（`vxworks`） | `staticlib` | `cffi` | `library` · `vxworks-wr` | — | `c-abi-symbols`, `vxworks-ld-check` | `self-hosted` | `false` | 无 UI、cffi、no_std；format = `library`（默认，静态库）或 `image`（部署 kit zip，配置 [targets.vxworks].format 覆盖）；wr-cc / wr-ld 缺席时产物用 nm 符号闭包 + 结构断言校验并 SKIP-with-print，绝不伪造可加载镜像 |
| L6 | RTOS（Zephyr / FreeRTOS）（`zephyr` + `freertos`） | `staticlib` | `cffi` | `west` · `zephyr-west`; `board` · `freertos-qemu` | — | `c-abi-symbols`, `zephyr-west-build`, `zephyr-qemu-smoke`, `freertos-qemu-smoke` | `self-hosted` | `false` | zephyr：无 UI、cffi、no_std；west 工程模板（zephyr module + 引擎静态库模块链入），qemu_x86 / qemu_cortex_m3 两板 `west build` 验收；最小 CONFIG_* 集随模板版本化；west / Zephyr SDK 缺席时按环境槽 SKIP-with-print；freertos：无 UI、cffi、no_std；FreeRTOS 示例板包（mps2-an385 / Cortex-M3，静态库 + 启动 / 中断 / UART / FreeRTOSConfig / Makefile 工程模板）；内核 V11.1.0 按 tag + sha256 现拉不入库；`make smoke` 在 QEMU 中调用 poly_engine_* 并以 semihosting 退出码 0 结束；arm-none-eabi-gcc / qemu-system-arm / 网络缺席时按环境槽 SKIP-with-print |
| L6 | Fitbit OS（`fitbit`） | `none` | `none` | `fba` · `fitbit-build` | — | `fitbit-structure-check` | `fitbit-gallery` | `false` | 核心不直接运行、能力由 JS 侧实现（runtime = none；Fitbit OS 侧 JS SDK 应用包 `.fba`：应用清单 + device-<family>.zip + 可选 companion.zip，结构同官方 SDK 6.1）；fitbit-build 缺席时 CLI 自检 SKIP-with-print，结构断言真跑；签名 / 提审归 Fitbit Gallery 平台侧 |
| L6 | Garmin（Connect IQ）（`garmin`） | `none` | `none` | `iq` · `garmin-monkeyc` | — | `iq-structure-check` | `connect-iq-store` | `true` | 实验：Monkey C 子集受限转译——仅 UI 子集 + 原生逻辑（Monkey C 重写，无 JS 运行时）；工程结构自检可用，Connect IQ 编译与真机实证待补（T-M7-10 触发前置；能力边界见 ui/adapters/garmin/CAPABILITY-BOUNDARY-DRAFT.md） |
| L6 | Zepp OS（`zepp`） | `none` | `none` | `zab` · `zeus` | — | `zepp-structure-check` | `zepp-store` | `false` | 核心不直接运行、能力由 Zepp JS API 侧实现（runtime = none）；Zepp 小程序 `.zab`（外层 manifest.json + <md5>.zpk，zpk 内含 device.zip / app-side.zip，结构同 zpm）；zeus / 模拟器缺席时安装与工具链校验 SKIP-with-print，结构断言真跑；签名 / 提审归 Zepp 商店平台侧 |
| L7 | PlayStation（规划中） | 官方 SDK 插件 | — | 平台包 | 厂商工具 | 厂商工具链 | 需 Sony 授权 | — | 规划中：官方 SDK 插件（需 Sony 授权），SDK 不入库、本地路径探测；尚未声明 TargetSpec。 |
| L7 | Xbox（规划中） | GDK 插件 | — | 平台包 | 厂商工具 | 厂商工具链 | 需微软授权 | — | 规划中：GDK 插件（需微软授权），SDK 不入库、本地路径探测；尚未声明 TargetSpec。 |
| L7 | Nintendo Switch（规划中） | NintendoSDK 插件 | — | 平台包 | 厂商工具 | 厂商工具链 | 需任天堂授权 | — | 规划中：NintendoSDK 插件（需任天堂授权），SDK 不入库、本地路径探测；尚未声明 TargetSpec。 |
<!-- END GENERATED: status-49 -->

### 3.2 L1 逐平台明细（7）

| 平台 | 核心运行时 · 桥 | 打包器与产物 | 签名 | 产物校验 | 发布渠道 | 备注 |
|---|---|---|---|---|---|---|
| Android | native `.so` · uniffi（JNI） | AAB + APK（AAPT2 + bundletool 直出，不依赖 Gradle） | apksigner（keystore 引用） | `apksigner verify` + bundletool validate | Google Play API | |
| iOS | native staticlib · uniffi（Swift） | IPA（xcodebuild archive + exportArchive） | codesign + notarytool | `codesign --verify` + spctl | App Store Connect API | 需 macOS 宿主 |
| HarmonyOS | native `.so` · NAPI（C ABI） | HAP（hvigor 构建） | hap-sign-tool | 签名校验 + HAP 结构检查 | AppGallery Connect API | RNOH 承载 UI |
| Windows | 无桥（Tauri IPC，核心即宿主） | MSI（WiX）+ NSIS | Authenticode（signtool） | `signtool verify` | MS Store（MSIX 提交） | WebView2 运行时 |
| macOS | 无桥（Tauri IPC） | DMG + PKG | codesign + notarize（notarytool） | codesign verify + stapler validate | App Store（可选）/ 自托管 | 公证必须 |
| Linux | 无桥（Tauri IPC） | AppImage / deb / rpm / Flatpak / Snap | 仓库级 GPG（deb/rpm） | lintian / rpmlint | Flathub / Snapcraft / 自托管 | |
| Web | wasm · wasm-bindgen | 静态站点 + PWA manifest | 无（可选 SRI） | html-validate + SRI 检查 | 任意静态托管 | |

### 3.3 L2 逐平台明细（9）

| 平台 | 核心运行时 · 桥 | 打包器与产物 | 签名 | 产物校验 | 发布渠道 | 备注 |
|---|---|---|---|---|---|---|
| 微信小程序 | wasm 进宿主沙箱 · wasm-bindgen | 小程序代码包（Taro 编译） | 平台侧（上传签名） | 微信开发者工具 CLI 校验 | 微信开放平台 API（提审） | |
| 支付宝小程序 | 同上 | 小程序代码包（Taro） | 平台侧 | 支付宝开发者工具 CLI | 支付宝开放平台 | |
| 抖音小程序 | 同上 | 小程序代码包（Taro） | 平台侧 | 抖音开发者工具 CLI | 抖音开放平台 | |
| 快应用 | wasm / JS（Taro 快应用目标） | `.rpk` | 平台侧 | hap-toolkit lint | 快应用联盟商店 | |
| TikTok Minis | HTML5 + Minis 桥 | zip | 平台侧 | 平台沙箱自检 | TikTok 平台 | |
| Google Play Instant | native（复用 Android） | Instant-enabled AAB | 同 Android | bundletool validate | Google Play | |
| Facebook Instant Games | HTML5 | zip | 无 | Meta 沙箱自检 | Meta 平台 | |
| Snapchat Mini | HTML5 | zip | 无 | Snap 沙箱自检 | Snap 平台 | |
| LINE Mini App | LIFF（web） | 静态包 + LIFF 配置 | HTTPS / 平台 TLS | LIFF 配置检查 | LINE 平台 | |

### 3.4 L3 逐平台明细（9）

| 平台 | 核心运行时 · 桥 | 打包器与产物 | 签名 | 产物校验 | 发布渠道 | 备注 |
|---|---|---|---|---|---|---|
| Android TV | native · uniffi | APK（Leanback 清单） | 同 Android | bundletool + 焦点导航冒烟 | Google Play（TV 轨道） | |
| Apple tvOS | native staticlib · uniffi | TV 包（xcodebuild） | codesign | codesign verify | App Store | RN-tvOS 承载 UI；macOS 宿主 |
| HarmonyOS TV | native · NAPI | HAP | hap-sign-tool | 同上 | 华为视频商店 | |
| Samsung Tizen | wasm · wasm-bindgen | `.wgt` | Tizen 作者证书（三星签名工具） | `wgt-lint` | Seller Office | web 运行时 |
| LG webOS | wasm · wasm-bindgen | `.ipk` | 平台侧（发布签名） | `ares-package` 校验 | LG Content Store | web 运行时 |
| Roku OS | **无**（核心不参与） | 频道包（zip） | dev / 平台证书 | 侧载自检 | Roku 渠道商店 | ※实验：SceneGraph 子集渲染器，仅 UI 子集 + 原生逻辑 |
| Amazon Fire TV | native · uniffi | APK | 同 Android | 同上 | 亚马逊应用商店 | |
| Vizio SmartCast | wasm · wasm-bindgen | 应用包 | 无 | 沙箱自检 | Vizio 平台 | web 运行时 |
| Hisense VIDAA | wasm · wasm-bindgen | 应用包 | 无 | 沙箱自检 | VIDAA 商店 | web 运行时 |

### 3.5 L4 逐平台明细（4）

| 平台 | 核心运行时 · 桥 | 打包器与产物 | 签名 | 产物校验 | 发布渠道 | 备注 |
|---|---|---|---|---|---|---|
| GrapheneOS | native · uniffi（无 GMS flavor） | APK | apksigner / 社区密钥 | apksigner verify + 可复现比对 | F-Droid / 自托管 | Android 构建变体 |
| KaiOS | wasm · wasm-bindgen | zip + manifest | 平台侧 | 开发者工具自检 | KaiStore | web 运行时 |
| Sailfish OS | native `.so` + Qt/Silica 壳 | RPM | RPM GPG | rpmlint | Jolla Store | |
| Ubuntu Touch | native `.so` + WebView 壳 | `.click`（Clickable） | Clickable 打包 | Clickable 构建检查 | OpenStore | |

### 3.6 L5 逐平台明细（7）

| 平台 | 核心运行时 · 桥 | 打包器与产物 | 签名 | 产物校验 | 发布渠道 | 备注 |
|---|---|---|---|---|---|---|
| Ubuntu | native（Tauri IPC） | deb / Snap / Flatpak | GPG | lintian | Snapcraft / 自托管 | |
| Fedora | native（Tauri IPC） | rpm / Flatpak | GPG | rpmlint | Flathub / 自托管 | |
| Linux Mint | native（Tauri IPC） | deb / Flatpak | GPG | lintian | 自托管 | |
| Chrome OS | native（Tauri IPC） | deb（Crostini）+ PWA | 无 | deb 安装冒烟 + PWA 审计 | 自托管 / PWA | 双轨交付 |
| FreeBSD | native | pkg | 仓库签名（可选） | `pkg check` | 自建仓库 / ports | |
| OpenBSD | native | ports | signify | portcheck | ports@ | |
| BSD 家族（NetBSD / DragonFly 等） | native | pkgsrc 等 | 仓库签名 | 打包器自检 | 自建仓库 | |

### 3.7 L6 逐平台明细（11 个 TargetSpec）

**已入 TargetSpec 的嵌入式 / 车机目标。** 下表由 `crates/poly-targets/src/builtin.rs` 的 `TargetSpec` 生成（`node tools/gen-target-table.mjs`），字段逐字同源，请勿手改；`--check` 在表与注册表漂移时失败。

<!-- BEGIN GENERATED: l6-embedded-status -->
| 目标 | 运行时 | 桥 | 打包器与产物 | 签名 | 产物校验 | 发布渠道 | experimental | notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `android-automotive` | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `oem-channel` | `false` | AAOS 清单（android.hardware.type.automotive）；车厂渠道，无统一公开 API——发布需 OEM 凭据引用（占位 Publisher，不伪造提交） |
| `autosar` | `staticlib` | `cffi` | `arxml` · `autosar-adaptive` | — | `arxml-schema`, `c-abi-symbols` | `self-hosted` | `false` | 无 UI、自部署；AUTOSAR Adaptive 清单（ARXML）由 poly_engine_* C ABI 单一来源派生（服务 / 端口与导出符号一一对应）；no_std 可选（引擎层 --no-default-features，应用提供 global_allocator / panic_handler） |
| `fitbit` | `none` | `none` | `fba` · `fitbit-build` | — | `fitbit-structure-check` | `fitbit-gallery` | `false` | 核心不直接运行、能力由 JS 侧实现（runtime = none；Fitbit OS 侧 JS SDK 应用包 `.fba`：应用清单 + device-<family>.zip + 可选 companion.zip，结构同官方 SDK 6.1）；fitbit-build 缺席时 CLI 自检 SKIP-with-print，结构断言真跑；签名 / 提审归 Fitbit Gallery 平台侧 |
| `freertos` | `staticlib` | `cffi` | `board` · `freertos-qemu` | — | `c-abi-symbols`, `freertos-qemu-smoke` | `self-hosted` | `false` | 无 UI、cffi、no_std；FreeRTOS 示例板包（mps2-an385 / Cortex-M3，静态库 + 启动 / 中断 / UART / FreeRTOSConfig / Makefile 工程模板）；内核 V11.1.0 按 tag + sha256 现拉不入库；`make smoke` 在 QEMU 中调用 poly_engine_* 并以 semihosting 退出码 0 结束；arm-none-eabi-gcc / qemu-system-arm / 网络缺席时按环境槽 SKIP-with-print |
| `garmin` | `none` | `none` | `iq` · `garmin-monkeyc` | — | `iq-structure-check` | `connect-iq-store` | `true` | 实验：Monkey C 子集受限转译——仅 UI 子集 + 原生逻辑（Monkey C 重写，无 JS 运行时）；工程结构自检可用，Connect IQ 编译与真机实证待补（T-M7-10 触发前置；能力边界见 ui/adapters/garmin/CAPABILITY-BOUNDARY-DRAFT.md） |
| `qnx` | `staticlib` | `cffi` | `image` · `qnx-sdp` | — | `qnx-image-structure`, `c-abi-symbols`, `qnx-qemu-smoke` | `self-hosted` | `false` | headless 核心（可选 HMI 归平台侧）；静态库 + 部署镜像（附 mkifs buildfile，可引导 .ifs / QEMU 归 T-M6-09）；交叉编译需 QNX SDP（$QNX_HOST / $QNX_TARGET）与 Rust QNX target，无 SDP 时按环境槽 SKIP-with-print，不伪造二进制；自部署 |
| `vxworks` | `staticlib` | `cffi` | `library` · `vxworks-wr` | — | `c-abi-symbols`, `vxworks-ld-check` | `self-hosted` | `false` | 无 UI、cffi、no_std；format = `library`（默认，静态库）或 `image`（部署 kit zip，配置 [targets.vxworks].format 覆盖）；wr-cc / wr-ld 缺席时产物用 nm 符号闭包 + 结构断言校验并 SKIP-with-print，绝不伪造可加载镜像 |
| `watchos` | `none` | `none` | `watch` · `watchos-xcodebuild` | `apple-codesign` | `codesign-verify` | `app-store` | `true` | 实验：SwiftUI 伴生模板（核心 API 调用桩 + WatchConnectivity 说明），核心不直接运行（runtime = none）；模板经 `poly init --target watchos` 生成，macOS 宿主 `xcodebuild` 构建；Linux / Windows 发起走远程农场 macOS worker（R3）；签名复用 M2 Apple 链（codesign，凭据只以 apple-codesign 引用出现） |
| `wearos` | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `google-play-wear` | `false` | Wear OS 清单（android.hardware.type.watch，minSdkVersion ≥ 26）；UI 运行时（RN on Wear）为 beta，打包与签名走 Android 基座 |
| `zephyr` | `staticlib` | `cffi` | `west` · `zephyr-west` | — | `c-abi-symbols`, `zephyr-west-build`, `zephyr-qemu-smoke` | `self-hosted` | `false` | 无 UI、cffi、no_std；west 工程模板（zephyr module + 引擎静态库模块链入），qemu_x86 / qemu_cortex_m3 两板 `west build` 验收；最小 CONFIG_* 集随模板版本化；west / Zephyr SDK 缺席时按环境槽 SKIP-with-print |
| `zepp` | `none` | `none` | `zab` · `zeus` | — | `zepp-structure-check` | `zepp-store` | `false` | 核心不直接运行、能力由 Zepp JS API 侧实现（runtime = none）；Zepp 小程序 `.zab`（外层 manifest.json + <md5>.zpk，zpk 内含 device.zip / app-side.zip，结构同 zpm）；zeus / 模拟器缺席时安装与工具链校验 SKIP-with-print，结构断言真跑；签名 / 提审归 Zepp 商店平台侧 |
<!-- END GENERATED: l6-embedded-status -->

**五个穿戴目标均已入 TargetSpec。** watchOS / Wear / Fitbit / Garmin / Zepp 的平台矩阵以上表为唯一事实源（由 TargetSpec 生成，不再维护手写第二份）；其中 `watchos` / `garmin` 为 `experimental = true`，Wear OS 为平台级目标（`experimental = false`）——UI 运行时（RN on Wear）为 beta，打包与签名走 Android 基座。

### 3.7.1 实验目标：能力 / 缺口 / 替代方案（watchOS / Garmin）

`watchos` / `garmin` 维持 `experimental = true`（受限档，继续决策）。完整三段式文档见 [EXPERIMENTAL-TARGETS.md](EXPERIMENTAL-TARGETS.md)（英文：[EXPERIMENTAL-TARGETS.en.md](EXPERIMENTAL-TARGETS.en.md)）：每个缺口均给出替代方案或明示「无替代」；Wear OS 的 beta 边界同章记录，且文档不为其添加 ※实验 标注。三处一致性由 `node tools/check-experimental.mjs` 校验。

### 3.8 L7 逐平台明细（3）

| 平台 | 核心运行时 · 桥 | 打包器与产物 | 签名 | 产物校验 | 发布渠道 | 备注 |
|---|---|---|---|---|---|---|
| PlayStation | 官方 SDK 插件 | 平台包 | 厂商工具 | 厂商工具链 | 需 Sony 授权 | SDK 不入库，本地路径探测 |
| Xbox | GDK 插件 | 平台包 | 厂商工具 | 厂商工具链 | 需微软授权 | 同上 |
| Nintendo Switch | NintendoSDK 插件 | 平台包 | 厂商工具 | 厂商工具链 | 需任天堂授权 | 同上 |

> 标注 ※实验 的目标运行时受限；L7 仅在授权环境提供，公开仓库以接口 mock 验证插件契约。

## 4. 技术架构

### 4.1 Crate 职责与主要接口

| Crate | 职责 | 主要接口（示意） | 关键依赖 |
|---|---|---|---|
| `poly-cli` | 命令解析、进度、补全、报告渲染 | `poly <verb> [--target …] [--profile ci\|local]` | clap, indicatif |
| `poly-core` | JobSpec、DAG 调度、事件总线、诊断 | `trait Stage { fn plan(&self, job) -> Plan; fn run(&self, ctx) -> StageOutcome }` | tokio, petgraph |
| `poly-targets` | TargetSpec 注册表（内建 + 插件注册） | `trait TargetRegistry { fn resolve(&self, id) -> TargetSpec }` | serde, toml |
| `poly-toolchain` | SDK / NDK / Xcode / emsdk 探测、安装 | `trait Probe { fn locate(&self) -> Option<Toolchain>; fn install(&self) -> Result<()> }` | which, semver |
| `poly-bundle` | 打包器矩阵 | `trait Packager { fn id(&self) -> TargetId; fn package(&self, inputs, spec) -> Artifact }` | zip, tar, objcopy |
| `poly-sign` | 签名、公证、加固；密钥库适配 | `trait Signer { fn sign(&self, artifact, credential_ref) -> Signed }` | keyring, sigstore |
| `poly-publish` | 商店与平台 API 客户端 | `trait Publisher { fn submit(&self, artifact, channel) -> Submission }` | reqwest |
| `poly-cache` | 内容寻址缓存 | `fn key(inputs: &BuildInputs) -> Digest; fn get/put(Digest) -> Option<Artifact>` | blake3 |
| `poly-plugin` | WASM 插件宿主（沙箱） | wasmtime 实例化 + WIT 接口（见 4.5） | wasmtime, wit-bindgen |

### 4.2 数据模型

**JobSpec**（一次构建请求的完整描述，贯穿 DAG）：

```text
JobSpec {
  id, profile(ci|local), targets[TargetSpec],
  stages[Stage], cache_policy, secrets: [CredentialRef],   # 只有引用
  budget: { size, startup_ms }                              # 产物预算
}
```

**BuildEvent**（事件总线，驱动进度条、报告与缓存写入）：

```text
BuildEvent =
  Planned{targets} | Probed{toolchains} | Compiled{crate, triple}
| UiBundled{adapter} | Signed{profile} | Packaged{format, bytes}
| Verified{checker, ok} | Published{channel, submission}
| Failed{stage, code, hint} | Cached{key, hit: bool}
```

**诊断报告**：`Failed` 事件携带稳定错误码（`poly::E_PKG_0007` 风格）+ 修复提示；`poly build --json` 输出机器可读全量事件流。

### 4.3 TargetSpec 完整字段

| 字段 | 类型 | 说明 | 示例 |
|---|---|---|---|
| `id` | string | 目标标识（= `[targets.*]` 表名） | `tizen-tv` |
| `tier` | 1–7 | 支持等级（决定 CI 强度） | `3` |
| `hosts` | list | 可发起构建的宿主 | `["win","linux","macos"]` |
| `runtime` | enum | 核心运行方式：`native` / `wasm` / `staticlib` / `none` | `wasm` |
| `bridge` | enum | 桥：`uniffi` / `napi` / `wasm-bindgen` / `cffi` / `tauri-ipc` / `none` | `wasm-bindgen` |
| `ui` | list | 渲染适配器 | `["react-dom"]` |
| `package` | table | 打包器与产物格式 | `{ format: "wgt", builder: "tizen-wgt" }` |
| `signing` | string | 凭据档案引用（密钥库条目名） | `"tizen-author"` |
| `verify` | list | 产物校验器 | `["wgt-lint"]` |
| `publish` | list | 发布渠道客户端 | `["tizen-seller-office"]` |
| `experimental` | bool | 是否受限/实验（影响文档与 CI 标注） | `false` |
| `notes` | string | 边界与降级说明（进入生成的文档表） | `"web 运行时，无推送"` |

**`--target all` 语义**：解析为当前 profile 下**可构建**的全部目标——排除需厂商授权的 L7 与宿主 / 凭据不可达的目标；plan 阶段输出排除清单与原因（`--explain` 展开），显式列出的目标同样执行可达性检查。

**多产物**：`package` 声明**主产物**（AAB / MSI / DMG / AppImage 等）；平台副产物矩阵（APK、NSIS、PKG、deb/rpm/Flatpak/Snap 等）在 M1 打包器矩阵落地时扩展为多产物字段（T-M1-05..08 的前置，届时随实现更新本节）。

### 4.4 桥接矩阵

| 宿主 | 桥 | 产物 |
|---|---|---|
| Node / Taro 运行时 | napi-rs | .node 原生模块 |
| Kotlin / Swift | uniffi | AAR / XCFramework |
| Web / WebView / KaiOS / 电视 web | wasm-bindgen | .wasm + JS glue |
| QNX / RTOS / 车机 | cffi | 静态库 + C 头文件 |
| 桌面（Tauri） | Tauri IPC | 核心即宿主，无跨语言桥 |

桥接层由工作区内的接口生成器统一产出（接口定义一处、四端生成），避免四套手写绑定漂移。

### 4.5 插件体系（WASM 沙箱）

插件以 WIT 接口描述，wasmtime 沙箱运行；默认**无网络、无文件系统**，能力按需显式授权（类似 WASI 的 capability 模型）：

```wit
world polypack-target {
  // 只读输入
  import inputs: func() -> build-inputs
  // 插件声明自己的目标规格
  export describe: func() -> target-spec
  // 承接打包阶段
  export package: func(bundle: ui-bundle, lib: core-lib) -> result<artifact, error>
}
```

约束：插件制品带内容摘要（SRI）校验；接口按 `target-spec/v1` 版本化，破坏性变更走新版本号；内建目标与插件走同一接口（原则 6）。

### 4.6 版本策略

| 对象 | 策略 |
|---|---|
| Rust | MSRV = 最近两个 stable 版本；edition 2024 |
| `polypack.toml` | 顶层 `schema = 1` 字段（缺省视为 1；未知版本直接拒绝并提示升级 `poly`） |
| TargetSpec / 插件接口 | `target-spec/v1` 版本化（破坏性变更走新版本号，见 4.5） |
| 本仓库版本 | tag 与 GitHub Release 自 v1.0.0 起按 `vX.Y.Z` 语义化递增 |
| 配置严格度 | `deny_unknown_fields`：未知键一律拒绝（绝不静默丢配置）；新增键视为破坏性变更 |

## 5. 构建流水线

### 5.1 DAG 阶段

```text
plan → probe → compile → bundle(ui) → package → sign → verify → publish
```

- **plan**：解析 polypack.toml → JobSpec；未知目标、循环依赖、缺凭据引用在此阶段失败（快速失败，不浪费编译）；
- **probe**：工具链探测与安装；Apple 目标此时决定「本地 or 远程农场」；
- **compile / bundle(ui)**：两条并行支线（Rust 交叉编译 ↔ React 打包），汇聚于 package；
- **package → sign → verify**：可独立重跑（`poly retry --from sign|verify`），签名阶段重新拉取凭据；
- **publish**：仅 `--publish` 时执行；提审后轮询审核状态并归档回执。

### 5.2 缓存键与失效

```text
cache_key = blake3(
  源文件树摘要 ‖ 依赖锁文件 ‖ TargetSpec 版本 ‖ 工具链版本 ‖ 构建参数(profile)
)
```

分级缓存：**库级**（compile 产物）→ **包级**（package 产物）→ **提审级**（publish 记录）。任一级失效即级联重算；跨机共享通过对象存储后端（实现为 trait，本地默认文件系统）。

### 5.3 可复现性

- 除数字签名与时间戳外，两次构建产物其余字节一致（`poly verify --reproducible`）；
- 编译期注入 `SOURCE_DATE_EPOCH`；产物中禁止出现绝对路径与本机环境变量；
- GrapheneOS 目标额外提供「可复现比对」通道（与上游构建对拍）。

### 5.4 构建时限预算

除产物预算（尺寸 / 启动时延，见 JobSpec）外，流水线自身有构建时限预算，超出即告警：

| 场景 | 预算（软目标） |
|---|---|
| 缓存全命中（单目标） | < 30s |
| 增量构建（单目标，源码级改动） | < 3min |
| L1 目标冷构建（本地五目标矩阵） | < 20min |

### 5.5 清理与保留策略

磁盘不会无声膨胀：缓存与产物都有上限与 LRU 淘汰，配置进 `polypack.toml`：

| 对象 | 默认保留 | 清理方式 |
|---|---|---|
| 构建缓存（CAS） | 最近 30 天或 20 GiB（先到为准） | 超限 LRU 淘汰；`poly clean --cache` 手动 |
| 构建产物 | 最近 10 次构建 / 每目标 | `poly clean --artifacts` |
| 提审记录 | 永久（审计需要） | 不清理 |

`poly clean` 默认 dry-run（列出可回收条目与空间），`--yes` 执行并输出释放统计（落地任务：T-M1-15）。

## 6. UI 适配层

包结构：

```text
@polypack/ui          # 设计系统：tokens（颜色/间距/字号）+ headless 原语 + 模式
@polypack/adapter-dom     # 桌面 / Web（react-dom）
@polypack/adapter-native  # Android 族 / iOS 族 / 鸿蒙（React Native / RNOH）
@polypack/adapter-taro    # 小程序族（Taro，React 语法）
@polypack/adapter-tv      # 电视（react-dom + 焦点导航系统）
@polypack/adapter-wear    # 穿戴（紧凑 tokens，实验）
```

适配器契约（示意）：`RendererAdapter { mount(root, app), platformStyle(tokens) -> SkinnedTokens, capabilities(): { focusNav, safeArea, gestures } }`——同一份页面组件树，按 `capabilities` 降级交互（无指针 → 焦点环；小屏 → 紧凑间距）。

**电视焦点导航系统**（L3 关键差异点）：方向键遍历图、焦点环渲染、Back 键语义（返回上一焦点记忆）、遥控器数字键快捷键；冒烟测试即「无指针完成核心任务」。

## 7. 安全与密钥

- **凭据引用模型**：`signing = "profile"` → 密钥库（keyring / Vault / CI 注入内存），仓库内明文凭据触发 secret-scan 失败；
- **secret-scan**：内置规则（keystore/JKS、证书私钥、API key 模式）+ 提交与构建双重检查；
- **产物脱敏**：打包前扫描产物元数据，剥绝对路径、环境变量、调试符号（可配置保留）；
- **插件沙箱**：无网络/无文件系统默认，显式能力授权 + SRI 校验；
- **供应链**：`cargo-deny`（许可/漏洞/重复）+ `cargo-audit`，依赖锁文件强制提交（落地任务：T-M0-11）。

## 8. 测试与 CI 矩阵

测试金字塔：

| 层级 | 内容 | 频率 |
|---|---|---|
| 单元 | crate 内部逻辑、TargetSpec 解析、缓存键 | 每次提交 |
| 契约（每目标一个） | 打包 + 校验器 + 产物结构断言（金样比对） | 每次提交（mock 模式）/ 每晚（真实模式） |
| 冒烟 | 可安装 + 核心任务（含 TV 焦点、Wear 表单） | 每晚 / 发版前 |
| 全量 | 真机/模拟器/商店沙箱提审 | 发版前 |

CI 宿主 × 目标层矩阵：

| 宿主 | L1 | L2 | L3 | L4–L6 | L7 |
|---|---|---|---|---|---|
| ubuntu-latest | Web/Linux 全跑 | 工具链 mock 全跑 | web 系全跑 | 交叉编译全跑 | 接口 mock |
| macos-14 | macOS/iOS 全跑 | mock | tvOS 全跑 | 交叉编译 | 接口 mock |
| windows-2022 | Windows 全跑 | mock | — | 交叉编译 | 接口 mock |
| 每晚扩展 | — | 真实开发者工具 CLI | 模拟器冒烟 | QEMU 冒烟（BSD 原生构建 + 安装；QNX / Zephyr / FreeRTOS） | — |

> BSD 原生构建腿由 [`bsd-ci`](../.github/workflows/bsd-ci.yml)（每夜 20:07 UTC + 手动）执行：
> FreeBSD 14.5 / OpenBSD 7.9 QEMU VM 内 `poly build` 原生出包并做安装冒烟（FreeBSD 走
> `pkg install` + `pkg check -s`，OpenBSD 走 `make package` + `pkg_add` / `pkg_check`）；
> FreeBSD 腿同时真跑自托管 pkg 仓库签名（`poly release --target freebsd`）与二跑幂等断言。
> OpenBSD 腿为尽力真跑：托管 runner 无 KVM，镜像在 TCG 下无法启动时按 SKIP-with-print 留痕
> （日志缺失以 warn 归档），待镜像或启动方式修复后转真跑。构建日志、签名仓库、回执与包随
> 作业归档 14 天。
>
> `embedded-ci`（每夜 19:27 UTC + 手动）以 `poly targets --tier 6 --headless --json` 生成矩阵
> （新增 tier-6 目标自动进矩阵），在 ubuntu-latest 上跑 QNX / AUTOSAR / VxWorks / Zephyr /
> FreeRTOS 交叉编译与 QEMU 冒烟；工具链按钉扎版本缓存，缺 SDP / wr-cc 等按 SKIP-with-print
> 并在 job summary 列 REAL/SKIP；日志、串口与 result.json 归档 14 天。
>
> `wear-smoke`（mock 每提交；每夜 + 手动真跑）在 KVM + `vars.POLYPACK_WEAR_FORM_APK` 门后跑
> Wear 表单套件（输入 / 滑动 / 返回）；缺门时 SKIP-with-print 并写全 SKIP 的 result.json。
> `wear-release-smoke` 为发版门（`workflow_call` / 手动）：发版流水线以 workflow_call 把它作为
> 前置依赖；设备腿失败或未真跑即阻断发版，证据归档 90 天。
> CI 工作流共十一个：`rust-ci` / `docs-check` /
> `secret-scan` / `mobile-contract` / `miniprogram-contract` / `tv-smoke` / `bridge` / `bsd-ci` /
> `embedded-ci` / `wear-smoke` / `wear-release-smoke`。

## 9. 里程碑与任务分解

> 估算按 1.5 FTE；每项任务以「可演示」为完成标准。任务级分解（113 任务 / 279.5 人日，含角色、依赖、验收标准与排期备注）见 **[TASKS.md](TASKS.md)** 与 [TASKS.en.md](TASKS.en.md)。

### M0 地基（2 周）

1. Cargo workspace 骨架（9 个 crate 空壳 + 依赖方向约束）；
2. 三宿主 CI（Linux / macOS / Windows），缓存 Rust 构建；
3. `polypack.toml` schema（serde + 导出 JSON Schema）；
4. TargetSpec DSL + L1 七目标完整声明；
5. `poly doctor`（工具链探测报告）；
6. 诊断框架（错误码前缀体系 + `--json` 事件流）；
7. 文档同步检查接入 CI（`render-diagrams.mjs --check` 校验 SVG 与 JSON 一致）。

**验收**：`poly doctor` 正确报告本机缺失工具链；schema 覆盖 L1 全部字段；CI 三宿主绿。

### M1 桌面与 Web（4 周）

1. Tauri 打包管线（triple 探测 → 核心编译 → 捆绑 → 产物）；
2. Web 管线（vite 驱动 + PWA manifest 生成）；
3. `poly-bundle` 首批 Packager：MSI/NSIS、DMG、AppImage/deb、静态站点；
4. `poly-cache`（blake3 CAS + 本地后端 + 对象存储 trait）；
5. `poly dev` 热预览 v0（Web 宿主内热更）；
6. `poly init` 示例应用模板（Rust core + React UI 最小骨架）；
7. **英文版图示**（英文标签 JSON → 生成 `docs/assets/en/*.svg`）。

**验收**：示例应用在 Win/macOS/Linux/Web 四端出包；二跑缓存命中 ≥ 90%；英文图示合入。

### M2 移动（5 周）

1. uniffi 桥生成器（Rust 接口 → Kotlin + Swift 绑定）；
2. `@polypack/adapter-native` v0（RN 宿主 + 焦点/导航基线）；
3. Android 打包（AAPT2 + bundletool + apksigner，免 Gradle 直出）；
4. iOS 打包（xcodebuild archive + exportArchive + notarytool 接入）；
5. 密钥库集成（keyring + Vault 后端 trait）；
6. secret-scan 接入提交与 CI；
7. 远程农场协议 v0（JobSpec 序列化 + worker 拉取执行）。

**验收**：示例 APK/IPA 可安装、签名可验证；密钥零入库（scan 全绿）；Linux 宿主经农场出 IPA。农场部署与「发起地 / 构建地」说明见 [FARM.md](FARM.md)。

### M3 鸿蒙与小程序（4 周）

1. RNOH 适配（HarmonyOS 宿主 + NAPI 桥接入）；
2. HAP 打包（hvigor 调用 + hap-sign-tool 签名 + 校验）；
3. `@polypack/adapter-taro`（React 语法 → 微信/支付宝/抖音/快应用目标）；
4. wasm 核心进小程序沙箱（API 面白名单 + 体积预算）；
5. 三平台 publish 客户端（上传 + 提审 + 状态轮询）；
6. 小程序契约测试（开发者工具 CLI 校验 + 金样比对）。

**验收**：三端小程序开发者工具预览通过；鸿蒙 HAP 签名安装；提审脚本化可重跑。

### M4 TV 家族（4 周）

1. `@polypack/adapter-tv`（焦点导航系统 + 10-foot tokens）；
2. Android TV / Fire TV（Leanback 清单 + 冒烟）与鸿蒙 TV（复用 M3 的 HAP 链 + TV 适配）；
3. tvOS（RN-tvOS + Xcode 打包链复用 M2）；
4. Tizen / webOS / VIDAA / Vizio（web 包 → wgt/ipk/应用包 + 各校验器）；
5. Roku 受限渲染器 spike（SceneGraph 子集的能力边界文档）；
6. 遥控器冒烟测试套件（无指针完成核心任务）。

**验收**：焦点遍历无死路；wgt/ipk 校验器通过；Roku 能力边界形成结论（继续/降级）。

### M5 桌面发行版与 BSD（3 周）

1. deb/rpm/Flatpak/Snap/AppImage 打包器成熟化（仓库元数据 + GPG）；
2. FreeBSD / OpenBSD 原生构建目标（CI 内 VM/交叉方案定型）；
3. ChromeOS 双轨（Crostini deb + PWA）；
4. 包仓库签名与 `poly release --target fedora,freebsd` 端到端。

**验收**：各格式在对应宿主/容器内安装成功；BSD 目标进 CI。

### M6 嵌入式与车机（5 周）

1. 核心分层（`poly-core` 内 feature 拆分出 no_std 引擎层，不新增 crate）；
2. QNX / AUTOSAR / VxWorks / Zephyr / FreeRTOS 交叉编译目标与 CI 矩阵；
3. Android Automotive（复用 Android 链 + 车厂渠道占位）；
4. QEMU 冒烟（QNX / Zephyr）。

**验收**：全部无 UI 目标进 CI 编译矩阵；QNX 冒烟通过。

### M7 穿戴（4 周）

1. Wear OS 冒烟（复用 Android 链 + Wear 清单）；
2. watchOS 伴生模板生成器（SwiftUI，实验标注）；
3. Fitbit（JS SDK）与 Zepp（JS API）打包器；
4. Garmin 转译 spike（Monkey C 子集，实验标注）。

**验收**：Wear OS 有设备冒烟；实验目标文档标注完备（能力/缺口/替代方案）。

### M8 生态 GA（6 周）

1. WASM 插件 SDK（WIT 发布 + 模板仓库 + 签名/SRI）；
2. 远程农场 v1（部署指南 + 并发/隔离/缓存共享）；
3. 商店发布流水线 GA（重试、回执归档、审核轮询）；
4. GA 文档与技术审查（含全部 49 目标的现状表）。

**验收**：第三方开发者按文档在 1 天内新增一个自定义目标并出包。

## 10. 风险登记册

| # | 风险 | 影响 | 对策 |
|---|---|---|---|
| R1 | 主机平台厂商授权与 NDA SDK | L7 无法公开发布 | 插件形态 + 授权开发者本地探测；公开仓库只留接口与 mock |
| R2 | Roku / watchOS / Garmin 无 React 原生运行时 | 目标功能面收窄 | 明确标注「实验 / 受限」（`experimental` 字段进文档表）；能力边界写入产物说明；**M4 决策（T-M4-12）：Roku 继续（受限档）**——`experimental` 维持，真机侧载实证为条件任务 T-M4-15 的触发前置 |
| R3 | Apple 系目标必须 macOS 宿主 | Win/Linux 用户无法本地出 IPA | 远程构建农场（M2 协议 v0 → M8 v1）；「发起地/构建地」的部署、凭据与签名验证见 [FARM.md](FARM.md) |
| R4 | 商店 / 小程序审核不可完全自动化 | 发布阶段仍需人工 | 自动化止步「提审」；审核轮询 + 回执归档；不承诺全自动过审 |
| R5 | 签名密钥与凭据泄露 | 供应链安全事件 | 引用模型 + secret-scan + 产物脱敏；CI 密钥只进内存 |
| R6 | 50+ 目标矩阵维护成本 | 目标腐坏（平台规则变化） | 每目标契约测试（第 8 节）；平台规则变更走 TargetSpec 版本化 |
| R7 | 双语文档同步成本 | 中英漂移 | 文档同步约定（第 11 节）作为提交检查项；图示单源（JSON）生成 |
| R8 | Taro / RN / RNOH 上游生态波动 | 适配层返工 | 适配器接口做薄（只依赖稳定 API 面）；每季度评估上游里程碑 |
| R9 | 工具链下载源不稳定（SDK/NDK/emsdk） | probe 阶段失败率 | 探测缓存 + 镜像源配置 + 校验和锁定 |
| R10 | 产物体积膨胀（wasm 进小程序沙箱） | 平台体积上限被触线 | 体积预算（`budget.size`）+ wasm-opt + 按目标裁剪 |

## 11. 文档同步约定（提交检查项）

任何影响**架构 / 功能 / 流程 / 平台矩阵**的改动，必须在同一次提交内完成：

1. 更新 `docs/diagrams/*.json`（图示唯一事实源），运行 `node tools/render-diagrams.mjs` 重新生成 `docs/assets/*.svg`（CI 以 `--check` 模式校验一致性；右下角 Pax 角标默认开启，`--no-brand` 关闭）；
2. 同步 `README.md` 与 `README.en.md`（双语镜像）；
3. 同步 `docs/ROADMAP.md` 与 `docs/ROADMAP.en.md`；任务计划同规矩（`docs/TASKS.md` / `docs/TASKS.en.md`），任务增删与估算调整须双文件同提交更新；
4. 平台矩阵增删 = 同时更新：README 分级表（×2 语言）+ 本文件逐平台明细（×2 语言）+ 里程碑验收 + TargetSpec；
5. Pax 由 `tools/build-pax.mjs` 单一源生成（五种表情·各自带专属动效 + 全表情循环动画版 + 项目图标 `pax-mark.svg` + 表情联系表 `pax-moods.svg` + 本地游乐场 `docs/pax.html`）；美术或表达变化时运行该脚本重新生成，并同步双 README（CI 以 `node tools/build-pax.mjs --check` 校验漂移）；
6. 实验目标（`watchos` / `garmin`）的能力 / 缺口 / 替代方案同步 `docs/EXPERIMENTAL-TARGETS.md` 与 `docs/EXPERIMENTAL-TARGETS.en.md`（每个缺口保留替代方案或明示「无替代」；`node tools/check-experimental.mjs` 校验 TargetSpec / 文档表 / CI 三处一致）。

## 12. 术语表

| 术语 | 含义 |
|---|---|
| TargetSpec | 目标平台的能力声明（运行时、桥、打包、签名、校验、发布） |
| JobSpec | 一次构建请求的完整描述，贯穿流水线 DAG |
| Tier（L1–L7） | 支持等级，决定工程策略与 CI 强度 |
| Packager / Signer / Publisher | 打包 / 签名 / 发布阶段的策略接口 |
| CAS | 内容寻址存储（缓存后端） |
| 金样（Golden） | 契约测试中用于比对的标准产物 |
| WIT | WebAssembly Interface Types，插件接口描述语言 |
| 10-foot UI | 电视场景界面（远距可读、遥控器操作） |
| Bridge（桥） | Rust 核心与宿主语言之间的绑定层 |
| 契约测试 | 每目标「打包 + 校验 + 结构断言」的自动化检查 |

## 13. 扩展方向（孵化池）

2026-10-14 缺口审计立项：以下方向**不占当前 M0–M8 排期**；每项给出「提升为正式里程碑的触发条件」与「首步动作」，满足触发条件并经需求验证评审后进入路线图（防止凭假设开工）。

| # | 扩展 | 价值 | 提升条件 | 首步动作 |
|---|---|---|---|---|
| E1 | **SBOM / 构建溯源** | 每个产物生成 SPDX/CycloneDX 清单与 SLSA 风格构建来源证明；供应链合规刚需（基础扫描已在 T-M0-11） | 首个合规或供应链审计需求出现 | 在 publish 旁路复用缓存键输入清单生成 SBOM |
| E2 | **monorepo / 多应用工作区** | 一份配置管理多个应用共享 Rust core，服务大型团队 | 首个多应用仓库用户出现 | 设计 `[workspace]` 表与 JobSpec 复合语义 |
| E3 | **增量发布 `poly release --changed`** | 只重发变更目标（内容寻址缓存判定），多端 CI 时间砍一个量级 | M1 缓存落地后立即试点 | 基于 cache_key 的变更判定原型 |
| E4 | **生产热更 / OTA 通道** | 补齐「发布 → 迭代」之外的线上更新通道 | 首批用户提出线上更新诉求 | 调研各端官方更新机制（Play In-App Updates / 小程序版本管理 / Tauri updater） |
| E5 | **商店元数据管线** | listing 文案 / 截图 / 关键词的版本化与多语言管理 | M2/M3 首个商店发布跑通 | 定义 metadata 目录结构与校验器 |
| E6 | **GUI 目标矩阵视图 / 构建历史** | poly-gui 从「控制台」升级为可用性仪表盘 | M1 Tauri 管线跑通 | 设计矩阵视图数据源（BuildEvent 流） |
| E7 | **发布前设备预览矩阵** | 真机 / 模拟器矩阵预览（同时解决冒烟设备来源问题） | M4 TV 或 M7 穿戴冒烟遇到设备瓶颈 | 调研设备农场方案与成本 |
| E8 | **决策记录（ADR）** | 把「为什么这么选」存档（双语文档、不拆 crate、wasm 插件等既有决策） | 可立即开始 | 建立 `docs/decisions/` 与模板，回填既有决策 |

## 14. 开放问题（待决策）

- **许可**：已定为 **Apache-2.0**（`LICENSE` 已落地）；若未来包含 GPL 系工具链安装脚本，再评估许可证兼容；
- **插件市场**：WASM 插件的分发形态（官方注册表 vs 去中心化索引）；
- **远程农场**：仅自建（文档提供部署指南）还是提供托管服务；
- **英文图示**：M1 落地（当前图示标签为中文，README.en 已注明）。
