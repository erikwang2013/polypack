# PolyPack Roadmap (detailed plan)

> This is the engineering source of truth for PolyPack (the README is its summary): vision, design principles, per-platform engineering detail, architecture and data models, pipeline and caching, security, testing and CI, milestone task breakdown, and the risk register.
> Chinese original: [ROADMAP.md](ROADMAP.md); platform overview and diagrams: [README](../README.en.md#platform-matrix).

- Project status: **planning phase (documentation baseline v1.0.0)**
- Plan date: 2026-10-14; revision: 2026-10-14 (per-platform detail; gap audit & extension incubation)
- Name: PolyPack (CLI command `poly`); project pet: Pax the packing robot
- Scheduling assumption: 1–2 engineers fluent in Rust (milestone estimates assume 1.5 FTE)

---

## 1. Vision

**In one line**: collapse multi-platform releases from N toolchains, N IDEs, and N packaging scripts into one `poly` command.

**What success looks like (measurable):**

1. A team ships one Rust core + one React UI to Android / iOS / Web / WeChat Mini Program in a single afternoon;
2. **Adding a new target platform costs ≤ 2 person-days**: one TargetSpec (declarative) plus an optional WASM adapter plugin;
3. **Reproducible**: identical inputs produce byte-identical artifacts (signatures and timestamps excluded); second-run cache hit rate ≥ 90%;
4. **Diagnosable**: failures report which step, why, and how to fix it — machine-readable (JSON) plus human-readable (terminal report);
5. Every per-target contract test of the support matrix is green in CI (including golden-artifact comparisons).

## 2. Product boundaries and design principles

**Non-goals:**

- Not an app framework (state management, routing, data layers remain the user's choice; PolyPack only packages and delivers);
- Does not host user code or keys (builds run locally or on self-hosted farms; keys appear only as references);
- Does not support sideloading on platforms that require vendor authorization (consoles especially);
- Not an app store — only an automation client for stores.

**Design principles:**

1. **Single source of truth** — target capability is declared in TargetSpec; docs, CLI, and verifiers all derive from it;
2. **Zero keys in-repo** — only credential references exist in the repository; plaintext credentials fail the build;
3. **Targets are declarations** — adding a platform must not touch core code paths (only new declarations and plugins);
4. **Budgeted builds** — every target has artifact budgets (size, startup latency); exceeding them warns;
5. **Honest tiering** — unsupported / experimental / constrained are labeled as such, never inflated;
6. **Plugins first** — built-in targets are simply "official plugins"; third-party plugins use the same interface;
7. **One identity** — Pax is the project's sole icon and mascot: wherever a "project icon" is needed (documentation favicons, the mini badge in the bottom-right corner of generated diagrams, and future CLI / GUI / installer icons) Pax is used (`docs/assets/pax.svg` / `pax-mark.svg`), never a generic placeholder; the twelve expressions map onto build states (success = joy, failure = crying, exception = shocked, needs-confirmation = coy, idle = smile). Generated image assets get the Pax mini badge in the bottom-right by default (`--no-brand` disables it); the same rule extends to the product — when PolyPack generates store preview images, it labels the bottom-right corner with the mini icon of the **corresponding target platform** (toggleable, on by default). (Delivery task: T-M1-14)

## 3. Support matrix, per-platform engineering detail

### 3.1 Tier overview

| Tier | Platforms | Scope | Unified strategy |
|---|---|---|---|
| L1 first-class | 7 | Android / iOS / HarmonyOS / Windows / macOS / Linux / Web | native library or wasm + RN / react-dom; full signing and publishing |
| L2 mini programs & instant apps | 9 | WeChat / Alipay / Douyin / Quick App / TikTok / Play Instant / FB / Snapchat / LINE | wasm into host sandbox (or HTML5); Taro / web packaging |
| L3 TVs & large screens | 9 | Android TV / tvOS / HarmonyOS TV / Tizen / webOS / Roku / Fire TV / Vizio / VIDAA | Android family reuses RN; web family packages wgt/ipk; Roku constrained renderer |
| L4 mobile OS variants | 4 | GrapheneOS / KaiOS / Sailfish / Ubuntu Touch | Android variant / web stack / Qt or WebView shell |
| L5 desktop distributions & BSD | 7 | Ubuntu / Fedora / Mint / ChromeOS / FreeBSD / OpenBSD / BSD family | distribution packager matrix + native builds |
| L6 wearables / automotive / embedded | 10 | watchOS / Wear / Auto / QNX / AUTOSAR / VxWorks / RTOS / Fitbit / Garmin / Zepp | mostly headless core libraries (no_std); UI optional |
| L7 consoles | 3 | PlayStation / Xbox / Switch | vendor-licensed SDK plugins; SDKs never in-repo |

### 3.1.1 49-target status table (generated)

The table below is the single source of truth for the status of all 49 platforms: 39 rows are generated field by field from `TargetSpec` (41 declared targets; the RTOS and Chrome OS rows each merge two; `notes` keeps the declaration's original Chinese text), and 10 planned platforms are explicitly marked "planned" with fields taken from the declarative manifest in `tools/gen-status-table.mjs`. Do not edit it by hand: regenerate with `node tools/gen-status-table.mjs`; `--check` fails on drift. Platform matrix additions/removals follow §11 item 4.

<!-- BEGIN GENERATED: status-49 -->
| Tier | Platform | Runtime | Bridge | Packager & artifact | Signing | Artifact verification | Publishing channel | experimental | notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| L1 | Android (`android`) | `native` | `uniffi` | `aab` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `google-play` | `false` | none |
| L1 | iOS (`ios`) | `native` | `uniffi` | `ipa` · `ios-xcodebuild` | `apple-codesign` | `codesign-verify`, `spctl` | `app-store-connect` | `false` | 需 macOS 宿主 |
| L1 | HarmonyOS (`harmonyos`) | `native` | `napi` | `hap` · `harmonyos-hvigor` | `harmonyos-hap-sign` | `hap-sign-verify`, `hap-structure-check` | `appgallery-connect` | `false` | RNOH 承载 UI |
| L1 | Windows (`windows`) | `native` | `tauri-ipc` | `msi` · `windows-wix` | `windows-authenticode` | `signtool-verify` | `ms-store` | `false` | WebView2 运行时 |
| L1 | macOS (`macos`) | `native` | `tauri-ipc` | `dmg` · `macos-dmg` | `apple-codesign` | `codesign-verify`, `stapler-validate` | `app-store`, `self-hosted` | `false` | 公证必须 |
| L1 | Linux (`linux`) | `native` | `tauri-ipc` | `appimage` · `linux-appimage` | `linux-repo-gpg` | `lintian`, `rpmlint`, `appimage-structure`, `appstream-validate` | `flathub`, `snapcraft`, `self-hosted` | `false` | none |
| L1 | Web (`web`) | `wasm` | `wasm-bindgen` | `static-site` · `web-static` | none | `html-validate`, `sri-check` | `static-hosting` | `false` | none |
| L2 | WeChat Mini Program (`wechat`) | `wasm` | `wasm-bindgen` | `weapp` · `taro-weapp` | none | `wechat-devtools-cli` | `wechat-open-platform` | `false` | 小程序代码包（Taro 编译）；wasm 进宿主沙箱（T-M3-07） |
| L2 | Alipay Mini Program (`alipay`) | `wasm` | `wasm-bindgen` | `alipay` · `taro-alipay` | none | `alipay-devtools-cli` | `alipay-open-platform` | `false` | 小程序代码包（Taro 编译）；wasm 进宿主沙箱（T-M3-07） |
| L2 | Douyin Mini Program (`douyin`) | `wasm` | `wasm-bindgen` | `tt` · `taro-tt` | none | `douyin-devtools-cli` | `douyin-open-platform` | `false` | 小程序代码包（Taro 编译，平台 `tt`）；wasm 进宿主沙箱（T-M3-07） |
| L2 | Quick App (快应用) (`quickapp`) | `wasm` | `wasm-bindgen` | `rpk` · `hap-toolkit` | none | `hap-toolkit-lint` | `quickapp-union-store` | `false` | 快应用 `.rpk`（hap-toolkit 构建）；lint 入 verify、提审走联盟商店（T-M3-12） |
| L2 | TikTok Minis (planned) | HTML5 + Minis bridge | HTML5 | zip | platform-side | platform sandbox self-check | TikTok platform | none | Planned: no TargetSpec yet; delivered through the HTML5 + Minis bridge; the zip is reviewed platform-side. |
| L2 | Google Play Instant (planned) | native | uniffi | instant-aab · android-aapt2 | android-keystore | bundletool-validate | google-play | none | Planned · discontinued: Google stopped accepting Instant Apps submissions and shut down Play services Instant APIs in Dec 2025; kept planned, no TargetSpec. |
| L2 | Facebook Instant Games (planned) | HTML5 | HTML5 | zip | none | Meta sandbox self-check | Meta platform | none | Planned: HTML5 runtime with Meta sandbox self-check; no TargetSpec yet. |
| L2 | Snapchat Mini (planned) | HTML5 | HTML5 | zip | none | Snap sandbox self-check | Snap platform | none | Planned: HTML5 runtime with Snap sandbox self-check; no TargetSpec yet. |
| L2 | LINE Mini App (planned) | LIFF (web) | HTTPS | static bundle + LIFF config | HTTPS / platform TLS | LIFF config check | LINE platform | none | Planned: delivered as LIFF (web) over HTTPS / platform TLS; no TargetSpec yet. |
| L3 | Android TV (`android-tv`) | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `google-play-tv` | `false` | Leanback 清单（android.software.leanback） |
| L3 | Apple tvOS (`tvos`) | `native` | `uniffi` | `ipa` · `tvos-xcodebuild` | `apple-codesign` | `codesign-verify`, `spctl` | `app-store-connect` | `false` | RN-tvOS 承载 UI；需 macOS 宿主 |
| L3 | HarmonyOS TV (`harmonyos-tv`) | `native` | `napi` | `hap` · `harmonyos-hvigor` | `harmonyos-hap-sign` | `hap-sign-verify`, `hap-structure-check` | `appgallery-connect` | `false` | RNOH TV 形态（T-M4-14：deviceTypes 含 `tv`、焦点 / 安全区复用 adapter-tv）；复用 HarmonyHap 打包链；无 hdc TV 设备——真机焦点走查 SKIP；发布走 AppGallery Connect（华为视频商店无独立渠道 id） |
| L3 | Samsung Tizen (TV) (`tizen-tv`) | `wasm` | `wasm-bindgen` | `wgt` · `tizen-wgt` | `tizen-author` | `wgt-lint` | `tizen-seller-office` | `false` | web 运行时（Tizen TV）；作者证书签名（凭据引用 tizen-author）与侧载安装归三星工具链（本机无 tizen CLI，SDK 校验器缺席时如实跳过） |
| L3 | LG webOS (`webos-tv`) | `wasm` | `wasm-bindgen` | `ipk` · `webos-ares` | none | `ares-package` | `lg-content-store` | `false` | web 运行时（LG webOS）；签名按平台侧（LG Content Store 发布签名），本地不签名（本机无 ares-cli，SDK 校验器缺席时如实跳过） |
| L3 | Roku OS (`roku`) | `none` | `none` | `zip` · `roku-channel` | none | `sideload-structure-check` | `roku-channel-store` | `true` | 实验：SceneGraph 子集受限渲染——仅 UI 子集 + 原生逻辑（BrightScript 重写，无 JS 运行时）；侧载结构自检可用，真机实证待补（T-M4-15 触发前置；T-M4-12 决策：继续受限档） |
| L3 | Amazon Fire TV (`fire-tv`) | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `amazon-appstore` | `false` | 复用 Android TV 同一 APK 链，渠道指向亚马逊应用商店 |
| L3 | Vizio SmartCast (`smartcast-tv`) | `wasm` | `wasm-bindgen` | `web-app` · `smartcast-web-app` | none | `sandbox-selfcheck` | `vizio-platform` | `false` | web 运行时（Vizio SmartCast，远程托管）；无签名、无推送——只出沙箱自检包（入口 / 清单字段 / 体积），提报与安装归平台侧 |
| L3 | Hisense VIDAA (`vidaa-tv`) | `wasm` | `wasm-bindgen` | `web-app` · `vidaa-web-app` | none | `sandbox-selfcheck` | `vidaa-store` | `false` | web 运行时（海信 VIDAA）；无签名、无推送——只出沙箱自检包（入口 / 清单字段 / 体积），提审与安装归平台商店侧 |
| L4 | GrapheneOS (`grapheneos`) | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `self-hosted` | `false` | 复用 Android APK 链（同 aapt2 / keystore / 校验）；无独立无 GMS 打包档（GMS 剔除归应用侧）；发布走自托管（`self-hosted`；无 F-Droid 客户端 id）；可复现性走 `poly verify --reproducible`（同输入二跑，未实现上游构建对拍） |
| L4 | KaiOS (`kaios`) | `wasm` | `wasm-bindgen` | `zip` · `kaios-webapp` | none | `kaios-structure-check` | none | `false` | react-dom web 栈（wasm · wasm-bindgen）→ 打包应用 zip（manifest.webapp + UI dist，确定性归档）；结构自检（清单 / 入口 / 条目）与 verify 腿 kaios-structure-check 同一实现。普通应用本地不签名，提审与平台侧签名归 KaiStore；poly-publish 无 kaistore 客户端，publish 留空；本机无 KaiOS 设备 / 模拟器与商店凭据——设备 / 商店腿 SKIP-with-print |
| L4 | Sailfish OS (planned) | native | cffi | rpm | RPM GPG | rpmlint | Jolla Store | none | Planned: the existing linux-rpm chain only emits a desktop Tauri RPM (bare product + .desktop); missing the Qt / Silica QML host, the Sailfish SDK cross toolchain (armv7hl / aarch64), and harbour packaging / signing; no TargetSpec yet. |
| L4 | Ubuntu Touch (planned) | native | cffi | .click (Clickable) | Clickable packaging | Clickable build check | OpenStore | none | Planned: the existing Linux deb chain cannot produce .click (different format / AppArmor / click manifest); missing Clickable / Ubuntu Touch SDK and the WebView shell; no TargetSpec yet. |
| L5 | Ubuntu (`ubuntu`) | `native` | `tauri-ipc` | `deb` · `linux-deb` | `linux-repo-gpg` | `lintian`, `appstream-validate`, `flatpak-lint`, `snap-lint` | `flathub`, `snapcraft`, `self-hosted` | `false` | L5 发行版：deb 主档；Flatpak 权限恰为 [--share=ipc, --socket=wayland, --socket=fallback-x11, --device=dri]，Snap plugs 恰为 [desktop, wayland, x11, opengl]（与打包器常量同源，T-M5-02/03） |
| L5 | Fedora (`fedora`) | `native` | `tauri-ipc` | `rpm` · `linux-rpm` | `linux-repo-gpg` | `rpmlint`, `appstream-validate`, `flatpak-lint` | `flathub`, `self-hosted` | `false` | L5 发行版：rpm 主档；Flatpak 权限恰为 [--share=ipc, --socket=wayland, --socket=fallback-x11, --device=dri]（与打包器常量同源，T-M5-02） |
| L5 | Linux Mint (`mint`) | `native` | `tauri-ipc` | `deb` · `linux-deb` | `linux-repo-gpg` | `lintian`, `appstream-validate`, `flatpak-lint` | `flathub`, `self-hosted` | `false` | L5 发行版：deb 主档（Linux Mint 兼容 Ubuntu 仓库）；Flatpak 权限恰为 [--share=ipc, --socket=wayland, --socket=fallback-x11, --device=dri]（与打包器常量同源，T-M5-02） |
| L5 | Chrome OS (`chromeos` + `chromeos-pwa`) | `native / wasm` | `tauri-ipc / wasm-bindgen` | `deb` · `linux-deb`; `static-site` · `web-static` | none | `lintian`, `pwa-audit` | `self-hosted` | `false` | chromeos：ChromeOS 双轨之 Crostini：复用 Linux deb 链（control / 维护者脚本 / AppStream 随载荷，不另写 deb 逻辑）；安装冒烟在 Crostini 容器不可得时 SKIP-with-print，结构检查（deb 数据段 / control）仍真跑；chromeos-pwa：ChromeOS 双轨之 PWA：复用 Web 管线 + T-M1-04 manifest + T-M1-08 静态站点打包器；离线能力 / manifest / 安装性审计由 pwa-audit 接入 verify。离线能力要求应用自带 service worker 注册与缓存（模板不生成 SW）；真 Chrome 腿（Page.getInstallabilityErrors + 断网重载）缺席时 SKIP-with-print |
| L5 | FreeBSD (`freebsd`) | `native` | `tauri-ipc` | `pkg` · `freebsd-pkg` | `freebsd-repo-pkg` | `pkg-check` | `freebsd-repo` | `false` | 原生 pkg create 链（Tauri IPC 核心即宿主，FreeBSD 用户态）；Linux 只做交叉发起 + 暂存树 / manifest 结构自检，pkg create / pkg check / pkg install 真跑归 FreeBSD VM（T-M5-09）；仓库签名走 pkg repo rsa\|ecdsa\|eddsa:<keyfile>（catalogue 内签名，客户端 PUBKEY） |
| L5 | OpenBSD (`openbsd`) | `native` | `tauri-ipc` | `ports` · `openbsd-ports` | `openbsd-signify` | `portcheck` | `openbsd-ports` | `false` | ports 目录骨架（Makefile / pkg/PLIST / pkg/DESCR + files 载荷，无 distfile）；signify 包签名（pkg_sign，钥经 POLYPACK_OPENBSD_SIGNIFY_KEY）；make package 与 portcheck 真跑归 OpenBSD VM（T-M5-09），Linux 宿主只生成骨架并做结构自检 |
| L5 | BSD family (NetBSD / DragonFly, …) (`netbsd`) | `native` | `tauri-ipc` | `pkgsrc` · `pkgsrc` | `netbsd-repo-gpg` | `pkgsrc-check` | `netbsd-repo` | `false` | pkgsrc 骨架（Makefile / PLIST / DESCR + files 载荷，无 distfile；NetBSD / DragonFly 等同源）；Linux 交叉发起只生成骨架并做结构自检，bmake package / pkg_add 真跑归 NetBSD VM（T-M5-09）；仓库签名走 pkg_admin gpg-sign-package（逐包 GPG，仓库为扁平 .tgz + pkg_summary.gz，客户端 GPG_KEYRING_VERIFY） |
| L6 | Apple watchOS (`watchos`) | `none` | `none` | `watch` · `watchos-xcodebuild` | `apple-codesign` | `codesign-verify` | `app-store` | `true` | 实验：SwiftUI 伴生模板（核心 API 调用桩 + WatchConnectivity 说明），核心不直接运行（runtime = none）；模板经 `poly init --target watchos` 生成，macOS 宿主 `xcodebuild` 构建；Linux / Windows 发起走远程农场 macOS worker（R3）；签名复用 M2 Apple 链（codesign，凭据只以 apple-codesign 引用出现） |
| L6 | Wear OS (`wearos`) | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `google-play-wear` | `false` | Wear OS 清单（android.hardware.type.watch，minSdkVersion ≥ 26）；UI 运行时（RN on Wear）为 beta，打包与签名走 Android 基座 |
| L6 | Android Automotive (`android-automotive`) | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `oem-channel` | `false` | AAOS 清单（android.hardware.type.automotive）；车厂渠道，无统一公开 API——发布需 OEM 凭据引用（占位 Publisher，不伪造提交） |
| L6 | QNX (`qnx`) | `staticlib` | `cffi` | `image` · `qnx-sdp` | none | `qnx-image-structure`, `c-abi-symbols`, `qnx-qemu-smoke` | `self-hosted` | `false` | headless 核心（可选 HMI 归平台侧）；静态库 + 部署镜像（附 mkifs buildfile，可引导 .ifs / QEMU 归 T-M6-09）；交叉编译需 QNX SDP（$QNX_HOST / $QNX_TARGET）与 Rust QNX target，无 SDP 时按环境槽 SKIP-with-print，不伪造二进制；自部署 |
| L6 | AUTOSAR (Adaptive) (`autosar`) | `staticlib` | `cffi` | `arxml` · `autosar-adaptive` | none | `arxml-schema`, `c-abi-symbols` | `self-hosted` | `false` | 无 UI、自部署；AUTOSAR Adaptive 清单（ARXML）由 poly_engine_* C ABI 单一来源派生（服务 / 端口与导出符号一一对应）；no_std 可选（引擎层 --no-default-features，应用提供 global_allocator / panic_handler） |
| L6 | VxWorks (`vxworks`) | `staticlib` | `cffi` | `library` · `vxworks-wr` | none | `c-abi-symbols`, `vxworks-ld-check` | `self-hosted` | `false` | 无 UI、cffi、no_std；format = `library`（默认，静态库）或 `image`（部署 kit zip，配置 [targets.vxworks].format 覆盖）；wr-cc / wr-ld 缺席时产物用 nm 符号闭包 + 结构断言校验并 SKIP-with-print，绝不伪造可加载镜像 |
| L6 | RTOS (Zephyr / FreeRTOS) (`zephyr` + `freertos`) | `staticlib` | `cffi` | `west` · `zephyr-west`; `board` · `freertos-qemu` | none | `c-abi-symbols`, `zephyr-west-build`, `zephyr-qemu-smoke`, `freertos-qemu-smoke` | `self-hosted` | `false` | zephyr：无 UI、cffi、no_std；west 工程模板（zephyr module + 引擎静态库模块链入），qemu_x86 / qemu_cortex_m3 两板 `west build` 验收；最小 CONFIG_* 集随模板版本化；west / Zephyr SDK 缺席时按环境槽 SKIP-with-print；freertos：无 UI、cffi、no_std；FreeRTOS 示例板包（mps2-an385 / Cortex-M3，静态库 + 启动 / 中断 / UART / FreeRTOSConfig / Makefile 工程模板）；内核 V11.1.0 按 tag + sha256 现拉不入库；`make smoke` 在 QEMU 中调用 poly_engine_* 并以 semihosting 退出码 0 结束；arm-none-eabi-gcc / qemu-system-arm / 网络缺席时按环境槽 SKIP-with-print |
| L6 | Fitbit OS (`fitbit`) | `none` | `none` | `fba` · `fitbit-build` | none | `fitbit-structure-check` | `fitbit-gallery` | `false` | 核心不直接运行、能力由 JS 侧实现（runtime = none；Fitbit OS 侧 JS SDK 应用包 `.fba`：应用清单 + device-<family>.zip + 可选 companion.zip，结构同官方 SDK 6.1）；fitbit-build 缺席时 CLI 自检 SKIP-with-print，结构断言真跑；签名 / 提审归 Fitbit Gallery 平台侧 |
| L6 | Garmin (Connect IQ) (`garmin`) | `none` | `none` | `iq` · `garmin-monkeyc` | none | `iq-structure-check` | `connect-iq-store` | `true` | 实验：Monkey C 子集受限转译——仅 UI 子集 + 原生逻辑（Monkey C 重写，无 JS 运行时）；工程结构自检可用，Connect IQ 编译与真机实证待补（T-M7-10 触发前置；能力边界见 ui/adapters/garmin/CAPABILITY-BOUNDARY-DRAFT.md） |
| L6 | Zepp OS (`zepp`) | `none` | `none` | `zab` · `zeus` | none | `zepp-structure-check` | `zepp-store` | `false` | 核心不直接运行、能力由 Zepp JS API 侧实现（runtime = none）；Zepp 小程序 `.zab`（外层 manifest.json + <md5>.zpk，zpk 内含 device.zip / app-side.zip，结构同 zpm）；zeus / 模拟器缺席时安装与工具链校验 SKIP-with-print，结构断言真跑；签名 / 提审归 Zepp 商店平台侧 |
| L7 | PlayStation (planned) | official SDK plugin | — | platform bundle | vendor tools | vendor toolchain | Sony license required | none | Planned: official SDK plugin (Sony license required); SDK never in-repo, local path probing; no TargetSpec yet. |
| L7 | Xbox (planned) | GDK plugin | — | platform bundle | vendor tools | vendor toolchain | Microsoft license required | none | Planned: GDK plugin (Microsoft license required); SDK never in-repo, local path probing; no TargetSpec yet. |
| L7 | Nintendo Switch (planned) | NintendoSDK plugin | — | platform bundle | vendor tools | vendor toolchain | Nintendo license required | none | Planned: NintendoSDK plugin (Nintendo license required); SDK never in-repo, local path probing; no TargetSpec yet. |
<!-- END GENERATED: status-49 -->

### 3.2 L1 per-platform detail (7)

| Platform | Core runtime · bridge | Packager and artifacts | Signing | Artifact verification | Publishing channel | Notes |
|---|---|---|---|---|---|---|
| Android | native `.so` · uniffi (JNI) | AAB + APK (AAPT2 + bundletool direct; no Gradle dependency) | apksigner (keystore reference) | `apksigner verify` + bundletool validate | Google Play API | |
| iOS | native staticlib · uniffi (Swift) | IPA (xcodebuild archive + exportArchive) | codesign + notarytool | `codesign --verify` + spctl | App Store Connect API | requires a macOS host |
| HarmonyOS | native `.so` · NAPI (C ABI) | HAP (hvigor build) | hap-sign-tool | signature check + HAP structure check | AppGallery Connect API | RNOH carries the UI |
| Windows | no bridge (Tauri IPC; core is the host) | MSI (WiX) + NSIS | Authenticode (signtool) | `signtool verify` | MS Store (MSIX submission) | WebView2 runtime |
| macOS | no bridge (Tauri IPC) | DMG + PKG | codesign + notarize (notarytool) | codesign verify + stapler validate | App Store (optional) / self-hosted | notarization required |
| Linux | no bridge (Tauri IPC) | AppImage / deb / rpm / Flatpak / Snap | repository GPG (deb/rpm) | lintian / rpmlint | Flathub / Snapcraft / self-hosted | |
| Web | wasm · wasm-bindgen | static site + PWA manifest | none (optional SRI) | html-validate + SRI check | any static host | |

### 3.3 L2 per-platform detail (9)

| Platform | Core runtime · bridge | Packager and artifacts | Signing | Artifact verification | Publishing channel | Notes |
|---|---|---|---|---|---|---|
| WeChat Mini Program | wasm in host sandbox · wasm-bindgen | mini-program bundle (Taro compile) | platform-side (upload signing) | WeChat devtools CLI check | WeChat Open Platform API (submit) | |
| Alipay Mini Program | same | mini-program bundle (Taro) | platform-side | Alipay devtools CLI | Alipay Open Platform | |
| Douyin Mini Program | same | mini-program bundle (Taro) | platform-side | Douyin devtools CLI | Douyin Open Platform | |
| Quick App | wasm / JS (Taro quick-app target) | `.rpk` | platform-side | hap-toolkit lint | Quick App alliance stores | |
| TikTok Minis | HTML5 + Minis bridge | zip | platform-side | platform sandbox self-check | TikTok platform | |
| Google Play Instant | native (Android reuse) | instant-enabled AAB | same as Android | bundletool validate | Google Play | ※discontinued: Google stopped accepting Instant Apps submissions and shut down Play services Instant APIs in Dec 2025; kept planned, no TargetSpec |
| Facebook Instant Games | HTML5 | zip | none | Meta sandbox self-check | Meta platform | |
| Snapchat Mini | HTML5 | zip | none | Snap sandbox self-check | Snap platform | |
| LINE Mini App | LIFF (web) | static bundle + LIFF config | HTTPS / platform TLS | LIFF config check | LINE platform | |

### 3.4 L3 per-platform detail (9)

| Platform | Core runtime · bridge | Packager and artifacts | Signing | Artifact verification | Publishing channel | Notes |
|---|---|---|---|---|---|---|
| Android TV | native · uniffi | APK (Leanback manifest) | same as Android | bundletool + focus-navigation smoke | Google Play (TV track) | |
| Apple tvOS | native staticlib · uniffi | TV bundle (xcodebuild) | codesign | codesign verify | App Store | RN-tvOS carries the UI; macOS host |
| HarmonyOS TV | native · NAPI | HAP (reuses HarmonyHap) | hap-sign-tool | `hap-sign-verify` + `hap-structure-check` | AppGallery Connect (TV; no separate Huawei video store channel id) | T-M4-14: deviceTypes includes `tv`, focus / safe area reuse adapter-tv; no hdc TV device, on-device focus walkthrough SKIP |
| Samsung Tizen | wasm · wasm-bindgen | `.wgt` | Tizen author certificate (Samsung signing tool) | `wgt-lint` | Seller Office | web runtime |
| LG webOS | wasm · wasm-bindgen | `.ipk` | platform-side (publish signing) | `ares-package` check | LG Content Store | web runtime |
| Roku OS | **none** (core does not run) | channel bundle (zip) | dev / platform certificate | sideload self-check | Roku channel store | ※experimental: SceneGraph-subset renderer; UI subset + native logic only |
| Amazon Fire TV | native · uniffi | APK | same as Android | same as Android | Amazon Appstore | |
| Vizio SmartCast | wasm · wasm-bindgen | app bundle | none | sandbox self-check | Vizio platform | web runtime |
| Hisense VIDAA | wasm · wasm-bindgen | app bundle | none | sandbox self-check | VIDAA store | web runtime |

### 3.5 L4 per-platform detail (4)

| Platform | Core runtime · bridge | Packager and artifacts | Signing | Artifact verification | Publishing channel | Notes |
|---|---|---|---|---|---|---|
| GrapheneOS | native · uniffi | APK (reuses Android APK chain) | apksigner / community keys (android-keystore) | apksigner verify + `poly verify --reproducible` (same-input two-run) | self-hosted (`self-hosted`; poly-publish has no F-Droid client) | no separate no-GMS flavor — GMS removal is an app-level concern; upstream-build diff not implemented |
| KaiOS | wasm · wasm-bindgen | zip (`manifest.webapp` + UI dist, deterministic archive; `kaios-webapp`) | none (ordinary apps are not signed locally; platform-side signing is KaiStore's) | `kaios-structure-check` (manifest / entry / entries; same implementation as the packager) | none — poly-publish has no kaistore client; KaiStore review | react-dom web stack; device / store legs SKIP-with-print |
| Sailfish OS | native `.so` + Qt/Silica shell (planned) | RPM (planned) | RPM GPG (planned) | rpmlint (planned) | Jolla Store (planned) | the existing linux-rpm chain only emits a desktop Tauri RPM; missing the Qt / Silica QML host, the Sailfish SDK cross toolchain (armv7hl / aarch64), and harbour packaging / signing |
| Ubuntu Touch | native `.so` + WebView shell (planned) | `.click` (Clickable, planned) | Clickable packaging (planned) | Clickable build check (planned) | OpenStore (planned) | the existing Linux deb chain cannot produce `.click` (different format / AppArmor / click manifest); missing Clickable / Ubuntu Touch SDK and the WebView shell |

### 3.6 L5 per-platform detail (7)

| Platform | Core runtime · bridge | Packager and artifacts | Signing | Artifact verification | Publishing channel | Notes |
|---|---|---|---|---|---|---|
| Ubuntu | native (Tauri IPC) | deb / Snap / Flatpak | GPG | lintian | Snapcraft / self-hosted | |
| Fedora | native (Tauri IPC) | rpm / Flatpak | GPG | rpmlint | Flathub / self-hosted | |
| Linux Mint | native (Tauri IPC) | deb / Flatpak | GPG | lintian | self-hosted | |
| Chrome OS | native (Tauri IPC) | deb (Crostini) + PWA | none | deb install smoke + PWA audit | self-hosted / PWA | dual-track delivery |
| FreeBSD | native | pkg | repository signing (optional) | `pkg check` | self-hosted repo / ports | |
| OpenBSD | native | ports | signify | portcheck | ports@ | |
| BSD family (NetBSD / DragonFly, …) | native | pkgsrc, etc. | repository signing | packager self-check | self-hosted repo | |

### 3.7 L6 per-platform detail (11 TargetSpecs)

**Embedded / automotive targets already in TargetSpec.** The table below is generated from the `TargetSpec` declarations in `crates/poly-targets/src/builtin.rs` (`node tools/gen-target-table.mjs`); every field is reproduced verbatim from the registry, so do not edit it by hand. `--check` fails when the table drifts.

<!-- BEGIN GENERATED: l6-embedded-status -->
| Target | Runtime | Bridge | Packager & artifact | Signing | Artifact verification | Publishing channel | experimental | notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `android-automotive` | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `oem-channel` | `false` | AAOS 清单（android.hardware.type.automotive）；车厂渠道，无统一公开 API——发布需 OEM 凭据引用（占位 Publisher，不伪造提交） |
| `autosar` | `staticlib` | `cffi` | `arxml` · `autosar-adaptive` | none | `arxml-schema`, `c-abi-symbols` | `self-hosted` | `false` | 无 UI、自部署；AUTOSAR Adaptive 清单（ARXML）由 poly_engine_* C ABI 单一来源派生（服务 / 端口与导出符号一一对应）；no_std 可选（引擎层 --no-default-features，应用提供 global_allocator / panic_handler） |
| `fitbit` | `none` | `none` | `fba` · `fitbit-build` | none | `fitbit-structure-check` | `fitbit-gallery` | `false` | 核心不直接运行、能力由 JS 侧实现（runtime = none；Fitbit OS 侧 JS SDK 应用包 `.fba`：应用清单 + device-<family>.zip + 可选 companion.zip，结构同官方 SDK 6.1）；fitbit-build 缺席时 CLI 自检 SKIP-with-print，结构断言真跑；签名 / 提审归 Fitbit Gallery 平台侧 |
| `freertos` | `staticlib` | `cffi` | `board` · `freertos-qemu` | none | `c-abi-symbols`, `freertos-qemu-smoke` | `self-hosted` | `false` | 无 UI、cffi、no_std；FreeRTOS 示例板包（mps2-an385 / Cortex-M3，静态库 + 启动 / 中断 / UART / FreeRTOSConfig / Makefile 工程模板）；内核 V11.1.0 按 tag + sha256 现拉不入库；`make smoke` 在 QEMU 中调用 poly_engine_* 并以 semihosting 退出码 0 结束；arm-none-eabi-gcc / qemu-system-arm / 网络缺席时按环境槽 SKIP-with-print |
| `garmin` | `none` | `none` | `iq` · `garmin-monkeyc` | none | `iq-structure-check` | `connect-iq-store` | `true` | 实验：Monkey C 子集受限转译——仅 UI 子集 + 原生逻辑（Monkey C 重写，无 JS 运行时）；工程结构自检可用，Connect IQ 编译与真机实证待补（T-M7-10 触发前置；能力边界见 ui/adapters/garmin/CAPABILITY-BOUNDARY-DRAFT.md） |
| `qnx` | `staticlib` | `cffi` | `image` · `qnx-sdp` | none | `qnx-image-structure`, `c-abi-symbols`, `qnx-qemu-smoke` | `self-hosted` | `false` | headless 核心（可选 HMI 归平台侧）；静态库 + 部署镜像（附 mkifs buildfile，可引导 .ifs / QEMU 归 T-M6-09）；交叉编译需 QNX SDP（$QNX_HOST / $QNX_TARGET）与 Rust QNX target，无 SDP 时按环境槽 SKIP-with-print，不伪造二进制；自部署 |
| `vxworks` | `staticlib` | `cffi` | `library` · `vxworks-wr` | none | `c-abi-symbols`, `vxworks-ld-check` | `self-hosted` | `false` | 无 UI、cffi、no_std；format = `library`（默认，静态库）或 `image`（部署 kit zip，配置 [targets.vxworks].format 覆盖）；wr-cc / wr-ld 缺席时产物用 nm 符号闭包 + 结构断言校验并 SKIP-with-print，绝不伪造可加载镜像 |
| `watchos` | `none` | `none` | `watch` · `watchos-xcodebuild` | `apple-codesign` | `codesign-verify` | `app-store` | `true` | 实验：SwiftUI 伴生模板（核心 API 调用桩 + WatchConnectivity 说明），核心不直接运行（runtime = none）；模板经 `poly init --target watchos` 生成，macOS 宿主 `xcodebuild` 构建；Linux / Windows 发起走远程农场 macOS worker（R3）；签名复用 M2 Apple 链（codesign，凭据只以 apple-codesign 引用出现） |
| `wearos` | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `google-play-wear` | `false` | Wear OS 清单（android.hardware.type.watch，minSdkVersion ≥ 26）；UI 运行时（RN on Wear）为 beta，打包与签名走 Android 基座 |
| `zephyr` | `staticlib` | `cffi` | `west` · `zephyr-west` | none | `c-abi-symbols`, `zephyr-west-build`, `zephyr-qemu-smoke` | `self-hosted` | `false` | 无 UI、cffi、no_std；west 工程模板（zephyr module + 引擎静态库模块链入），qemu_x86 / qemu_cortex_m3 两板 `west build` 验收；最小 CONFIG_* 集随模板版本化；west / Zephyr SDK 缺席时按环境槽 SKIP-with-print |
| `zepp` | `none` | `none` | `zab` · `zeus` | none | `zepp-structure-check` | `zepp-store` | `false` | 核心不直接运行、能力由 Zepp JS API 侧实现（runtime = none）；Zepp 小程序 `.zab`（外层 manifest.json + <md5>.zpk，zpk 内含 device.zip / app-side.zip，结构同 zpm）；zeus / 模拟器缺席时安装与工具链校验 SKIP-with-print，结构断言真跑；签名 / 提审归 Zepp 商店平台侧 |
<!-- END GENERATED: l6-embedded-status -->

**All five wearable targets are now in TargetSpec.** The platform matrix for watchOS / Wear / Fitbit / Garmin / Zepp is the table above and nothing else (generated from TargetSpec; there is no second hand-written copy); `watchos` / `garmin` have `experimental = true`, while Wear OS is platform-level (`experimental = false`) — its UI runtime (RN on Wear) is beta, and packaging and signing use the Android base.

### 3.7.1 Experimental targets: capability / gap / alternatives (watchOS / Garmin)

`watchos` / `garmin` keep `experimental = true` (constrained tier, continue decision). The full three-part document is [EXPERIMENTAL-TARGETS.en.md](EXPERIMENTAL-TARGETS.en.md) (Chinese: [EXPERIMENTAL-TARGETS.md](EXPERIMENTAL-TARGETS.md)): every gap has an alternative or an explicit "no alternative"; the Wear OS beta boundary is recorded in the same chapter, and no ※experimental marker is added for it. Three-way consistency is checked by `node tools/check-experimental.mjs`.

### 3.8 L7 per-platform detail (3)

| Platform | Core runtime · bridge | Packager and artifacts | Signing | Artifact verification | Publishing channel | Notes |
|---|---|---|---|---|---|---|
| PlayStation | official SDK plugin | platform bundle | vendor tools | vendor toolchain | Sony license required | SDKs never in-repo; local path discovery |
| Xbox | GDK plugin | platform bundle | vendor tools | vendor toolchain | Microsoft license required | same |
| Nintendo Switch | NintendoSDK plugin | platform bundle | vendor tools | vendor toolchain | Nintendo license required | same |

> Targets marked ※experimental have constrained runtimes; L7 exists only in licensed environments — the public repository validates the plugin contract against mocks.

## 4. Architecture

### 4.1 Crate responsibilities and key interfaces

| Crate | Responsibility | Key interface (sketch) | Key deps |
|---|---|---|---|
| `poly-cli` | command parsing, progress, completions, report rendering | `poly <verb> [--target …] [--profile ci\|local]` | clap, indicatif |
| `poly-core` | JobSpec, DAG scheduling, event bus, diagnostics | `trait Stage { fn plan(&self, job) -> Plan; fn run(&self, ctx) -> StageOutcome }` | tokio, petgraph |
| `poly-targets` | TargetSpec registry (built-in + plugin-registered) | `trait TargetRegistry { fn resolve(&self, id) -> TargetSpec }` | serde, toml |
| `poly-toolchain` | SDK / NDK / Xcode / emsdk probing and installation | `trait Probe { fn locate(&self) -> Option<Toolchain>; fn install(&self) -> Result<()> }` | which, semver |
| `poly-bundle` | packager matrix | `trait Packager { fn id(&self) -> TargetId; fn package(&self, inputs, spec) -> Artifact }` | zip, tar, objcopy |
| `poly-sign` | signing, notarization, hardening; vault adapters | `trait Signer { fn sign(&self, artifact, credential_ref) -> Signed }` | keyring, sigstore |
| `poly-publish` | store and platform API clients | `trait Publisher { fn submit(&self, artifact, channel) -> Submission }` | reqwest |
| `poly-cache` | content-addressed cache | `fn key(inputs: &BuildInputs) -> Digest; fn get/put(Digest) -> Option<Artifact>` | blake3 |
| `poly-plugin` | WASM plugin host (sandboxed) | wasmtime instantiation + WIT interface (see 4.5) | wasmtime, wit-bindgen |

### 4.2 Data model

**JobSpec** (the full description of one build request, flowing through the DAG):

```text
JobSpec {
  id, profile(ci|local), targets[TargetSpec],
  stages[Stage], cache_policy, secrets: [CredentialRef],   # references only
  budget: { size, startup_ms }                              # artifact budgets
}
```

**BuildEvent** (event bus driving progress bars, reports, and cache writes):

```text
BuildEvent =
  Planned{targets} | Probed{toolchains} | Compiled{crate, triple}
| UiBundled{adapter} | Signed{profile} | Packaged{format, bytes}
| Verified{checker, ok} | Published{channel, submission}
| Failed{stage, code, hint} | Cached{key, hit: bool}
```

**Diagnostic report**: `Failed` events carry stable error codes (`poly::E_PKG_0007` style) plus fix hints; `poly build --json` emits the full machine-readable event stream.

### 4.3 TargetSpec full field reference

| Field | Type | Meaning | Example |
|---|---|---|---|
| `id` | string | target identifier (= `[targets.*]` table name) | `tizen-tv` |
| `tier` | 1–7 | support tier (drives CI intensity) | `3` |
| `hosts` | list | hosts allowed to initiate the build | `["win","linux","macos"]` |
| `runtime` | enum | core runtime: `native` / `wasm` / `staticlib` / `none` | `wasm` |
| `bridge` | enum | bridge: `uniffi` / `napi` / `wasm-bindgen` / `cffi` / `tauri-ipc` / `none` | `wasm-bindgen` |
| `ui` | list | render adapters | `["react-dom"]` |
| `package` | table | packager and artifact format | `{ format: "wgt", builder: "tizen-wgt" }` |
| `signing` | string | credential profile reference (vault entry name) | `"tizen-author"` |
| `verify` | list | artifact verifiers | `["wgt-lint"]` |
| `publish` | list | publishing channel clients | `["tizen-seller-office"]` |
| `experimental` | bool | constrained/experimental (affects docs and CI labeling) | `false` |
| `notes` | string | boundary and degradation notes (rendered into generated doc tables) | `"web runtime, no push"` |

**`--target all` semantics**: resolves to every **buildable** target under the current profile — excluding license-gated L7 and targets whose host/credentials are unreachable; the plan stage prints the exclusion list and reasons (`--explain` expands them), and explicitly listed targets go through the same reachability check.

**Multiple artifacts**: `package` declares the **primary artifact** (AAB / MSI / DMG / AppImage, …); each platform's secondary matrix (APK, NSIS, PKG, deb/rpm/Flatpak/Snap, …) becomes a multi-artifact field when the M1 packager matrix lands (a prerequisite of T-M1-05..08; this section updates with the implementation).

### 4.4 Bridge matrix

| Host | Bridge | Artifact |
|---|---|---|
| Node / Taro runtime | napi-rs | .node native module |
| Kotlin / Swift | uniffi | AAR / XCFramework |
| Web / WebView / KaiOS / TV web | wasm-bindgen | .wasm + JS glue |
| QNX / RTOS / automotive | cffi | static library + C headers |
| Desktop (Tauri) | Tauri IPC | core is the host; no cross-language bridge |

The bridge layer is generated from one interface definition across all four ends by an in-workspace code generator, so the bindings cannot drift apart.

### 4.5 Plugin system (WASM sandbox)

Plugins are described by a WIT interface and run in a wasmtime sandbox; by default there is **no network and no filesystem** — capabilities are explicitly granted, WASI-capability style:

```wit
world polypack-target {
  // read-only inputs
  import inputs: func() -> build-inputs
  // the plugin declares its own target spec
  export describe: func() -> target-spec
  // the plugin carries the packaging stage
  export package: func(bundle: ui-bundle, lib: core-lib) -> result<artifact, error>
}
```

Constraints: plugin artifacts carry a content digest (SRI) check; the interface is versioned as `target-spec/v1`, and breaking changes get a new version number; built-in targets and plugins share one interface (principle 6).

### 4.6 Versioning policy

| Object | Policy |
|---|---|
| Rust | MSRV = the two most recent stable releases; edition 2024 |
| `polypack.toml` | top-level `schema = 1` field (absent = 1; unknown versions are rejected with an upgrade hint) |
| TargetSpec / plugin interface | versioned as `target-spec/v1` (breaking changes get a new version, see 4.5) |
| This repository | tags and GitHub Releases increment `vX.Y.Z` semantically since v1.0.0 |
| Config strictness | `deny_unknown_fields`: unknown keys are rejected outright (config is never silently dropped); new keys count as breaking changes |

## 5. Build pipeline

### 5.1 DAG stages

```text
plan → probe → compile → bundle(ui) → package → sign → verify → publish
```

- **plan**: parse polypack.toml into a JobSpec; unknown targets, dependency cycles, and missing credential references fail here (fail fast, before any compilation);
- **probe**: toolchain probing and installation; Apple targets decide "local or remote farm" at this point;
- **compile / bundle(ui)**: two parallel branches (Rust cross-compile ↔ React bundling) meeting at package;
- **package → sign → verify**: independently re-runnable (`poly retry --from sign|verify`); the signing stage re-fetches credentials;
- **publish**: only with `--publish`; after submission, review status is polled and receipts archived.

### 5.2 Cache keys and invalidation

```text
cache_key = blake3(
  source tree digest ‖ lockfiles ‖ TargetSpec version ‖ toolchain versions ‖ build parameters (profile)
)
```

Tiered caches: **library-level** (compile output) → **package-level** (package output) → **submission-level** (publish records). Invalidating any tier cascades recomputation; cross-machine sharing goes through an object-storage backend (a trait; the default is the local filesystem).

### 5.3 Reproducibility

- Except for digital signatures and timestamps, artifacts are byte-identical across builds (`poly verify --reproducible`);
- `SOURCE_DATE_EPOCH` is injected at compile time; absolute paths and host environment variables are banned from artifacts;
- The GrapheneOS target also uses `poly verify --reproducible` (same-input two-run comparison); a diff against upstream builds is not implemented.

### 5.4 Build-time budgets

Beyond artifact budgets (size / startup latency, see JobSpec), the pipeline itself carries build-time budgets, and breaches warn:

| Scenario | Budget (soft target) |
|---|---|
| Cache fully hit (single target) | < 30s |
| Incremental build (single target, source-level change) | < 3min |
| L1 cold build (local five-target matrix) | < 20min |

### 5.5 Cleanup and retention policy

Disks never balloon silently: caches and artifacts carry caps and LRU eviction, configured in `polypack.toml`:

| Object | Default retention | Cleanup |
|---|---|---|
| Build cache (CAS) | most recent 30 days or 20 GiB, whichever comes first | LRU eviction on breach; `poly clean --cache` manually |
| Build artifacts | the most recent 10 builds per target | `poly clean --artifacts` |
| Submission records | forever (audit needs them) | never cleaned |

`poly clean` is dry-run by default (listing reclaimable entries and space); `--yes` executes and prints reclaimed-space stats (delivery task: T-M1-15).

## 6. UI adapter layer

Package structure:

```text
@polypack/ui          # design system: tokens (color/spacing/type) + headless primitives + patterns
@polypack/adapter-dom     # desktop / web (react-dom)
@polypack/adapter-native  # Android family / iOS family / HarmonyOS (React Native / RNOH)
@polypack/adapter-taro    # mini-program family (Taro, React syntax)
@polypack/adapter-tv      # TV (react-dom + focus navigation system)
@polypack/adapter-wear    # wearables (compact tokens, experimental)
```

Adapter contract (sketch): `RendererAdapter { mount(root, app), platformStyle(tokens) -> SkinnedTokens, capabilities(): { focusNav, safeArea, gestures } }` — one page component tree, interactions degraded per `capabilities` (no pointer → focus ring; small screen → compact spacing).

**TV focus navigation system** (the key L3 differentiator): D-pad traversal graph, focus-ring rendering, Back-key semantics (focus memory on return), remote numeric-key shortcuts; its smoke test is "complete the core tasks without a pointer".

## 7. Security and key management

- **Credential-reference model**: `signing = "profile"` → the vault (keyring / Vault / CI memory injection); plaintext credentials in the repo fail secret-scan;
- **secret-scan**: built-in rules (keystore/JKS, certificate private keys, API-key patterns) checked both on commit and during builds;
- **Artifact redaction**: artifact metadata is scanned before packaging — absolute paths, environment variables, and debug symbols stripped (retention configurable);
- **Plugin sandbox**: no network/filesystem by default, explicit capability grants plus SRI verification;
- **Supply chain**: `cargo-deny` (licenses/vulnerabilities/duplicates) + `cargo-audit`; lockfiles are mandatory (delivery task: T-M0-11).

## 8. Testing and CI matrix

Test pyramid:

| Level | Content | Frequency |
|---|---|---|
| Unit | crate internals, TargetSpec parsing, cache keys | every commit |
| Contract (one per target) | packaging + verifier + artifact structure assertions (golden comparison) | every commit (mock mode) / nightly (real mode) |
| Smoke | installable + core tasks (incl. TV focus, Wear forms) | nightly / before release |
| Full | real devices/emulators/store sandboxes | before release |

CI host × target-tier matrix:

| Host | L1 | L2 | L3 | L4–L6 | L7 |
|---|---|---|---|---|---|
| ubuntu-latest | Web/Linux full | toolchain mocks full | web family full | cross-compile full | interface mocks |
| macos-14 | macOS/iOS full | mocks | tvOS full | cross-compile | interface mocks |
| windows-2022 | Windows full | mocks | — | cross-compile | interface mocks |
| nightly extension | — | real devtools CLIs | emulator smoke | QEMU smoke (BSD native build + install; QNX / Zephyr / FreeRTOS) | — |

> The BSD native-build leg runs in [`bsd-ci`](../.github/workflows/bsd-ci.yml) (nightly 20:07 UTC +
> manual dispatch): inside FreeBSD 14.5 / OpenBSD 7.9 QEMU VMs it runs a native `poly build` plus an
> install smoke (`pkg install` + `pkg check -s` on FreeBSD; `make package` + `pkg_add` / `pkg_check`
> on OpenBSD). The FreeBSD leg also exercises self-hosted pkg-repo signing via
> `poly release --target freebsd` and asserts second-run idempotency. The OpenBSD leg is
> best-effort: hosted runners have no KVM, so when the image cannot boot under TCG it records
> SKIP-with-print (missing logs are archived as a warning) until the image or boot path is fixed.
> Build logs, the signed repo, receipts, and packages are retained with the job for 14 days.
>
> `embedded-ci` (nightly 19:27 UTC + manual dispatch) derives its matrix from
> `poly targets --tier 6 --headless --json` (a new tier-6 target joins automatically), then runs
> QNX / AUTOSAR / VxWorks / Zephyr / FreeRTOS cross-compiles and QEMU smokes on ubuntu-latest;
> toolchains are cached under pinned versions, missing SDP / wr-cc and friends record
> SKIP-with-print with a REAL/SKIP job summary; logs, serial output, and result.json are retained
> for 14 days.
>
> `wear-smoke` (mock per push; nightly + manual real run) runs the Wear form suite (input / swipe /
> back) behind the KVM + `vars.POLYPACK_WEAR_FORM_APK` gate; when the gate is absent it records
> SKIP-with-print and writes an all-SKIP result.json. `wear-release-smoke` is the release gate
> (`workflow_call` / manual): a release pipeline uses workflow_call to make it a prerequisite; a
> failed or non-real device leg blocks the release, with evidence retained for 90 days. Eleven CI workflows total: `rust-ci` /
> `docs-check` / `secret-scan` / `mobile-contract` / `miniprogram-contract` / `tv-smoke` / `bridge` /
> `bsd-ci` / `embedded-ci` / `wear-smoke` / `wear-release-smoke`.

## 9. Milestones and task breakdown

> Estimates assume 1.5 FTE; every task is "done" when it is demonstrable. The task-level breakdown (113 tasks / 279.5 person-days, with roles, dependencies, acceptance criteria, and scheduling notes) is in **[TASKS.en.md](TASKS.en.md)** and [TASKS.md](TASKS.md).

### M0 foundation (2 weeks)

1. Cargo workspace skeleton (9 crate stubs + dependency-direction constraints);
2. three-host CI (Linux / macOS / Windows) with Rust build caching;
3. `polypack.toml` schema (serde + exported JSON Schema);
4. TargetSpec DSL + full declarations for the seven L1 targets;
5. `poly doctor` (toolchain probe report);
6. diagnostics framework (error-code prefix scheme + `--json` event stream);
7. doc-sync check wired into CI (`render-diagrams.mjs --check` verifies SVG/JSON consistency).

**Acceptance**: `poly doctor` correctly reports missing local toolchains; the schema covers every L1 field; CI is green on all three hosts.

### M1 desktop & web (4 weeks)

1. Tauri packaging pipeline (triple detection → core compile → bundling → artifact);
2. web pipeline (vite-driven + PWA manifest generation);
3. first `poly-bundle` packagers: MSI/NSIS, DMG, AppImage/deb, static site;
4. `poly-cache` (blake3 CAS + local backend + object-storage trait);
5. `poly dev` hot preview v0 (hot update inside the web host);
6. `poly init` sample-app template (minimal Rust core + React UI skeleton);
7. **English-labeled diagrams** (English-label JSON → `docs/assets/en/*.svg`).

**Acceptance**: the sample app packages for Win/macOS/Linux/Web; second-run cache hit ≥ 90%; English diagrams merged.

### M2 mobile (5 weeks)

1. uniffi bridge generator (Rust interface → Kotlin + Swift bindings);
2. `@polypack/adapter-native` v0 (RN host + baseline focus/navigation);
3. Android packaging (AAPT2 + bundletool + apksigner, Gradle-free);
4. iOS packaging (xcodebuild archive + exportArchive + notarytool integration);
5. vault integration (keyring + Vault backend trait);
6. secret-scan wired into commits and CI;
7. remote-farm protocol v0 (JobSpec serialization + worker pull-and-execute).

**Acceptance**: sample APK/IPA installs and signatures verify; zero keys in-repo (scan green); a Linux host produces an IPA through the farm. Farm deployment and the "origin / build site" split are documented in [FARM.en.md](FARM.en.md).

### M3 HarmonyOS & mini programs (4 weeks)

1. RNOH adapter (HarmonyOS host + NAPI bridge integration);
2. HAP packaging (hvigor invocation + hap-sign-tool signing + verification);
3. `@polypack/adapter-taro` (React syntax → WeChat/Alipay/Douyin/Quick App targets);
4. wasm core into mini-program sandboxes (API allowlist + size budget);
5. publish clients for the three platforms (upload + submit + status polling);
6. mini-program contract tests (devtools CLI checks + golden comparison).

**Acceptance**: all three mini programs preview in devtools; HarmonyOS HAP signs and installs; submission scripting is re-runnable.

### M4 TV family (4 weeks)

1. `@polypack/adapter-tv` (focus navigation system + 10-foot tokens);
2. Android TV / Fire TV (Leanback manifest + smoke) and HarmonyOS TV (reusing the M3 HAP chain + TV adaptation);
3. tvOS (RN-tvOS + reuse of the M2 Xcode chain);
4. Tizen / webOS / VIDAA / Vizio (web bundles → wgt/ipk/app bundles + verifiers);
5. Roku constrained-renderer spike (documented capability boundary of the SceneGraph subset);
6. remote-control smoke suite (core tasks with no pointer).

**Acceptance**: focus traversal has no dead ends; wgt/ipk verifiers pass; Roku's capability boundary produces a decision (continue / degrade).

### M5 desktop distributions & BSD (3 weeks)

1. deb/rpm/Flatpak/Snap/AppImage packagers maturing (repo metadata + GPG);
2. FreeBSD / OpenBSD native build targets (CI VM/cross decision finalized);
3. ChromeOS dual-track (Crostini deb + PWA);
4. package-repo signing and an end-to-end `poly release --target fedora,freebsd`.

**Acceptance**: each format installs on its host/container; BSD targets run in CI.

### M6 embedded & automotive (5 weeks)

1. core layering (feature split inside `poly-core` for a no_std engine layer; no new crate);
2. QNX / AUTOSAR / VxWorks / Zephyr / FreeRTOS cross-compile targets + CI matrix;
3. Android Automotive (Android chain reuse + OEM channel placeholders);
4. QEMU smoke (QNX / Zephyr).

**Acceptance**: every headless target is in the CI compile matrix; QNX smoke passes.

### M7 wearables (4 weeks)

1. Wear OS smoke (Android chain reuse + Wear manifest);
2. watchOS companion-template generator (SwiftUI, marked experimental);
3. Fitbit (JS SDK) and Zepp (JS API) packagers;
4. Garmin transpile spike (Monkey C subset, marked experimental).

**Acceptance**: Wear OS smoke on device; experimental targets fully documented (capability/gap/alternatives).

### M8 ecosystem GA (6 weeks)

1. WASM plugin SDK (published WIT + template repo + signing/SRI);
2. remote farm v1 (deployment guide + concurrency/isolation/cache sharing);
3. store publishing pipelines GA (retries, receipt archival, review polling);
4. GA documentation and technical review (including the status table for all 49 targets).

**Acceptance**: a third-party developer adds a custom target and ships a package in under a day, following the docs.

## 10. Risk register

| # | Risk | Impact | Mitigation |
|---|---|---|---|
| R1 | Console vendor licensing and NDA SDKs | L7 cannot ship publicly | Plugin shape + local discovery by licensed developers; the public repo keeps interfaces and mocks only |
| R2 | Roku / watchOS / Garmin lack a native React runtime | narrowed feature surface | Explicitly labeled "experimental / constrained" (the `experimental` field lands in doc tables); capability boundaries ship with the artifact notes; **M4 decision (T-M4-12): Roku continues in constrained mode** — `experimental` stays, and on-device sideload evidence is the trigger prerequisite for conditional task T-M4-15 |
| R3 | Apple targets require a macOS host | Windows/Linux users cannot build an IPA locally | Remote build farm (protocol v0 in M2 → v1 in M8); docs separate "where you initiate" from "where it builds"; deployment, credentials, and signature checks in [FARM.en.md](FARM.en.md) |
| R4 | Store / mini-program review cannot be fully automated | human step at publish time | Automation stops at "submit"; review polling + receipt archival; no promise of automatic approval |
| R5 | Signing keys and credential leakage | supply-chain incident | Reference model + secret-scan + artifact redaction; CI secrets only in memory |
| R6 | 50+ target matrix maintenance cost | target rot (platform rule changes) | Per-target contract tests (section 8); platform rule changes go through TargetSpec versioning |
| R7 | Bilingual documentation drift | CN/EN divergence | the sync convention (section 11) is a commit checklist; diagrams are single-sourced (JSON) |
| R8 | Upstream churn in Taro / RN / RNOH | adapter rework | keep adapter interfaces thin (stable API surface only); quarterly upstream milestone review |
| R9 | Toolchain download sources unstable (SDK/NDK/emsdk) | probe-stage failure rate | probe caching + mirror configuration + checksum pinning |
| R10 | Artifact size bloat (wasm in mini-program sandboxes) | platform size limits tripped | size budgets (`budget.size`) + wasm-opt + per-target trimming |

## 11. Documentation sync convention (commit checklist)

Any change affecting **architecture / features / flows / platform matrix** must, in the same commit:

1. Update `docs/diagrams/*.json` (the single source of truth for diagrams) and run `node tools/render-diagrams.mjs` to regenerate `docs/assets/*.svg` (CI verifies consistency in `--check` mode; the bottom-right Pax badge is on by default, `--no-brand` disables it);
2. Sync `README.md` and `README.en.md` (bilingual mirrors);
3. Sync `docs/ROADMAP.md` and `docs/ROADMAP.en.md`; the task plan follows the same rule (`docs/TASKS.md` / `docs/TASKS.en.md`), with additions/removals and estimate changes landing in both files in one commit;
4. Platform matrix additions/removals = update together: the README tiered list (×2 languages) + this file's per-platform detail (×2 languages) + milestone acceptance + the TargetSpec;
5. Pax is generated from a single source by `tools/build-pax.mjs` (twelve colorful expressions, each with its own motion + the full-cycle animated version + the project icon `pax-mark.svg` + the expression contact sheet `pax-moods.svg` + the local playground `docs/pax.html`); re-run it after art or expression changes and sync both READMEs (CI verifies drift with `node tools/build-pax.mjs --check`);
6. Keep `docs/EXPERIMENTAL-TARGETS.md` and `docs/EXPERIMENTAL-TARGETS.en.md` in sync for the experimental targets (`watchos` / `garmin`) capability / gap / alternatives (every gap keeps an alternative or explicitly says "no alternative"; `node tools/check-experimental.mjs` checks TargetSpec / documentation tables / CI three-way consistency).

## 12. Glossary

| Term | Meaning |
|---|---|
| TargetSpec | a target platform's capability declaration (runtime, bridge, packaging, signing, verification, publishing) |
| JobSpec | the full description of one build request, flowing through the pipeline DAG |
| Tier (L1–L7) | support tier; drives engineering strategy and CI intensity |
| Packager / Signer / Publisher | strategy interfaces for the package / sign / publish stages |
| CAS | content-addressed storage (cache backend) |
| Golden | the reference artifact used for comparison in contract tests |
| WIT | WebAssembly Interface Types, the plugin interface description language |
| 10-foot UI | TV-optimized interfaces (readable at distance, remote-operated) |
| Bridge | the binding layer between the Rust core and the host language |
| Contract test | the per-target automated check "package + verify + structure assertions" |

## 13. Extension directions (incubation pool)

Projected from the 2026-10-14 gap audit: the directions below **do not consume the current M0–M8 schedule**; each carries a "promotion trigger" and a "first step", and enters the roadmap only after the trigger fires and a needs-validation review passes (to prevent building on assumptions).

| # | Extension | Value | Promotion trigger | First step |
|---|---|---|---|---|
| E1 | **SBOM / build provenance** | SPDX/CycloneDX manifests plus SLSA-style provenance per artifact; a supply-chain compliance staple (baseline scanning already lands in T-M0-11) | first compliance or supply-chain audit demand | generate SBOMs alongside publish, reusing the cache-key input list |
| E2 | **monorepo / multi-app workspace** | one config managing several apps sharing a Rust core, for larger teams | first multi-app repository user | design the `[workspace]` table and composite JobSpec semantics |
| E3 | **Incremental releases `poly release --changed`** | republish only changed targets (decided by the content-addressed cache), cutting multi-target CI time by an order of magnitude | pilot right after M1 ships the cache | prototype change detection on cache keys |
| E4 | **Production hot update / OTA channel** | completes the "release → iterate" loop with an online update channel | first users asking for over-the-air updates | survey per-platform update mechanisms (Play In-App Updates / mini-program version management / Tauri updater) |
| E5 | **Store metadata pipeline** | versioned, multilingual management of listing copy / screenshots / keywords (publish currently only uploads and submits) | the first M2/M3 store release lands | define the metadata directory structure and its checker |
| E6 | **GUI target-matrix view / build history** | upgrade poly-gui from "console" to a usability dashboard | the M1 Tauri pipeline lands | design the matrix view's data source (the BuildEvent stream) |
| E7 | **Pre-release device preview matrix** | real-device/emulator matrix previews (also supplies the smoke-test devices noted in the audit) | M4 TV or M7 wearables smoke testing hits a device bottleneck | survey device-farm options and cost |
| E8 | **Decision records (ADR)** | archive the "why" behind choices (bilingual docs, no crate split, wasm plugins, …) | can start immediately | `docs/decisions/` exists (template + 3 backfilled records); append as needed |

## 14. Open questions (to decide)

- **License**: settled — **Apache-2.0** (the `LICENSE` file is in place); re-evaluate compatibility if GPL-family toolchain installers are ever included;
- **Plugin marketplace**: distribution model for WASM plugins (official registry vs decentralized index);
- **Remote farm**: self-hosted only (docs + deployment guide) or a hosted offering.
