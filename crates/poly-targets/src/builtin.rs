//! 内建目标声明。
//!
//! L1 七目标（android / ios / harmonyos / windows / macos / linux / web）的完整声明
//! 按 ROADMAP §3.2 逐列填写（T-M0-05）：运行时 · 桥、打包器与产物、签名引用、
//! 校验器、发布渠道、备注；tier 均为 1，凭据只以密钥库引用形式出现。
//! M3 增补 L2 小程序族四目标（wechat / alipay / douyin / quickapp，tier 2，ROADMAP §3.3
//! 前四行，T-M3-05 / T-M3-06）：Taro 承载（ui = "taro"）、wasm 进宿主沙箱、签名在平台侧。
//! M4 增补 L3 电视族（tier 3，ROADMAP §3.4）：Apple tvOS（T-M4-07，RN-tvOS 承载 UI、
//! 复用 iOS Xcode 链，产物格式同 `ipa` 按目标切 SDK / 切片）；android-tv / fire-tv（T-M4-04，
//! 复用同一条 Android 打包链 `apk` / `android-aapt2`，差异在 Leanback 清单与发布渠道）；
//! web 系 TV 四目标（tizen-tv / webos-tv / vidaa-tv / smartcast-tv，T-M4-08/09/10：
//! wasm 核心 + react-dom UI，wgt / ipk / web-app 出包）。
//! M5 增补 L5 BSD 家族目标（tier 5，ROADMAP §3.6）：OpenBSD ports（T-M5-07，ports 骨架 +
//! signify / portcheck；Linux 宿主可发起的交叉面，真构建归 OpenBSD VM / T-M5-09）与
//! ChromeOS 双轨（T-M5-10：Crostini deb + PWA 两目标，分别复用 Linux deb / Web 链）。
//! M6 增补 L6 嵌入式 / 车机目标（tier 6，ROADMAP §3.7）：Android Automotive（T-M6-11）、
//! QNX（T-M6-03，staticlib + cffi + 镜像 / 库）、AUTOSAR Adaptive（T-M6-04，staticlib +
//! cffi + ARXML 清单；无 UI、自部署）、VxWorks（T-M6-05，库 / 镜像 kit）与 Zephyr
//! （T-M6-06，west 工程模板 + no_std 引擎模块）。
//! M7 增补 Wear OS（tier 6，ROADMAP §3.7 穿戴行，T-M7-02）：native · uniffi，复用 Android
//! 打包链出 Wear APK（差异只有 `android.hardware.type.watch` 清单变体与 minSdk 下限）。

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use crate::spec::{Bridge, Host, PackageSpec, Runtime, TargetSpec};

/// 全部内建目标（L1 七 + L2 小程序族四 + L3 电视族八 + L5 发行版三 / OpenBSD / ChromeOS 双轨）。
pub fn builtin() -> Vec<TargetSpec> {
    vec![
        TargetSpec {
            id: "android".to_string(),
            tier: 1,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Native,
            bridge: Bridge::Uniffi,
            ui: vec!["react-native".to_string()],
            package: PackageSpec {
                format: "aab".to_string(),
                builder: "android-aapt2".to_string(),
            },
            signing: Some("android-keystore".to_string()),
            verify: vec![
                "apksigner-verify".to_string(),
                "bundletool-validate".to_string(),
            ],
            publish: vec!["google-play".to_string()],
            experimental: false,
            notes: None,
        },
        TargetSpec {
            id: "ios".to_string(),
            tier: 1,
            hosts: vec![Host::Macos],
            runtime: Runtime::Native,
            bridge: Bridge::Uniffi,
            ui: vec!["react-native".to_string()],
            package: PackageSpec {
                format: "ipa".to_string(),
                builder: "ios-xcodebuild".to_string(),
            },
            signing: Some("apple-codesign".to_string()),
            verify: vec!["codesign-verify".to_string(), "spctl".to_string()],
            publish: vec!["app-store-connect".to_string()],
            experimental: false,
            notes: Some("需 macOS 宿主".to_string()),
        },
        // ---- L3 电视族（M4，ROADMAP §3.4）：Apple tvOS（T-M4-07，复用 iOS Xcode 链） ----
        TargetSpec {
            id: "tvos".to_string(),
            tier: 3,
            hosts: vec![Host::Macos],
            runtime: Runtime::Native,
            bridge: Bridge::Uniffi,
            ui: vec!["react-native-tvos".to_string()],
            package: PackageSpec {
                format: "ipa".to_string(),
                builder: "tvos-xcodebuild".to_string(),
            },
            signing: Some("apple-codesign".to_string()),
            verify: vec!["codesign-verify".to_string(), "spctl".to_string()],
            publish: vec!["app-store-connect".to_string()],
            experimental: false,
            notes: Some("RN-tvOS 承载 UI；需 macOS 宿主".to_string()),
        },
        TargetSpec {
            id: "harmonyos".to_string(),
            tier: 1,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Native,
            bridge: Bridge::Napi,
            ui: vec!["react-native-ohos".to_string()],
            package: PackageSpec {
                format: "hap".to_string(),
                builder: "harmonyos-hvigor".to_string(),
            },
            signing: Some("harmonyos-hap-sign".to_string()),
            verify: vec![
                "hap-sign-verify".to_string(),
                "hap-structure-check".to_string(),
            ],
            publish: vec!["appgallery-connect".to_string()],
            experimental: false,
            notes: Some("RNOH 承载 UI".to_string()),
        },
        TargetSpec {
            id: "windows".to_string(),
            tier: 1,
            hosts: vec![Host::Win],
            runtime: Runtime::Native,
            bridge: Bridge::TauriIpc,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "msi".to_string(),
                builder: "windows-wix".to_string(),
            },
            signing: Some("windows-authenticode".to_string()),
            verify: vec!["signtool-verify".to_string()],
            publish: vec!["ms-store".to_string()],
            experimental: false,
            notes: Some("WebView2 运行时".to_string()),
        },
        TargetSpec {
            id: "macos".to_string(),
            tier: 1,
            hosts: vec![Host::Macos],
            runtime: Runtime::Native,
            bridge: Bridge::TauriIpc,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "dmg".to_string(),
                builder: "macos-dmg".to_string(),
            },
            signing: Some("apple-codesign".to_string()),
            verify: vec![
                "codesign-verify".to_string(),
                "stapler-validate".to_string(),
            ],
            publish: vec!["app-store".to_string(), "self-hosted".to_string()],
            experimental: false,
            notes: Some("公证必须".to_string()),
        },
        TargetSpec {
            id: "linux".to_string(),
            tier: 1,
            hosts: vec![Host::Linux],
            runtime: Runtime::Native,
            bridge: Bridge::TauriIpc,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "appimage".to_string(),
                builder: "linux-appimage".to_string(),
            },
            signing: Some("linux-repo-gpg".to_string()),
            verify: vec![
                "lintian".to_string(),
                "rpmlint".to_string(),
                "appimage-structure".to_string(),
                "appstream-validate".to_string(),
            ],
            publish: vec![
                "flathub".to_string(),
                "snapcraft".to_string(),
                "self-hosted".to_string(),
            ],
            experimental: false,
            notes: None,
        },
        // ---- L5 发行版目标（T-M5-01..03）：deb / rpm 主档 + Flatpak / Snap 共用打包器 ----
        TargetSpec {
            id: "ubuntu".to_string(),
            tier: 5,
            hosts: vec![Host::Linux],
            runtime: Runtime::Native,
            bridge: Bridge::TauriIpc,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "deb".to_string(),
                builder: "linux-deb".to_string(),
            },
            signing: Some("linux-repo-gpg".to_string()),
            verify: vec![
                "lintian".to_string(),
                "appstream-validate".to_string(),
                "flatpak-lint".to_string(),
                "snap-lint".to_string(),
            ],
            publish: vec![
                "flathub".to_string(),
                "snapcraft".to_string(),
                "self-hosted".to_string(),
            ],
            experimental: false,
            notes: Some(
                "L5 发行版：deb 主档；Flatpak 权限恰为 [--share=ipc, --socket=wayland, \
                 --socket=fallback-x11, --device=dri]，Snap plugs 恰为 [desktop, wayland, x11, \
                 opengl]（与打包器常量同源，T-M5-02/03）"
                    .to_string(),
            ),
        },
        TargetSpec {
            id: "fedora".to_string(),
            tier: 5,
            hosts: vec![Host::Linux],
            runtime: Runtime::Native,
            bridge: Bridge::TauriIpc,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "rpm".to_string(),
                builder: "linux-rpm".to_string(),
            },
            signing: Some("linux-repo-gpg".to_string()),
            verify: vec![
                "rpmlint".to_string(),
                "appstream-validate".to_string(),
                "flatpak-lint".to_string(),
            ],
            publish: vec!["flathub".to_string(), "self-hosted".to_string()],
            experimental: false,
            notes: Some(
                "L5 发行版：rpm 主档；Flatpak 权限恰为 [--share=ipc, --socket=wayland, \
                 --socket=fallback-x11, --device=dri]（与打包器常量同源，T-M5-02）"
                    .to_string(),
            ),
        },
        TargetSpec {
            id: "mint".to_string(),
            tier: 5,
            hosts: vec![Host::Linux],
            runtime: Runtime::Native,
            bridge: Bridge::TauriIpc,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "deb".to_string(),
                builder: "linux-deb".to_string(),
            },
            signing: Some("linux-repo-gpg".to_string()),
            verify: vec![
                "lintian".to_string(),
                "appstream-validate".to_string(),
                "flatpak-lint".to_string(),
            ],
            publish: vec!["flathub".to_string(), "self-hosted".to_string()],
            experimental: false,
            notes: Some(
                "L5 发行版：deb 主档（Linux Mint 兼容 Ubuntu 仓库）；Flatpak 权限恰为 \
                 [--share=ipc, --socket=wayland, --socket=fallback-x11, --device=dri]\
                 （与打包器常量同源，T-M5-02）"
                    .to_string(),
            ),
        },
        // ---- L5 ChromeOS 双轨（T-M5-10，ROADMAP §3.6）：Crostini deb + PWA ----
        // 两目标从同一次 JobSpec 的各出产物：Crostini 轨复用 Linux deb 打包器（不动 deb 逻辑，
        // 只扩 serves），PWA 轨复用 Web 管线 / manifest / 静态站点打包器；PWA 三腿审计
        // （manifest / 离线能力 / 安装性）由 `pwa-audit` 校验器接入 verify，真浏览器腿在
        // Chrome 缺席时 SKIP-with-print（结构断言仍真跑）。Crostini 容器不可得时安装冒烟
        // 同样 SKIP-with-print（契约测试以 Debian 容器 + dpkg -i 真跑替代并注记）。
        TargetSpec {
            id: "chromeos".to_string(),
            tier: 5,
            hosts: vec![Host::Linux],
            runtime: Runtime::Native,
            bridge: Bridge::TauriIpc,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "deb".to_string(),
                builder: "linux-deb".to_string(),
            },
            signing: None,
            verify: vec!["lintian".to_string()],
            publish: vec!["self-hosted".to_string()],
            experimental: false,
            notes: Some(
                "ChromeOS 双轨之 Crostini：复用 Linux deb 链（control / 维护者脚本 / AppStream \
                 随载荷，不另写 deb 逻辑）；安装冒烟在 Crostini 容器不可得时 SKIP-with-print，\
                 结构检查（deb 数据段 / control）仍真跑"
                    .to_string(),
            ),
        },
        TargetSpec {
            id: "chromeos-pwa".to_string(),
            tier: 5,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Wasm,
            bridge: Bridge::WasmBindgen,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "static-site".to_string(),
                builder: "web-static".to_string(),
            },
            signing: None,
            verify: vec!["pwa-audit".to_string()],
            publish: vec!["self-hosted".to_string()],
            experimental: false,
            notes: Some(
                "ChromeOS 双轨之 PWA：复用 Web 管线 + T-M1-04 manifest + T-M1-08 静态站点\
                 打包器；离线能力 / manifest / 安装性审计由 pwa-audit 接入 verify。离线能力\
                 要求应用自带 service worker 注册与缓存（模板不生成 SW）；真 Chrome 腿\
                 （Page.getInstallabilityErrors + 断网重载）缺席时 SKIP-with-print"
                    .to_string(),
            ),
        },
        TargetSpec {
            id: "web".to_string(),
            tier: 1,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Wasm,
            bridge: Bridge::WasmBindgen,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "static-site".to_string(),
                builder: "web-static".to_string(),
            },
            signing: None,
            verify: vec!["html-validate".to_string(), "sri-check".to_string()],
            publish: vec!["static-hosting".to_string()],
            experimental: false,
            notes: None,
        },
        // ---- L2 小程序族（T-M3-05 / T-M3-06，ROADMAP §3.3 前四行）：Taro 编译四目标 ----
        // 签名一律 None：上传签名在平台侧（微信 / 支付宝 / 抖音开放平台、快应用联盟商店），
        // 本地不做签名（§3.3「签名」列为「平台侧（上传签名）」）。
        TargetSpec {
            id: "wechat".to_string(),
            tier: 2,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Wasm,
            bridge: Bridge::WasmBindgen,
            ui: vec!["taro".to_string()],
            package: PackageSpec {
                format: "weapp".to_string(),
                builder: "taro-weapp".to_string(),
            },
            signing: None,
            verify: vec!["wechat-devtools-cli".to_string()],
            publish: vec!["wechat-open-platform".to_string()],
            experimental: false,
            notes: Some("小程序代码包（Taro 编译）；wasm 进宿主沙箱（T-M3-07）".to_string()),
        },
        TargetSpec {
            id: "alipay".to_string(),
            tier: 2,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Wasm,
            bridge: Bridge::WasmBindgen,
            ui: vec!["taro".to_string()],
            package: PackageSpec {
                format: "alipay".to_string(),
                builder: "taro-alipay".to_string(),
            },
            signing: None,
            verify: vec!["alipay-devtools-cli".to_string()],
            publish: vec!["alipay-open-platform".to_string()],
            experimental: false,
            notes: Some("小程序代码包（Taro 编译）；wasm 进宿主沙箱（T-M3-07）".to_string()),
        },
        TargetSpec {
            id: "douyin".to_string(),
            tier: 2,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Wasm,
            bridge: Bridge::WasmBindgen,
            ui: vec!["taro".to_string()],
            package: PackageSpec {
                format: "tt".to_string(),
                builder: "taro-tt".to_string(),
            },
            signing: None,
            verify: vec!["douyin-devtools-cli".to_string()],
            publish: vec!["douyin-open-platform".to_string()],
            experimental: false,
            notes: Some(
                "小程序代码包（Taro 编译，平台 `tt`）；wasm 进宿主沙箱（T-M3-07）".to_string(),
            ),
        },
        TargetSpec {
            id: "quickapp".to_string(),
            tier: 2,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Wasm,
            bridge: Bridge::WasmBindgen,
            ui: vec!["taro".to_string()],
            package: PackageSpec {
                // T-M3-12 收口（T-M3-06 预告的翻法）：产物格式 = `rpk`、打包器 = `hap-toolkit`
                // ——与 ROADMAP §3.3 快应用行「打包器与产物：`.rpk`」逐字对齐；产物顶形态仍是
                // `miniapp/` 目录（`.rpk` 落在 `dist/`，随包 wasm 出口在 `src/`）。
                format: "rpk".to_string(),
                builder: "hap-toolkit".to_string(),
            },
            signing: None,
            verify: vec!["hap-toolkit-lint".to_string()],
            publish: vec!["quickapp-union-store".to_string()],
            experimental: false,
            notes: Some(
                "快应用 `.rpk`（hap-toolkit 构建）；lint 入 verify、提审走联盟商店（T-M3-12）"
                    .to_string(),
            ),
        },
        // ---- L3 电视与大屏（T-M4-04，ROADMAP §3.4 前两行）：Android TV / Fire TV ----
        // 两目标复用同一条 Android 打包链（format = `apk`、builder = `android-aapt2`），
        // 签名引用与校验器同 Android（§3.4「同 Android」列）；差异只在清单形态（Leanback）
        // 与发布渠道（TV 轨道 / 亚马逊应用商店）。§3.4「产物校验」列的「焦点导航冒烟」是
        // 设备侧走查（T-M4-05），不产生校验器 id，因此 verify 只列 bundletool（+ apksigner 取证）。
        TargetSpec {
            id: "android-tv".to_string(),
            tier: 3,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Native,
            bridge: Bridge::Uniffi,
            ui: vec!["react-native".to_string()],
            package: PackageSpec {
                format: "apk".to_string(),
                builder: "android-aapt2".to_string(),
            },
            signing: Some("android-keystore".to_string()),
            verify: vec![
                "apksigner-verify".to_string(),
                "bundletool-validate".to_string(),
            ],
            publish: vec!["google-play-tv".to_string()],
            experimental: false,
            notes: Some("Leanback 清单（android.software.leanback）".to_string()),
        },
        TargetSpec {
            id: "fire-tv".to_string(),
            tier: 3,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Native,
            bridge: Bridge::Uniffi,
            ui: vec!["react-native".to_string()],
            package: PackageSpec {
                format: "apk".to_string(),
                builder: "android-aapt2".to_string(),
            },
            signing: Some("android-keystore".to_string()),
            verify: vec![
                "apksigner-verify".to_string(),
                "bundletool-validate".to_string(),
            ],
            publish: vec!["amazon-appstore".to_string()],
            experimental: false,
            notes: Some("复用 Android TV 同一 APK 链，渠道指向亚马逊应用商店".to_string()),
        },
        // ---- L3 web 系 TV 四目标（T-M4-08 / T-M4-09 / T-M4-10，ROADMAP §3.4）：wasm 核心 +
        // react-dom UI，一份 UI 产物多目标复用（同源同摘要，T-M4-09 验收口径）。签名边界如实
        // 入 notes：Tizen 作者证书归三星工具链（本机无 tizen CLI）；webOS 发布签名在平台侧；
        // VIDAA / Vizio 无签名、无推送（只出沙箱自检包）。 ----
        TargetSpec {
            id: "tizen-tv".to_string(),
            tier: 3,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Wasm,
            bridge: Bridge::WasmBindgen,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "wgt".to_string(),
                builder: "tizen-wgt".to_string(),
            },
            signing: Some("tizen-author".to_string()),
            verify: vec!["wgt-lint".to_string()],
            publish: vec!["tizen-seller-office".to_string()],
            experimental: false,
            notes: Some(
                "web 运行时（Tizen TV）；作者证书签名（凭据引用 tizen-author）与侧载安装归三星\
                 工具链（本机无 tizen CLI，SDK 校验器缺席时如实跳过）"
                    .to_string(),
            ),
        },
        TargetSpec {
            id: "webos-tv".to_string(),
            tier: 3,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Wasm,
            bridge: Bridge::WasmBindgen,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "ipk".to_string(),
                builder: "webos-ares".to_string(),
            },
            signing: None,
            verify: vec!["ares-package".to_string()],
            publish: vec!["lg-content-store".to_string()],
            experimental: false,
            notes: Some(
                "web 运行时（LG webOS）；签名按平台侧（LG Content Store 发布签名），本地不签名\
                 （本机无 ares-cli，SDK 校验器缺席时如实跳过）"
                    .to_string(),
            ),
        },
        TargetSpec {
            id: "vidaa-tv".to_string(),
            tier: 3,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Wasm,
            bridge: Bridge::WasmBindgen,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "web-app".to_string(),
                builder: "vidaa-web-app".to_string(),
            },
            signing: None,
            verify: vec!["sandbox-selfcheck".to_string()],
            publish: vec!["vidaa-store".to_string()],
            experimental: false,
            notes: Some(
                "web 运行时（海信 VIDAA）；无签名、无推送——只出沙箱自检包（入口 / 清单字段 / \
                 体积），提审与安装归平台商店侧"
                    .to_string(),
            ),
        },
        TargetSpec {
            id: "smartcast-tv".to_string(),
            tier: 3,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Wasm,
            bridge: Bridge::WasmBindgen,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "web-app".to_string(),
                builder: "smartcast-web-app".to_string(),
            },
            signing: None,
            verify: vec!["sandbox-selfcheck".to_string()],
            publish: vec!["vizio-platform".to_string()],
            experimental: false,
            notes: Some(
                "web 运行时（Vizio SmartCast，远程托管）；无签名、无推送——只出沙箱自检包\
                 （入口 / 清单字段 / 体积），提报与安装归平台侧"
                    .to_string(),
            ),
        },
        TargetSpec {
            id: "roku".to_string(),
            tier: 3,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::None,
            bridge: Bridge::None,
            ui: vec!["scene-graph-subset".to_string()],
            package: PackageSpec {
                format: "zip".to_string(),
                builder: "roku-channel".to_string(),
            },
            signing: None,
            verify: vec!["sideload-structure-check".to_string()],
            publish: vec!["roku-channel-store".to_string()],
            experimental: true,
            notes: Some(
                "实验：SceneGraph 子集受限渲染——仅 UI 子集 + 原生逻辑（BrightScript 重写，\
                 无 JS 运行时）；侧载结构自检可用，真机实证待补（T-M4-15 触发前置；T-M4-12 决策：\
                 继续受限档）"
                    .to_string(),
            ),
        },
        // ---- L5 BSD 家族（T-M5-06，ROADMAP §3.6）：FreeBSD pkg ----
        // hosts = Linux 是**交叉发起面**：Linux 宿主生成 pkg 暂存树 + manifest 并做结构自检；
        // `pkg create` / `pkg install` / `pkg check` 真跑需 FreeBSD 用户态（T-M5-09 VM），缺
        // pkg(8) 时打包器跳过并如实注记。
        TargetSpec {
            id: "freebsd".to_string(),
            tier: 5,
            // Linux 交叉发起（无 pkg 时 package 降级）+ FreeBSD 原生（pkg create / pkg repo）。
            hosts: vec![Host::Linux, Host::Bsd],
            runtime: Runtime::Native,
            bridge: Bridge::TauriIpc,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "pkg".to_string(),
                builder: "freebsd-pkg".to_string(),
            },
            signing: Some("freebsd-repo-pkg".to_string()),
            verify: vec!["pkg-check".to_string()],
            publish: vec!["freebsd-repo".to_string()],
            experimental: false,
            notes: Some(
                "原生 pkg create 链（Tauri IPC 核心即宿主，FreeBSD 用户态）；Linux 只做交叉\
                 发起 + 暂存树 / manifest 结构自检，pkg create / pkg check / pkg install 真跑归 \
                 FreeBSD VM（T-M5-09）；仓库签名走 pkg repo rsa|ecdsa|eddsa:<keyfile>\
                 （catalogue 内签名，客户端 PUBKEY）"
                    .to_string(),
            ),
        },
        // ---- L5 BSD 家族（T-M5-07，ROADMAP §3.6）：OpenBSD ports ----
        // hosts = Linux 是**交叉面**：Linux 宿主生成 ports 骨架（Makefile / PLIST / DESCR +
        // 载荷）并做结构自检；`make package` / portcheck / pkg_sign 真跑需 OpenBSD VM
        // （T-M5-09），缺 ports 基础设施时打包器逐项跳过并如实注记。
        TargetSpec {
            id: "openbsd".to_string(),
            tier: 5,
            // Linux 交叉发起（结构骨架）+ OpenBSD 原生（make package / pkg_sign）。
            hosts: vec![Host::Linux, Host::Bsd],
            runtime: Runtime::Native,
            bridge: Bridge::TauriIpc,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "ports".to_string(),
                builder: "openbsd-ports".to_string(),
            },
            signing: Some("openbsd-signify".to_string()),
            verify: vec!["portcheck".to_string()],
            publish: vec!["openbsd-ports".to_string()],
            experimental: false,
            notes: Some(
                "ports 目录骨架（Makefile / pkg/PLIST / pkg/DESCR + files 载荷，无 distfile）；\
                 signify 包签名（pkg_sign，钥经 POLYPACK_OPENBSD_SIGNIFY_KEY）；make package 与 \
                 portcheck 真跑归 OpenBSD VM（T-M5-09），Linux 宿主只生成骨架并做结构自检"
                    .to_string(),
            ),
        },
        // ---- L5 BSD 家族（T-M5-08，ROADMAP §3.6）：NetBSD / DragonFly pkgsrc ----
        // hosts = Linux 是**交叉发起面**：Linux 宿主生成 pkgsrc 骨架（Makefile / PLIST /
        // DESCR + 载荷）并做结构自检；`bmake package` / `pkg_add` / pkg_admin 真跑需 pkgsrc
        // 树与 NetBSD 用户态（T-M5-09 VM），骨架在树外时打包器如实跳过并注记。
        TargetSpec {
            id: "netbsd".to_string(),
            tier: 5,
            hosts: vec![Host::Linux],
            runtime: Runtime::Native,
            bridge: Bridge::TauriIpc,
            ui: vec!["react-dom".to_string()],
            package: PackageSpec {
                format: "pkgsrc".to_string(),
                builder: "pkgsrc".to_string(),
            },
            signing: Some("netbsd-repo-gpg".to_string()),
            verify: vec!["pkgsrc-check".to_string()],
            publish: vec!["netbsd-repo".to_string()],
            experimental: false,
            notes: Some(
                "pkgsrc 骨架（Makefile / PLIST / DESCR + files 载荷，无 distfile；NetBSD / \
                 DragonFly 等同源）；Linux 交叉发起只生成骨架并做结构自检，bmake package / \
                 pkg_add 真跑归 NetBSD VM（T-M5-09）；仓库签名走 pkg_admin gpg-sign-package\
                 （逐包 GPG，仓库为扁平 .tgz + pkg_summary.gz，客户端 GPG_KEYRING_VERIFY）"
                    .to_string(),
            ),
        },
        // ---- L6 嵌入式（T-M6-05/06/07，ROADMAP §3.7）：VxWorks / Zephyr / FreeRTOS ----
        // 三者都复用 T-M6-02 的 `poly_engine_*` C ABI（唯一生成头文件），无 UI；
        // 引擎层以应用侧 wrapper staticlib 链入（poly-core 只出 rlib，应用提供 allocator /
        // panic_handler）。VxWorks 默认出库（配置可覆盖为 `image` 部署 kit）；`wr-cc` 缺席时
        // verify 用 nm 闭包 / 结构断言真跑并 SKIP-with-print。Zephyr 出 west 工程模板，
        // `west build` 覆盖 qemu_x86 / qemu_cortex_m3 两板。FreeRTOS 出 QEMU 示例板包，
        // mps2-an385 上真跑引擎调用并以 semihosting 退出码 0 结束。
        TargetSpec {
            id: "vxworks".to_string(),
            tier: 6,
            hosts: vec![Host::Win, Host::Linux],
            runtime: Runtime::Staticlib,
            bridge: Bridge::Cffi,
            ui: vec![],
            package: PackageSpec {
                format: "library".to_string(),
                builder: "vxworks-wr".to_string(),
            },
            signing: None,
            verify: vec!["c-abi-symbols".to_string(), "vxworks-ld-check".to_string()],
            publish: vec!["self-hosted".to_string()],
            experimental: false,
            notes: Some(
                "无 UI、cffi、no_std；format = `library`（默认，静态库）或 `image`（部署 kit zip，\
                 配置 [targets.vxworks].format 覆盖）；wr-cc / wr-ld 缺席时产物用 nm 符号闭包 \
                 + 结构断言校验并 SKIP-with-print，绝不伪造可加载镜像"
                    .to_string(),
            ),
        },
        TargetSpec {
            id: "zephyr".to_string(),
            tier: 6,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Staticlib,
            bridge: Bridge::Cffi,
            ui: vec![],
            package: PackageSpec {
                format: "west".to_string(),
                builder: "zephyr-west".to_string(),
            },
            signing: None,
            verify: vec![
                "c-abi-symbols".to_string(),
                "zephyr-west-build".to_string(),
                "zephyr-qemu-smoke".to_string(),
            ],
            publish: vec!["self-hosted".to_string()],
            experimental: false,
            notes: Some(
                "无 UI、cffi、no_std；west 工程模板（zephyr module + 引擎静态库模块链入），\
                 qemu_x86 / qemu_cortex_m3 两板 `west build` 验收；最小 CONFIG_* 集随模板版本化；\
                 west / Zephyr SDK 缺席时按环境槽 SKIP-with-print"
                    .to_string(),
            ),
        },
        TargetSpec {
            id: "freertos".to_string(),
            tier: 6,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Staticlib,
            bridge: Bridge::Cffi,
            ui: vec![],
            package: PackageSpec {
                format: "board".to_string(),
                builder: "freertos-qemu".to_string(),
            },
            signing: None,
            verify: vec![
                "c-abi-symbols".to_string(),
                "freertos-qemu-smoke".to_string(),
            ],
            publish: vec!["self-hosted".to_string()],
            experimental: false,
            notes: Some(
                "无 UI、cffi、no_std；FreeRTOS 示例板包（mps2-an385 / Cortex-M3，静态库 + \
                 启动 / 中断 / UART / FreeRTOSConfig / Makefile 工程模板）；内核 V11.1.0 按 \
                 tag + sha256 现拉不入库；`make smoke` 在 QEMU 中调用 poly_engine_* 并以 \
                 semihosting 退出码 0 结束；arm-none-eabi-gcc / qemu-system-arm / 网络缺席时\
                 按环境槽 SKIP-with-print"
                    .to_string(),
            ),
        },
        // ---- L6 车机（T-M6-11，ROADMAP §3.7）：Android Automotive ----
        // 复用 Android 链（format = `apk`、builder = `android-aapt2`；AAPT2 + bundletool +
        // apksigner 不另起第二套实现），差异只在清单变体（`android.hardware.type.automotive`）
        // 与发布渠道。车厂 / OEM 渠道无统一公开 API：`oem-channel` 是占位 Publisher，返回
        // 「需 OEM 凭据引用」的明确错误，绝不伪造提交。
        TargetSpec {
            id: "android-automotive".to_string(),
            tier: 6,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Native,
            bridge: Bridge::Uniffi,
            ui: vec!["react-native".to_string()],
            package: PackageSpec {
                format: "apk".to_string(),
                builder: "android-aapt2".to_string(),
            },
            signing: Some("android-keystore".to_string()),
            verify: vec![
                "apksigner-verify".to_string(),
                "bundletool-validate".to_string(),
            ],
            publish: vec!["oem-channel".to_string()],
            experimental: false,
            notes: Some(
                "AAOS 清单（android.hardware.type.automotive）；车厂渠道，无统一公开 API——\
                 发布需 OEM 凭据引用（占位 Publisher，不伪造提交）"
                    .to_string(),
            ),
        },
        // ---- L6 穿戴（T-M7-02，ROADMAP §3.7 穿戴行）：Wear OS ----
        // 复用 Android 链（format = `apk`、builder = `android-aapt2`；AAPT2 + bundletool +
        // apksigner 不另起第二套实现），差异只有清单变体（`android.hardware.type.watch`）与
        // minSdk 下限（26 = Wear OS 2.0）；发布走 Google Play（穿戴轨道）。`experimental =
        // false`：正式打包链；RN on Wear 的 UI 运行时仍为 beta，如实入 notes。
        TargetSpec {
            id: "wearos".to_string(),
            tier: 6,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Native,
            bridge: Bridge::Uniffi,
            ui: vec!["react-native".to_string()],
            package: PackageSpec {
                format: "apk".to_string(),
                builder: "android-aapt2".to_string(),
            },
            signing: Some("android-keystore".to_string()),
            verify: vec![
                "apksigner-verify".to_string(),
                "bundletool-validate".to_string(),
            ],
            publish: vec!["google-play-wear".to_string()],
            experimental: false,
            notes: Some(
                "Wear OS 清单（android.hardware.type.watch，minSdkVersion ≥ 26）；\
                 UI 运行时（RN on Wear）为 beta，打包与签名走 Android 基座"
                    .to_string(),
            ),
        },
        // ---- L6 嵌入式 / 车机（T-M6-03/04，ROADMAP §3.7）：QNX / AUTOSAR Adaptive ----
        // 两目标共享 T-M6-02 的 `poly_engine_*` C ABI（头文件由 poly-core 生成器唯一产出）。
        // QNX：`image` 主产物（部署镜像 tar；SDP 在场时另出 mkifs `.ifs` 与静态库），配置
        // `[targets.qnx].format = "library"` 可把 `.a` 提为主产物；无 SDP 时交叉编译按环境槽
        // SKIP-with-print，绝不伪造二进制。AUTOSAR：宿主静态库 + ARXML 清单，ARXML 的
        // 服务 / 端口由同一 C ABI 清单派生（一一对应），无 UI、自部署。
        TargetSpec {
            id: "qnx".to_string(),
            tier: 6,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Staticlib,
            bridge: Bridge::Cffi,
            ui: Vec::new(),
            package: PackageSpec {
                format: "image".to_string(),
                builder: "qnx-sdp".to_string(),
            },
            signing: None,
            verify: vec![
                "qnx-image-structure".to_string(),
                "c-abi-symbols".to_string(),
                "qnx-qemu-smoke".to_string(),
            ],
            publish: vec!["self-hosted".to_string()],
            experimental: false,
            notes: Some(
                "headless 核心（可选 HMI 归平台侧）；静态库 + 部署镜像（附 mkifs buildfile，\
                 可引导 .ifs / QEMU 归 T-M6-09）；交叉编译需 QNX SDP（$QNX_HOST / \
                 $QNX_TARGET）与 Rust QNX target，无 SDP 时按环境槽 SKIP-with-print，\
                 不伪造二进制；自部署"
                    .to_string(),
            ),
        },
        TargetSpec {
            id: "autosar".to_string(),
            tier: 6,
            hosts: vec![Host::Win, Host::Linux, Host::Macos],
            runtime: Runtime::Staticlib,
            bridge: Bridge::Cffi,
            ui: Vec::new(),
            package: PackageSpec {
                format: "arxml".to_string(),
                builder: "autosar-adaptive".to_string(),
            },
            signing: None,
            verify: vec!["arxml-schema".to_string(), "c-abi-symbols".to_string()],
            publish: vec!["self-hosted".to_string()],
            experimental: false,
            notes: Some(
                "无 UI、自部署；AUTOSAR Adaptive 清单（ARXML）由 poly_engine_* C ABI 单一来源\
                 派生（服务 / 端口与导出符号一一对应）；no_std 可选（引擎层 \
                 --no-default-features，应用提供 global_allocator / panic_handler）"
                    .to_string(),
            ),
        },
    ]
}
