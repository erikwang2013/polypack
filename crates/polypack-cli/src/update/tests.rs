//! 清单纯函数面的单元测试（渲染确定性 / 输入校验 / 日期算法）。

use polypack_core::codes;

use super::manifest::*;

#[test]
fn manifest_render_is_byte_identical_and_sorted() {
    let first = render_manifest(
        "1.2.3",
        "fixes",
        "2026-10-20T00:00:00Z",
        "linux-x86_64",
        b"artifact-signature\n",
        "https://updates.example.com/stable/app.AppImage",
    );
    let second = render_manifest(
        "1.2.3",
        "fixes",
        "2026-10-20T00:00:00Z",
        "linux-x86_64",
        b"artifact-signature\n",
        "https://updates.example.com/stable/app.AppImage",
    );
    assert_eq!(first, second, "同输入二跑必须字节一致");
    let notes = first.find("\"notes\"").expect("notes 键");
    let platforms = first.find("\"platforms\"").expect("platforms 键");
    let pub_date = first.find("\"pub_date\"").expect("pub_date 键");
    let version = first.find("\"version\"").expect("version 键");
    assert!(
        notes < platforms && platforms < pub_date && pub_date < version,
        "顶层键必须字典序：{first}"
    );
    assert!(first.ends_with("}\n"), "固定末尾换行");
    let parsed: serde_json::Value = serde_json::from_str(&first).expect("合法 JSON");
    assert_eq!(
        parsed["platforms"]["linux-x86_64"]["signature"],
        "artifact-signature\n"
    );
}

#[test]
fn channel_and_platform_validation_are_stable() {
    assert!(validate_channel("stable").is_ok());
    assert!(validate_channel("beta-2.1").is_ok());
    for bad in ["", "../stable", ".hidden", "Stable", "a/b"] {
        let err = validate_channel(bad).expect_err("非法渠道必须失败");
        assert_eq!(err.code, codes::E_UPDATE_0001);
    }
    assert!(validate_platform("linux", "x86_64").is_ok());
    assert!(validate_platform("linux", "mips").is_err());
    assert!(validate_platform("android", "x86_64").is_err());
}

#[test]
fn rfc3339_and_version_edges() {
    assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
    assert_eq!(rfc3339(1_661_212_800), "2022-08-23T00:00:00Z");
    assert!(valid_rfc3339("2026-10-20T00:00:00Z"));
    assert!(valid_rfc3339("2026-10-20T00:00:00+08:00"));
    assert!(!valid_rfc3339("2026-10-20 00:00:00"));
    assert!(validate_version("1.2.3").is_ok());
    assert!(validate_version("v1.2.3").is_ok());
    assert!(validate_version("1.2.3-beta.1+build").is_ok());
    for bad in ["1.2", "1.2.3/4", "1.2.x", "v1.2.3 extra"] {
        assert!(validate_version(bad).is_err(), "{bad} 应被拒绝");
    }
}

#[test]
fn artifact_url_is_percent_encoded() {
    assert_eq!(
        percent_encode("Pax Notes_1.2.3_x64.dmg"),
        "Pax%20Notes_1.2.3_x64.dmg"
    );
    assert_eq!(percent_encode("app.AppImage"), "app.AppImage");
}
