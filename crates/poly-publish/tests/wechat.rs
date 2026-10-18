//! T-M3-09 验收（mock HTTP 契约）：微信 publish 客户端 上传 → 提审 → 轮询 + 回执归档。
//!
//! 口径锚点：上传通道无公开文档，契约按 miniprogram-ci 2.1.31 源码（`dist/ci/upload.js` /
//! `dist/utils/sign.js` / `dist/utils/pack.js`）建模——本文件断言 query 原样、B 协议容器布局、
//! **raw PKCS#1 v1.5 签名可被独立复算**（Node `crypto.privateEncrypt` 语义）；提审 / 轮询按
//! 官方文档口径（`item_list` 必填 1–5 项、GET `get_latest_auditstatus` 无需 auditid）。
//!
//! 无真平台凭据 / 账号：本文件全部走 [`FakeTransport`]（零网络）；真跑腿见 `publish_real.rs`
//! （`POLY_PUBLISH_REAL=1`，缺配置即 SKIP 并打印口径）。

mod common;

use std::time::Duration;

use common::{FakeTransport, TempRoot};
use poly_core::CredentialRef;
use poly_publish::wechat::CI_PROTOCOL_VERSION;
use poly_publish::{
    Archive, AuditState, Channel, HttpResponse, Package, PollPolicy, PublishError, PublishRequest,
    Receipt, SCHEMA_VERSION, WechatPublisher, codes, publish, receipt_key,
};

/// 形态样本（非真实账号）。
const APPID: &str = "wx1234567890abcdef";
/// 版本号。
const VERSION: &str = "1.0.0";
/// 版本描述（含空格：压 query 百分号编码）。
const DESC: &str = "首个版本 v1";
/// 签名随机串（平台 getrandstr 返回值样本）。
const RAND: &str = "randString-abc123";
/// access_token 样本（≥6 字节，会被脱敏器收录）。
const TOKEN: &str = "TOKEN-abcdef123456";

const RAND_URL: &str = "https://servicewechat.com/wxa/ci/getrandstr";
const UPLOAD_URL: &str = "https://servicewechat.com/wxa/ci/upload";
const TOKEN_URL: &str = "https://api.weixin.qq.com/cgi-bin/token";
const SUBMIT_URL: &str = "https://api.weixin.qq.com/wxa/submit_audit";
const POLL_URL: &str = "https://api.weixin.qq.com/wxa/get_latest_auditstatus";

/// 提审材料样本（类目是平台侧配置的如实转述：本客户端不猜类目）。
fn materials() -> serde_json::Value {
    serde_json::json!([{
        "address": "pages/index/index",
        "tag": "教育",
        "first_class": "教育",
        "second_class": "在线教育",
        "first_id": 1,
        "second_id": 100
    }])
}

/// token 响应（`expires_in` 走缓存）。
fn token_response() -> HttpResponse {
    common::json_ok(serde_json::json!({"access_token": TOKEN, "expires_in": 7200}))
}

/// 一次完整流程的平台侧响应（随机串 → 上传受理 → token → 提审受理 → 审核中 → 通过）。
///
/// 审核中那轮的 `reason` 里嵌入 token：压「平台自述文本进回执前须脱敏」。
fn approved_script() -> Vec<HttpResponse> {
    vec![
        common::json_ok(serde_json::json!({"errCode": 0, "data": {"randomString": RAND}})),
        common::json_ok(serde_json::json!({"errCode": 0, "errMsg": "ok"})),
        token_response(),
        common::json_ok(serde_json::json!({"errcode": 0, "errmsg": "ok", "auditid": 42})),
        common::json_ok(serde_json::json!({
            "errcode": 0,
            "errmsg": "ok",
            "status": 2,
            "reason": format!("审核中 token={TOKEN}"),
        })),
        common::json_ok(serde_json::json!({"errcode": 0, "errmsg": "ok", "status": 0})),
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

/// 当前代码包的回执键（摘要与容器无关：同一目录确定性）。
fn receipt_key_of(root: &TempRoot) -> String {
    let digest = Package::load(&root.dist())
        .expect("读代码包")
        .digest()
        .to_string();
    receipt_key(APPID, VERSION, &digest)
}

/// 微信凭据 + 内存后端（引用模型注入）。
fn wechat_credential(
    name: &str,
) -> (
    CredentialRef,
    Vec<Box<dyn poly_publish::CredentialProvider>>,
) {
    common::credential(name, common::wechat_payload(APPID, &common::hex32()))
}

#[test]
fn full_flow_upload_submit_poll_and_receipt_contract() {
    let root = TempRoot::new("full");
    let fake = FakeTransport::new(approved_script());
    let (reference, providers) = wechat_credential("wechat-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    let publisher = WechatPublisher::new(fake.clone());

    let outcome = publish(&publisher, &request, &providers, &archive).expect("全流程走通");
    assert_eq!(outcome.resumed_from, None, "全新执行");
    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(
        outcome.path,
        archive.receipt_path(Channel::Wechat.id(), &receipt_key_of(&root)),
        "回执路径口径固定"
    );

    // ---- 请求时序：随机串 → 上传 → 取 token（一次）→ 提审 → 轮询 ×2 ----
    let requests = fake.requests();
    assert_eq!(requests.len(), 6, "六次请求");
    let endpoints: Vec<&str> = requests.iter().map(|r| r.endpoint()).collect();
    assert_eq!(
        endpoints,
        [
            RAND_URL, UPLOAD_URL, TOKEN_URL, SUBMIT_URL, POLL_URL, POLL_URL
        ],
        "请求顺序固定：取随机串 → 上传 → token → 提审 → 轮询（token 只取一次）"
    );

    // ① 取随机串：JSON 正文，clientRand 为 < 1e8 的整数（miniprogram-ci 同款体）。
    assert_eq!(requests[0].header("Content-Type"), Some("application/json"));
    let rand_body: serde_json::Value =
        serde_json::from_str(&requests[0].body_text()).expect("随机串请求为 JSON");
    assert_eq!(rand_body["appid"], APPID);
    let client_rand = rand_body["clientRand"].as_u64().expect("clientRand 为整数");
    assert!(client_rand < 100_000_000, "clientRand < 1e8：{client_rand}");
    assert_eq!(rand_body.as_object().expect("对象").len(), 2, "体只含两键");

    // ② 上传：query 原样口径（desc / debugLaunchInfo 百分号编码）+ 不显式设 Content-Type。
    assert_eq!(
        requests[1].query(),
        format!(
            "codeprotect=0&type=miniProgram&appid={APPID}&version={VERSION}\
             &desc=%E9%A6%96%E4%B8%AA%E7%89%88%E6%9C%AC%20v1&robot=1\
             &debugLaunchInfo=%7B%22scene%22%3A1011%7D"
        ),
        "上传 query 与 miniprogram-ci 同口径（含编码）"
    );
    assert_eq!(
        requests[1].header("Content-Type"),
        None,
        "口径同 miniprogram-ci：Buffer body、不显式设 Content-Type"
    );

    // ③ 上传体：gzip(B 协议容器)，条目 = 代码包（升序、`/` 前缀）+ ci.signature。
    let container = common::unpack_container(&common::gunzip(&requests[1].body));
    let names: Vec<&str> = container.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "/app.js",
            "/app.json",
            "/pages/index/index.js",
            "/ci.signature"
        ],
        "代码包条目升序 + 签名条目追加在尾（miniprogram-ci pack 同序）"
    );
    assert_eq!(container[0].1, b"App({})".to_vec(), "app.js 内容原样");
    assert_eq!(
        container[1].1,
        br#"{"pages":["pages/index/index"]}"#.to_vec(),
        "app.json 内容原样"
    );

    // ④ 签名：raw PKCS#1 v1.5 over `{"appid":…,"rand_str":…}`——独立复算核验。
    let signature_entry: serde_json::Value =
        serde_json::from_slice(&container[3].1).expect("ci.signature 为 JSON");
    assert_eq!(
        signature_entry["version"], CI_PROTOCOL_VERSION,
        "协议代际随 ci.signature 上报"
    );
    let message = format!("{{\"appid\":\"{APPID}\",\"rand_str\":\"{RAND}\"}}");
    common::verify_raw_signature(
        message.as_bytes(),
        signature_entry["signature"].as_str().expect("签名为字符串"),
    );

    // ⑤ token：appid + secret 走 query（该 URL 不进回执 / 错误）。
    let token_query = requests[2].query();
    assert!(
        token_query.contains("grant_type=client_credential"),
        "{token_query}"
    );
    assert!(
        token_query.contains(&format!("appid={APPID}")),
        "{token_query}"
    );
    assert!(
        token_query.contains(&format!("secret={}", common::hex32())),
        "{token_query}"
    );

    // ⑥ 提审：`{"item_list":…}` 原样透传 + access_token 走 query。
    assert_eq!(requests[3].header("Content-Type"), Some("application/json"));
    assert_eq!(requests[3].query(), format!("access_token={TOKEN}"));
    let submit_body: serde_json::Value =
        serde_json::from_str(&requests[3].body_text()).expect("提审请求为 JSON");
    assert_eq!(
        submit_body,
        serde_json::json!({"item_list": materials()}),
        "item_list 原样透传，不加料"
    );

    // ⑦ 轮询：GET、无需 auditid（断点续走友好）、无正文。
    for poll in &requests[4..] {
        assert_eq!(poll.method, poly_publish::Method::Get, "轮询为 GET");
        assert_eq!(poll.query(), format!("access_token={TOKEN}"));
        assert!(poll.body.is_empty(), "GET 无正文");
    }

    // ---- 回执：状态流水 + 快照 + 计数 ----
    let receipt = &outcome.receipt;
    assert_eq!(receipt.schema_version, SCHEMA_VERSION);
    assert_eq!(receipt.channel, "wechat-open-platform");
    assert_eq!(receipt.appid, APPID);
    assert_eq!(receipt.version, VERSION);
    assert_eq!(receipt.runs, 1);
    assert!(receipt.upload.is_some() && receipt.submit.is_some());
    assert_eq!(receipt.polls.len(), 2);
    assert_eq!(receipt.polls[0].state, AuditState::InReview);
    assert_eq!(receipt.polls[0].raw["status"], 2);
    assert_eq!(receipt.polls[1].state, AuditState::Approved);
    assert_eq!(receipt.polls[1].reason, None, "通过轮不带原因");

    // ---- 脱敏：token 出现在平台自述文本里 → 回执必须已抹除；回执不含任何凭据材料 ----
    let reason = receipt.polls[0].reason.as_deref().expect("审核中带 reason");
    assert!(reason.contains("<脱敏>"), "平台文本先过脱敏器：{reason}");
    assert!(!reason.contains(TOKEN), "token 不得落盘：{reason}");
    let file = std::fs::read_to_string(&outcome.path).expect("读回执");
    let secret = common::hex32();
    let key_line = common::test_private_key_pem()
        .lines()
        .nth(1)
        .expect("PEM 有正文行")
        .to_string();
    for needle in [TOKEN, secret.as_str(), key_line.as_str()] {
        assert!(!file.contains(needle), "回执不得含凭据材料");
    }
}

#[test]
fn rerun_after_terminal_is_noop_without_network() {
    let root = TempRoot::new("noop");
    let fake = FakeTransport::new(approved_script());
    let (reference, providers) = wechat_credential("wechat-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    publish(
        &WechatPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("首跑");
    assert_eq!(fake.count(), 6);

    // 二跑：同键同内容。新传输层脚本为空——一旦触网即报错。
    let second = FakeTransport::new(Vec::new());
    let outcome = publish(
        &WechatPublisher::new(second.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("终态重跑 = no-op");
    assert_eq!(outcome.resumed_from, Some(AuditState::Approved));
    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(outcome.receipt.runs, 2, "重跑如实计数（被跑过几次）");
    assert_eq!(outcome.receipt.polls.len(), 2, "流水不追加");
    assert_eq!(second.count(), 0, "终态重跑零触网");
}

#[test]
fn resume_from_uploaded_skips_upload_and_submits() {
    let root = TempRoot::new("resume-uploaded");
    let archive = Archive::new(root.path());
    // 预置断点：上传已受理、提审未受理（受理前后崩溃的恢复路径）。
    let digest = Package::load(&root.dist())
        .expect("读包")
        .digest()
        .to_string();
    let mut seeded = Receipt::new(Channel::Wechat.id(), APPID, VERSION, &digest);
    seeded.upload = Some(serde_json::json!({"errCode": 0, "errMsg": "ok"}));
    archive.save(&seeded).expect("预置回执");
    let key = receipt_key_of(&root);

    let fake = FakeTransport::new(vec![
        token_response(),
        common::json_ok(serde_json::json!({"errcode": 0, "errmsg": "ok"})),
        common::json_ok(serde_json::json!({"errcode": 0, "status": 2})),
        common::json_ok(serde_json::json!({"errcode": 0, "status": 0})),
    ]);
    let (reference, providers) = wechat_credential("wechat-mini");
    let request = request(&root, reference);
    let outcome = publish(
        &WechatPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("从 uploaded 续走");

    assert_eq!(outcome.resumed_from, Some(AuditState::Uploaded));
    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(outcome.receipt.runs, 2);
    assert_eq!(
        outcome.receipt.upload,
        Some(serde_json::json!({"errCode": 0, "errMsg": "ok"})),
        "既有上传快照保留"
    );

    // 只取 token → 提审 → 轮询 ×2：不重传代码包（getrandstr / upload 均未出现）。
    let requests = fake.requests();
    assert_eq!(requests.len(), 4);
    let endpoints: Vec<&str> = requests.iter().map(|r| r.endpoint()).collect();
    assert_eq!(endpoints, [TOKEN_URL, SUBMIT_URL, POLL_URL, POLL_URL]);
    assert_eq!(
        archive
            .load(Channel::Wechat.id(), &key)
            .expect("读")
            .map(|r| r.state),
        Some(AuditState::Approved)
    );
}

#[test]
fn resume_from_in_review_polls_only_until_rejected() {
    let root = TempRoot::new("resume-in-review");
    let archive = Archive::new(root.path());
    let digest = Package::load(&root.dist())
        .expect("读包")
        .digest()
        .to_string();
    let mut seeded = Receipt::new(Channel::Wechat.id(), APPID, VERSION, &digest);
    seeded.state = AuditState::InReview;
    seeded.upload = Some(serde_json::json!({"errCode": 0}));
    seeded.submit = Some(serde_json::json!({"errcode": 0, "auditid": 42}));
    seeded.polls.push(poly_publish::PollEntry {
        at_unix: 1,
        state: AuditState::Submitted,
        reason: None,
        raw: serde_json::json!({"status": 2}),
    });
    archive.save(&seeded).expect("预置回执");

    let fake = FakeTransport::new(vec![
        token_response(),
        common::json_ok(serde_json::json!({
            "errcode": 0,
            "status": 1,
            "reason": "类目与内容不符，请核对 first_class",
        })),
    ]);
    let (reference, providers) = wechat_credential("wechat-mini");
    let request = request(&root, reference);
    let outcome = publish(
        &WechatPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("从 in_review 续走");

    assert_eq!(outcome.resumed_from, Some(AuditState::InReview));
    assert_eq!(outcome.receipt.state, AuditState::Rejected);
    assert_eq!(outcome.receipt.polls.len(), 2, "旧流水保留 + 新的一行");
    assert_eq!(
        outcome.receipt.polls[1].reason.as_deref(),
        Some("类目与内容不符，请核对 first_class"),
        "平台自述拒绝原因随行入档"
    );

    let requests = fake.requests();
    assert_eq!(requests.len(), 2, "只取 token + 轮询一次");
    assert!(
        requests
            .iter()
            .all(|r| r.method == poly_publish::Method::Get),
        "续走只轮询：不重传、不重提审"
    );
}

#[test]
fn duplicate_submit_85009_is_accepted_as_idempotent() {
    let root = TempRoot::new("85009");
    let fake = FakeTransport::new(vec![
        common::json_ok(serde_json::json!({"errCode": 0, "data": {"randomString": RAND}})),
        common::json_ok(serde_json::json!({"errCode": 0})),
        token_response(),
        // 平台侧已有审核中版本：幂等重发（超时后重试 / 崩溃后重跑）的正常形态。
        common::json_ok(serde_json::json!({"errcode": 85009, "errmsg": "already auditing"})),
        common::json_ok(serde_json::json!({"errcode": 0, "status": 2})),
        common::json_ok(serde_json::json!({"errcode": 0, "status": 0})),
    ]);
    let (reference, providers) = wechat_credential("wechat-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    let outcome = publish(
        &WechatPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("85009 = 平台已受理，按幂等续走");

    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(
        outcome.receipt.submit,
        Some(serde_json::json!({"errcode": 85009, "errmsg": "already auditing"})),
        "85009 响应作为受理快照留档"
    );
    let submits: Vec<_> = fake
        .requests()
        .into_iter()
        .filter(|record| record.endpoint() == SUBMIT_URL)
        .collect();
    assert_eq!(submits.len(), 1, "重复提审不产生第二次提交");
    let digest = Package::load(&root.dist())
        .expect("读包")
        .digest()
        .to_string();
    let expected = poly_publish::submission_key(Channel::Wechat.id(), APPID, VERSION, &digest);
    assert_eq!(
        submits[0].header(poly_publish::transport::IDEMPOTENCY_HEADER),
        Some(expected.as_str()),
        "提审带稳定幂等键"
    );
}

#[test]
fn missing_materials_refuses_to_invent_categories() {
    let root = TempRoot::new("materials");
    let fake = FakeTransport::new(vec![
        common::json_ok(serde_json::json!({"errCode": 0, "data": {"randomString": RAND}})),
        common::json_ok(serde_json::json!({"errCode": 0})),
        token_response(),
    ]);
    let (reference, providers) = wechat_credential("wechat-mini");
    let request =
        PublishRequest::new(APPID, VERSION, DESC, root.dist(), reference).with_poll(PollPolicy {
            interval: Duration::ZERO,
            timeout: Duration::from_secs(30),
        });
    let archive = Archive::new(root.path());
    let err = publish(
        &WechatPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect_err("缺类目须拒");

    assert_eq!(err.code(), codes::E_PUB_0004);
    assert!(err.to_string().contains("item_list"), "{err}");
    let hint = err.hint().expect("带修复提示");
    assert!(hint.contains("类目"), "提示指向平台后台类目配置：{hint}");

    // 止步提审前一步：没有 submit 请求；回执停在 uploaded（补材料即可续走）。
    let endpoints: Vec<String> = fake
        .requests()
        .iter()
        .map(|r| r.endpoint().to_string())
        .collect();
    assert_eq!(
        endpoints,
        [RAND_URL, UPLOAD_URL, TOKEN_URL],
        "无 submit 请求"
    );
    let receipt = archive
        .load(Channel::Wechat.id(), &receipt_key_of(&root))
        .expect("读回执")
        .expect("回执存在");
    assert_eq!(receipt.state, AuditState::Uploaded);
}

#[test]
fn poll_timeout_reports_e005_and_resumes_from_receipt() {
    let root = TempRoot::new("timeout");
    let fake = FakeTransport::new(vec![
        common::json_ok(serde_json::json!({"errCode": 0, "data": {"randomString": RAND}})),
        common::json_ok(serde_json::json!({"errCode": 0})),
        token_response(),
        common::json_ok(serde_json::json!({"errcode": 0, "auditid": 42})),
        common::json_ok(serde_json::json!({"errcode": 0, "status": 2})),
    ]);
    let (reference, providers) = wechat_credential("wechat-mini");
    // 预算 0：第一次轮询（审核中）落盘后即超预算——确定性、不靠睡等。
    let request = request(&root, reference).with_poll(PollPolicy {
        interval: Duration::ZERO,
        timeout: Duration::ZERO,
    });
    let archive = Archive::new(root.path());
    let err = publish(&WechatPublisher::new(fake), &request, &providers, &archive)
        .expect_err("超预算须报 E_PUB_0005");
    assert!(matches!(err, PublishError::PollTimeout { .. }), "{err}");
    assert_eq!(err.code(), codes::E_PUB_0005);

    // 回执已带状态流水落盘（R4：审核结果以平台为准，本地只如实记账）。
    let key = receipt_key_of(&root);
    let receipt = archive
        .load(Channel::Wechat.id(), &key)
        .expect("读回执")
        .expect("回执存在");
    assert_eq!(receipt.state, AuditState::InReview);
    assert_eq!(receipt.polls.len(), 1, "超预算前的流水已落盘");

    // 续跑：只取 token + 轮询一次（获批）——不重传、不重提审。
    let resume = FakeTransport::new(vec![
        token_response(),
        common::json_ok(serde_json::json!({"errcode": 0, "status": 0})),
    ]);
    let outcome = publish(
        &WechatPublisher::new(resume.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("续走获批");
    assert_eq!(outcome.resumed_from, Some(AuditState::InReview));
    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(outcome.receipt.polls.len(), 2);
    assert_eq!(resume.count(), 2);
    assert!(
        resume
            .requests()
            .iter()
            .all(|r| r.method == poly_publish::Method::Get),
        "续走只轮询"
    );
}
