//! T-M0-04 验收：TargetSpec 校验、注册表与 `--target all` 语义（ROADMAP §4.3）。

use std::collections::BTreeSet;

use poly_targets::spec::id_is_valid;
use poly_targets::{Host, Registry, SpecError, TargetRegistry, TargetRequest, TargetSpec};

/// 内建目标（L1 七 + L2 小程序族四 + L3 电视族八 + L5 发行版三 / OpenBSD / FreeBSD /
/// NetBSD / ChromeOS 双轨 + L6 七）+ 一个 L7 占位目标（用于覆盖厂商授权排除分支）。
fn registry_with_l7() -> Registry {
    let mut specs = poly_targets::builtin::builtin();
    specs.push(TargetSpec::new_stub("vendor-console", 7, &[Host::Linux]));
    Registry::from_specs(specs).expect("合法目标集合应可构建注册表")
}

fn selected_ids(registry: &Registry, hosts: &[Host], allow_unreachable: bool) -> BTreeSet<String> {
    registry
        .resolve_all(&TargetRequest::All, hosts, allow_unreachable)
        .expect("All 请求不应失败")
        .selected
        .iter()
        .map(|s| s.id.clone())
        .collect()
}

#[test]
fn all_on_linux_excludes_windows_only_and_l7_with_reasons() {
    let resolved = registry_with_l7()
        .resolve_all(&TargetRequest::All, &[Host::Linux], false)
        .expect("All 请求不应失败");

    let selected: BTreeSet<&str> = resolved.selected.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        selected,
        // L1 宿主可达者 + L2 小程序族四（Taro 编译不绑定本地宿主，任何宿主下都入选）
        // + L3 电视两（复用 Android 工具链，三宿主声明）+ roku（三宿主声明）
        // + L3 web 系 TV 四（T-M4-08/09/10，web 运行时三宿主全可达）
        // + L6 AAOS / Wear 二（三宿主声明，复用 Android 链）+ QNX / AUTOSAR（三宿主声明，
        //   无 SDP 时打包按环境槽 SKIP，目标本身仍入选）+ garmin（T-M7-09 三宿主声明，
        //   无 Connect IQ SDK 时编译腿 SKIP-with-print，目标本身仍入选）+ 穿戴二
        //   （fitbit / zepp，三宿主声明，T-M7-07/08）
        BTreeSet::from([
            "android",
            "harmonyos",
            "linux",
            "web",
            "wechat",
            "alipay",
            "douyin",
            "quickapp",
            "android-tv",
            "fire-tv",
            "tizen-tv",
            "webos-tv",
            "vidaa-tv",
            "smartcast-tv",
            "roku",
            "android-automotive",
            "wearos",
            "vxworks",
            "zephyr",
            "freertos",
            "openbsd",
            "freebsd",
            "netbsd",
            "ubuntu",
            "fedora",
            "mint",
            "chromeos",
            "chromeos-pwa",
            "qnx",
            "autosar",
            "garmin",
            "fitbit",
            "zepp",
        ])
    );

    let excluded: BTreeSet<&str> = resolved.excluded.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        excluded,
        BTreeSet::from([
            "ios",
            "macos",
            "tvos",
            "watchos",
            "windows",
            "vendor-console",
        ])
    );

    for target in &resolved.excluded {
        assert!(!target.reason.trim().is_empty(), "{} 缺排除原因", target.id);
        match target.id.as_str() {
            // 宿主不可达：原因须点明宿主
            "ios" | "macos" | "tvos" | "watchos" | "windows" => {
                assert!(
                    target.reason.contains("宿主"),
                    "{}: {}",
                    target.id,
                    target.reason
                )
            }
            // L7：原因须点明厂商授权
            "vendor-console" => {
                assert!(
                    target.reason.contains("L7") && target.reason.contains("授权"),
                    "{}: {}",
                    target.id,
                    target.reason
                )
            }
            other => panic!("意外的排除目标 {other}"),
        }
    }
}

#[test]
fn explicit_unknown_target_returns_diagnostic_not_panic() {
    let registry = registry_with_l7();
    let err = registry
        .resolve_all(
            &TargetRequest::Explicit(vec!["nope".into()]),
            &[Host::Linux],
            false,
        )
        .expect_err("未知目标应报错");
    assert_eq!(err, SpecError::UnknownTarget("nope".into()));

    // trait 接口同样返回诊断
    assert_eq!(
        registry.resolve("nope"),
        Err(SpecError::UnknownTarget("nope".into()))
    );
}

#[test]
fn explicit_unreachable_target_is_error_unless_allowed() {
    let registry = registry_with_l7();
    let hosts = [Host::Linux];

    let err = registry
        .resolve_all(&TargetRequest::Explicit(vec!["ios".into()]), &hosts, false)
        .expect_err("不可达目标应报错");
    match err {
        SpecError::Unreachable(id, reason) => {
            assert_eq!(id, "ios");
            assert!(reason.contains("宿主"), "原因应点明宿主: {reason}");
        }
        other => panic!("期望 Unreachable，实际 {other:?}"),
    }

    // 显式列出 L7 同样被拦下
    let err = registry
        .resolve_all(
            &TargetRequest::Explicit(vec!["vendor-console".into()]),
            &hosts,
            false,
        )
        .expect_err("L7 目标应报错");
    assert!(matches!(err, SpecError::Unreachable(id, _) if id == "vendor-console"));

    // allow_unreachable 强制入选
    let resolved = registry
        .resolve_all(&TargetRequest::Explicit(vec!["ios".into()]), &hosts, true)
        .expect("allow_unreachable 下不应报错");
    let selected: BTreeSet<&str> = resolved.selected.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(selected, BTreeSet::from(["ios"]));
    assert!(resolved.excluded.is_empty());
}

#[test]
fn allow_unreachable_keeps_all_targets() {
    let selected = selected_ids(&registry_with_l7(), &[Host::Linux], true);
    assert_eq!(
        selected.len(),
        39, // L1 七 + L2 四 + L3 电视族八 + L5 八 + L6 十一 + L7 一
        "allow_unreachable 下 All 应全选: {selected:?}"
    );

    // 未开 allow_unreachable 时排除清单非空，两态互为对照
    assert_eq!(
        selected_ids(&registry_with_l7(), &[Host::Linux], false).len(),
        33 // L1 四（Linux 可达）+ L2 四 + L3 电视族七（两 + web 系四 + roku）+ L5 八
           // + L6 十（AAOS + Wear + QNX + AUTOSAR + VxWorks + Zephyr + FreeRTOS + Garmin
           // + Fitbit + Zepp）——tvos / watchos 为 macOS 专宿主；chromeos 双轨均 Linux 可达
    );
}

#[test]
fn id_validation_matrix() {
    for bad in [
        "",
        "Android",
        "my_target",
        "1android",
        "-android",
        "android.tv",
        "安卓",
    ] {
        assert!(!id_is_valid(bad), "{bad:?} 不应通过 id 形态校验");
        let err = TargetSpec::new_stub(bad, 1, &[Host::Linux])
            .validate()
            .expect_err("非法 id 应报错");
        assert_eq!(err, SpecError::InvalidId(bad.to_string()));
    }
    for good in ["android", "tizen-tv", "web2", "a"] {
        assert!(id_is_valid(good), "{good:?} 应为合法 id");
        TargetSpec::new_stub(good, 1, &[Host::Linux])
            .validate()
            .expect("合法 id 应通过校验");
    }
}

#[test]
fn tier_out_of_range_rejected() {
    for tier in [0u8, 8, 255] {
        let err = TargetSpec::new_stub("android", tier, &[Host::Linux])
            .validate()
            .expect_err("越界 tier 应报错");
        assert_eq!(err, SpecError::TierOutOfRange(tier));
    }
    for tier in [1u8, 7] {
        TargetSpec::new_stub("android", tier, &[Host::Linux])
            .validate()
            .expect("1–7 内的 tier 应通过");
    }
}

#[test]
fn duplicate_id_rejected() {
    let specs = vec![
        TargetSpec::new_stub("android", 1, &[Host::Linux]),
        TargetSpec::new_stub("android", 2, &[Host::Win]),
    ];
    assert_eq!(
        Registry::from_specs(specs).expect_err("重复 id 应报错"),
        SpecError::Duplicate("android".into())
    );
}

#[test]
fn from_specs_propagates_validation_errors() {
    let bad_tier = vec![TargetSpec::new_stub("android", 0, &[Host::Linux])];
    assert_eq!(
        Registry::from_specs(bad_tier).expect_err("非法目标应被拒绝"),
        SpecError::TierOutOfRange(0)
    );
}

#[test]
fn validate_rejects_missing_host_and_package() {
    let mut no_host = TargetSpec::new_stub("android", 1, &[]);
    assert_eq!(
        no_host.validate(),
        Err(SpecError::MissingHost("android".into()))
    );

    let mut no_package = TargetSpec::new_stub("android", 1, &[Host::Linux]);
    no_package.package.format = "  ".to_string();
    assert_eq!(
        no_package.validate(),
        Err(SpecError::MissingPackage("android".into()))
    );

    no_host.package.builder = String::new();
    assert_eq!(
        no_host.validate(),
        Err(SpecError::MissingHost("android".into())),
        "宿主缺失优先于打包缺失"
    );
}

#[test]
fn full_spec_toml_roundtrip() {
    for spec in poly_targets::builtin::builtin() {
        let encoded =
            toml::to_string(&spec).unwrap_or_else(|e| panic!("{} 序列化失败: {e}", spec.id));
        let decoded: TargetSpec =
            toml::from_str(&encoded).unwrap_or_else(|e| panic!("{} 反序列化失败: {e}", spec.id));
        assert_eq!(decoded, spec, "{} 全字段往返应一致", spec.id);
    }

    // 可选项缺省：最小 TOML 只给必填字段即可解析
    let spec: TargetSpec =
        toml::from_str(&minimal_toml("native", "linux")).expect("缺省可选项应可解析");
    assert_eq!(spec.signing, None);
    assert!(spec.verify.is_empty() && spec.publish.is_empty() && !spec.experimental);
}

#[test]
fn unknown_enum_values_fail_to_decode() {
    assert!(toml::from_str::<TargetSpec>(&minimal_toml("jit", "linux")).is_err());
    assert!(toml::from_str::<TargetSpec>(&minimal_toml("native", "beos")).is_err());
}

/// 最小合法 TOML（仅必填字段），用于解析侧边界测试。
fn minimal_toml(runtime: &str, host: &str) -> String {
    format!(
        r#"
        id = "stub"
        tier = 1
        hosts = ["{host}"]
        runtime = "{runtime}"
        bridge = "uniffi"
        ui = []
        [package]
        format = "zip"
        builder = "stub"
    "#
    )
}
