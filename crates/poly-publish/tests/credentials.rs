//! 凭据引用模型（T-M2-11 语义的发布侧同形实现）：后端链解析、载荷契约、脱敏边界。
//!
//! 依赖方向约束（poly-cli/tests/dependency_direction.rs）不允许复用 poly-sign 的后端链，
//! 故这里有独立实现——本文件即它的验收：**任何凭据问题都在触网前报错**，
//! 且错误 / `Debug` 输出里绝不出现凭据值。
//!
//! env 注入腿（CI 场景）：Rust 2024 中进程内 `set_var` 是 unsafe（本仓库 forbid unsafe），
//! 按「子进程重跑本测试」注入，口径同 poly-sign/tests/secret.rs。

mod common;

use common::{FakeTransport, TempRoot};
use poly_core::{CredentialRef, CredentialSource};
use poly_publish::secret::wechat_credentials;
use poly_publish::{
    Archive, CredentialProvider, EnvProvider, MemoryProvider, PollPolicy, PublishError,
    PublishRequest, WechatPublisher, codes, publish,
};

/// 形态样本（非真实账号）。
const APPID: &str = "wx1234567890abcdef";
/// env 注入腿的变量名。
const ENV_VARIABLE: &str = "POLYPACK_PUBLISH_TEST_PAYLOAD";
/// 子进程标记（区分父子两条腿）。
const CHILD: &str = "POLYPACK_PUBLISH_ENV_CHILD";

/// 最简发布请求（凭据问题应在任何 HTTP 之前暴露）。
fn request(root: &TempRoot, credential: CredentialRef) -> PublishRequest {
    PublishRequest::new(APPID, "1.0.0", "凭据用例", root.dist(), credential).with_poll(PollPolicy {
        interval: std::time::Duration::ZERO,
        timeout: std::time::Duration::from_secs(1),
    })
}

#[test]
fn empty_provider_chain_reports_unavailable_with_pointer() {
    let root = TempRoot::new("cred-empty");
    let fake = FakeTransport::new(Vec::new());
    let reference = CredentialRef {
        name: "wechat-mini".to_string(),
        source: CredentialSource::Keyring,
    };
    let err = publish(
        &WechatPublisher::new(fake.clone()),
        &request(&root, reference),
        &[],
        &Archive::new(root.path()),
    )
    .expect_err("空后端链须拒");
    assert_eq!(err.code(), codes::E_PUB_0001);
    assert!(
        err.to_string().contains("wechat-mini"),
        "点名凭据引用：{err}"
    );
    let hint = err.hint().expect("带修复提示");
    assert!(
        hint.contains("Env") || hint.contains("内存") || hint.contains("keyring"),
        "提示指向注入方式：{hint}"
    );
    assert_eq!(fake.count(), 0, "凭据问题零触网");
}

#[test]
fn payload_must_be_base64_json_with_required_fields() {
    let root = TempRoot::new("cred-shape");
    let cases: Vec<(String, &str)> = vec![
        ("不是 base64 的载荷".to_string(), "base64"),
        (
            common::payload(&serde_json::json!(["不是对象"])),
            "JSON 结构",
        ),
        (
            common::payload(&serde_json::json!({
                "appid": APPID,
                "secret": common::hex32(),
            })),
            "private_key",
        ),
    ];
    for (payload, needle) in cases {
        let fake = FakeTransport::new(Vec::new());
        let reference = CredentialRef {
            name: "wechat-mini".to_string(),
            source: CredentialSource::Keyring,
        };
        let providers: Vec<Box<dyn CredentialProvider>> =
            vec![Box::new(MemoryProvider::with_entry("wechat-mini", payload))];
        let err = publish(
            &WechatPublisher::new(fake.clone()),
            &request(&root, reference),
            &providers,
            &Archive::new(root.path()),
        )
        .expect_err("载荷非法须拒");
        assert_eq!(err.code(), codes::E_PUB_0001, "{err}");
        assert!(
            err.to_string().contains(needle),
            "命中原因应含 {needle}：{err}"
        );
        assert_eq!(fake.count(), 0, "凭据问题零触网");
    }
}

#[test]
fn transport_error_with_credential_in_url_is_redacted() {
    let root = TempRoot::new("cred-redact");
    // 脚本只喂随机串 + 上传受理；取 token 那步（URL 里带 secret）脚本用尽 →
    // FakeTransport 的报错文本带完整 URL——正是「传输层错误可能带出凭据」的形态。
    let fake = FakeTransport::new(vec![
        common::json_ok(serde_json::json!({"errCode": 0, "data": {"randomString": "r1"}})),
        common::json_ok(serde_json::json!({"errCode": 0})),
    ]);
    let secret = common::hex32();
    let (reference, providers) =
        common::credential("wechat-mini", common::wechat_payload(APPID, &secret));
    let err = publish(
        &WechatPublisher::new(fake),
        &request(&root, reference),
        &providers,
        &Archive::new(root.path()),
    )
    .expect_err("取 token 失败");
    assert_eq!(err.code(), codes::E_PUB_0002);
    let text = err.to_string();
    assert!(!text.contains(&secret), "错误里不得出现 secret：{text}");
    assert!(!text.contains("private_key"), "{text}");
    assert!(text.contains("<脱敏>"), "凭据位点应被抹除：{text}");
}

#[test]
fn provider_chain_falls_through_to_memory_and_records_source() {
    let secret = common::hex32();
    let reference = CredentialRef {
        name: "wechat-mini".to_string(),
        // 引用声明走 env，但变量未设 → 链条落到内存后端（CI 注入形态）。
        source: CredentialSource::Env {
            variable: "POLYPACK_PUBLISH_DEFINITELY_MISSING".to_string(),
        },
    };
    let providers: Vec<Box<dyn CredentialProvider>> = vec![
        Box::new(EnvProvider),
        Box::new(MemoryProvider::with_entry(
            "wechat-mini",
            common::payload(&common::wechat_payload(APPID, &secret)),
        )),
    ];
    let creds = wechat_credentials(&reference, &providers).expect("链条落到内存后端");
    assert_eq!(creds.appid, APPID);
    assert_eq!(creds.secret, secret);
    assert_eq!(creds.source, "memory", "如实记录命中的后端");

    // Debug 手写脱敏：任何格式化的诊断都不带凭据值。
    let debug = format!("{creds:?}");
    assert!(debug.contains("<脱敏>"), "{debug}");
    assert!(!debug.contains(&secret), "{debug}");
    let key_line = common::test_private_key_pem()
        .lines()
        .nth(1)
        .expect("PEM 有正文行");
    assert!(!debug.contains(key_line), "私钥材料不得进 Debug：{debug}");
}

#[test]
fn alipay_payload_parses_with_optional_app_auth_token() {
    use poly_publish::secret::alipay_credentials;

    let key_line = common::test_private_key_pem()
        .lines()
        .nth(1)
        .expect("PEM 有正文行")
        .to_string();
    for (payload, expected_token) in [
        (
            serde_json::json!({"app_id": "2021000000000000", "private_key": common::test_private_key_pem()}),
            None,
        ),
        (
            serde_json::json!({
                "app_id": "2021000000000000",
                "private_key": common::test_private_key_pem(),
                "app_auth_token": "app-auth-token-123456",
            }),
            Some("app-auth-token-123456"),
        ),
    ] {
        let reference = CredentialRef {
            name: "alipay-mini".to_string(),
            source: CredentialSource::Keyring,
        };
        let providers: Vec<Box<dyn CredentialProvider>> = vec![Box::new(
            MemoryProvider::with_entry("alipay-mini", common::payload(&payload)),
        )];
        let creds = alipay_credentials(&reference, &providers).expect("载荷合法");
        assert_eq!(creds.app_id, "2021000000000000");
        assert_eq!(creds.app_auth_token.as_deref(), expected_token);
        assert_eq!(creds.source, "memory");

        let debug = format!("{creds:?}");
        assert!(debug.contains("<脱敏>"), "{debug}");
        assert!(!debug.contains(&key_line), "私钥材料不得进 Debug：{debug}");
        if let Some(token) = expected_token {
            assert!(!debug.contains(token), "授权 token 不得进 Debug：{debug}");
        }
    }

    // 缺 private_key（必填）→ 指名缺字段，不触网不可能发生在这里，但错误文本要可诊断。
    let reference = CredentialRef {
        name: "alipay-mini".to_string(),
        source: CredentialSource::Keyring,
    };
    let providers: Vec<Box<dyn CredentialProvider>> = vec![Box::new(MemoryProvider::with_entry(
        "alipay-mini",
        common::payload(&serde_json::json!({"app_id": "2021000000000000"})),
    ))];
    let err = alipay_credentials(&reference, &providers).expect_err("缺必填须拒");
    assert_eq!(err.code(), codes::E_PUB_0001);
    assert!(err.to_string().contains("private_key"), "点名缺字段：{err}");
}

#[test]
fn douyin_payload_parses_and_redacts_debug() {
    use poly_publish::secret::douyin_credentials;

    // 抖音凭据 = access_token 外部注入（铸造链路未确证 → 客户端不做；never fake）。
    let token = common::hex32();
    let reference = CredentialRef {
        name: "douyin-mini".to_string(),
        source: CredentialSource::Keyring,
    };
    let providers: Vec<Box<dyn CredentialProvider>> = vec![Box::new(MemoryProvider::with_entry(
        "douyin-mini",
        common::payload(&serde_json::json!({"access_token": token})),
    ))];
    let creds = douyin_credentials(&reference, &providers).expect("载荷合法");
    assert_eq!(creds.access_token, token);
    assert_eq!(creds.source, "memory");

    let debug = format!("{creds:?}");
    assert!(debug.contains("<脱敏>"), "{debug}");
    assert!(
        !debug.contains(&token),
        "access_token 不得进 Debug：{debug}"
    );
    assert!(
        creds
            .redactor()
            .redact(&format!("x={token}"))
            .contains("<脱敏>")
    );

    // 缺 access_token（必填）→ 指名缺字段。
    let providers: Vec<Box<dyn CredentialProvider>> = vec![Box::new(MemoryProvider::with_entry(
        "douyin-mini",
        common::payload(&serde_json::json!({"appid": "tt1234567890abcdef"})),
    ))];
    let err = douyin_credentials(&reference, &providers).expect_err("缺必填须拒");
    assert_eq!(err.code(), codes::E_PUB_0001);
    assert!(
        err.to_string().contains("access_token"),
        "点名缺字段：{err}"
    );
}

#[test]
fn env_source_resolves_injected_payload() {
    // 载荷的 private_key 用占位文本：env 腿只验解析链，不碰签名（避免父进程把私钥塞进环境）。
    let placeholder = "env-test-placeholder";
    let expected = serde_json::json!({
        "appid": APPID,
        "secret": common::hex32(),
        "private_key": placeholder,
    });

    if std::env::var_os(CHILD).is_some() {
        let reference = CredentialRef {
            name: "wechat-mini".to_string(),
            source: CredentialSource::Env {
                variable: ENV_VARIABLE.to_string(),
            },
        };
        let creds = wechat_credentials(&reference, &[Box::new(EnvProvider)])
            .expect("子进程注入的变量应解析成功");
        assert_eq!(creds.appid, APPID);
        assert_eq!(creds.secret, common::hex32(), "变量值原样（base64(JSON)）");
        assert_eq!(creds.private_key, placeholder);
        assert_eq!(creds.source, "env");
        return;
    }

    let exe = std::env::current_exe().expect("测试可执行路径");
    let status = std::process::Command::new(exe)
        .args([
            "env_source_resolves_injected_payload",
            "--exact",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env(ENV_VARIABLE, common::payload(&expected))
        .status()
        .expect("子进程可执行");
    assert!(status.success(), "子进程断言失败（见上方输出）");
}

#[test]
fn env_source_missing_variable_names_it() {
    let reference = CredentialRef {
        name: "wechat-mini".to_string(),
        source: CredentialSource::Env {
            variable: "POLYPACK_PUBLISH_DEFINITELY_MISSING".to_string(),
        },
    };
    let err = wechat_credentials(&reference, &[Box::new(EnvProvider)]).expect_err("变量缺失须拒");
    assert_eq!(err.code(), codes::E_PUB_0001);
    assert!(
        err.to_string()
            .contains("POLYPACK_PUBLISH_DEFINITELY_MISSING"),
        "错误须点名环境变量：{err}"
    );
    assert!(matches!(err, PublishError::CredentialUnavailable { .. }));
}
