//! T-M0-05 验收：L1 七目标声明（ROADMAP §3.2 逐列对齐）。

use std::collections::BTreeSet;

use poly_targets::{Bridge, Host, Registry, Runtime, TargetRequest};

/// §3.2 判别列：id / hosts / runtime / bridge / ui / package.format / builder / signing。
#[allow(clippy::type_complexity)] // 测试数据表，拆成 struct 反而更难与表格对照
const L1_COLUMNS: &[(
    &str,
    &[Host],
    Runtime,
    Bridge,
    &[&str],
    &str,
    &str,
    Option<&str>,
)] = &[
    (
        "android",
        &[Host::Win, Host::Linux, Host::Macos],
        Runtime::Native,
        Bridge::Uniffi,
        &["react-native"],
        "aab",
        "android-aapt2",
        Some("android-keystore"),
    ),
    (
        "ios",
        &[Host::Macos],
        Runtime::Native,
        Bridge::Uniffi,
        &["react-native"],
        "ipa",
        "ios-xcodebuild",
        Some("apple-codesign"),
    ),
    (
        "harmonyos",
        &[Host::Win, Host::Linux, Host::Macos],
        Runtime::Native,
        Bridge::Napi,
        &["react-native-ohos"],
        "hap",
        "harmonyos-hvigor",
        Some("harmonyos-hap-sign"),
    ),
    (
        "windows",
        &[Host::Win],
        Runtime::Native,
        Bridge::TauriIpc,
        &["react-dom"],
        "msi",
        "windows-wix",
        Some("windows-authenticode"),
    ),
    (
        "macos",
        &[Host::Macos],
        Runtime::Native,
        Bridge::TauriIpc,
        &["react-dom"],
        "dmg",
        "macos-dmg",
        Some("apple-codesign"),
    ),
    (
        "linux",
        &[Host::Linux],
        Runtime::Native,
        Bridge::TauriIpc,
        &["react-dom"],
        "appimage",
        "linux-appimage",
        Some("linux-repo-gpg"),
    ),
    (
        "web",
        &[Host::Win, Host::Linux, Host::Macos],
        Runtime::Wasm,
        Bridge::WasmBindgen,
        &["react-dom"],
        "static-site",
        "web-static",
        None, // 无签名（可选 SRI，见 verify）
    ),
];

#[test]
fn builtin_has_seven_valid_targets() {
    let registry = Registry::builtin();
    assert_eq!(
        registry.all().filter(|spec| spec.tier == 1).count(),
        7,
        "L1 恰为七目标"
    );
    assert_eq!(
        registry.all().count(),
        12,
        "M4 起内建 = L1 七 + L2 小程序族四 + L3 Apple tvOS"
    );
    for spec in registry.all() {
        spec.validate()
            .unwrap_or_else(|e| panic!("内建目标 {} 未通过校验: {e}", spec.id));
    }
}

#[test]
fn builtin_ids_are_the_l1_set() {
    let registry = Registry::builtin();
    let ids: BTreeSet<String> = registry
        .all()
        .filter(|spec| spec.tier == 1)
        .map(|spec| spec.id.clone())
        .collect();
    let expected: BTreeSet<String> = [
        "android",
        "ios",
        "harmonyos",
        "windows",
        "macos",
        "linux",
        "web",
    ]
    .map(String::from)
    .into_iter()
    .collect();
    assert_eq!(ids, expected);

    let l2: BTreeSet<String> = registry
        .all()
        .filter(|spec| spec.tier == 2)
        .map(|spec| spec.id.clone())
        .collect();
    let expected_l2: BTreeSet<String> = ["wechat", "alipay", "douyin", "quickapp"]
        .map(String::from)
        .into_iter()
        .collect();
    assert_eq!(l2, expected_l2, "L2 恰为小程序族四目标");
}

#[test]
fn builtin_matches_roadmap_3_2_columns() {
    let registry = Registry::builtin();
    for (id, hosts, runtime, bridge, ui, format, builder, signing) in L1_COLUMNS {
        let spec = registry.get(id).unwrap_or_else(|| panic!("缺少目标 {id}"));
        assert_eq!(spec.tier, 1, "{id}: tier");
        assert_eq!(&spec.hosts, hosts, "{id}: hosts");
        assert_eq!(spec.runtime, *runtime, "{id}: runtime");
        assert_eq!(spec.bridge, *bridge, "{id}: bridge");
        assert_eq!(
            spec.ui.iter().map(String::as_str).collect::<Vec<_>>(),
            *ui,
            "{id}: ui"
        );
        assert_eq!(spec.package.format, *format, "{id}: package.format");
        assert_eq!(spec.package.builder, *builder, "{id}: package.builder");
        assert_eq!(spec.signing.as_deref(), *signing, "{id}: signing");
        assert!(!spec.experimental, "{id}: L1 非实验目标");
    }
}

#[test]
fn every_l1_target_has_verifier_and_publisher() {
    for spec in Registry::builtin().all() {
        assert!(!spec.verify.is_empty(), "{} 缺少产物校验器", spec.id);
        assert!(!spec.publish.is_empty(), "{} 缺少发布渠道", spec.id);
        if let Some(signing) = &spec.signing {
            assert!(
                poly_targets::spec::id_is_valid(signing),
                "{}: 签名必须是凭据引用（id 形态），实际为 {signing}",
                spec.id
            );
        }
    }
}

#[test]
fn linux_host_resolves_four_l1_targets_from_all() {
    let resolved = Registry::builtin()
        .resolve_all(&TargetRequest::All, &[Host::Linux], false)
        .expect("内建目标解析不应失败");

    let selected: BTreeSet<&str> = resolved.selected.iter().map(|s| s.id.as_str()).collect();
    let l1: BTreeSet<&str> = resolved
        .selected
        .iter()
        .filter(|s| s.tier == 1)
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        l1,
        BTreeSet::from(["android", "harmonyos", "linux", "web"]),
        "Linux 宿主下 iOS / macOS / Windows 因宿主不可达被排除"
    );
    // L2 小程序族声明三宿主（Taro 编译不绑定本地宿主）→ 任何宿主下都入选
    let l2: BTreeSet<&str> = resolved
        .selected
        .iter()
        .filter(|s| s.tier == 2)
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        l2,
        BTreeSet::from(["wechat", "alipay", "douyin", "quickapp"]),
        "小程序族四目标应全部入选"
    );
    assert_eq!(selected.len(), l1.len() + l2.len());

    let excluded: BTreeSet<&str> = resolved.excluded.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        excluded,
        BTreeSet::from(["ios", "macos", "tvos", "windows"]),
        "含 M4 的 tvos（声明宿主 = macOS）"
    );
    for target in &resolved.excluded {
        assert!(!target.reason.trim().is_empty(), "{} 缺排除原因", target.id);
    }

    // 内建目标为 L1 / L2 / L3，不涉及厂商授权分支。
    assert!(resolved.excluded.iter().all(|e| !e.reason.contains("L7")));
}

/// T-M3-05 / T-M3-06 验收（声明侧）：L2 小程序族四目标按 ROADMAP §3.3 前四行逐列对齐。
#[allow(clippy::type_complexity)] // 同上：测试数据表与表格对照优先
const L2_COLUMNS: &[(&str, &str, &str, &str, &str)] = &[
    // id / ui / package.format / builder / verify[0]
    (
        "wechat",
        "taro",
        "weapp",
        "taro-weapp",
        "wechat-devtools-cli",
    ),
    (
        "alipay",
        "taro",
        "alipay",
        "taro-alipay",
        "alipay-devtools-cli",
    ),
    ("douyin", "taro", "tt", "taro-tt", "douyin-devtools-cli"),
    // T-M3-12 收口：快应用 format/builder 按 T-M3-06 预告翻到 `rpk` / `hap-toolkit`
    // （§3.3 快应用行「打包器与产物：`.rpk`」）
    ("quickapp", "taro", "rpk", "hap-toolkit", "hap-toolkit-lint"),
];

#[test]
fn builtin_matches_roadmap_3_3_mini_programs() {
    let registry = Registry::builtin();
    for (id, ui, format, builder, verify) in L2_COLUMNS {
        let spec = registry.get(id).unwrap_or_else(|| panic!("缺少目标 {id}"));
        assert_eq!(spec.tier, 2, "{id}: 小程序族为 L2");
        assert_eq!(spec.runtime, Runtime::Wasm, "{id}: wasm 进宿主沙箱");
        assert_eq!(spec.bridge, Bridge::WasmBindgen, "{id}: wasm-bindgen");
        assert_eq!(
            spec.ui.iter().map(String::as_str).collect::<Vec<_>>(),
            vec![*ui],
            "{id}: ui"
        );
        assert_eq!(spec.package.format, *format, "{id}: package.format");
        assert_eq!(spec.package.builder, *builder, "{id}: package.builder");
        assert_eq!(
            spec.verify.first().map(String::as_str),
            Some(*verify),
            "{id}: verify"
        );
        assert!(
            spec.signing.is_none(),
            "{id}: 签名在平台侧（上传签名），本地无签名引用"
        );
        assert_eq!(spec.hosts.len(), 3, "{id}: Taro 编译不绑定本地宿主");
        assert!(spec.notes.is_some(), "{id}: 需边界 / 降级说明");
    }
}

/// T-M4-07 验收（声明侧）：L3 电视族首行 Apple tvOS 按 ROADMAP §3.4 逐列对齐。
#[test]
fn builtin_matches_roadmap_3_4_apple_tvos() {
    let tvos = Registry::builtin()
        .get("tvos")
        .expect("内建 tvos 目标")
        .clone();
    assert_eq!(tvos.tier, 3, "L3 电视与大屏");
    assert_eq!(tvos.hosts, vec![Host::Macos], "需 macOS 宿主");
    assert_eq!(tvos.runtime, Runtime::Native, "native staticlib");
    assert_eq!(tvos.bridge, Bridge::Uniffi);
    assert_eq!(
        tvos.ui.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["react-native-tvos"],
        "RN-tvOS 承载 UI"
    );
    // 产物格式同 ios（ipa）：打包链按目标切 tvOS SDK / 切片，格式不分裂（T-M4-07）
    assert_eq!(tvos.package.format, "ipa", "TV 包");
    assert_eq!(
        tvos.package.builder, "tvos-xcodebuild",
        "xcodebuild（复用 iOS 链）"
    );
    assert_eq!(tvos.signing.as_deref(), Some("apple-codesign"), "codesign");
    assert_eq!(
        tvos.verify.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["codesign-verify", "spctl"]
    );
    assert_eq!(
        tvos.publish.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["app-store-connect"]
    );
    assert!(!tvos.experimental, "T-M4-07 为正式链（非实验档）");
    assert!(
        tvos.notes.as_deref().unwrap_or("").contains("macOS"),
        "需宿主边界说明"
    );
}
