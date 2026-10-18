//! 共享发布驱动（T-M3-09 / T-M3-10 / T-M8-10）：三渠道零分叉的
//! 「建包 → 查回执 → 上传 / 提审 / 轮询（断点续走）→ 归档」。
//!
//! 幂等 / 去重口径：
//! - [`Package`]（目录 / 单文件两种载具，摘要口径见其文档）给**容器无关**的 blake3 内容摘要：
//!   目录载具 / 单文件载具（如快应用 `.rpk`）两种形态同走本驱动——载具只定身份，与各平台
//!   上传容器（微信 B 协议 / zip）无关，换容器不改摘要；
//! - 回执键 = 渠道 + appid + 版本 + 摘要前 16 hex，重跑同键：
//!   终态 → no-op（不触网，只加 `runs`）；`uploaded` → 只提审；`submitted` / `in_review`
//!   / `delayed` → 只轮询；
//! - 提审另有一份落盘幂等状态（[`SubmitJournal`] + 请求头 `Idempotency-Key`）：发送前先
//!   记意图，受理后记 `Accepted`；平台受理后本地落盘前崩溃，重跑看到 `Accepted` 不再重发。
//!   平台侧另有原生去重码（微信 85009 / 抖音 11303 / 支付宝 CAN_NOT_SUBMIT_WITH_AUDIT*），
//!   客户端按「已受理」续走——重复提审不产生第二次提交记录；
//! - 每个阶段按 [`RetryPolicy`] 做指数退避重试（5xx / 超时 / 429 / 平台限流码）。
//!
//! R4：自动化止步提审；轮询超预算返回 [`PublishError::PollTimeout`]（回执已带状态流水
//! 落盘，重跑只续轮询）。不可恢复失败经 [`PublishFailure`] 给出 `Failed{stage, code, hint}`。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use poly_core::CredentialRef;

use crate::archive::{
    Archive, AuditState, PollEntry, Receipt, SubmitJournal, SubmitStatus, receipt_key,
    submission_key,
};
use crate::error::PublishError;
use crate::package::Package;
use crate::retry::{self, PublishFailure, RetryPolicy, Stage};
use crate::secret::CredentialProvider;
use crate::time::unix_now;
use crate::{Job, Publisher};

/// 轮询策略（默认 60s × 15min；重跑只续轮询、不重复提审）。
///
/// 亦为平台**构建等待**的预算来源（如支付宝 `build.query` 轮询——客户端实现内消费
/// [`Job::poll`]），口径统一：等待耗尽 → [`PublishError::PollTimeout`]。
#[derive(Debug, Clone)]
pub struct PollPolicy {
    /// 两次查询间隔。
    pub interval: Duration,
    /// 轮询总预算；耗尽 → [`PublishError::PollTimeout`]（回执已落盘）。
    pub timeout: Duration,
}

impl Default for PollPolicy {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            timeout: Duration::from_secs(900),
        }
    }
}

/// 一次发布请求（字段齐备即复现；凭据只给引用，值经后端链现取）。
pub struct PublishRequest {
    /// 平台应用 id。
    pub appid: String,
    /// 版本号（平台侧唯一性由平台保证）。
    pub version: String,
    /// 版本描述（上传 / 提审都会带上）。
    pub description: String,
    /// 代码包载具路径：目录（T-M3-06 产物，如 `<…>/miniapp/dist`）或单文件
    /// （如快应用 `.rpk`）——形态由 [`Package::load`] 自动判别。
    pub package: PathBuf,
    /// 凭据引用（T-M2-11 引用模型）。
    pub credential: CredentialRef,
    /// 提审材料（平台各自解释：微信 = `item_list` 数组（类目；真提审必填，且须已在账号
    /// 后台配好）；无 → 客户端按平台要求报缺）。
    pub materials: Option<serde_json::Value>,
    /// 轮询策略。
    pub poll: PollPolicy,
    /// 重试策略（指数退避；只作用于可恢复错误，见 [`PublishError::is_retryable`]）。
    pub retry: RetryPolicy,
}

impl PublishRequest {
    /// 最小构造（materials = None、轮询用默认策略）。
    pub fn new(
        appid: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
        package: impl Into<PathBuf>,
        credential: CredentialRef,
    ) -> Self {
        Self {
            appid: appid.into(),
            version: version.into(),
            description: description.into(),
            package: package.into(),
            credential,
            materials: None,
            poll: PollPolicy::default(),
            retry: RetryPolicy::default(),
        }
    }

    /// 带提审材料。
    pub fn with_materials(mut self, materials: serde_json::Value) -> Self {
        self.materials = Some(materials);
        self
    }

    /// 换轮询策略。
    pub fn with_poll(mut self, poll: PollPolicy) -> Self {
        self.poll = poll;
        self
    }

    /// 换重试策略。
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }
}

/// 发布结果（回执已脱敏，可安全 Debug）。
#[derive(Debug)]
pub struct PublishOutcome {
    /// 最终回执（已落盘）。
    pub receipt: Receipt,
    /// 回执路径。
    pub path: PathBuf,
    /// 续走起点：`None` = 全新执行；`Some(终态)` = no-op 重跑；`Some(其余)` = 从该状态续走。
    pub resumed_from: Option<AuditState>,
}

/// 共享驱动：建包 → 查回执 → 上传 / 提审 / 轮询（续走）→ 归档。
///
/// 与 [`publish_checked`] 同一条路径，只是丢弃阶段信息（既有调用方兼容）。
pub fn publish(
    publisher: &dyn Publisher,
    request: &PublishRequest,
    providers: &[Box<dyn CredentialProvider>],
    archive: &Archive,
) -> Result<PublishOutcome, PublishError> {
    publish_checked(publisher, request, providers, archive).map_err(|failure| failure.error)
}

/// 共享驱动（带阶段失败信息）：建包 → 查回执 → 上传 / 提审 / 轮询（续走）→ 归档。
///
/// 渠道取自 `publisher.channel()`（单一事实源）；`providers` 为凭据后端链（引用模型）。
/// 每个阶段包一层指数退避重试（[`PublishRequest::retry`]），失败归因到 [`Stage`]；
/// 提审另经落盘幂等状态去重（[`SubmitJournal`]），重跑不产生第二次提交。
pub fn publish_checked(
    publisher: &dyn Publisher,
    request: &PublishRequest,
    providers: &[Box<dyn CredentialProvider>],
    archive: &Archive,
) -> Result<PublishOutcome, PublishFailure> {
    let channel = publisher.channel().id();
    let package = Package::load(&request.package).map_err(|error| fail(Stage::Package, error))?;
    let key = receipt_key(&request.appid, &request.version, package.digest());

    let (mut receipt, resumed_from) = match archive.load(channel, &key) {
        Ok(Some(mut receipt)) if receipt.state.is_terminal() => {
            // 终态重跑 = no-op：不触网，只如实记一次执行。
            receipt.runs += 1;
            receipt.last_unix = unix_now();
            let state = receipt.state;
            let path = archive
                .save(&receipt)
                .map_err(|error| fail(Stage::Receipt, error))?;
            return Ok(PublishOutcome {
                receipt,
                path,
                resumed_from: Some(state),
            });
        }
        Ok(Some(mut receipt)) => {
            receipt.runs += 1;
            receipt.last_unix = unix_now();
            let state = receipt.state;
            (receipt, Some(state))
        }
        Ok(None) => (
            Receipt::new(channel, &request.appid, &request.version, package.digest()),
            None,
        ),
        Err(error) => return Err(fail(Stage::Receipt, error)),
    };

    let job = Job {
        appid: &request.appid,
        version: &request.version,
        description: &request.description,
        materials: request.materials.as_ref(),
        package: &package,
        credential: &request.credential,
        providers,
        poll: &request.poll,
    };

    // 阶段 1：上传（仅全新执行；已有回执视为已上传）。
    if resumed_from.is_none() {
        let uploaded = retry::attempt(Stage::Upload, &request.retry, || publisher.upload(&job))?;
        receipt.upload = Some(uploaded.raw);
        receipt.state = AuditState::Uploaded;
        receipt.last_unix = unix_now();
        archive
            .save(&receipt)
            .map_err(|error| fail(Stage::Upload, error))?;
    }

    // 就绪等待（默认 no-op，幂等可重入：上传后与续走时都会经过）。
    retry::attempt(Stage::Ready, &request.retry, || publisher.await_ready(&job))?;

    // 阶段 2：提审（全新，或停在 uploaded）；落盘幂等状态防重复提交。
    if matches!(resumed_from, None | Some(AuditState::Uploaded)) {
        submit_idempotent(
            publisher,
            &job,
            &request.retry,
            archive,
            &mut receipt,
            channel,
            &key,
        )?;
    }

    // 阶段 3：轮询（到终态，或预算耗尽——每轮流水都先落回执再判断）。
    let started = Instant::now();
    loop {
        let outcome = retry::attempt(Stage::Poll, &request.retry, || publisher.poll(&job))?;
        receipt.state = outcome.state;
        receipt.last_unix = unix_now();
        // 提交号可能只在轮询响应里（微信 auditid / 抖音 data.audit.audit_id）：首次见到即落档。
        if receipt.submission_id.is_none() {
            receipt.submission_id = submission_id_of(&outcome.raw);
        }
        receipt.polls.push(PollEntry {
            at_unix: receipt.last_unix,
            state: outcome.state,
            reason: outcome.reason,
            raw: outcome.raw,
        });
        if outcome.state.is_terminal() {
            receipt.decided_unix = Some(receipt.last_unix);
        }
        let path = archive
            .save(&receipt)
            .map_err(|error| fail(Stage::Poll, error))?;
        if outcome.state.is_terminal() {
            return Ok(PublishOutcome {
                receipt,
                path,
                resumed_from,
            });
        }
        if started.elapsed() >= request.poll.timeout {
            return Err(fail(
                Stage::Poll,
                PublishError::PollTimeout {
                    channel,
                    last: outcome.state.to_string(),
                },
            ));
        }
        std::thread::sleep(request.poll.interval);
    }
}

/// 提审（幂等）：先落盘意图 → 重发（同幂等键）→ 受理后记 `Accepted`。
///
/// - 幂等键 = 渠道 + appid + 版本 + 内容摘要（跨进程稳定），随请求头下发；
/// - 本地 `Accepted` 存在（平台已受理、回执未落盘的崩溃窗口）→ 不再发提审，直接补记
///   `Submitted` 续轮询；
/// - 失败时尽力记 `Unknown`（传输层结果不明）或保留 `Pending`（确定未受理），重跑同键续走。
fn submit_idempotent(
    publisher: &dyn Publisher,
    job: &Job<'_>,
    policy: &RetryPolicy,
    archive: &Archive,
    receipt: &mut Receipt,
    channel: &'static str,
    key: &str,
) -> Result<(), PublishFailure> {
    let idempotency = submission_key(channel, job.appid, job.version, job.package.digest());
    let mut journal = archive
        .load_submit(channel, key)
        .map_err(|error| fail(Stage::Submit, error))?
        .filter(|journal| journal.key == idempotency)
        .unwrap_or_else(|| SubmitJournal::new(&idempotency));

    if journal.status == SubmitStatus::Accepted {
        // 平台已受理但回执未落盘：不再重发提审，补记 Submitted 后直接续轮询。
        // 提交号 / 受理时刻从去重状态复原（崩溃窗口后的时间线仍可追溯）。
        receipt.state = AuditState::Submitted;
        receipt.last_unix = unix_now();
        if receipt.submission_id.is_none() {
            receipt.submission_id = journal.submission_id.clone();
        }
        if receipt.submitted_unix.is_none() {
            receipt.submitted_unix = journal.submitted_unix;
        }
        archive
            .save(receipt)
            .map_err(|error| fail(Stage::Submit, error))?;
        return Ok(());
    }

    journal.attempts += 1;
    journal.status = SubmitStatus::Pending;
    journal.last_unix = unix_now();
    archive
        .save_submit(channel, key, &journal)
        .map_err(|error| fail(Stage::Submit, error))?;

    match retry::attempt(Stage::Submit, policy, || publisher.submit(job)) {
        Ok(submission) => {
            let now = unix_now();
            journal.status = SubmitStatus::Accepted;
            journal.last_unix = now;
            if journal.submission_id.is_none() {
                journal.submission_id = submission_id_of(&submission.raw);
            }
            journal.submitted_unix.get_or_insert(now);
            archive
                .save_submit(channel, key, &journal)
                .map_err(|error| fail(Stage::Submit, error))?;
            receipt.submit = Some(submission.raw);
            receipt.state = AuditState::Submitted;
            receipt.last_unix = now;
            receipt.submission_id = journal.submission_id.clone();
            receipt.submitted_unix = journal.submitted_unix;
            archive
                .save(receipt)
                .map_err(|error| fail(Stage::Submit, error))?;
            Ok(())
        }
        Err(failure) => {
            // 传输层失败 = 平台是否受理不明；平台业务拒绝 = 确定未受理。
            journal.status = if matches!(failure.error, PublishError::Http { .. }) {
                SubmitStatus::Unknown
            } else {
                SubmitStatus::Pending
            };
            journal.last_unix = unix_now();
            // 原始失败已在案：去重状态写失败不掩盖它。
            let _ = archive.save_submit(channel, key, &journal);
            Err(failure)
        }
    }
}

/// 失败归因。
fn fail(stage: Stage, error: PublishError) -> PublishFailure {
    PublishFailure { stage, error }
}

/// 平台提交号归一：各平台字段名不同（微信 `auditid`、抖音 `data.audit.audit_id` /
/// `data.audit_id`、支付宝 `audit_order_no`）；平台从未给过则 `None`（原始响应仍可追溯）。
fn submission_id_of(raw: &serde_json::Value) -> Option<String> {
    [
        "/auditid",
        "/audit_id",
        "/audit_order_no",
        "/data/audit_id",
        "/data/audit/audit_id",
        "/data/audit_order_no",
    ]
    .iter()
    .find_map(|pointer| raw.pointer(pointer).filter(|value| !value.is_null()))
    .map(|value| match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_id_normalizes_platform_receipt_fields() {
        assert_eq!(
            submission_id_of(&serde_json::json!({"errcode": 0, "auditid": 42})).as_deref(),
            Some("42"),
            "微信提审单号"
        );
        assert_eq!(
            submission_id_of(&serde_json::json!({"data": {"audit": {"audit_id": "dy-1"}}}))
                .as_deref(),
            Some("dy-1"),
            "抖音轮询单号"
        );
        assert_eq!(
            submission_id_of(&serde_json::json!({"data": {"audit_id": "dy-2"}})).as_deref(),
            Some("dy-2"),
            "抖音提审响应单号"
        );
        assert_eq!(
            submission_id_of(&serde_json::json!({"audit_order_no": "ali-1"})).as_deref(),
            Some("ali-1"),
            "支付宝提审单号"
        );
        assert_eq!(
            submission_id_of(&serde_json::json!({"errcode": 0, "errmsg": "ok"})),
            None,
            "平台未给单号 → 缺省，不编造"
        );
    }
}
