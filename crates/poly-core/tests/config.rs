//! T-M0-03：`polypack.toml` 解析、JSON Schema 导出与 `to_job` 目标解析。

mod common;

use common::{fixture, spec_with};
use poly_core::{CONFIG_SCHEMA, ConfigError, CredentialSource, PolypackConfig, Profile, codes};
use poly_targets::{Host, Registry, TargetRequest};

/// 测试注册表：android / ios / macos / web 四个 L1 目标 + 一个 L7 目标。
fn registry() -> Registry {
    Registry::from_specs(vec![
        spec_with(
            "android",
            1,
            vec![Host::Win, Host::Linux, Host::Macos],
            Some("android-keystore"),
        ),
        spec_with("ios", 1, vec![Host::Macos], Some("apple-dev")),
        spec_with("macos", 1, vec![Host::Macos], Some("apple-dev")),
        spec_with("web", 1, vec![Host::Win, Host::Linux, Host::Macos], None),
        spec_with("tizen-tv", 7, vec![Host::Linux], None),
    ])
    .expect("测试目标必须通过校验")
}

fn full_config() -> PolypackConfig {
    PolypackConfig::parse(&fixture("polypack.full.toml")).expect("full 夹具必须可解析")
}

#[test]
fn min_fixture_parses_with_defaults() {
    let config = PolypackConfig::parse(&fixture("polypack.min.toml")).expect("min 夹具必须可解析");
    assert_eq!(config.schema, 1);
    assert_eq!(config.package.name, "my-app");
    assert_eq!(config.ui.entry, "ui/src/main.tsx");
    assert_eq!(config.core.crate_path, "crates/app-core");
    assert!(config.targets.is_empty(), "min 夹具不应声明目标参数");
    assert_eq!(config.publish, None);
    assert_eq!(config.retention.cache_days, 30);
    assert_eq!(config.retention.cache_gib, 20);
    assert_eq!(config.retention.artifact_builds, 10);
}

#[test]
fn full_fixture_parses_all_sections() {
    let config = full_config();
    let ids: Vec<&str> = config.targets.keys().map(String::as_str).collect();
    assert_eq!(ids, ["android", "ios", "macos", "web"]);
    assert_eq!(
        config.targets["android"]["min-sdk"].as_integer(),
        Some(24),
        "[targets.*] 内的任意键必须原样保留"
    );
    assert_eq!(
        config.targets["ios"]["team"].as_str(),
        Some("ABCDE12345"),
        "kebab-case 目标参数必须可读"
    );
    assert_eq!(
        config.publish.as_ref().map(|p| p.stores.clone()),
        Some(vec![
            "play".to_string(),
            "appstore".to_string(),
            "appgallery".to_string()
        ])
    );
    assert_eq!(config.retention.cache_days, 14);
    assert_eq!(config.retention.cache_gib, 8);
    assert_eq!(config.retention.artifact_builds, 5);
}

#[test]
fn fixtures_round_trip_through_toml() {
    for name in ["polypack.min.toml", "polypack.full.toml"] {
        let config = PolypackConfig::parse(&fixture(name)).expect("夹具必须可解析");
        let text = toml::to_string(&config).expect("配置必须可序列化");
        let reparsed = PolypackConfig::parse(&text)
            .unwrap_or_else(|err| panic!("{name} 序列化往返失败: {err}"));
        assert_eq!(config, reparsed, "{name} 序列化往返必须一致");
    }
}

#[test]
fn schema_defaults_to_current_version_when_omitted() {
    let text = fixture("polypack.min.toml").replace("schema = 1\n", "");
    let config = PolypackConfig::parse(&text).expect("schema 缺省必须被接受（§4.6：缺省视为 1）");
    assert_eq!(config.schema, CONFIG_SCHEMA);
}

#[test]
fn unsupported_schema_versions_are_rejected() {
    for version in [0, 2] {
        let text =
            fixture("polypack.min.toml").replace("schema = 1", &format!("schema = {version}"));
        let err = PolypackConfig::parse(&text).expect_err("非法 schema 版本必须被拒绝");
        assert!(
            matches!(err, ConfigError::UnsupportedSchema(v) if v == version),
            "实得 {err:?}"
        );
        assert_eq!(err.diagnostic().code, codes::E_CFG_0001);
    }
}

#[test]
fn missing_required_field_is_reported_with_stable_code() {
    let text = fixture("polypack.min.toml").replace(
        "[ui]\nframework = \"react\"\nentry = \"ui/src/main.tsx\"\n",
        "",
    );
    let err = PolypackConfig::parse(&text).expect_err("缺少 [ui] 必须报错");
    assert!(matches!(err, ConfigError::Toml(_)), "实得 {err:?}");
    let diagnostic = err.diagnostic();
    assert_eq!(diagnostic.code, codes::E_CFG_0002);
    assert!(
        diagnostic.message.contains("ui"),
        "报错应指出缺失字段，实得 {}",
        diagnostic.message
    );
}

#[test]
fn unknown_key_is_rejected_with_stable_code() {
    let text = format!("{}\nunknown_key = 1\n", fixture("polypack.min.toml"));
    let err = PolypackConfig::parse(&text).expect_err("未知顶层键必须报错");
    assert!(matches!(err, ConfigError::Toml(_)), "实得 {err:?}");
    assert_eq!(err.diagnostic().code, codes::E_CFG_0002);
}

#[test]
fn json_schema_covers_readme_keys() {
    let schema = PolypackConfig::json_schema();
    assert_eq!(schema["$schema"], "http://json-schema.org/draft-07/schema#");

    let properties = schema["properties"]
        .as_object()
        .expect("schema 必须有 properties");
    for key in [
        "schema",
        "package",
        "ui",
        "core",
        "targets",
        "publish",
        "retention",
    ] {
        assert!(properties.contains_key(key), "schema 缺少属性 {key}");
    }
    let mut required: Vec<&str> = schema["required"]
        .as_array()
        .expect("schema 必须有 required")
        .iter()
        .filter_map(|value| value.as_str())
        .collect();
    required.sort_unstable();
    assert_eq!(
        required,
        ["core", "package", "ui"],
        "仅 TOML 必填段进入 required（其余有默认值或可选）"
    );

    // README 示例配置全部键（段结构以 $ref 指向 definitions）。
    let sections = schema["definitions"]
        .as_object()
        .expect("schema 必须有 definitions");
    assert_eq!(
        sections["PackageSection"]["properties"]["name"]["type"],
        "string"
    );
    assert_eq!(
        sections["PackageSection"]["properties"]["version"]["type"],
        "string"
    );
    assert_eq!(
        sections["UiSection"]["properties"]["framework"]["type"],
        "string"
    );
    assert_eq!(
        sections["UiSection"]["properties"]["entry"]["type"],
        "string"
    );
    assert_eq!(
        sections["CoreSection"]["properties"]["crate"]["type"], "string",
        "README 用 `crate` 键，schema 必须同名（而非 Rust 字段名 crate_path）"
    );
    assert_eq!(
        sections["PublishSection"]["properties"]["stores"]["type"],
        "array"
    );
    assert_eq!(
        sections["RetentionSection"]["properties"]["cache_days"]["type"],
        "integer"
    );
    // [targets.*] 是 toml::Value，导出为宽松对象 + 说明描述。
    let targets = &schema["properties"]["targets"];
    assert_eq!(targets["type"], "object");
    assert!(
        targets["description"]
            .as_str()
            .is_some_and(|d| !d.is_empty()),
        "targets 段必须有说明性描述"
    );
    // serde deny_unknown_fields 同步进 schema：未知键被 schema 拒绝。
    assert_eq!(schema["additionalProperties"], serde_json::json!(false));
}

/// README「`polypack.toml` 示意」逐键可解析（含 `crate` 与 kebab-case 目标参数）。
#[test]
fn readme_example_config_parses() {
    let text = r#"
schema = 1

[package]
name = "my-app"
version = "1.2.0"

[ui]
framework = "react"
entry = "ui/src/main.tsx"

[core]
crate = "crates/app-core"    # Rust 业务核心，编译为各端库

[targets.android]
min-sdk = 24
signing = "keystore"          # 引用密钥库条目，密钥不入库

[targets.ios]
bundle-id = "dev.example.polypack"
team = "ABCDE12345"

[targets.wechat]
appid = "wx0000000000000000"

[publish]
stores = ["play", "appstore", "appgallery"]
"#;
    let config = PolypackConfig::parse(text).expect("README 示例配置必须可解析");
    assert_eq!(config.core.crate_path, "crates/app-core");
    assert_eq!(config.targets.len(), 3);
}

#[test]
fn to_job_selects_targets_and_maps_secrets() {
    let config = full_config();
    let request = TargetRequest::Explicit(
        ["android", "ios", "macos", "web"]
            .iter()
            .map(|id| id.to_string())
            .collect(),
    );
    let hosts = [Host::Win, Host::Linux, Host::Macos];

    let job = config
        .to_job(&registry(), &request, Profile::Local, &hosts)
        .expect("显式目标在可达宿主上必须解析成功");

    assert_eq!(job.id, "my-app-1.2.0-local", "M0 job id 为确定性占位");
    assert_eq!(job.profile, Profile::Local);
    assert_eq!(
        job.targets
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["android", "ios", "macos", "web"]
    );
    assert!(job.excluded.is_empty(), "显式目标不产生排除清单");
    let secrets: Vec<(&str, CredentialSource)> = job
        .secrets
        .iter()
        .map(|s| (s.name.as_str(), s.source.clone()))
        .collect();
    assert_eq!(
        secrets,
        [
            ("android-keystore", CredentialSource::Keyring),
            ("apple-dev", CredentialSource::Keyring),
        ],
        "签名档案进 secrets 并按档案名去重（ios / macos 共用 apple-dev）"
    );
}

#[test]
fn to_job_all_excludes_l7_and_unreachable_hosts() {
    let config = full_config();
    let hosts = [Host::Linux];

    let job = config
        .to_job(&registry(), &TargetRequest::All, Profile::Ci, &hosts)
        .expect("--target all 永不因排除项失败");

    assert_eq!(
        job.targets
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        ["android", "web"],
        "仅宿主可达的 L1 目标入选"
    );
    assert_eq!(
        job.excluded
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["ios", "macos", "tizen-tv"]
    );
    assert!(
        job.excluded.iter().all(|e| !e.reason.is_empty()),
        "每个排除项必须带原因"
    );
    assert_eq!(job.id, "my-app-1.2.0-ci");
}

#[test]
fn to_job_unknown_explicit_target_fails_with_e_tgt_0001() {
    let err = full_config()
        .to_job(
            &registry(),
            &TargetRequest::Explicit(vec!["ghost".to_string()]),
            Profile::Local,
            &[Host::Linux],
        )
        .expect_err("未注册的显式目标必须失败");
    assert_eq!(err.diagnostic.code, codes::E_TGT_0001);
    assert!(err.diagnostic.hint.is_some(), "错误必须带修复提示");
}

#[test]
fn to_job_unknown_config_target_fails_with_e_tgt_0001() {
    let text = format!(
        "{}\n[targets.ghost]\nfoo = 1\n",
        fixture("polypack.min.toml")
    );
    let config = PolypackConfig::parse(&text).expect("夹具本体合法");
    let err = config
        .to_job(
            &registry(),
            &TargetRequest::Explicit(vec!["web".to_string()]),
            Profile::Local,
            &[Host::Linux],
        )
        .expect_err("配置里未注册的目标必须失败");
    assert_eq!(err.diagnostic.code, codes::E_TGT_0001);
}

#[test]
fn to_job_unreachable_explicit_target_fails_with_e_tgt_0002() {
    let err = full_config()
        .to_job(
            &registry(),
            &TargetRequest::Explicit(vec!["ios".to_string()]),
            Profile::Local,
            &[Host::Linux],
        )
        .expect_err("显式目标不可达必须失败");
    assert_eq!(err.diagnostic.code, codes::E_TGT_0002);
    assert!(err.diagnostic.hint.is_some(), "错误必须带修复提示");
}
