//! T-M8-11 验收：提审回执归档（提交号 / 时间戳）与审核状态轮询；中断续跑不重复提交。
//!
//! 故障注入走进程内 mock 服务器（[`ScriptServer`]，零网络）：正常响应 / 网络中断按序吐，
//! 提审请求单独计数。口径：
//! - 终态回执落盘且含 `submission_id` / `submitted_unix` / `decided_unix`，字段可追溯；
//! - 崩溃窗口（平台已受理 + journal `Accepted`、回执未更新）重跑只续轮询，零提审请求；
//! - 轮询中途网络中断后仍能到终态并归档（失败轮不写流水，成功轮全落盘）。

mod common;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Recorded, TempRoot};
use poly_core::CredentialRef;
use poly_publish::{
    Archive, AuditState, Channel, HttpRequest, HttpResponse, Package, PollPolicy, PublishError,
    PublishRequest, Receipt, RetryPolicy, SUBMIT_SCHEMA_VERSION, SubmitJournal, SubmitStatus,
    Transport, WechatPublisher, publish, receipt_key, submission_key,
};

const APPID: &str = "wx1234567890abcdef";
const VERSION: &str = "3.0.0";
const DESC: &str = "回执归档验收";
const RAND: &str = "rand-receipt-abc123";
const TOKEN: &str = "TOKEN-receipt-abcdef123456";
const SUBMIT_URL: &str = "https://api.weixin.qq.com/wxa/submit_audit";

fn materials() -> serde_json::Value {
    serde_json::json!([{ "address": "pages/index/index", "tag": "教育" }])
}

fn fast_retry() -> RetryPolicy {
    RetryPolicy {
        max_attempts: 3,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(2),
    }
}

fn request(root: &TempRoot, credential: CredentialRef) -> PublishRequest {
    PublishRequest::new(APPID, VERSION, DESC, root.dist(), credential)
        .with_materials(materials())
        .with_poll(PollPolicy {
            interval: Duration::ZERO,
            timeout: Duration::from_secs(30),
        })
        .with_retry(fast_retry())
}

fn wechat_credential(
    name: &str,
) -> (
    CredentialRef,
    Vec<Box<dyn poly_publish::CredentialProvider>>,
) {
    common::credential(name, common::wechat_payload(APPID, &common::hex32()))
}

fn token_response() -> HttpResponse {
    common::json_ok(serde_json::json!({"access_token": TOKEN, "expires_in": 7200}))
}

fn digest_of(root: &TempRoot) -> String {
    Package::load(&root.dist())
        .expect("读代码包")
        .digest()
        .to_string()
}

fn receipt_key_of(root: &TempRoot) -> String {
    receipt_key(APPID, VERSION, &digest_of(root))
}

// ---------------------------------------------------------------- mock 服务器

/// 脚本动作：正常响应 / 网络中断。
enum Step {
    Reply(HttpResponse),
    Timeout,
}

/// 按序吐响应的 mock 服务器；单独统计提审请求（无重复提交的判据）。
struct ScriptServer {
    steps: Mutex<VecDeque<Step>>,
    requests: Mutex<Vec<Recorded>>,
    submit_requests: AtomicUsize,
}

impl ScriptServer {
    fn new(steps: Vec<Step>) -> Arc<Self> {
        Arc::new(Self {
            steps: Mutex::new(steps.into()),
            requests: Mutex::new(Vec::new()),
            submit_requests: AtomicUsize::new(0),
        })
    }

    fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().expect("记录锁").clone()
    }

    fn submit_requests(&self) -> usize {
        self.submit_requests.load(Ordering::SeqCst)
    }
}

impl Transport for ScriptServer {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, PublishError> {
        self.requests.lock().expect("记录锁").push(Recorded {
            method: request.method,
            url: request.url.clone(),
            headers: request.headers.clone(),
            body: request.body.clone(),
        });
        let endpoint = request.url.split(['?', '#']).next().unwrap_or(&request.url);
        if endpoint == SUBMIT_URL {
            self.submit_requests.fetch_add(1, Ordering::SeqCst);
        }
        match self
            .steps
            .lock()
            .expect("脚本锁")
            .pop_front()
            .expect("ScriptServer 脚本用尽（请求序与测试预期不符）")
        {
            Step::Reply(response) => Ok(response),
            Step::Timeout => Err(PublishError::Http {
                endpoint: endpoint.to_string(),
                status: None,
                reason: "模拟网络中断".to_string(),
            }),
        }
    }
}

// ---------------------------------------------------------------- 验收

/// 终态（approved）后回执落盘：提交号 + 提审 / 终态时间戳齐全且时序自洽。
#[test]
fn terminal_receipt_archives_submission_id_and_timestamps() {
    let root = TempRoot::new("receipt-terminal");
    let server = ScriptServer::new(vec![
        Step::Reply(common::json_ok(
            serde_json::json!({"errCode": 0, "data": {"randomString": RAND}}),
        )),
        Step::Reply(common::json_ok(serde_json::json!({"errCode": 0}))),
        Step::Reply(token_response()),
        // 提审受理回执带提审单号（微信 auditid）。
        Step::Reply(common::json_ok(
            serde_json::json!({"errcode": 0, "errmsg": "ok", "auditid": 42}),
        )),
        Step::Reply(common::json_ok(
            serde_json::json!({"errcode": 0, "status": 2}),
        )),
        Step::Reply(common::json_ok(
            serde_json::json!({"errcode": 0, "status": 0}),
        )),
    ]);
    let (reference, providers) = wechat_credential("wechat-receipt");
    let archive = Archive::new(root.path());
    let outcome = publish(
        &WechatPublisher::new(server.clone()),
        &request(&root, reference),
        &providers,
        &archive,
    )
    .expect("终态轮询须成功");

    let receipt = &outcome.receipt;
    assert_eq!(receipt.state, AuditState::Approved);
    assert_eq!(receipt.submission_id.as_deref(), Some("42"), "提交号入档");
    let submitted = receipt.submitted_unix.expect("提审受理时刻");
    let decided = receipt.decided_unix.expect("终态判定时刻");
    assert!(
        receipt.first_unix <= submitted && submitted <= decided && decided <= receipt.last_unix,
        "时间戳须时序自洽：first={} submitted={submitted} decided={decided} last={}",
        receipt.first_unix,
        receipt.last_unix
    );
    assert_eq!(
        receipt.polls.last().map(|poll| poll.state),
        Some(AuditState::Approved),
        "终态流水随回执落盘"
    );
    assert_eq!(
        receipt.polls.last().map(|poll| poll.at_unix),
        Some(decided),
        "终态时刻 = 终态流水的落盘时刻"
    );

    // 落盘回执（不只是内存对象）：字段在文件里，可追溯。
    let key = receipt_key_of(&root);
    let stored = archive
        .load(Channel::Wechat.id(), &key)
        .expect("读回执")
        .expect("回执存在");
    assert_eq!(stored.schema_version, poly_publish::SCHEMA_VERSION);
    assert_eq!(stored.submission_id.as_deref(), Some("42"));
    assert_eq!(stored.submitted_unix, Some(submitted));
    assert_eq!(stored.decided_unix, Some(decided));
    let text = std::fs::read_to_string(&outcome.path).expect("读回执文件");
    for field in [
        "\"submission_id\"",
        "\"submitted_unix\"",
        "\"decided_unix\"",
    ] {
        assert!(text.contains(field), "回执文件缺字段 {field}：{text}");
    }

    // 提审去重状态同步记提交号 / 受理时刻（崩溃续跑的时间线锚点）。
    let journal = archive
        .load_submit(Channel::Wechat.id(), &key)
        .expect("读去重状态")
        .expect("去重状态存在");
    assert_eq!(journal.schema_version, SUBMIT_SCHEMA_VERSION);
    assert_eq!(journal.status, SubmitStatus::Accepted);
    assert_eq!(journal.submission_id.as_deref(), Some("42"));
    assert_eq!(journal.submitted_unix, Some(submitted));
    assert_eq!(server.submit_requests(), 1, "一次提审");
}

/// 崩溃窗口：平台已受理且 journal `Accepted`、回执仍 `uploaded` → 重跑零提审请求，
/// 提交号 / 受理时刻从 journal 复原，终态回执补齐 `decided_unix`。
#[test]
fn interrupted_rerun_recovers_receipt_without_repeating_submission() {
    let root = TempRoot::new("receipt-crash");
    let archive = Archive::new(root.path());
    let digest = digest_of(&root);
    let key = receipt_key(APPID, VERSION, &digest);
    let idempotency = submission_key(Channel::Wechat.id(), APPID, VERSION, &digest);

    // 预置崩溃现场：上传完成、平台已受理（模拟受理后、回执更新前进程被杀）。
    let mut receipt = Receipt::new(Channel::Wechat.id(), APPID, VERSION, &digest);
    receipt.upload = Some(serde_json::json!({"errCode": 0}));
    archive.save(&receipt).expect("预置 uploaded 回执");
    let mut journal = SubmitJournal::new(&idempotency);
    journal.status = SubmitStatus::Accepted;
    journal.attempts = 1;
    journal.submission_id = Some("audit-crash-42".to_string());
    journal.submitted_unix = Some(1_700_000_000);
    archive
        .save_submit(Channel::Wechat.id(), &key, &journal)
        .expect("预置已受理状态");

    let server = ScriptServer::new(vec![
        Step::Reply(token_response()),
        Step::Reply(common::json_ok(
            serde_json::json!({"errcode": 0, "status": 0}),
        )),
    ]);
    let (reference, providers) = wechat_credential("wechat-crash");
    let outcome = publish(
        &WechatPublisher::new(server.clone()),
        &request(&root, reference),
        &providers,
        &archive,
    )
    .expect("从 journal 续走");

    assert_eq!(outcome.resumed_from, Some(AuditState::Uploaded));
    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(server.submit_requests(), 0, "已受理 → 绝不重发提审");
    assert!(
        !server
            .requests()
            .iter()
            .any(|record| record.endpoint() == SUBMIT_URL),
        "请求序里不得出现提审端点"
    );
    assert_eq!(
        outcome.receipt.submission_id.as_deref(),
        Some("audit-crash-42"),
        "提交号从 journal 复原"
    );
    assert_eq!(
        outcome.receipt.submitted_unix,
        Some(1_700_000_000),
        "受理时刻从 journal 复原（不用重跑时刻覆盖）"
    );
    assert!(outcome.receipt.decided_unix.is_some(), "终态时刻补齐");

    // 落盘口径与内存一致。
    let stored = archive
        .load(Channel::Wechat.id(), &key)
        .expect("读回执")
        .expect("回执存在");
    assert_eq!(stored.submission_id.as_deref(), Some("audit-crash-42"));
    assert_eq!(stored.submitted_unix, Some(1_700_000_000));
    assert_eq!(stored.decided_unix, outcome.receipt.decided_unix);
}

/// 轮询中途网络中断（重试后成功）：终态回执仍归档；失败轮不写流水，提审只一次。
#[test]
fn poll_interruption_still_archives_terminal_receipt() {
    let root = TempRoot::new("receipt-poll-interrupt");
    let server = ScriptServer::new(vec![
        Step::Reply(common::json_ok(
            serde_json::json!({"errCode": 0, "data": {"randomString": RAND}}),
        )),
        Step::Reply(common::json_ok(serde_json::json!({"errCode": 0}))),
        Step::Reply(token_response()),
        Step::Reply(common::json_ok(
            serde_json::json!({"errcode": 0, "errmsg": "ok"}),
        )),
        Step::Timeout, // 轮询中断（可恢复）
        // 提审响应未给单号（微信真实形态）：轮询响应里的 auditid 首次出现即补录。
        Step::Reply(common::json_ok(serde_json::json!({
            "errcode": 0,
            "status": 1,
            "reason": "类目不符",
            "auditid": 77,
        }))),
    ]);
    let (reference, providers) = wechat_credential("wechat-poll-interrupt");
    let archive = Archive::new(root.path());
    let outcome = publish(
        &WechatPublisher::new(server.clone()),
        &request(&root, reference),
        &providers,
        &archive,
    )
    .expect("中断重试后到终态");

    let receipt = &outcome.receipt;
    assert_eq!(receipt.state, AuditState::Rejected);
    assert_eq!(receipt.polls.len(), 1, "失败轮不写流水；成功终态轮才落盘");
    assert_eq!(receipt.polls[0].reason.as_deref(), Some("类目不符"));
    assert!(receipt.decided_unix.is_some(), "中断不丢终态时间戳");
    assert_eq!(
        receipt.submission_id.as_deref(),
        Some("77"),
        "提交号可从轮询响应补录（提审响应未给单号）"
    );
    assert_eq!(server.submit_requests(), 1, "中断不触发重新提审");
    let stored = archive
        .load(Channel::Wechat.id(), &receipt_key_of(&root))
        .expect("读回执")
        .expect("回执存在");
    assert_eq!(stored.state, AuditState::Rejected);
    assert_eq!(stored.decided_unix, receipt.decided_unix);
}
