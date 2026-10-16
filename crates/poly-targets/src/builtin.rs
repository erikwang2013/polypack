//! 内建目标声明。
//!
//! L1 七目标（android / ios / harmonyos / windows / macos / linux / web）的完整声明
//! 按 ROADMAP §3.2 逐列填写（T-M0-05）：运行时 · 桥、打包器与产物、签名引用、
//! 校验器、发布渠道、备注；tier 均为 1，凭据只以密钥库引用形式出现。
//! M3 增补 L2 小程序族四目标（wechat / alipay / douyin / quickapp，tier 2，ROADMAP §3.3
//! 前四行，T-M3-05 / T-M3-06）：Taro 承载（ui = "taro"）、wasm 进宿主沙箱、签名在平台侧。

use crate::spec::{Bridge, Host, PackageSpec, Runtime, TargetSpec};

/// 全部内建目标（L1 七 + L2 小程序族四）。
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
                // T-M3-06 止于「编译工程」（Taro quickapp 目标）；`.rpk` 收口归 T-M3-12
                // （hap-toolkit 构建，届时本行 format / builder 翻到 `rpk` / `hap-toolkit`）。
                format: "quickapp".to_string(),
                builder: "taro-quickapp".to_string(),
            },
            signing: None,
            verify: vec!["hap-toolkit-lint".to_string()],
            publish: vec!["quickapp-union-store".to_string()],
            experimental: false,
            notes: Some("快应用编译工程（hap-toolkit 就绪）；.rpk 与 lint 归 T-M3-12".to_string()),
        },
    ]
}
