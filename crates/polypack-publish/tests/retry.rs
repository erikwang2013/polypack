//! T-M8-10 验收：publish 阶段的重试 / 指数退避 / 幂等提交 / 结构化失败。
//!
//! 故障注入走进程内 mock 服务器（[`FaultServer`]，零网络）：按序吐 5xx / 429 /
//! 超时（含「平台已受理但响应丢失」），并按提审请求的 `Idempotency-Key` 去重。
//! 断言口径：注入故障后最终成功，且平台侧只产生**一次**提交记录、无重复提交。

mod common;

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{FakeTransport, Recorded, TempRoot};
use polypack_core::CredentialRef;
use polypack_publish::{
    Archive, AuditState, Channel, HttpRequest, HttpResponse, Package, PollPolicy, PublishError,
    PublishRequest, Receipt, RetryPolicy, Stage, SubmitJournal, SubmitStatus, Transport,
    WechatPublisher, codes, publish, publish_checked, receipt_key, submission_key,
};

const APPID: &str = "wx1234567890abcdef";
const VERSION: &str = "2.0.0";
const DESC: &str = "重试验收";
const RAND: &str = "rand-retry-abc123";
const TOKEN: &str = "TOKEN-retry-abcdef123456";

const SUBMIT_URL: &str = "https://api.weixin.qq.com/wxa/submit_audit";
const AUDIT_URL: &str = "https://open.douyin.com/api/apps/v2/package_version/audit/";

fn materials() -> serde_json::Value {
    serde_json::json!([{ "address": "pages/index/index", "tag": "教育" }])
}

/// 测试用短退避（逻辑与默认策略相同，只是不等真秒）。
fn fast_retry(max_attempts: u32) -> RetryPolicy {
    RetryPolicy {
        max_attempts,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(2),
    }
}

fn request(root: &TempRoot, credential: CredentialRef, retry: RetryPolicy) -> PublishRequest {
    PublishRequest::new(APPID, VERSION, DESC, root.dist(), credential)
        .with_materials(materials())
        .with_poll(PollPolicy {
            interval: Duration::ZERO,
            timeout: Duration::from_secs(30),
        })
        .with_retry(retry)
}

fn wechat_credential(
    name: &str,
) -> (
    CredentialRef,
    Vec<Box<dyn polypack_publish::CredentialProvider>>,
) {
    common::credential(name, common::wechat_payload(APPID, &common::hex32()))
}

fn token_response() -> HttpResponse {
    common::json_ok(serde_json::json!({"access_token": TOKEN, "expires_in": 7200}))
}

fn receipt_key_of(root: &TempRoot) -> String {
    let digest = Package::load(&root.dist())
        .expect("读代码包")
        .digest()
        .to_string();
    receipt_key(APPID, VERSION, &digest)
}

// ---------------------------------------------------------------- mock 服务器

/// 脚本动作：正常响应 / 网络超时（未受理）/ 平台已受理但响应丢失。
enum Step {
    /// 返回脚本响应。
    Reply(HttpResponse),
    /// 传输层超时，平台未受理。
    Timeout,
    /// 平台已受理，但响应在网络上丢失。
    AcceptThenTimeout,
}

/// 故障注入 mock 服务器：按序吐响应，并像真实平台一样按幂等键去重提审。
struct FaultServer {
    /// 待执行动作。
    steps: Mutex<VecDeque<Step>>,
    /// 已收请求。
    requests: Mutex<Vec<Recorded>>,
    /// 已受理的幂等键。
    accepted: Mutex<HashSet<String>>,
    /// 平台侧实际提交记录（每次真实受理一条）。
    submissions: Mutex<Vec<String>>,
    /// 命中幂等去重的提审次数。
    duplicates: AtomicUsize,
}

impl FaultServer {
    fn new(steps: Vec<Step>) -> Arc<Self> {
        Arc::new(Self {
            steps: Mutex::new(steps.into()),
            requests: Mutex::new(Vec::new()),
            accepted: Mutex::new(HashSet::new()),
            submissions: Mutex::new(Vec::new()),
            duplicates: AtomicUsize::new(0),
        })
    }

    fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().expect("记录锁").clone()
    }

    fn submissions(&self) -> usize {
        self.submissions.lock().expect("提交记录锁").len()
    }

    fn duplicates(&self) -> usize {
        self.duplicates.load(Ordering::SeqCst)
    }

    fn next_step(&self) -> Step {
        self.steps
            .lock()
            .expect("脚本锁")
            .pop_front()
            .expect("FaultServer 脚本用尽（请求序与测试预期不符）")
    }
}

/// 提审幂等键（缺头 = 客户端违约，测试直接失败）。
fn idempotency_key(request: &HttpRequest) -> String {
    request
        .headers
        .iter()
        .find(|(name, _)| {
            name.eq_ignore_ascii_case(polypack_publish::transport::IDEMPOTENCY_HEADER)
        })
        .map(|(_, value)| value.clone())
        .expect("提审请求必须带 Idempotency-Key")
}

fn endpoint_of(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

fn timeout(endpoint: &str) -> PublishError {
    PublishError::Http {
        endpoint: endpoint.to_string(),
        status: None,
        reason: "模拟网络超时".to_string(),
    }
}

impl Transport for FaultServer {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, PublishError> {
        self.requests.lock().expect("记录锁").push(Recorded {
            method: request.method,
            url: request.url.clone(),
            headers: request.headers.clone(),
            body: request.body.clone(),
        });
        let endpoint = endpoint_of(&request.url);
        if endpoint != SUBMIT_URL {
            return match self.next_step() {
                Step::Reply(response) => Ok(response),
                Step::Timeout | Step::AcceptThenTimeout => Err(timeout(endpoint)),
            };
        }

        let key = idempotency_key(request);
        if self.accepted.lock().expect("受理集锁").contains(&key) {
            // 平台原生去重：已有审核中的提审单（微信 85009），不新增提交记录。
            self.duplicates.fetch_add(1, Ordering::SeqCst);
            return Ok(common::json_ok(serde_json::json!({
                "errcode": 85009,
                "errmsg": "already auditing",
            })));
        }
        match self.next_step() {
            Step::Reply(response) => Ok(response),
            Step::Timeout => Err(timeout(endpoint)),
            Step::AcceptThenTimeout => {
                self.accepted.lock().expect("受理集锁").insert(key.clone());
                self.submissions.lock().expect("提交记录锁").push(key);
                Err(timeout(endpoint))
            }
        }
    }
}

// ---------------------------------------------------------------- 验收

/// 5xx（随机串）/ 限流 429（提审）/ 超时（提审，平台已受理）→ 最终成功，
/// 提审请求带同一幂等键，平台侧只有一条提交记录。
#[test]
fn faults_5xx_timeout_and_rate_limit_end_in_one_submission() {
    let root = TempRoot::new("retry-faults");
    let server = FaultServer::new(vec![
        Step::Reply(common::raw(503, "service unavailable")), // 取随机串：5xx
        Step::Reply(common::json_ok(
            serde_json::json!({"errCode": 0, "data": {"randomString": RAND}}),
        )),
        Step::Reply(common::json_ok(serde_json::json!({"errCode": 0}))), // 上传
        Step::Reply(token_response()),
        Step::Reply(common::raw(429, "too many requests")), // 提审：限流
        Step::AcceptThenTimeout,                            // 提审：平台已受理，响应丢失
        // 第三次提审命中幂等键 → 85009（不消费脚本）
        Step::Timeout, // 轮询：网络超时
        Step::Reply(common::json_ok(
            serde_json::json!({"errcode": 0, "status": 2}),
        )),
        Step::Reply(common::json_ok(
            serde_json::json!({"errcode": 0, "status": 0}),
        )),
    ]);
    let (reference, providers) = wechat_credential("wechat-retry");
    let request = request(&root, reference, fast_retry(3));
    let archive = Archive::new(root.path());
    let outcome = publish(
        &WechatPublisher::new(server.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("注入 5xx / 限流 / 超时后最终成功");

    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(server.submissions(), 1, "平台侧只有一条提交记录");
    assert!(server.duplicates() >= 1, "重发被平台按幂等键去重");
    assert_eq!(
        outcome.receipt.submit.as_ref().expect("受理快照")["errcode"],
        85009,
        "去重响应作为受理证据留档"
    );

    // 三次提审请求（429 / 超时 / 去重）带同一幂等键。
    let submits: Vec<Recorded> = server
        .requests()
        .into_iter()
        .filter(|record| record.endpoint() == SUBMIT_URL)
        .collect();
    assert_eq!(submits.len(), 3, "提审重发三次（限流 + 超时 + 去重命中）");
    let keys: HashSet<&str> = submits
        .iter()
        .map(|record| {
            record
                .header(polypack_publish::transport::IDEMPOTENCY_HEADER)
                .expect("幂等键")
        })
        .collect();
    assert_eq!(keys.len(), 1, "重发共用同一幂等键");

    // 落盘去重状态：Accepted（崩溃窗口后重跑不再提审）。
    let digest = Package::load(&root.dist())
        .expect("读代码包")
        .digest()
        .to_string();
    let journal = archive
        .load_submit(Channel::Wechat.id(), &receipt_key_of(&root))
        .expect("读去重状态")
        .expect("已落盘");
    assert_eq!(journal.status, SubmitStatus::Accepted);
    assert_eq!(
        journal.attempts, 1,
        "一次执行 = 一次提审阶段（重试不重复计数）"
    );
    assert_eq!(
        journal.key,
        submission_key(Channel::Wechat.id(), APPID, VERSION, &digest)
    );
}

/// 不可恢复失败（类目材料被平台拒）→ `Failed{stage, code, hint}`，不重试、不假装受理。
#[test]
fn unrecoverable_submit_failure_is_structured_failed() {
    let root = TempRoot::new("retry-fatal");
    let server = FaultServer::new(vec![
        Step::Reply(common::json_ok(
            serde_json::json!({"errCode": 0, "data": {"randomString": RAND}}),
        )),
        Step::Reply(common::json_ok(serde_json::json!({"errCode": 0}))),
        Step::Reply(token_response()),
        Step::Reply(common::json_ok(serde_json::json!({
            "errcode": 85008,
            "errmsg": "invalid category",
        }))),
    ]);
    let (reference, providers) = wechat_credential("wechat-fatal");
    let request = request(&root, reference, fast_retry(4));
    let archive = Archive::new(root.path());
    let failure = publish_checked(
        &WechatPublisher::new(server.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect_err("平台拒绝不可恢复");

    assert_eq!(failure.stage, Stage::Submit);
    assert_eq!(failure.error.code(), codes::E_PUB_0003);
    let rendered = failure.render();
    assert!(
        rendered.starts_with("Failed{stage=submit, code=polypack::E_PUB_0003, hint="),
        "{rendered}"
    );
    assert!(rendered.contains("类目"), "提示须可执行：{rendered}");
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|record| record.endpoint() == SUBMIT_URL)
            .count(),
        1,
        "业务拒绝不重试"
    );

    // 受理失败：回执停在 uploaded，未落提审快照。
    let receipt = archive
        .load(Channel::Wechat.id(), &receipt_key_of(&root))
        .expect("读回执")
        .expect("回执存在");
    assert_eq!(receipt.state, AuditState::Uploaded);
    assert!(receipt.submit.is_none());
}

/// 重试耗尽：尝试次数 = policy.max_attempts，失败仍带阶段与稳定码。
#[test]
fn retry_exhaustion_reports_stage_and_attempt_count() {
    let root = TempRoot::new("retry-exhaust");
    let server = FaultServer::new(vec![
        Step::Reply(common::raw(503, "down")),
        Step::Reply(common::raw(503, "down")),
    ]);
    let (reference, providers) = wechat_credential("wechat-exhaust");
    let request = request(&root, reference, fast_retry(2));
    let archive = Archive::new(root.path());
    let failure = publish_checked(
        &WechatPublisher::new(server.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect_err("5xx 持续 → 重试耗尽");

    assert_eq!(failure.stage, Stage::Upload);
    assert_eq!(failure.error.code(), codes::E_PUB_0002);
    assert_eq!(server.requests().len(), 2, "max_attempts=2（含首次）");
}

/// 崩溃窗口：平台已受理 + 去重状态 Accepted、回执仍 uploaded → 重跑只轮询，不再提审。
#[test]
fn accepted_journal_prevents_resubmission_after_crash_window() {
    let root = TempRoot::new("retry-journal");
    let archive = Archive::new(root.path());
    let digest = Package::load(&root.dist())
        .expect("读代码包")
        .digest()
        .to_string();
    let key = receipt_key(APPID, VERSION, &digest);
    let idempotency = submission_key(Channel::Wechat.id(), APPID, VERSION, &digest);

    let mut receipt = Receipt::new(Channel::Wechat.id(), APPID, VERSION, &digest);
    receipt.upload = Some(serde_json::json!({"errCode": 0}));
    archive.save(&receipt).expect("预置 uploaded 回执");
    let mut journal = SubmitJournal::new(&idempotency);
    journal.status = SubmitStatus::Accepted;
    journal.attempts = 1;
    archive
        .save_submit(Channel::Wechat.id(), &key, &journal)
        .expect("预置已受理状态");

    let server = FaultServer::new(vec![
        Step::Reply(token_response()),
        Step::Reply(common::json_ok(
            serde_json::json!({"errcode": 0, "status": 0}),
        )),
    ]);
    let (reference, providers) = wechat_credential("wechat-journal");
    let request = request(&root, reference, fast_retry(3));
    let outcome = publish(
        &WechatPublisher::new(server.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("从去重状态续走");

    assert_eq!(outcome.resumed_from, Some(AuditState::Uploaded));
    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert!(
        !server
            .requests()
            .iter()
            .any(|record| record.endpoint() == SUBMIT_URL),
        "已受理状态存在时绝不重发提审"
    );
    assert_eq!(server.submissions(), 0, "无第二次提交记录");
}

/// 平台侧限流码（抖音 11301）走同一重试层，且重发带同一幂等键。
#[test]
fn platform_rate_limit_code_is_retried_with_stable_idempotency_key() {
    let root = TempRoot::new("retry-douyin");
    let fake = FakeTransport::new(vec![
        common::json_ok(serde_json::json!({"err_no": 0})), // 上传
        common::json_ok(serde_json::json!({
            "err_no": 0,
            "data": {"host_names": ["https://app.example.invalid"]},
        })),
        common::json_ok(serde_json::json!({"err_no": 11301, "err_msg": "too frequent"})),
        common::json_ok(serde_json::json!({
            "err_no": 0,
            "data": {"host_names": ["https://app.example.invalid"]},
        })),
        common::json_ok(serde_json::json!({"err_no": 0})),
        common::json_ok(serde_json::json!({"err_no": 0, "data": {"audit": {"status": 0}}})),
        common::json_ok(serde_json::json!({"err_no": 0, "data": {"audit": {"status": 1}}})),
    ]);
    let (reference, providers) =
        common::credential("douyin-retry", common::douyin_payload("token-retry-abcdef"));
    let request = PublishRequest::new("app-1", VERSION, "限流重试", root.dist(), reference)
        .with_materials(serde_json::json!({"template_id": 123}))
        .with_poll(PollPolicy {
            interval: Duration::ZERO,
            timeout: Duration::from_secs(30),
        })
        .with_retry(fast_retry(3));
    let archive = Archive::new(root.path());
    let outcome = publish(
        &polypack_publish::DouyinPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("11301 按可恢复码重试");

    assert_eq!(outcome.receipt.state, AuditState::Approved);
    let audits: Vec<Recorded> = fake
        .requests()
        .into_iter()
        .filter(|record| record.endpoint() == AUDIT_URL)
        .collect();
    assert_eq!(audits.len(), 2, "限流后重发一次提审");
    let keys: HashSet<&str> = audits
        .iter()
        .map(|record| {
            record
                .header(polypack_publish::transport::IDEMPOTENCY_HEADER)
                .expect("幂等键")
        })
        .collect();
    assert_eq!(keys.len(), 1, "限流重发共用同一幂等键");
}
