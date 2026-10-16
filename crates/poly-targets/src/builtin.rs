//! 内建目标声明。
//!
//! L1 七目标（android / ios / harmonyos / windows / macos / linux / web）的完整声明
//! 按 ROADMAP §3.2 逐列填写（T-M0-05）：运行时 · 桥、打包器与产物、签名引用、
//! 校验器、发布渠道、备注；tier 均为 1，凭据只以密钥库引用形式出现。
//! M3 增补 L2 小程序族四目标（wechat / alipay / douyin / quickapp，tier 2，ROADMAP §3.3
//! 前四行，T-M3-05 / T-M3-06）：Taro 承载（ui = "taro"）、wasm 进宿主沙箱、签名在平台侧。
//! M4 增补 L3 电视族（tier 3，ROADMAP §3.4）：Apple tvOS（T-M4-07，RN-tvOS 承载 UI、
//! 复用 iOS Xcode 链，产物格式同 `ipa` 按目标切 SDK / 切片）与 web 系 TV 四目标
//! （tizen-tv / webos-tv / vidaa-tv / smartcast-tv，T-M4-08/09/10：wasm 核心 + react-dom UI，
//! wgt / ipk / web-app 出包）。

use crate::spec::{Bridge, Host, PackageSpec, Runtime, TargetSpec};

/// 全部内建目标（L1 七 + L2 小程序族四 + L3 电视族五：tvos + web 系四）。
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
            verify: vec!["lintian".to_string(), "rpmlint".to_string()],
            publish: vec![
                "flathub".to_string(),
                "snapcraft".to_string(),
                "self-hosted".to_string(),
            ],
            experimental: false,
            notes: None,
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
    ]
}
