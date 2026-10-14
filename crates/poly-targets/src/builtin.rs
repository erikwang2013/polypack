//! 内建目标声明。
//!
//! L1 七目标（android / ios / harmonyos / windows / macos / linux / web）的完整声明
//! 按 ROADMAP §3.2 逐列填写（T-M0-05）：运行时 · 桥、打包器与产物、签名引用、
//! 校验器、发布渠道、备注；tier 均为 1，凭据只以密钥库引用形式出现。

use crate::spec::{Bridge, Host, PackageSpec, Runtime, TargetSpec};

/// 全部内建目标。
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
    ]
}
