# PolyPack

**Package once, deliver everywhere** — a Rust core × React UI adapters × 50+ target platforms

[简体中文](README.md) · [Roadmap](docs/ROADMAP.en.md) · [Architecture](#architecture) · [Platforms](#platform-matrix)

> **Project status: M8 ecosystem GA delivered and closed** (2026-10-18, 13/13 — the wasmtime plugin sandbox and the `target-spec/v1` WIT contract (deny-by-default, explicit grants, three resource limits), SRI + GPG detached-signature chain (CredentialRef trust root), the standalone template repo and `poly plugin new|digest`, built-in targets unified behind `TargetProvider` (built-ins are official plugins, no builtin special-casing), farm v1 (concurrency/worker pool/timeout retries/isolation/shared CAS), the publishing GA (backoff/idempotency/receipt v2/`poly retry --from publish`) and the 49-target status table, plus the bilingual [GA docs](docs/GA.en.md), technical review and the "add a target in one day" rehearsal (an independent developer agent completed scaffolding→custom packaging in 38 min). M0–M8 milestones closed (conditional tasks and environment slots recorded in TASKS); M7 details: adapter-wear compact tokens, the Wear OS target with its dual form-smoke tracks (incl. a release gate), the watchOS SwiftUI template plus packaging/signing chain (experimental), Fitbit `.fba` / Zepp `.zab`, the Garmin Monkey C spike (experimental), the experimental-target three-section docs, and wearable contracts with goldens; xcodebuild / codesign / Wear-device environment slots record SKIP-with-print, and the Garmin iq packager is a recorded experimental gap). Available commands: `poly doctor` / `poly init` / `poly build` (deb / AppDir / web static bundles; **Android AAB/APK**; **iOS unsigned IPA** on macOS hosts; **HarmonyOS HAP packaging + hap-sign-tool signing/verification**; **WeChat/Alipay/Douyin/Quick-App bundles** (wasm sandbox outlet shipped inside); **TV family: android-tv / fire-tv (Leanback), tvos (Apple chain reuse), tizen-tv / webos-tv / vidaa-tv / smartcast-tv (web tier)**; **wearables: Wear OS / Fitbit / Zepp / watchOS※experimental / Garmin※experimental**; sign/verify wired into the DAG; `[budget]` size gate warn/fail) / `poly bridge` (uniffi + **ohos NAPI** + **wasm** outlets) / `poly farm worker|submit` (remote farm) / `poly scan` (secret scan) / `poly verify --reproducible` / `poly targets` (target-matrix JSON for CI derivation) / `poly retry --from sign|verify` / `poly plugin new|digest` (plugin scaffolding and SRI digest) / `poly dev` / `poly clean` / `poly release` (self-hosted rpm/deb/pkg repo metadata + signing + receipt archival, `--target fedora,freebsd`). Eleven workflows green across three hosts (new `wear-smoke` nightly form smoke and `wear-release-smoke` release gate; `bsd-ci`: native FreeBSD 14.5 QEMU build + install smoke runs for real; OpenBSD 7.9 is a best-effort leg that records SKIP-with-print when the image cannot boot under TCG on KVM-less runners; nightly. New `embedded-ci`: the tier-6 matrix is derived from TargetSpec, with QNX / Zephyr / FreeRTOS QEMU smokes and a REAL/SKIP job summary, missing toolchains recorded as SKIP-with-print; nightly); the tvOS chain proven on macos-14 (XCFramework→archive→IPA 7,065 B); Android TV device smoke 6/6 walk + "core task with no pointer" + focus-ring pixel evidence; the remote-control smoke suite runs three legs, 14 scenarios / 69 key steps, all green. Milestone and environment-slot notes live in [docs/TASKS.en.md](docs/TASKS.en.md); planning in [docs/ROADMAP.en.md](docs/ROADMAP.en.md).
>
> **Note on diagram labels:** the embedded diagrams are labeled in Chinese (the project's primary language). English-labeled variants are scheduled for milestone M1.

---

## Meet Pax

![Pax's animated expression loop: smile → joy → crying → dejected → shocked → angry → coy → cute → ingratiating → knocking → covering face → sticking tongue](docs/assets/pax.svg)

**Pax** is the PolyPack project pet — a pocket-sized packing robot. Its rust-orange shell is a nod to Rust; its cyan screen face and orbit stamp echo React. It packs your one codebase into a little parcel, sticks a shipping label on it, and delivers it to phones, watches, and TVs. A rounded cardboard-box body, stubby arms and legs, and a tiny antenna make it look like a cartoon courier on the job.

Pax is animated and multi-form: it idly bobs and cycles through twelve colorful expressions — **smile / joy / crying / dejected / shocked / angry / coy / cute / ingratiating / knocking / covering face / sticking tongue**; every individual expression carries its own motion too — crying drips tears, shock shivers and sweats, anger throbs and steams, coy floats hearts, knocking taps with its parcel, dejection sighs and sinks, sticking tongue wiggles... All pure CSS, no scripts; with "reduce motion" enabled every form settles into its static pose. All twelve at a glance (every one in the grid is moving):

![Pax's twelve colorful expressions, each animated: smile / joy / crying / dejected / shocked / angry / coy / cute / ingratiating / knocking / covering face / sticking tongue](docs/assets/pax-moods.svg)

Individual expression files (each with its own motion): [smile](docs/assets/pax-smile.svg) · [joy](docs/assets/pax-joy.svg) · [crying](docs/assets/pax-cry.svg) · [dejected](docs/assets/pax-dejected.svg) · [shocked](docs/assets/pax-shock.svg) · [angry](docs/assets/pax-angry.svg) · [coy](docs/assets/pax-coquettish.svg) · [cute](docs/assets/pax-cute.svg) · [ingratiating](docs/assets/pax-ingratiating.svg) · [knocking](docs/assets/pax-knock.svg) · [covering face](docs/assets/pax-facepalm.svg) · [sticking tongue](docs/assets/pax-tongue.svg); open [docs/pax.html](docs/pax.html) locally to switch expressions by hand.

**Brand rule**: Pax *is* the PolyPack project icon — the docs favicon ([pax-mark.svg](docs/assets/pax-mark.svg)), the mini badge in the bottom-right corner of generated diagrams, and future CLI / GUI / installer icons all use it; the twelve expressions map onto build states (success = joy · failure = crying · exception = shocked · needs-confirmation = coy · idle = smile). Every form is generated from a single source by `tools/build-pax.mjs`.

Pax shares the same color variables as every diagram in this repository — you will meet its rust orange and orbit cyan again in the architecture diagram.

## What PolyPack is

PolyPack is a **write-once, package-everywhere build tool**: a single Rust core uses a declarative target registry (TargetSpec) to compile, sign, package, and publish one **Rust business core + React interface** to **50+ target platforms** — from Android / iOS / HarmonyOS / Windows / macOS / Linux / Web, all the way to mini programs, quick apps, TVs, car systems, wearables, embedded RTOS, and (with vendor licensing) game consoles.

### Why it exists

- **Fragmented targets.** Multi-platform builds today are separate fiefdoms — Gradle for Android, Xcode for iOS, a different vendor IDE per mini-program platform, bespoke packaging scripts per desktop OS. PolyPack collapses them into one `poly` command.
- **One stack.** Business logic in Rust, written once, reused everywhere; UI in React, written once, re-skinned per platform by the adapter layer.
- **One chain.** Toolchain probe → cross-compile → UI bundle → sign / notarize / harden → package → store submission, orchestrated, cached, and reproducible end to end.
- **Extensible.** A new platform is a TargetSpec plus a WASM plugin — no waiting for an official release.

### How it works (the 90-second version)

```bash
poly release --target android,ios,wechat,tizen --publish
```

1. Load `polypack.toml` and resolve a **TargetSpec** for every requested target (runtime, package format, signing requirements, distribution channel);
2. **Probe toolchains** (Android SDK/NDK, Xcode, emsdk, …) and reuse the content-addressed build cache incrementally;
3. **Cross-compile Rust** and **bundle the React adapter layer** in parallel; artifacts flow into the packager matrix;
4. **Sign → notarize → harden → verify**, then package per target: APK / IPA / HAP / MSI / DMG / AppImage / wgt / ipk / mini-program bundles… and emit a build report.

The full 13-step journey is in [Request lifecycle](#request-lifecycle) below.

---

## Platform matrix

| Tier | Platform | UI runtime | Packaging / distribution |
|---|---|---|---|
| L1 | Android | React Native · Material 3 | AAB / APK → Google Play |
| L1 | iOS | React Native · HIG | IPA → App Store |
| L1 | HarmonyOS | RN for OpenHarmony · HarmonyOS Design | HAP → AppGallery |
| L1 | Windows | react-dom (Tauri) | MSI / NSIS → MS Store |
| L1 | macOS | react-dom (Tauri) | DMG / PKG + notarization |
| L1 | Linux | react-dom (Tauri) | AppImage · deb · rpm · Flatpak · Snap |
| L1 | Web | react-dom | static bundle + PWA |
| L2 | WeChat Mini Program | Taro (React syntax) | mini-program bundle → WeChat review |
| L2 | Alipay Mini Program | Taro | mini-program bundle → Alipay review |
| L2 | Douyin Mini Program | Taro | mini-program bundle → Douyin review |
| L2 | Quick App (快应用) | Taro / Quick App framework | .rpk → alliance stores |
| L2 | TikTok Minis | HTML5 + Minis bridge | zip → TikTok platform |
| L2 | Google Play Instant | React Native | instant-enabled AAB |
| L2 | Facebook Instant Games | HTML5 | zip → Meta platform |
| L2 | Snapchat Mini | HTML5 | zip → Snap platform |
| L2 | LINE Mini App | LIFF (web) | LIFF app → LINE platform |
| L3 | Android TV | RN · Leanback | APK → Play (TV track) |
| L3 | Apple tvOS | react-native-tvos | TV bundle → App Store |
| L3 | HarmonyOS TV | RNOH | HAP → Huawei video store |
| L3 | Samsung Tizen (TV) | react-dom (web bundle) | .wgt → Seller Office |
| L3 | LG webOS | react-dom (web bundle) | .ipk → LG Content Store |
| L3 | Roku OS | constrained renderer (SceneGraph subset) ※experimental | channel bundle → Roku channel store |
| L3 | Amazon Fire TV | React Native (Android) | APK → Amazon Appstore |
| L3 | Vizio SmartCast | react-dom (web bundle) | app bundle → Vizio platform |
| L3 | Hisense VIDAA | react-dom (web bundle) | app bundle → VIDAA store |
| L4 | GrapheneOS | React Native (no-GMS build) | APK → self-signed / F-Droid |
| L4 | KaiOS | react-dom (web stack) | zip → KaiStore |
| L4 | Sailfish OS | Qt / Silica shell + WebView | RPM → Jolla Store |
| L4 | Ubuntu Touch | WebView shell (Clickable) | .click → OpenStore |
| L5 | Ubuntu | react-dom / Tauri | deb · Snap · Flatpak |
| L5 | Fedora | react-dom / Tauri | rpm · Flatpak |
| L5 | Linux Mint | react-dom / Tauri | deb · Flatpak |
| L5 | Chrome OS | Linux (Crostini) + PWA | deb + PWA |
| L5 | FreeBSD | native build | pkg |
| L5 | OpenBSD | native build | ports |
| L5 | BSD family (NetBSD / DragonFly, …) | native build | pkgsrc, etc. |
| L6 | Apple watchOS | companion app (SwiftUI template) ※experimental | watch bundle (Xcode) → App Store |
| L6 | Wear OS | RN / lightweight runtime (beta); packaging and signing on the Android base | APK (Wear) → Play |
| L6 | Android Automotive | React Native (Android) | APK → OEM channels |
| L6 | QNX | Rust core + optional HMI layer | image / static library |
| L6 | AUTOSAR (Adaptive) | headless (core library) | ARXML / static library |
| L6 | VxWorks | headless (core library) | static library / image |
| L6 | RTOS (Zephyr / FreeRTOS) | headless (no_std core) | static library |
| L6 | Fitbit OS | Fitbit JS SDK | `.fba` app package → Fitbit Gallery |
| L6 | Garmin (Connect IQ) | Monkey C transpile ※experimental | `.iq` package (spike: no built-in iq packager → SKIP-with-print) |
| L6 | Zepp OS | Zepp JS API | `.zab` mini app → Zepp store |
| L7 | PlayStation | official SDK plugin (license required) | platform bundle |
| L7 | Xbox | GDK plugin (license required) | platform bundle |
| L7 | Nintendo Switch | NintendoSDK plugin (license required) | platform bundle |

> Tier legend: **L1** first-class, full pipeline support; **L2** mini programs and instant apps via the Taro / HTML5 adapters; **L3** TVs and large screens; **L4** mobile OS variants; **L5** desktop distributions and BSD; **L6** wearables / automotive / embedded (several are headless core libraries); **L7** consoles, gated by vendor licensing and shipped only as official SDK plugins.
>
> The list above contains **49 explicit platforms**; counting the BSD, RTOS, and Linux-distribution families it reaches **50+ targets**. Targets marked ※experimental have constrained runtimes — see the risk register in [docs/ROADMAP.en.md](docs/ROADMAP.en.md).

### L6 embedded / automotive status table (generated from TargetSpec)

The table below covers the eleven registered L6 embedded / automotive / wearable targets. Every field is reproduced from the `TargetSpec` declarations in `crates/poly-targets/src/builtin.rs` (`notes` keeps the declaration's original Chinese text). Do not edit it by hand: run `node tools/gen-target-table.mjs` to regenerate; `node tools/gen-target-table.mjs --check` fails when the table drifts from the registry.

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

**`garmin` packager gap**: T-M7-09 is a Monkey C transpile spike (a standalone Node module under `ui/adapters/garmin`, same form as the M4 Roku work); `poly-bundle` has no built-in `iq` packager yet — the package stage of `poly build --target garmin` SKIP-with-prints with "no built-in packager (format = iq)" and never fakes a `.iq`; the Rust packager is a conditional task. See [docs/EXPERIMENTAL-TARGETS.en.md](docs/EXPERIMENTAL-TARGETS.en.md) for watchOS / Garmin capability / gap / alternatives; `wearos` is platform-level (its beta boundary is recorded in the same chapter and carries no ※experimental marker).

### 49-target status table (generated from TargetSpec and the planning manifest)

The table below covers all 49 platforms: 36 rows are generated field by field from `TargetSpec` (38 declared targets; the RTOS and Chrome OS rows each merge two targets; `notes` keeps the declaration's original Chinese text), and 13 rows are planned platforms without a `TargetSpec` yet, explicitly marked "planned" with fields taken from the declarative manifest in `tools/gen-status-table.mjs`. Do not edit it by hand: run `node tools/gen-status-table.mjs` to regenerate; `node tools/gen-status-table.mjs --check` fails when the table drifts from TargetSpec / the manifest.

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
| L2 | Google Play Instant (planned) | native | uniffi | instant-aab · android-aapt2 | android-keystore | bundletool-validate | google-play | none | Planned: reuses the Android base (native · uniffi) and emits an instant-enabled AAB; no TargetSpec yet. |
| L2 | Facebook Instant Games (planned) | HTML5 | HTML5 | zip | none | Meta sandbox self-check | Meta platform | none | Planned: HTML5 runtime with Meta sandbox self-check; no TargetSpec yet. |
| L2 | Snapchat Mini (planned) | HTML5 | HTML5 | zip | none | Snap sandbox self-check | Snap platform | none | Planned: HTML5 runtime with Snap sandbox self-check; no TargetSpec yet. |
| L2 | LINE Mini App (planned) | LIFF (web) | HTTPS | static bundle + LIFF config | HTTPS / platform TLS | LIFF config check | LINE platform | none | Planned: delivered as LIFF (web) over HTTPS / platform TLS; no TargetSpec yet. |
| L3 | Android TV (`android-tv`) | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `google-play-tv` | `false` | Leanback 清单（android.software.leanback） |
| L3 | Apple tvOS (`tvos`) | `native` | `uniffi` | `ipa` · `tvos-xcodebuild` | `apple-codesign` | `codesign-verify`, `spctl` | `app-store-connect` | `false` | RN-tvOS 承载 UI；需 macOS 宿主 |
| L3 | HarmonyOS TV (planned) | native | napi | hap · harmonyos-hvigor | harmonyos-hap-sign | hap-sign-verify, hap-structure-check | Huawei video store | none | Planned: reuses the HarmonyOS base (native · napi / RNOH); no TargetSpec yet. |
| L3 | Samsung Tizen (TV) (`tizen-tv`) | `wasm` | `wasm-bindgen` | `wgt` · `tizen-wgt` | `tizen-author` | `wgt-lint` | `tizen-seller-office` | `false` | web 运行时（Tizen TV）；作者证书签名（凭据引用 tizen-author）与侧载安装归三星工具链（本机无 tizen CLI，SDK 校验器缺席时如实跳过） |
| L3 | LG webOS (`webos-tv`) | `wasm` | `wasm-bindgen` | `ipk` · `webos-ares` | none | `ares-package` | `lg-content-store` | `false` | web 运行时（LG webOS）；签名按平台侧（LG Content Store 发布签名），本地不签名（本机无 ares-cli，SDK 校验器缺席时如实跳过） |
| L3 | Roku OS (`roku`) | `none` | `none` | `zip` · `roku-channel` | none | `sideload-structure-check` | `roku-channel-store` | `true` | 实验：SceneGraph 子集受限渲染——仅 UI 子集 + 原生逻辑（BrightScript 重写，无 JS 运行时）；侧载结构自检可用，真机实证待补（T-M4-15 触发前置；T-M4-12 决策：继续受限档） |
| L3 | Amazon Fire TV (`fire-tv`) | `native` | `uniffi` | `apk` · `android-aapt2` | `android-keystore` | `apksigner-verify`, `bundletool-validate` | `amazon-appstore` | `false` | 复用 Android TV 同一 APK 链，渠道指向亚马逊应用商店 |
| L3 | Vizio SmartCast (`smartcast-tv`) | `wasm` | `wasm-bindgen` | `web-app` · `smartcast-web-app` | none | `sandbox-selfcheck` | `vizio-platform` | `false` | web 运行时（Vizio SmartCast，远程托管）；无签名、无推送——只出沙箱自检包（入口 / 清单字段 / 体积），提报与安装归平台侧 |
| L3 | Hisense VIDAA (`vidaa-tv`) | `wasm` | `wasm-bindgen` | `web-app` · `vidaa-web-app` | none | `sandbox-selfcheck` | `vidaa-store` | `false` | web 运行时（海信 VIDAA）；无签名、无推送——只出沙箱自检包（入口 / 清单字段 / 体积），提审与安装归平台商店侧 |
| L4 | GrapheneOS (planned) | native | uniffi | apk · android-aapt2 | apksigner / community keys | apksigner-verify + reproducible diff | F-Droid / self-hosted | none | Planned: Android build variant (no-GMS flavor), self-signed APK / F-Droid; no TargetSpec yet. |
| L4 | KaiOS (planned) | wasm | wasm-bindgen | zip + manifest | platform-side | devtools self-check | KaiStore | none | Planned: react-dom web stack (wasm · wasm-bindgen), zip + manifest to KaiStore; no TargetSpec yet. |
| L4 | Sailfish OS (planned) | native | cffi | rpm | RPM GPG | rpmlint | Jolla Store | none | Planned: native .so + Qt / Silica shell, RPM to Jolla Store; no TargetSpec yet. |
| L4 | Ubuntu Touch (planned) | native | cffi | .click (Clickable) | Clickable packaging | Clickable build check | OpenStore | none | Planned: native .so + WebView shell, .click (Clickable) to OpenStore; no TargetSpec yet. |
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

---

## Architecture

![PolyPack architecture overview](docs/assets/en/architecture.svg)

Five layers, top to bottom:

1. **Entry layer** — the `poly` CLI (single binary), the `poly-gui` console (Tauri + React), and CI pipelines all submit build requests to the core;
2. **Rust core workspace** — `poly-core` pipeline orchestration, `poly-targets` target registry (50+ TargetSpecs), `poly-toolchain` toolchain probing, `poly-bundle` the packager matrix, `poly-sign` signing / notarization / hardening, `poly-publish` store submission;
3. **Side resources** — the build cache (content-addressed, incremental), the remote build farm (Apple targets require a macOS host; a macOS worker pool covers them), and the key vault (Keychain / Vault references — keys never enter the repository);
4. **React UI adapter layer** — the `@polypack/ui` design system (tokens + headless primitives) plus platform render adapters (react-dom / react-native / Taro / TV / wearable) that re-skin one interface per platform;
5. **Artifacts and recipients** — APK / IPA / HAP / MSI / DMG / AppImage / wgt / ipk / mini-program bundles… delivered to 50+ stores and channels.

Rust and React communicate across four bridges: **napi-rs** (Node / Taro side), **uniffi** (Kotlin / Swift), **wasm-bindgen** (Web / WebView side), and **cffi** (QNX / RTOS and other embedded sides).

## Request lifecycle

![Request lifecycle of one poly release](docs/assets/en/request-cycle.svg)

The full journey of one `poly release`: 13 steps in 5 phases.

| Phase | Steps |
|---|---|
| 01 resolve & probe | command enters the CLI → `polypack.toml` loaded into a JobSpec → SDK / NDK probed → toolchain ready (cache hits are reused outright) |
| 02 compile & UI | Rust cross-compiles per-target libraries (.a / .so / .wasm) → React is bundled with adapter transforms into bundle.js + assets |
| 03 signing | credentials referenced from the key vault → signing / notarization / hardening complete |
| 04 package & verify | per-target packaging (APK / IPA / HAP / wgt…) → artifact manifest verified |
| 05 delivery | build report + artifact paths returned; with `--publish`, submission to stores continues |

## Features

![PolyPack capability map](docs/assets/en/features.svg)

Five capability families:

- **Targets & coverage** — declarative target registry (50+ TargetSpecs), one-command multi-target releases, OS-variant support (GrapheneOS / KaiOS / HarmonyOS family);
- **Build & cache** — automatic toolchain probing and installation, content-addressed caching (incremental, reproducible), remote build farm (macOS worker pool);
- **Cross-platform UI** — the `@polypack/ui` design system, platform component adaptation (Material / HIG / HarmonyOS styles), `poly dev` hot preview;
- **Signing & publishing** — vault integration (Keychain / Vault references), signing · notarization · hardening (notarize / AVB), store pipelines (Play / ASC / AGC / mini-program platforms);
- **Ecosystem & operations** — WASM plugins (custom targets, sandboxed), CI integrations, diagnostics (build explanations, failure localization).

## Lifecycle

![Project and artifact lifecycle](docs/assets/en/lifecycle.svg)

A state machine from bootstrap to archival, with two **recovery loops**:

- **Main path**: init → dev iteration → build → sign & verify → release;
- **Waiting states**: signing blocked on vault unlock; store and mini-program human reviews (waiting consumes no build resources);
- **Recovery loops**: a failed build retries back into build after a fix; a rejected review loops back to dev iteration — every failure has a defined way home;
- **Terminal**: archive / EOL is the only exit, retiring the artifact while keeping it traceable.

## UI component strategy

The interface is written once (React) and mapped by the adapter layer onto **each platform's own component system and plugins**:

| Platform family | React runtime | Component system |
|---|---|---|
| Web / desktop (Win / macOS / Linux) | react-dom (Tauri WebView) | `@polypack/ui` desktop tokens |
| Android family (incl. GrapheneOS / Wear / Auto / Fire TV / Android TV) | React Native (Fabric + JSI) | Material 3 variants |
| Apple family (iOS / macOS / tvOS) | React Native / react-native-tvos | HIG variants |
| HarmonyOS family (incl. TV) | React Native for OpenHarmony | HarmonyOS Design |
| Mini-program family (WeChat / Alipay / Douyin / Quick App) | Taro (React syntax) | each platform's native components |
| TV web runtime (Tizen / webOS / VIDAA / Vizio / KaiOS) | react-dom (packaged as wgt / ipk / app bundle) | 10-foot UI + focus navigation |
| Wearables (Wear OS / watchOS / Fitbit / Garmin / Zepp) | RN (Wear beta) / SwiftUI template / JS side / Monkey C subset | compact tokens |
| Embedded / automotive (QNX / AUTOSAR / RTOS) | headless core (optional HMI layer) | platform HMI specs |

## Quick start (planned API)

```bash
# available today (M1): init / build / dev / clean / doctor (see below); release / publish are target shape
cargo install polypack      # install the poly CLI
poly init                   # generate polypack.toml
poly dev                    # hot preview
poly release --target android,ios,wechat
poly release --target all --publish
```

> `--target all` = every **buildable** target in the current environment; exclusions (license-gated, unreachable host/credentials) and their reasons are listed during the plan stage.
>
> Available today (M1): `cargo run -p poly-cli -- init|build|dev|clean|doctor` (toolchain probes, sample-app scaffolding, cache-addressed multi-target build & packaging, full-reload hot preview, cache/artifact cleanup).

Sketch of `polypack.toml`:

```toml
schema = 1

[package]
name = "my-app"
version = "1.2.0"

[ui]
framework = "react"
entry = "ui/src/main.tsx"

[core]
crate = "crates/app-core"    # Rust business core, compiled per target

[targets.android]
min-sdk = 24
signing = "keystore"          # references a vault entry; keys never in-repo

[targets.ios]
bundle-id = "dev.example.polypack"
team = "ABCDE12345"

[targets.wechat]
appid = "wx0000000000000000"

[publish]
stores = ["play", "appstore", "appgallery"]
```

## Repository layout

```text
polypack/
├── crates/
│   ├── poly-cli/          # command line entry (clap)
│   ├── poly-core/         # pipeline orchestration, JobSpec, event bus
│   ├── poly-targets/      # TargetSpec registry (50+ target declarations)
│   ├── poly-toolchain/    # SDK / NDK / Xcode / emsdk probe & install
│   ├── poly-bundle/       # packager matrix (apk / ipa / hap / msi / dmg / deb / rpm / wgt / …)
│   ├── poly-sign/         # signing, notarization, hardening
│   ├── poly-publish/      # store and mini-program platform submission
│   ├── poly-cache/        # content-addressed build cache
│   └── poly-plugin/       # WASM plugin host (custom targets)
├── ui/
│   ├── polypack-ui/       # @polypack/ui design system (tokens + headless primitives)
│   └── adapters/          # react-dom / react-native / taro / tv / wear adapters
├── app/                   # poly-gui console (Tauri + React)
├── docs/
│   ├── ROADMAP.en.md      # detailed plan (milestones, risks, support matrix)
│   ├── TASKS.en.md        # task plan (116 tasks / 283 person-days)
│   ├── FARM.en.md         # remote farm: origin / build site (deployment, credentials, signature checks)
│   ├── assets/            # every SVG embedded in this document (incl. Pax)
│   └── diagrams/          # diagram sources (JSON IR) + rebuild pipeline
└── tools/
    ├── render-diagrams.mjs  # diagram pipeline: JSON IR → SVG (with the Pax corner badge)
    └── build-pax.mjs        # Pax generator: expressions + icon + contact sheet + playground
```

(M0 delivered: the 9-crate workspace skeleton plus `poly-cli` / `poly-core` / `poly-targets` / `poly-toolchain`; since M2 `ui/` has landed — `ui/adapters/native` native adapter v0 + RN host skeleton (T-M2-04/05); M3 adds `ui/adapters/taro` (mini-program family) and the `ui/adapters/native/host/harmony` RNOH host template; `app/` remains target shape.)

## Documentation sync convention

Docs are part of the product; diagrams and prose must stay single-sourced:

1. **`docs/diagrams/*.json` is the single source of truth for the diagrams** — change the JSON first, then run:

   ```bash
   node tools/render-diagrams.mjs   # validate → render → extract to docs/assets/*.svg
   ```

   CI catches drift automatically via `render-diagrams.mjs --check`, `build-pax.mjs --check`, and `tools/check-docs.mjs` (see `.github/workflows/docs-check.yml`).

2. `README.md` and [`README.en.md`](README.en.md) must be updated **in the same commit** (bilingual mirrors);
3. Milestone and platform-matrix changes sync to `docs/ROADMAP.md` / [`ROADMAP.en.md`](docs/ROADMAP.en.md);
4. When Pax's art or copy changes, update `docs/assets/pax.svg` to stay consistent with the design system.

## Roadmap

Releases: since v1.0.0, tags and GitHub Release names follow `vX.Y.Z` semantic increments (v1.0.0 = this documentation and planning baseline).

Incubated extension directions (SBOM / monorepo / incremental releases / OTA / store metadata / GUI dashboard / device previews / ADR) live in [docs/ROADMAP.en.md](docs/ROADMAP.en.md) §13.

| Milestone | Scope |
|---|---|
| M0 | repo skeleton, `polypack.toml` schema, TargetSpec DSL, `poly doctor` |
| M1 | Windows / macOS / Linux / Web end-to-end + content-addressed cache + **English-labeled diagrams** |
| M2 | Android / iOS (RN bridge + signing pipeline) |
| M3 | HarmonyOS (RNOH) + WeChat / Alipay / Douyin mini programs (Taro) |
| M4 | TV family (Android TV / Fire TV / tvOS / Tizen / webOS / VIDAA / Vizio; Roku constrained adapter) |
| M5 | desktop distributions & BSD (deb / rpm / Flatpak / Snap / AppImage / pkg / ports / ChromeOS) |
| M6 | embedded & automotive (headless core: QNX / AUTOSAR / VxWorks / RTOS / Android Automotive) |
| M7 | wearables (Wear OS platform-level; watchOS / Garmin experimental; Fitbit / Zepp JS packaging) |
| M8 | WASM plugin SDK, remote build farm, store publishing pipelines, ecosystem GA |

The complete plan (per-platform support detail, risks, and mitigations) lives in **[docs/ROADMAP.en.md](docs/ROADMAP.en.md)**; the task-level breakdown (116 tasks / 283 person-days, with roles, dependencies, acceptance criteria, and scheduling notes) is in **[docs/TASKS.en.md](docs/TASKS.en.md)**.

Cross-host builds (Linux origin → macOS worker produces an IPA) — deployment, credential model, and signature verification — are documented in **[docs/FARM.en.md](docs/FARM.en.md)**.

## License

[Apache-2.0](LICENSE) (the `LICENSE` file is in place; contribution rules formalize with the code).

<!-- assets-license:start -->
## Visual Assets License

The project pets, images, icons and emoji/expression forms in this repository
(including the SVG/PNG files under `docs/assets/`) are **NOT open-source assets**.
All rights are reserved. No copying, modification, redistribution, public
transmission or commercial use is permitted without prior written permission.
For commercial licensing contact **erik@erik.xyz**. See
[ASSETS-LICENSE.md](ASSETS-LICENSE.md).

> The code remains under the repository's existing licence (Apache-2.0). Third-party
> assets (e.g. the Go Gopher where explicitly noted) keep their original licences.
<!-- assets-license:end -->
