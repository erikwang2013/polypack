//! T-M3-10 验收（mock HTTP 契约）：抖音 publish 客户端 测试版本创建 → 提审 → 轮询 + 回执归档。
//!
//! 口径锚点：官方文档 SSR 原文（upload / audit v2 / versions）+ tt-ide-cli 0.1.33 交叉——本文件
//! 断言信封（HTTP 200 / `err_no`）、`access-token` 头、body 逐字（`template_id` 走 Int64）、
//! 提审前置 `get_audit_hosts`、`data.audit.status` 枚举；幂等码 11303 两处（上传 / 提审）
//! 按「已在队列」续走。**凭据 = access_token 外部注入**（铸造链路未确证 → 客户端不做）。
//!
//! 无真平台凭据 / 账号：本文件全部走 [`FakeTransport`]（零网络）；真跑腿见 `publish_real.rs`
//! （`POLY_PUBLISH_REAL_DOUYIN=1`，缺配置即 SKIP 并打印口径）。

mod common;

use std::time::Duration;

use common::{FakeTransport, TempRoot};
use poly_core::CredentialRef;
use poly_publish::{
    Archive, AuditState, Channel, DouyinPublisher, HttpResponse, Package, PollPolicy, PublishError,
    PublishRequest, Receipt, codes, publish, receipt_key,
};

/// 形态样本（非真实账号）。
const APPID: &str = "tt1234567890abcdef";
/// 版本号。
const VERSION: &str = "1.0.0";
/// 版本描述（进 `user_desc`）。
const DESC: &str = "首个版本 v1";
/// access_token 样本（外部注入；≥6 字节会被脱敏器收录）。
const TOKEN: &str = "isvact-token-abcdef123456";

const UPLOAD_URL: &str = "https://open.douyin.com/api/apps/v1/package_version/upload/";
const HOSTS_URL: &str = "https://open.douyin.com/api/apps/v1/package_version/get_audit_hosts/";
const AUDIT_URL: &str = "https://open.douyin.com/api/apps/v2/package_version/audit/";
const VERSIONS_URL: &str = "https://open.douyin.com/api/apps/v1/package_version/versions/";

/// 材料样本（模板 id + 透传字段；代码包经模板库通道，本客户端不传包体）。
fn materials() -> serde_json::Value {
    serde_json::json!({
        "template_id": 7361234567890123456u64,
        "ext_json": "{\"env\":\"prod\"}",
        "tag": "",
        "audit_note": "契约测试提审",
        "audit_way": 1,
    })
}

/// 平台侧响应构造（err_no=0 信封；log_id 恒有）。
fn ok(data: Option<serde_json::Value>, log_id: &str) -> HttpResponse {
    let mut envelope = serde_json::Map::new();
    envelope.insert("err_no".to_string(), serde_json::json!(0));
    envelope.insert("err_msg".to_string(), serde_json::json!(""));
    if let Some(data) = data {
        envelope.insert("data".to_string(), data);
    }
    envelope.insert("log_id".to_string(), serde_json::json!(log_id));
    common::json_ok(serde_json::Value::Object(envelope))
}

/// 平台侧错误响应。
fn err(err_no: i64, err_msg: &str) -> HttpResponse {
    common::json_ok(serde_json::json!({
        "err_no": err_no, "err_msg": err_msg, "log_id": "LOG-ERR-1",
    }))
}

/// 一次完整流程的平台侧响应（版本创建 → 审核域名 → 提审受理 → 审核中 → 通过）。
///
/// 审核中那轮的 `reason` 里嵌入 access_token：压「平台自述文本进回执前须脱敏」。
fn approved_script() -> Vec<HttpResponse> {
    vec![
        ok(None, "LOG-UPLOAD-1"),
        ok(
            Some(serde_json::json!({"host_names": ["tma-test.example.com", "www.example.org"]})),
            "LOG-HOSTS-1",
        ),
        ok(None, "LOG-AUDIT-1"),
        ok(
            Some(serde_json::json!({
                "audit": {
                    "status": 0, "version": VERSION,
                    "reason": format!("审核中（含 token={TOKEN} 压脱敏）"),
                },
                "current": {"version": VERSION},
            })),
            "LOG-VERSIONS-1",
        ),
        ok(
            Some(serde_json::json!({"audit": {"status": 1, "version": VERSION}})),
            "LOG-VERSIONS-2",
        ),
    ]
}

/// 标准发布请求（轮询间隔 0 → 测试即时；超预算默认 30s）。
fn request(root: &TempRoot, credential: CredentialRef) -> PublishRequest {
    PublishRequest::new(APPID, VERSION, DESC, root.dist(), credential)
        .with_materials(materials())
        .with_poll(PollPolicy {
            interval: Duration::ZERO,
            timeout: Duration::from_secs(30),
        })
}

/// 当前代码包的回执键。
fn receipt_key_of(root: &TempRoot) -> String {
    let digest = Package::load(&root.dist())
        .expect("读代码包")
        .digest()
        .to_string();
    receipt_key(APPID, VERSION, &digest)
}

/// 抖音凭据 + 内存后端（引用模型注入；载荷 = access_token）。
fn douyin_credential(
    name: &str,
) -> (
    CredentialRef,
    Vec<Box<dyn poly_publish::CredentialProvider>>,
) {
    common::credential(name, common::douyin_payload(TOKEN))
}

#[test]
fn full_flow_upload_submit_poll_and_receipt_contract() {
    let root = TempRoot::new("dy-full");
    let fake = FakeTransport::new(approved_script());
    let (reference, providers) = douyin_credential("douyin-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    let publisher = DouyinPublisher::new(fake.clone());

    let outcome = publish(&publisher, &request, &providers, &archive).expect("全流程走通");
    assert_eq!(outcome.resumed_from, None, "全新执行");
    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(
        outcome.path,
        archive.receipt_path(Channel::Douyin.id(), &receipt_key_of(&root)),
        "回执路径口径固定"
    );

    // ---- 请求时序：版本创建 → 审核域名 → 提审 → 轮询 ×2 ----
    let requests = fake.requests();
    assert_eq!(requests.len(), 5, "五次请求");
    for (recorded, endpoint) in
        requests
            .iter()
            .zip([UPLOAD_URL, HOSTS_URL, AUDIT_URL, VERSIONS_URL, VERSIONS_URL])
    {
        assert_eq!(recorded.endpoint(), endpoint, "端点口径固定");
        assert_eq!(
            recorded.header("access-token"),
            Some(TOKEN),
            "鉴权走 access-token 头（连字符形态）"
        );
    }
    let methods: Vec<poly_publish::Method> = requests.iter().map(|r| r.method).collect();
    assert_eq!(
        methods,
        [
            poly_publish::Method::Post,
            poly_publish::Method::Get,
            poly_publish::Method::Post,
            poly_publish::Method::Get,
            poly_publish::Method::Get,
        ],
        "提审前置为 GET、其余 POST/GET 分工固定"
    );

    // ① 版本创建：body 逐字（template_id 走 Int64；ext_json 为字符串形式；tag 原样）。
    assert_eq!(requests[0].header("Content-Type"), Some("application/json"));
    let upload_body: serde_json::Value =
        serde_json::from_str(&requests[0].body_text()).expect("版本创建请求为 JSON");
    assert_eq!(
        upload_body,
        serde_json::json!({
            "template_id": 7361234567890123456u64,
            "user_version": VERSION,
            "user_desc": DESC,
            "ext_json": "{\"env\":\"prod\"}",
            "tag": "",
        }),
        "body 逐字（无包体：代码先经模板库通道）"
    );

    // ② 审核域名：GET、无正文。
    assert!(requests[1].body.is_empty(), "GET 无正文");
    assert_eq!(requests[1].query(), "", "无查询参数");

    // ③ 提审：host_names 来自前置响应 + 可选字段透传。
    let audit_body: serde_json::Value =
        serde_json::from_str(&requests[2].body_text()).expect("提审请求为 JSON");
    assert_eq!(
        audit_body,
        serde_json::json!({
            "host_names": ["tma-test.example.com", "www.example.org"],
            "audit_note": "契约测试提审",
            "audit_way": 1,
            "tag": "",
        }),
        "host_names 来自 get_audit_hosts，其余透传"
    );

    // ④ 轮询：GET、无查询参数、无正文。
    for poll in &requests[3..] {
        assert!(poll.body.is_empty() && poll.query().is_empty(), "无参轮询");
    }

    // ---- 回执：状态流水 + 快照 + 计数 ----
    let receipt = &outcome.receipt;
    assert_eq!(receipt.schema_version, poly_publish::SCHEMA_VERSION);
    assert_eq!(receipt.channel, "douyin-open-platform");
    assert_eq!(receipt.appid, APPID);
    assert_eq!(receipt.version, VERSION);
    assert_eq!(receipt.runs, 1);
    let upload = receipt.upload.as_ref().expect("上传快照");
    assert_eq!(upload["err_no"], 0);
    assert_eq!(upload["log_id"], "LOG-UPLOAD-1", "log_id 随快照留档");
    assert!(receipt.submit.is_some());
    assert_eq!(receipt.polls.len(), 2);
    assert_eq!(receipt.polls[0].state, AuditState::InReview);
    assert_eq!(receipt.polls[0].raw["data"]["audit"]["status"], 0);
    assert_eq!(receipt.polls[1].state, AuditState::Approved);
    assert_eq!(receipt.polls[1].reason, None, "通过轮不带原因");

    // ---- 脱敏：token 出现在平台自述文本里 → 回执必须已抹除 ----
    let reason = receipt.polls[0].reason.as_deref().expect("审核中带 reason");
    assert!(reason.contains("<脱敏>"), "平台文本先过脱敏器：{reason}");
    assert!(!reason.contains(TOKEN), "token 不得落盘：{reason}");
    let file = std::fs::read_to_string(&outcome.path).expect("读回执");
    assert!(!file.contains(TOKEN), "回执不得含 access_token");
}

#[test]
fn rerun_after_terminal_is_noop_without_network() {
    let root = TempRoot::new("dy-noop");
    let fake = FakeTransport::new(approved_script());
    let (reference, providers) = douyin_credential("douyin-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    publish(
        &DouyinPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("首跑");
    assert_eq!(fake.count(), 5);

    let second = FakeTransport::new(Vec::new());
    let outcome = publish(
        &DouyinPublisher::new(second.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("终态重跑 = no-op");
    assert_eq!(outcome.resumed_from, Some(AuditState::Approved));
    assert_eq!(outcome.receipt.runs, 2, "重跑如实计数");
    assert_eq!(second.count(), 0, "终态重跑零触网");
}

#[test]
fn resume_from_in_review_polls_only_until_rejected() {
    let root = TempRoot::new("dy-resume");
    let archive = Archive::new(root.path());
    let digest = Package::load(&root.dist())
        .expect("读包")
        .digest()
        .to_string();
    let mut seeded = Receipt::new(Channel::Douyin.id(), APPID, VERSION, &digest);
    seeded.state = AuditState::InReview;
    seeded.upload = Some(serde_json::json!({"err_no": 0}));
    seeded.submit = Some(serde_json::json!({"err_no": 0}));
    archive.save(&seeded).expect("预置回执");

    let fake = FakeTransport::new(vec![ok(
        Some(serde_json::json!({
            "audit": {"status": 2, "version": VERSION, "reason": "内容与类目不符，请核对"},
        })),
        "LOG-VERSIONS-3",
    )]);
    let (reference, providers) = douyin_credential("douyin-mini");
    let request = request(&root, reference);
    let outcome = publish(
        &DouyinPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("从 in_review 续走");

    assert_eq!(outcome.resumed_from, Some(AuditState::InReview));
    assert_eq!(outcome.receipt.state, AuditState::Rejected);
    assert_eq!(
        outcome.receipt.polls[0].reason.as_deref(),
        Some("内容与类目不符，请核对"),
        "平台自述拒绝原因随行入档"
    );
    assert_eq!(fake.count(), 1, "续走只轮询一次：不重传、不重提审");
    assert_eq!(fake.requests()[0].method, poly_publish::Method::Get);
}

#[test]
fn upload_11303_is_treated_as_already_created() {
    let root = TempRoot::new("dy-upload-11303");
    let mut script = approved_script();
    script[0] = err(11303, "has auditing version");
    let fake = FakeTransport::new(script);
    let (reference, providers) = douyin_credential("douyin-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    let outcome = publish(
        &DouyinPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("已有版本在审核中 = 版本已创建（断点续走正常形态）");

    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(
        outcome.receipt.upload.as_ref().expect("上传快照")["err_no"],
        11303,
        "幂等受理原样留档"
    );
    assert_eq!(fake.count(), 5, "续走全链路");
}

#[test]
fn submit_11303_is_treated_as_already_in_queue() {
    let root = TempRoot::new("dy-submit-11303");
    let mut script = approved_script();
    script[2] = err(11303, "has auditing version");
    let fake = FakeTransport::new(script);
    let (reference, providers) = douyin_credential("douyin-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    let outcome = publish(
        &DouyinPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("已在提审队列 = at-least-once 续走");

    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(
        outcome.receipt.submit.as_ref().expect("提审快照")["err_no"],
        11303,
        "幂等受理原样留档"
    );
}

#[test]
fn missing_test_version_surfaces_hint_and_receipt_stays_uploaded() {
    let root = TempRoot::new("dy-11302");
    let mut script = approved_script();
    script[2] = err(11302, "test version not found");
    script.truncate(3);
    let fake = FakeTransport::new(script);
    let (reference, providers) = douyin_credential("douyin-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    let err = publish(
        &DouyinPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect_err("测试版本未找到须硬错");
    assert_eq!(err.code(), codes::E_PUB_0003);
    match &err {
        PublishError::Platform {
            channel,
            code,
            message,
            hint,
        } => {
            assert_eq!(*channel, "douyin-open-platform");
            assert_eq!(code, "11302");
            assert!(
                message.contains("log_id=LOG-ERR-1"),
                "log_id 随错误：{message}"
            );
            let hint = hint.as_deref().expect("带修复提示");
            assert!(hint.contains("上传"), "{hint}");
        }
        other => panic!("应为 Platform 错误：{other}"),
    }

    assert_eq!(fake.count(), 3, "止步提审：没有轮询请求");
    let receipt = archive
        .load(Channel::Douyin.id(), &receipt_key_of(&root))
        .expect("读回执")
        .expect("回执存在");
    assert_eq!(receipt.state, AuditState::Uploaded);
}

#[test]
fn empty_host_names_is_refused_before_audit() {
    let root = TempRoot::new("dy-hosts");
    let fake = FakeTransport::new(vec![
        ok(None, "LOG-UPLOAD-1"),
        ok(Some(serde_json::json!({"host_names": []})), "LOG-HOSTS-2"),
    ]);
    let (reference, providers) = douyin_credential("douyin-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    let err = publish(
        &DouyinPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect_err("无可用审核域名须拒");
    assert_eq!(err.code(), codes::E_PUB_0003);
    let hint = err.hint().expect("带修复提示");
    assert!(hint.contains("域名"), "{hint}");
    assert_eq!(fake.count(), 2, "没有 audit 请求");
}

#[test]
fn missing_template_refuses_before_any_network() {
    let root = TempRoot::new("dy-no-template");
    let fake = FakeTransport::new(Vec::new());
    let (reference, providers) = douyin_credential("douyin-mini");
    let request =
        PublishRequest::new(APPID, VERSION, DESC, root.dist(), reference).with_poll(PollPolicy {
            interval: Duration::ZERO,
            timeout: Duration::from_secs(30),
        });
    let archive = Archive::new(root.path());
    let err = publish(
        &DouyinPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect_err("缺 template_id 须拒");
    assert_eq!(err.code(), codes::E_PUB_0004);
    assert!(err.to_string().contains("template_id"), "{err}");
    assert_eq!(fake.count(), 0, "材料问题零触网");
}

#[test]
fn token_error_surfaces_refresh_hint() {
    let root = TempRoot::new("dy-token");
    let fake = FakeTransport::new(vec![err(28001008, "access_token expired")]);
    let (reference, providers) = douyin_credential("douyin-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    let err = publish(
        &DouyinPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect_err("token 无效须硬错");
    assert_eq!(err.code(), codes::E_PUB_0003);
    let hint = err.hint().expect("带修复提示");
    assert!(hint.contains("access_token"), "{hint}");
    assert!(hint.contains("凭据"), "提示指向重新注入：{hint}");
    assert_eq!(fake.count(), 1, "token 问题在首个请求暴露");
}
