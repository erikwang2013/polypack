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
        26,
        "M5 起内建 = L1 七 + L2 小程序族四（wechat / alipay / douyin / quickapp）+ \
         L3 电视族八（tvos + android-tv / fire-tv + tizen-tv / webos-tv / vidaa-tv / smartcast-tv \
         + roku※实验，T-M4-04/07/08/09/10/12）+ L5 OpenBSD / FreeBSD（T-M5-07/06）+ 发行版三\
         （ubuntu / fedora / mint，T-M5-01）+ ChromeOS 双轨（chromeos / chromeos-pwa，T-M5-10）"
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

    let l3: BTreeSet<String> = registry
        .all()
        .filter(|spec| spec.tier == 3)
        .map(|spec| spec.id.clone())
        .collect();
    let expected_l3: BTreeSet<String> = [
        "tvos",
        "android-tv",
        "fire-tv",
        "tizen-tv",
        "webos-tv",
        "vidaa-tv",
        "smartcast-tv",
        "roku",
    ]
    .map(String::from)
    .into_iter()
    .collect();
    assert_eq!(
        l3, expected_l3,
        "L3 恰为电视族八（tvos + android-tv / fire-tv + web 系四 + roku※实验）"
    );

    let l5: BTreeSet<String> = registry
        .all()
        .filter(|spec| spec.tier == 5)
        .map(|spec| spec.id.clone())
        .collect();
    assert_eq!(
        l5,
        BTreeSet::from([
            "openbsd".to_string(),
            "freebsd".to_string(),
            "ubuntu".to_string(),
            "fedora".to_string(),
            "mint".to_string(),
            "chromeos".to_string(),
            "chromeos-pwa".to_string(),
        ]),
        "L5 = OpenBSD ports / FreeBSD pkg（T-M5-07/06）+ 发行版三（T-M5-01）+ ChromeOS 双轨（T-M5-10）"
    );
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
    // L3 电视两目标同声明三宿主（复用 Android 工具链）→ Linux 宿主下也全部入选；
    // L3 web 系 TV 四目标声明三宿主（web 运行时，同 web 的宿主面）→ 任何宿主下都入选
    let l3: BTreeSet<&str> = resolved
        .selected
        .iter()
        .filter(|s| s.tier == 3)
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        l3,
        BTreeSet::from([
            "android-tv",
            "fire-tv",
            "tizen-tv",
            "webos-tv",
            "vidaa-tv",
            "smartcast-tv",
            "roku"
        ]),
        "电视两目标 + web 系 TV 四目标 + roku（三宿主声明）应全部入选"
    );
    // L5：openbsd / freebsd、发行版三与 ChromeOS deb 轨声明 Linux 宿主；chromeos-pwa
    // 声明三宿主（web 运行时）→ 全部入选
    let l5: BTreeSet<&str> = resolved
        .selected
        .iter()
        .filter(|s| s.tier == 5)
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        l5,
        BTreeSet::from([
            "openbsd",
            "freebsd",
            "ubuntu",
            "fedora",
            "mint",
            "chromeos",
            "chromeos-pwa"
        ]),
        "L5 OpenBSD / FreeBSD + 发行版三 + ChromeOS 双轨应全部入选"
    );
    assert_eq!(selected.len(), l1.len() + l2.len() + l3.len() + l5.len());

    let excluded: BTreeSet<&str> = resolved.excluded.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        excluded,
        BTreeSet::from(["ios", "macos", "tvos", "windows"]),
        "含 M4 的 tvos（声明宿主 = macOS）"
    );
    for target in &resolved.excluded {
        assert!(!target.reason.trim().is_empty(), "{} 缺排除原因", target.id);
    }

    // 内建目标为 L1 / L2 / L3 / L5，不涉及厂商授权分支。
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

/// T-M4-04 验收（声明侧）：L3 电视两目标按 ROADMAP §3.4 前两行逐列对齐。
/// §3.4「产物校验」列 = `bundletool + 焦点导航冒烟`：bundletool 落到 `bundletool-validate`
/// （另列 apksigner-verify 覆盖签名链取证）；焦点冒烟是设备侧走查（T-M4-05），不产生校验器 id。
#[allow(clippy::type_complexity)] // 同上：测试数据表与表格对照优先
const L3_COLUMNS: &[(&str, &str, &str, &str, &str)] = &[
    // id / package.format / builder / publish[0] / notes 判词
    (
        "android-tv",
        "apk",
        "android-aapt2",
        "google-play-tv",
        "Leanback",
    ),
    (
        "fire-tv",
        "apk",
        "android-aapt2",
        "amazon-appstore",
        "Android TV",
    ),
];

#[test]
fn builtin_matches_roadmap_3_4_tv() {
    let registry = Registry::builtin();
    for (id, format, builder, publish, note) in L3_COLUMNS {
        let spec = registry.get(id).unwrap_or_else(|| panic!("缺少目标 {id}"));
        assert_eq!(spec.tier, 3, "{id}: 电视族为 L3");
        assert_eq!(spec.runtime, Runtime::Native, "{id}: native 运行时");
        assert_eq!(spec.bridge, Bridge::Uniffi, "{id}: uniffi 桥");
        assert_eq!(
            spec.ui.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["react-native"],
            "{id}: RN 承载 UI"
        );
        assert_eq!(spec.package.format, *format, "{id}: package.format");
        assert_eq!(spec.package.builder, *builder, "{id}: package.builder");
        assert_eq!(
            spec.signing.as_deref(),
            Some("android-keystore"),
            "{id}: 签名同 Android（§3.4「同 Android」列）"
        );
        assert_eq!(
            spec.verify,
            ["apksigner-verify", "bundletool-validate"].map(String::from),
            "{id}: 校验器（bundletool 覆盖 §3.4「产物校验」列）"
        );
        assert_eq!(
            spec.publish,
            [(*publish).to_string()],
            "{id}: 发布渠道（§3.4 发布列）"
        );
        assert_eq!(spec.hosts.len(), 3, "{id}: 三宿主");
        assert!(
            spec.notes.as_deref().unwrap_or("").contains(note),
            "{id}: 备注应含 {note}"
        );
        assert!(!spec.experimental, "{id}: 非实验目标");
    }
}

/// T-M4-08 / T-M4-09 / T-M4-10 验收（声明侧）：L3 web 系 TV 四目标按 ROADMAP §3.4 逐列对齐；
/// T-M4-10 的边界口径（无签名 / 无推送）落在 `notes`。
#[allow(clippy::type_complexity)] // 同上：测试数据表与表格对照优先
const L3_WEB_TV_COLUMNS: &[(&str, &str, &str, &str, Option<&str>)] = &[
    // id / package.format / builder / verify[0] / signing
    (
        "tizen-tv",
        "wgt",
        "tizen-wgt",
        "wgt-lint",
        Some("tizen-author"), // Tizen 作者证书（三星签名工具，凭据引用）
    ),
    (
        "webos-tv",
        "ipk",
        "webos-ares",
        "ares-package",
        None, // 平台侧（LG Content Store 发布签名）
    ),
    (
        "vidaa-tv",
        "web-app",
        "vidaa-web-app",
        "sandbox-selfcheck",
        None,
    ),
    (
        "smartcast-tv",
        "web-app",
        "smartcast-web-app",
        "sandbox-selfcheck",
        None,
    ),
];

#[test]
fn builtin_matches_roadmap_3_4_web_tv() {
    let registry = Registry::builtin();
    for (id, format, builder, verify, signing) in L3_WEB_TV_COLUMNS {
        let spec = registry.get(id).unwrap_or_else(|| panic!("缺少目标 {id}"));
        assert_eq!(spec.tier, 3, "{id}: web 系 TV 为 L3");
        assert_eq!(spec.runtime, Runtime::Wasm, "{id}: wasm 核心");
        assert_eq!(spec.bridge, Bridge::WasmBindgen, "{id}: wasm-bindgen");
        assert_eq!(
            spec.ui.iter().map(String::as_str).collect::<Vec<_>>(),
            vec!["react-dom"],
            "{id}: ui"
        );
        assert_eq!(spec.package.format, *format, "{id}: package.format");
        assert_eq!(spec.package.builder, *builder, "{id}: package.builder");
        assert_eq!(
            spec.verify.first().map(String::as_str),
            Some(*verify),
            "{id}: verify"
        );
        assert_eq!(spec.signing.as_deref(), *signing, "{id}: signing");
        assert_eq!(spec.hosts.len(), 3, "{id}: web 运行时三宿主全可达");
        assert!(spec.notes.is_some(), "{id}: 边界须如实入 notes");
    }

    // T-M4-10 边界口径：VIDAA / Vizio 的 notes 明确「无签名、无推送」
    for id in ["vidaa-tv", "smartcast-tv"] {
        let notes = registry
            .get(id)
            .and_then(|spec| spec.notes.clone())
            .unwrap_or_else(|| panic!("{id} 缺 notes"));
        assert!(
            notes.contains("无签名") && notes.contains("无推送"),
            "{id}: {notes}"
        );
    }
    // webOS 的发布签名按平台侧标注（T-M4-09）
    let notes = registry
        .get("webos-tv")
        .and_then(|spec| spec.notes.clone())
        .expect("webos-tv 缺 notes");
    assert!(notes.contains("平台侧"), "webos-tv: {notes}");
}

/// T-M5-10 验收（声明侧）：ChromeOS 双轨按 ROADMAP §3.6 逐列对齐。
/// Crostini 轨 = deb（复用 Linux deb 链）；PWA 轨 = 静态站点 + PWA manifest（复用 Web 链）；
/// 两轨均无签名 / 自托管，PWA 审计（`pwa-audit`）是 PWA 轨的产物校验器。
#[test]
fn builtin_matches_roadmap_3_6_chromeos_dual_track() {
    let registry = Registry::builtin();

    let crostini = registry
        .get("chromeos")
        .expect("内建 chromeos（Crostini）目标")
        .clone();
    assert_eq!(crostini.tier, 5, "ChromeOS 属 L5（ROADMAP §3.6）");
    assert_eq!(crostini.hosts, vec![Host::Linux], "deb 打包需 Linux 宿主");
    assert_eq!(crostini.runtime, Runtime::Native);
    assert_eq!(crostini.bridge, Bridge::TauriIpc);
    assert_eq!(
        crostini.ui.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["react-dom"]
    );
    assert_eq!(crostini.package.format, "deb");
    assert_eq!(
        crostini.package.builder, "linux-deb",
        "复用 Linux deb 打包器"
    );
    assert_eq!(crostini.signing, None, "§3.6 签名列 = 无");
    assert_eq!(
        crostini.verify,
        vec!["lintian".to_string()],
        "deb 结构 + lintian 腿"
    );
    assert_eq!(crostini.publish, vec!["self-hosted".to_string()]);
    assert!(
        crostini.notes.as_deref().unwrap_or("").contains("Crostini"),
        "Crostini 安装冒烟口径须入 notes"
    );

    let pwa = registry
        .get("chromeos-pwa")
        .expect("内建 chromeos-pwa 目标")
        .clone();
    assert_eq!(pwa.tier, 5);
    assert_eq!(
        pwa.hosts.len(),
        3,
        "web 运行时三宿主全可达（Win / Linux / macOS）"
    );
    assert_eq!(pwa.runtime, Runtime::Wasm);
    assert_eq!(pwa.bridge, Bridge::WasmBindgen);
    assert_eq!(
        pwa.ui.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["react-dom"]
    );
    assert_eq!(pwa.package.format, "static-site");
    assert_eq!(pwa.package.builder, "web-static", "复用 Web 静态站点打包器");
    assert_eq!(pwa.signing, None, "§3.6 签名列 = 无");
    assert_eq!(
        pwa.verify,
        vec!["pwa-audit".to_string()],
        "PWA 三腿审计接入 verify"
    );
    assert_eq!(pwa.publish, vec!["self-hosted".to_string()]);
    assert!(
        pwa.notes
            .as_deref()
            .unwrap_or("")
            .contains("service worker"),
        "离线能力对 SW 的要求须如实入 notes"
    );
    assert!(
        pwa.notes
            .as_deref()
            .unwrap_or("")
            .contains("SKIP-with-print"),
        "Chrome 缺席时的跳过口径须入 notes"
    );
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
