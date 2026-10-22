//! T-M0-05 验收：L1 七目标声明（ROADMAP §3.2 逐列对齐）。

use std::collections::BTreeSet;

use polypack_targets::{Bridge, Host, Registry, Runtime, TargetRequest};

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
        43,
        "M7 起内建 = L1 七 + L2 小程序族四（wechat / alipay / douyin / quickapp）+ L2 HTML5 \
         二（tiktok-minis / facebook-instant-games※实验）+ \
         L3 电视族九（tvos + android-tv / fire-tv + tizen-tv / webos-tv / vidaa-tv / smartcast-tv \
         + roku※实验，T-M4-04/07/08/09/10/12 + harmonyos-tv，T-M4-14）+ L4 GrapheneOS / KaiOS \
         （grapheneos；kaios）+ L5 OpenBSD / FreeBSD / NetBSD（T-M5-07/06/08）+\
         发行版三（ubuntu / fedora / mint，T-M5-01）+ ChromeOS 双轨（chromeos / chromeos-pwa，\
         T-M5-10）+ L6 车机 / 嵌入式 / 穿戴十一目标（android-automotive，T-M6-11；qnx / autosar，\
         T-M6-03/04；vxworks / zephyr，T-M6-05/06；freertos，T-M6-07；garmin※实验，T-M7-09；\
         watchos※实验，T-M7-05/06；fitbit / zepp，T-M7-07/08；wearos，T-M7-02）"
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
    let expected_l2: BTreeSet<String> = [
        "wechat",
        "alipay",
        "douyin",
        "quickapp",
        "tiktok-minis",
        "facebook-instant-games",
    ]
    .map(String::from)
    .into_iter()
    .collect();
    assert_eq!(l2, expected_l2, "L2 = 小程序族四 + HTML5 平台二");

    let l3: BTreeSet<String> = registry
        .all()
        .filter(|spec| spec.tier == 3)
        .map(|spec| spec.id.clone())
        .collect();
    let expected_l3: BTreeSet<String> = [
        "tvos",
        "android-tv",
        "fire-tv",
        "harmonyos-tv",
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
        "L3 恰为电视族九（tvos + android-tv / fire-tv / harmonyos-tv + web 系四 + roku※实验）"
    );

    let l4: BTreeSet<String> = registry
        .all()
        .filter(|spec| spec.tier == 4)
        .map(|spec| spec.id.clone())
        .collect();
    assert_eq!(
        l4,
        BTreeSet::from(["grapheneos".to_string(), "kaios".to_string()]),
        "L4 已声明目标 = GrapheneOS（复用 Android 普通 APK 链）+ KaiOS（web 栈 zip）"
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
            "netbsd".to_string(),
            "ubuntu".to_string(),
            "fedora".to_string(),
            "mint".to_string(),
            "chromeos".to_string(),
            "chromeos-pwa".to_string(),
        ]),
        "L5 = OpenBSD ports / FreeBSD pkg / NetBSD pkgsrc（T-M5-07/06/08）+ 发行版三（T-M5-01）\
         + ChromeOS 双轨（T-M5-10）"
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
        if spec.publish.is_empty() {
            // 例外：KaiStore / TikTok Minis / Facebook Instant Games 尚无 polypack-publish
            // 客户端（Channel 枚举不含对应渠道），空 publish 必须在 notes 如实说明，
            // 绝不编造渠道。
            assert!(
                matches!(
                    spec.id.as_str(),
                    "kaios" | "tiktok-minis" | "facebook-instant-games"
                ),
                "{} 缺少发布渠道",
                spec.id
            );
            let notes = spec.notes.as_deref().unwrap_or_default();
            assert!(
                notes.contains("polypack-publish"),
                "{}: 空 publish 须在 notes 说明缺失的客户端：{notes}",
                spec.id
            );
            continue;
        }
        if let Some(signing) = &spec.signing {
            assert!(
                polypack_targets::spec::id_is_valid(signing),
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
        BTreeSet::from([
            "wechat",
            "alipay",
            "douyin",
            "quickapp",
            "tiktok-minis",
            "facebook-instant-games"
        ]),
        "小程序族四 + HTML5 平台二应全部入选"
    );
    // L3 电视三目标同声明三宿主（Android / HarmonyOS 链）→ Linux 宿主下也全部入选；
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
            "harmonyos-tv",
            "tizen-tv",
            "webos-tv",
            "vidaa-tv",
            "smartcast-tv",
            "roku"
        ]),
        "电视三目标 + web 系 TV 四目标 + roku（三宿主声明）应全部入选"
    );
    // L4 GrapheneOS / KaiOS 均声明三宿主（APK 链 / web 栈）→ Linux 宿主下入选
    let l4: BTreeSet<&str> = resolved
        .selected
        .iter()
        .filter(|s| s.tier == 4)
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        l4,
        BTreeSet::from(["grapheneos", "kaios"]),
        "GrapheneOS / KaiOS 应入选"
    );
    // L5：openbsd / freebsd / netbsd、发行版三与 ChromeOS deb 轨声明 Linux 宿主；
    // chromeos-pwa 声明三宿主（web 运行时）→ 全部入选
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
            "netbsd",
            "ubuntu",
            "fedora",
            "mint",
            "chromeos",
            "chromeos-pwa"
        ]),
        "L5 OpenBSD / FreeBSD / NetBSD + 发行版三 + ChromeOS 双轨应全部入选"
    );
    // L6 车机 / 嵌入式 / 穿戴：android-automotive / fitbit / zepp / wearos 声明三宿主；
    // qnx / autosar / vxworks / zephyr / freertos / garmin（T-M7-09 实验）声明 Linux 宿主
    // → 在 Linux 宿主下全部入选；watchos 声明 macOS 专宿主 → 排除
    let l6: BTreeSet<&str> = resolved
        .selected
        .iter()
        .filter(|s| s.tier == 6)
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        l6,
        BTreeSet::from([
            "android-automotive",
            "wearos",
            "qnx",
            "autosar",
            "vxworks",
            "zephyr",
            "freertos",
            "garmin",
            "fitbit",
            "zepp"
        ]),
        "L6 车机 + 嵌入式 + 穿戴十目标应入选（watchos 为 macOS 专宿主）"
    );
    assert_eq!(
        selected.len(),
        l1.len() + l2.len() + l3.len() + l4.len() + l5.len() + l6.len()
    );

    let excluded: BTreeSet<&str> = resolved.excluded.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        excluded,
        BTreeSet::from(["ios", "macos", "tvos", "watchos", "windows"]),
        "含 M4 的 tvos / M7 的 watchos（声明宿主 = macOS）"
    );
    for target in &resolved.excluded {
        assert!(!target.reason.trim().is_empty(), "{} 缺排除原因", target.id);
    }

    // 内建目标为 L1 / L2 / L3 / L4 / L5 / L6，不涉及厂商授权分支。
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

/// L2 HTML5 平台（TikTok Minis / Facebook Instant Games）：runtime / bridge = `html5`，
/// react-dom 静态产物 + 确定性 zip；平台侧沙箱 / 审核不可得 → experimental，notes 引用
/// 来源并如实说明无发布客户端。
#[test]
fn builtin_matches_html5_l2_platforms() {
    let registry = Registry::builtin();
    for (id, builder, verify) in [
        (
            "tiktok-minis",
            "tiktok-minis",
            "tiktok-minis-structure-check",
        ),
        (
            "facebook-instant-games",
            "facebook-instant-games",
            "facebook-instant-games-structure-check",
        ),
    ] {
        let spec = registry.get(id).unwrap_or_else(|| panic!("缺少目标 {id}"));
        assert_eq!(spec.tier, 2, "{id}: L2");
        assert_eq!(spec.runtime, Runtime::Html5, "{id}: HTML5 运行时");
        assert_eq!(spec.bridge, Bridge::Html5, "{id}: HTML5 桥");
        assert_eq!(
            spec.ui.iter().map(String::as_str).collect::<Vec<_>>(),
            ["react-dom"],
            "{id}: ui"
        );
        assert_eq!(spec.package.format, "zip", "{id}: package.format");
        assert_eq!(spec.package.builder, builder, "{id}: package.builder");
        assert_eq!(
            spec.verify.first().map(String::as_str),
            Some(verify),
            "{id}: verify"
        );
        assert!(spec.signing.is_none(), "{id}: 签名 / 审核在平台侧");
        assert!(spec.experimental, "{id}: 平台侧沙箱不可得 → experimental");
        assert_eq!(spec.hosts.len(), 3, "{id}: 三宿主");
        let notes = spec.notes.as_deref().unwrap_or_default();
        assert!(notes.contains("SKIP-with-print"), "{id}: {notes}");
        assert!(notes.contains("polypack-publish"), "{id}: {notes}");
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

/// T-M4-14 / L4 复用链验收（声明侧）：HarmonyOS TV 镜像 `harmonyos`，GrapheneOS 复用
/// Android 普通 APK 链；边界（无 hdc 设备、无 GMS flavor、无 F-Droid 客户端）落在 notes。
#[test]
fn builtin_matches_harmonyos_tv_and_grapheneos_reuse_specs() {
    let registry = Registry::builtin();

    let tv = registry
        .get("harmonyos-tv")
        .expect("内建 harmonyos-tv 目标");
    assert_eq!(tv.tier, 3, "harmonyos-tv: 电视族为 L3");
    assert_eq!(tv.hosts, vec![Host::Win, Host::Linux, Host::Macos]);
    assert_eq!(tv.runtime, Runtime::Native);
    assert_eq!(tv.bridge, Bridge::Napi);
    assert_eq!(tv.ui, ["react-native-ohos"]);
    assert_eq!(tv.package.format, "hap");
    assert_eq!(tv.package.builder, "harmonyos-hvigor");
    assert_eq!(tv.signing.as_deref(), Some("harmonyos-hap-sign"));
    assert_eq!(
        tv.verify,
        ["hap-sign-verify", "hap-structure-check"].map(String::from)
    );
    assert_eq!(tv.publish, ["appgallery-connect"]);
    assert!(!tv.experimental);
    let notes = tv.notes.as_deref().expect("harmonyos-tv 缺 notes");
    assert!(
        notes.contains("T-M4-14") && notes.contains("SKIP") && notes.contains("无独立渠道"),
        "harmonyos-tv: {notes}"
    );

    let graphene = registry.get("grapheneos").expect("内建 grapheneos 目标");
    assert_eq!(graphene.tier, 4, "grapheneos: L4 移动系统变体");
    assert_eq!(graphene.hosts, vec![Host::Win, Host::Linux, Host::Macos]);
    assert_eq!(graphene.runtime, Runtime::Native);
    assert_eq!(graphene.bridge, Bridge::Uniffi);
    assert_eq!(graphene.ui, ["react-native"]);
    assert_eq!(graphene.package.format, "apk");
    assert_eq!(graphene.package.builder, "android-aapt2");
    assert_eq!(graphene.signing.as_deref(), Some("android-keystore"));
    assert_eq!(
        graphene.verify,
        ["apksigner-verify", "bundletool-validate"].map(String::from)
    );
    assert_eq!(graphene.publish, ["self-hosted"]);
    assert!(!graphene.experimental);
    let notes = graphene.notes.as_deref().expect("grapheneos 缺 notes");
    assert!(
        notes.contains("GMS") && notes.contains("reproducible") && notes.contains("self-hosted"),
        "grapheneos: {notes}"
    );
}

/// KaiOS 验收（声明侧）：react-dom web 栈 → `manifest.webapp` + UI dist 的确定性 zip，
/// 结构自检与 verify 腿同名；无 polypack-publish KaiStore 客户端，publish 留空并在 notes 说明。
#[test]
fn builtin_matches_kaios_webapp_spec() {
    let registry = Registry::builtin();
    let kaios = registry.get("kaios").expect("内建 kaios 目标");
    assert_eq!(kaios.tier, 4, "kaios: L4 移动系统");
    assert_eq!(kaios.hosts, vec![Host::Win, Host::Linux, Host::Macos]);
    assert_eq!(kaios.runtime, Runtime::Wasm);
    assert_eq!(kaios.bridge, Bridge::WasmBindgen);
    assert_eq!(kaios.ui, ["react-dom"]);
    assert_eq!(kaios.package.format, "zip");
    assert_eq!(kaios.package.builder, "kaios-webapp");
    assert_eq!(kaios.signing, None);
    assert_eq!(kaios.verify, ["kaios-structure-check"].map(String::from));
    assert!(
        kaios.publish.is_empty(),
        "KaiStore 尚无 polypack-publish 客户端"
    );
    assert!(!kaios.experimental, "包结构真实且有测试，不应标实验");
    let notes = kaios.notes.as_deref().expect("kaios 缺 notes");
    assert!(
        notes.contains("manifest.webapp")
            && notes.contains("KaiStore")
            && notes.contains("polypack-publish")
            && notes.contains("SKIP"),
        "kaios: {notes}"
    );
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

#[test]
fn bsd_targets_are_reachable_on_bsd_hosts() {
    let registry = Registry::builtin();
    for id in ["freebsd", "openbsd"] {
        let spec = registry.get(id).unwrap_or_else(|| panic!("{id} 应内建"));
        assert!(
            spec.hosts.contains(&Host::Bsd),
            "{id}: BSD 原生宿主要可达（T-M5-09 VM 真跑）"
        );
        assert!(
            spec.hosts.contains(&Host::Linux),
            "{id}: Linux 交叉发起保留"
        );
    }
}

/// T-M6-11 验收（声明侧）：Android Automotive 按 ROADMAP §3.7 逐列对齐。
/// §3.7 行 = native · uniffi / APK（AAOS）/ 签名同 Android / bundletool / 车厂 · OEM 渠道；
/// 打包链复用 M2（builder `android-aapt2`），发布渠道是 `oem-channel` 占位 Publisher。
#[test]
fn builtin_matches_roadmap_3_7_automotive() {
    let spec = Registry::builtin()
        .get("android-automotive")
        .expect("内建 android-automotive 目标")
        .clone();
    assert_eq!(spec.tier, 6, "Android Automotive 属 L6（ROADMAP §3.7）");
    assert_eq!(
        spec.hosts,
        vec![Host::Win, Host::Linux, Host::Macos],
        "复用 Android 工具链（三宿主）"
    );
    assert_eq!(spec.runtime, Runtime::Native, "§3.7 核心运行时 = native");
    assert_eq!(spec.bridge, Bridge::Uniffi, "§3.7 桥 = uniffi");
    assert_eq!(
        spec.ui.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["react-native"],
        "RN 承载 UI（同 Android）"
    );
    assert_eq!(spec.package.format, "apk", "§3.7 产物 = APK（AAOS）");
    assert_eq!(
        spec.package.builder, "android-aapt2",
        "复用 M2 Android 打包链（不新增第二套实现）"
    );
    assert_eq!(
        spec.signing.as_deref(),
        Some("android-keystore"),
        "§3.7 签名 = 同 Android（凭据引用）"
    );
    assert_eq!(
        spec.verify,
        ["apksigner-verify", "bundletool-validate"].map(String::from),
        "§3.7 产物校验 = bundletool（+ apksigner 取证）"
    );
    assert_eq!(
        spec.publish,
        vec!["oem-channel".to_string()],
        "§3.7 发布渠道 = 车厂 / OEM（占位 Publisher）"
    );
    assert!(
        !spec.experimental,
        "§3.7 未标 ※实验：打包链为正式 Android 链"
    );
    let notes = spec.notes.as_deref().unwrap_or("");
    assert!(
        notes.contains("车厂渠道，无统一公开 API"),
        "边界口径须如实入 notes：{notes}"
    );
    assert!(
        notes.contains("android.hardware.type.automotive") && notes.contains("不伪造提交"),
        "AAOS 清单变体与占位发布边界须入 notes：{notes}"
    );
}

/// T-M7-02 验收（声明侧）：Wear OS 按 ROADMAP §3.7 穿戴行逐列对齐。
/// §3.7 行 = native · uniffi / APK（Wear）/ 签名同 Android / bundletool / Google Play 穿戴轨道；
/// 打包链复用 M2（builder `android-aapt2`），清单变体 `android.hardware.type.watch`。
#[test]
fn builtin_matches_roadmap_3_7_wearos() {
    let spec = Registry::builtin()
        .get("wearos")
        .expect("内建 wearos 目标")
        .clone();
    assert_eq!(spec.tier, 6, "Wear OS 属 L6（ROADMAP §3.7）");
    assert_eq!(
        spec.hosts,
        vec![Host::Win, Host::Linux, Host::Macos],
        "复用 Android 工具链（三宿主）"
    );
    assert_eq!(spec.runtime, Runtime::Native, "§3.7 核心运行时 = native");
    assert_eq!(spec.bridge, Bridge::Uniffi, "§3.7 桥 = uniffi");
    assert_eq!(
        spec.ui.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["react-native"],
        "RN 承载 UI（同 Android）"
    );
    assert_eq!(spec.package.format, "apk", "§3.7 产物 = APK（Wear）");
    assert_eq!(
        spec.package.builder, "android-aapt2",
        "复用 M2 Android 打包链（不新增第二套实现）"
    );
    assert_eq!(
        spec.signing.as_deref(),
        Some("android-keystore"),
        "§3.7 签名 = 同 Android（凭据引用）"
    );
    assert_eq!(
        spec.verify,
        ["apksigner-verify", "bundletool-validate"].map(String::from),
        "§3.7 产物校验 = bundletool（+ apksigner 取证）"
    );
    assert_eq!(
        spec.publish,
        vec!["google-play-wear".to_string()],
        "§3.7 发布渠道 = Google Play 穿戴轨道"
    );
    assert!(
        !spec.experimental,
        "§3.7 未标 ※实验：打包链为正式 Android 链"
    );
    let notes = spec.notes.as_deref().unwrap_or("");
    assert!(
        notes.contains("android.hardware.type.watch") && notes.contains("minSdkVersion ≥ 26"),
        "Wear 清单变体与最小 API 须入 notes：{notes}"
    );
    assert!(
        notes.contains("RN on Wear）为 beta") && notes.contains("打包与签名走 Android 基座"),
        "UI 运行时 beta 边界须如实入 notes：{notes}"
    );
}

/// T-M6-03/04 验收（声明侧）：QNX 与 AUTOSAR Adaptive 按 ROADMAP §3.7 逐列对齐。
///
/// §3.7：QNX = staticlib · cffi / 镜像 · 库 / 平台侧签名 / QNX 交叉编译冒烟 / 自部署；
/// AUTOSAR = staticlib · cffi（no_std 可选）/ ARXML + 库 / 无签名 / CI 编译矩阵 / 自部署 / 无 UI。
#[test]
fn builtin_matches_roadmap_3_7_qnx_autosar() {
    let registry = Registry::builtin();
    let qnx = registry.get("qnx").expect("内建 qnx 目标").clone();
    assert_eq!(qnx.tier, 6, "QNX 属 L6（ROADMAP §3.7）");
    assert_eq!(qnx.hosts, vec![Host::Win, Host::Linux, Host::Macos]);
    assert_eq!(qnx.runtime, Runtime::Staticlib, "§3.7 运行时 = staticlib");
    assert_eq!(qnx.bridge, Bridge::Cffi, "§3.7 桥 = cffi");
    assert!(qnx.ui.is_empty(), "headless 核心（UI 可选层归平台侧）");
    assert_eq!(qnx.package.format, "image", "主产物格式 = image");
    assert_eq!(qnx.package.builder, "qnx-sdp", "打包器 = QNX SDP");
    assert!(qnx.signing.is_none(), "§3.7 签名 = 平台侧，本地无凭据引用");
    assert!(
        qnx.verify.iter().any(|id| id == "c-abi-symbols"),
        "复用 T-M6-02 C ABI 的符号校验"
    );
    let notes = qnx.notes.as_deref().unwrap_or("");
    assert!(
        notes.contains("SKIP-with-print") && notes.contains("QNX SDP"),
        "无 SDP 的 SKIP 口径须入 notes：{notes}"
    );

    let autosar = registry.get("autosar").expect("内建 autosar 目标").clone();
    assert_eq!(autosar.tier, 6, "AUTOSAR 属 L6（ROADMAP §3.7）");
    assert_eq!(autosar.hosts, vec![Host::Win, Host::Linux, Host::Macos]);
    assert_eq!(
        autosar.runtime,
        Runtime::Staticlib,
        "§3.7 运行时 = staticlib"
    );
    assert_eq!(autosar.bridge, Bridge::Cffi, "§3.7 桥 = cffi");
    assert!(autosar.ui.is_empty(), "§3.7 无 UI");
    assert_eq!(autosar.package.format, "arxml", "产物 = ARXML 清单 + 库");
    assert_eq!(autosar.package.builder, "autosar-adaptive");
    assert!(autosar.signing.is_none(), "§3.7 签名 = —");
    assert!(
        autosar.verify.iter().any(|id| id == "arxml-schema"),
        "ARXML 需 schema 校验器"
    );
    let notes = autosar.notes.as_deref().unwrap_or("");
    assert!(
        notes.contains("无 UI、自部署"),
        "验收字面要求 notes 声明「无 UI、自部署」：{notes}"
    );
    assert!(
        notes.contains("no_std"),
        "no_std 可选口径须入 notes：{notes}"
    );
}

/// T-M6-07 验收（声明侧）：FreeRTOS QEMU 示例板目标。
#[test]
fn builtin_matches_roadmap_3_7_freertos() {
    let freertos = Registry::builtin()
        .get("freertos")
        .expect("内建 freertos 目标")
        .clone();
    assert_eq!(freertos.tier, 6, "FreeRTOS 属 L6（ROADMAP §3.7）");
    assert_eq!(freertos.hosts, vec![Host::Win, Host::Linux, Host::Macos]);
    assert_eq!(freertos.runtime, Runtime::Staticlib);
    assert_eq!(freertos.bridge, Bridge::Cffi);
    assert!(freertos.ui.is_empty(), "§3.7 无 UI");
    assert_eq!(freertos.package.format, "board", "产物 = 板级包");
    assert_eq!(freertos.package.builder, "freertos-qemu");
    assert!(freertos.signing.is_none(), "§3.7 签名 = —");
    assert!(
        freertos.verify.iter().any(|id| id == "c-abi-symbols"),
        "复用 T-M6-02 C ABI 的符号校验"
    );
    assert!(
        freertos.verify.iter().any(|id| id == "freertos-qemu-smoke"),
        "QEMU 冒烟须为独立 verify id"
    );
    let notes = freertos.notes.as_deref().unwrap_or("");
    for needle in ["mps2-an385", "QEMU", "sha256", "SKIP-with-print"] {
        assert!(notes.contains(needle), "notes 缺 {needle}：{notes}");
    }
}

/// T-M7-05/06 验收（声明侧）：watchOS 伴生模板目标（tier 6，实验）。
#[test]
fn builtin_matches_roadmap_3_7_watchos() {
    let watchos = Registry::builtin()
        .get("watchos")
        .expect("内建 watchos 目标")
        .clone();
    assert_eq!(watchos.tier, 6, "watchOS 属 L6（ROADMAP §3.7）");
    assert_eq!(
        watchos.hosts,
        vec![Host::Macos],
        "打包依赖 Xcode → macOS 专宿主"
    );
    assert_eq!(watchos.runtime, Runtime::None, "手表端不直接运行核心");
    assert_eq!(watchos.bridge, Bridge::None);
    assert_eq!(
        watchos.ui.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["swiftui"],
        "§3.7 UI = SwiftUI 伴生模板"
    );
    assert_eq!(watchos.package.format, "watch", "产物 = watch 包");
    assert_eq!(watchos.package.builder, "watchos-xcodebuild");
    assert_eq!(
        watchos.signing.as_deref(),
        Some("apple-codesign"),
        "签名复用 M2 Apple 链（凭据引用）"
    );
    assert!(
        watchos.verify.iter().any(|id| id == "codesign-verify"),
        "校验复用 ios 链的 codesign-verify"
    );
    assert!(
        watchos.publish.iter().any(|id| id == "app-store"),
        "§3.7 发布 = App Store"
    );
    assert!(watchos.experimental, "伴生模板为实验目标");
    let notes = watchos.notes.as_deref().unwrap_or("");
    for needle in ["实验", "SwiftUI", "WatchConnectivity", "macOS", "远程农场"] {
        assert!(notes.contains(needle), "notes 缺 {needle}：{notes}");
    }
}
