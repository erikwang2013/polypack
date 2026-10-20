//! `poly publish` 单元测试：参数映射 / 输入解析 / 载具 pre-flight / 失败事件。

use std::path::PathBuf;
use std::sync::Arc;

use base64::Engine;
use poly_core::CredentialSource;
use poly_publish::metadata::DEFAULT_LOCALE;
use poly_publish::secret::CredentialProvider;
use poly_publish::{MemoryProvider, Package, PublishError, PublishFailure, Stage};
use poly_targets::{Registry, TargetSpec};

use super::prepare::{locate_rpk, payload_fields, prepare};
use super::{
    Args, UNION_STORE_CHANNEL, failure_diagnostic, failure_event, find_publisher, publish_error,
};

/// 最小工程骨架：`polypack.toml` + 空的产物落点。
fn fixture(tag: &str) -> (PathBuf, poly_core::Project) {
    let root = std::env::temp_dir().join(format!("polypack-publish-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("工程目录可建");
    // 根形态归一：`project::load` 内部 canonicalize（mac 的 /var→/private/var；win 的 8.3
    // 短名→`\\?\` 长名）——测试从这里派生的一切路径须同一形态，否则交叉断言在 mac/win
    // 上错位（CI 实证 2026-10-17）。
    let root = root.canonicalize().expect("工程根可规范化");
    std::fs::write(
        root.join("polypack.toml"),
        "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("写配置");
    let project = crate::project::load(&root.join("polypack.toml")).expect("配置可加载");
    (root, project)
}

/// 种一个 `.rpk` 到目标产物落点（内容随意——本层只看定位与字节数）。
fn seed_rpk(root: &std::path::Path, target: &str, name: &str) -> PathBuf {
    // 逐段 join（不夹带 `/` 的多段字符串）：Windows 上分隔符形态与定位器一致。
    let dist = root
        .join(".polypack")
        .join("packages")
        .join(target)
        .join("miniapp")
        .join("dist");
    std::fs::create_dir_all(&dist).expect("dist 可建");
    let path = dist.join(name);
    std::fs::write(&path, b"PK\x03\x04stub-rpk").expect("写 .rpk");
    path
}

fn spec(target: &str) -> TargetSpec {
    Registry::builtin()
        .get(target)
        .cloned()
        .unwrap_or_else(|| panic!("注册表缺少 {target}"))
}

fn args(target: &str) -> Args {
    Args {
        config: PathBuf::from("polypack.toml"),
        target: target.to_string(),
        channel: None,
        appid: Some("com.polypack.fixture".to_string()),
        version: Some("0.1.0".to_string()),
        description: "契约测试".to_string(),
        package: None,
        // 条目名经 `keyring:条目名` 覆盖档案名（env 的变量名不覆盖）；后端用 memory 注入值
        credential: Some("keyring:union".to_string()),
        materials: None,
        metadata_dir: None,
        metadata_locale: DEFAULT_LOCALE.to_string(),
        check_metadata: false,
        api_base: None,
        dry_run: true,
    }
}

/// 内存后端一条载荷（值 = base64({"token":"s3cret-value"})；测试断言它绝不落屏）。
fn memory() -> Vec<Box<dyn CredentialProvider>> {
    vec![Box::new(MemoryProvider::with_entry(
        "union",
        base64::engine::general_purpose::STANDARD.encode(r#"{"token":"s3cret-value"}"#),
    ))]
}

#[test]
fn rpk_locator_picks_by_name_order_and_reports_bytes() {
    let (root, project) = fixture("locate");
    seed_rpk(&root, "quickapp", "a.debug.0.1.0.rpk");
    let last = seed_rpk(&root, "quickapp", "com.polypack.fixture.debug.0.1.0.rpk");
    let (path, bytes) = locate_rpk(&project, &spec("quickapp")).expect("定位须成功");
    assert_eq!(path, last, "名序取尾（口径同 verify 的快应用腿）");
    assert_eq!(
        bytes,
        Some(12),
        "文件载体给字节数（PK\\x03\\x04 + stub-rpk）"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rpk_locator_without_artifact_is_input_diagnostic() {
    let (root, project) = fixture("locate-empty");
    std::fs::create_dir_all(root.join(".polypack/packages/quickapp/miniapp/dist"))
        .expect("空 dist");
    let err = locate_rpk(&project, &spec("quickapp")).expect_err("无 .rpk 必须报输入未就绪");
    assert_eq!(err.code.to_string(), "poly::E_PUBLISH_0001");
    assert!(err.message.contains("没有 .rpk"), "{}", err.message);
    assert!(
        err.hint.unwrap_or_default().contains("poly build"),
        "提示须给动作"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn union_store_requires_explicit_endpoint() {
    let (root, project) = fixture("endpoint");
    seed_rpk(&root, "quickapp", "x.debug.0.1.0.rpk");
    let err = prepare(&args("quickapp"), &project, &spec("quickapp"), &memory())
        .expect_err("联盟商店缺 --api-base 必须报错");
    assert_eq!(err.code.to_string(), "poly::E_PUBLISH_0001");
    assert!(err.message.contains("--api-base"), "{}", err.message);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn prepare_reports_credential_shape_never_values() {
    let (root, project) = fixture("prepare");
    seed_rpk(&root, "quickapp", "x.debug.0.1.0.rpk");
    let mut a = args("quickapp");
    a.api_base = Some("https://example.invalid/union".to_string());
    let prepared = prepare(&a, &project, &spec("quickapp"), &memory()).expect("解析须成功");
    assert_eq!(prepared.channel, "quickapp-union-store", "渠道来自目标声明");
    assert_eq!(prepared.provider, "memory");
    assert_eq!(prepared.payload_fields, vec!["token".to_string()]);
    assert!(
        !format!("{prepared:?}").contains("s3cret"),
        "凭据值绝不进 Debug"
    );
    assert_eq!(
        prepared.api_base.as_deref(),
        Some("https://example.invalid/union")
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 自动激活接缝：渠道备位 / 客户端缺席 → 缺口诊断；落位后同一断言走下一阶段（返回客户端）。
#[test]
fn union_store_gap_is_named_until_the_client_lands() {
    struct PanicTransport;
    impl poly_publish::Transport for PanicTransport {
        fn send(
            &self,
            _request: &poly_publish::HttpRequest,
        ) -> Result<poly_publish::HttpResponse, PublishError> {
            panic!("查找客户端不得触网");
        }
    }
    match find_publisher(UNION_STORE_CHANNEL, Arc::new(PanicTransport)) {
        Err(diagnostic) => {
            // 现态（main@cd1fcdb 后）：备位已反解，缺口在 builtin() 内客户端——
            // 如实点名归属与阶段，绝不假绿
            assert_eq!(diagnostic.code.to_string(), "poly::E_PUBLISH_0002");
            assert!(
                diagnostic.message.contains("quickapp-union-store"),
                "{}",
                diagnostic.message
            );
            assert!(
                diagnostic.message.contains("builtin()"),
                "备位已入主干（from_id 已反解），缺口应精确到客户端：{}",
                diagnostic.message
            );
            let hint = diagnostic.hint.unwrap_or_default();
            assert!(hint.contains("publish-client"), "缺口须点名归属：{hint}");
            assert!(hint.contains("T-M3-10"), "归属到任务：{hint}");
        }
        Ok(publisher) => {
            // 客户端进 builtin() 当日（自动激活）：下一阶段是端点注入面接线
            assert_eq!(publisher.channel().id(), UNION_STORE_CHANNEL);
        }
    }
}

/// rpk 腿接线（main@cd1fcdb 单文件载具）：pre-flight 摘要 / 回执键与四步驱动同源。
///
/// pre-flight 走 `Package::load`（驱动同一入口）——断言逐字符相等 = 驱动日后重算的
/// 回执键必与本层预演的相同（幂等重跑锚点），零分叉。
#[test]
fn rpk_leg_preflight_digest_and_receipt_key_match_the_driver_entry() {
    let (root, project) = fixture("rpk-leg");
    let seeded = seed_rpk(&root, "quickapp", "com.polypack.fixture.debug.0.1.0.rpk");
    let mut a = args("quickapp");
    a.api_base = Some("https://example.invalid/union".to_string());
    let prepared = prepare(&a, &project, &spec("quickapp"), &memory()).expect("解析须成功");
    assert_eq!(prepared.carrier, "single-file", "rpk 判为单文件载具");
    let driven = Package::load(&seeded).expect("驱动入口可读同一 rpk");
    assert_eq!(
        prepared.digest,
        driven.digest(),
        "pre-flight 摘要 = 驱动同源摘要（Package::load 同入口同口径）"
    );
    assert_eq!(
        prepared.receipt_key,
        poly_publish::receipt_key(
            a.appid.as_deref().expect("fixture appid"),
            a.version.as_deref().expect("fixture version"),
            driven.digest(),
        ),
        "回执键 = appid-版本-摘要前 16（驱动落回执同键）"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 载具 pre-flight 的拒绝面：空 .rpk（构建未完成）与目录载具一样要如实判别。
#[test]
fn carrier_preflight_rejects_empty_rpk_and_accepts_dir() {
    let (root, project) = fixture("carrier");
    let empty = seed_rpk(&root, "quickapp", "empty.debug.0.1.0.rpk");
    std::fs::write(&empty, b"").expect("空文件");
    let mut a = args("quickapp");
    a.api_base = Some("https://example.invalid/union".to_string());
    let err =
        prepare(&a, &project, &spec("quickapp"), &memory()).expect_err("空 .rpk 必须报输入未就绪");
    assert_eq!(err.code.to_string(), "poly::E_PUBLISH_0001");
    assert!(
        err.hint.unwrap_or_default().contains("poly build"),
        "提示须给重建动作"
    );

    // 目录载具（T-M3-06 产物形态）同样过 pre-flight：显式 --package 指向产物目录
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).expect("建目录");
    std::fs::write(dist.join("app.json"), r#"{"pages":["pages/index/index"]}"#).expect("写");
    std::fs::write(dist.join("app.js"), "App({})").expect("写");
    let mut a = args("quickapp");
    a.api_base = Some("https://example.invalid/union".to_string());
    a.package = Some(dist);
    let prepared = prepare(&a, &project, &spec("quickapp"), &memory()).expect("目录载具须过");
    assert_eq!(prepared.carrier, "dir");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dry_run_end_to_end_stays_offline_and_writes_no_receipt() {
    let (root, project) = fixture("dry-run");
    seed_rpk(&root, "quickapp", "x.debug.0.1.0.rpk");
    let mut a = args("quickapp");
    a.api_base = Some("https://example.invalid/union".to_string());
    let prepared = prepare(&a, &project, &spec("quickapp"), &memory()).expect("解析须成功");
    assert!(prepared.package.ends_with("x.debug.0.1.0.rpk"));
    assert!(
        !root.join(".polypack/publish").exists(),
        "dry-run 不得落回执目录"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn credential_payload_must_be_base64_json_object() {
    let fields = payload_fields(b"eyJhIjoxfQ==").expect("合法载荷"); // {"a":1}
    assert_eq!(fields, vec!["a".to_string()]);
    let err = payload_fields(b"not-base64!").expect_err("非 base64 必须报错");
    assert_eq!(err.code.to_string(), "poly::E_PUBLISH_0001");
    let err = payload_fields(b"bm90LWpzb24=").expect_err("非 JSON 必须报错"); // not-json
    assert_eq!(err.code.to_string(), "poly::E_PUBLISH_0001");
    let err = payload_fields(b"IjQyIg==").expect_err("非对象必须报错"); // "42"
    assert!(err.message.contains("顶层应为对象"), "{}", err.message);
}

fn failed(stage: Stage) -> PublishFailure {
    PublishFailure {
        stage,
        error: PublishError::Platform {
            channel: "wechat-open-platform",
            code: "85008".to_string(),
            message: "类目材料不合法".to_string(),
            hint: Some("核对 item_list".to_string()),
        },
    }
}

/// T-M8-10 验收：不可恢复失败 → 机器可读 `Failed{stage, code, hint}`。
#[test]
fn failure_event_is_machine_readable_failed_shape() {
    let event = failure_event(&failed(Stage::Submit));
    assert_eq!(event["event"], "failed");
    assert_eq!(event["stage"], "submit");
    assert_eq!(event["code"], "poly::E_PUB_0003");
    assert_eq!(event["hint"], "核对 item_list");
    // 码 / 提示原样透传，不重新编号。
    assert_eq!(
        event.as_object().expect("对象").len(),
        4,
        "只含 event/stage/code/hint"
    );
}

#[test]
fn failure_diagnostic_carries_failed_shape_and_hint() {
    let diagnostic = failure_diagnostic(&failed(Stage::Submit));
    assert_eq!(diagnostic.code.to_string(), "poly::E_PUB_0003");
    assert!(
        diagnostic
            .message
            .contains("Failed{stage=submit, code=poly::E_PUB_0003, hint=核对 item_list}"),
        "{}",
        diagnostic.message
    );
    assert_eq!(diagnostic.hint.as_deref(), Some("核对 item_list"));
}

#[test]
fn foreign_error_codes_pass_through_unrenumbered() {
    let err = PublishError::CredentialUnavailable {
        name: "n".to_string(),
        providers: "env".to_string(),
        reason: "unset".to_string(),
    };
    let diagnostic = publish_error(err);
    assert_eq!(
        diagnostic.code.to_string(),
        "poly::E_PUB_0001",
        "码原样透传"
    );
    assert!(diagnostic.hint.is_some(), "提示取 hint()");
}

#[test]
fn credential_source_is_the_referenced_env_variable() {
    // 引用翻译：env:VAR 的变量名进 CredentialSource（值仍由后端链现取）
    let (source, alias) = crate::retry::parse_credential("env:POLY_ANY").expect("引用合法");
    assert!(matches!(source, CredentialSource::Env { .. }));
    assert!(alias.is_none());
}
