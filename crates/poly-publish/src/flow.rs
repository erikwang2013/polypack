//! 共享发布驱动（T-M3-09 / T-M3-10）：三渠道零分叉的
//! 「建包 → 查回执 → 上传 / 提审 / 轮询（断点续走）→ 归档」。
//!
//! 幂等 / 去重口径：
//! - [`Package`]（目录 / 单文件两种载具，摘要口径见其文档）给**容器无关**的 blake3 内容摘要：
//!   目录载具 / 单文件载具（如快应用 `.rpk`）两种形态同走本驱动——载具只定身份，与各平台
//!   上传容器（微信 B 协议 / zip）无关，换容器不改摘要；
//! - 回执键 = 渠道 + appid + 版本 + 摘要前 16 hex，重跑同键：
//!   终态 → no-op（不触网，只加 `runs`）；`uploaded` → 只提审；`submitted` / `in_review`
//!   / `delayed` → 只轮询；
//! - 提审边界是 at-least-once：平台受理后本地落盘前崩溃，重跑会重发提审——平台以自有
//!   错误码点破（如微信 85009「已有审核中版本」），客户端给修复提示，不假装成功。
//!
//! R4：自动化止步提审；轮询超预算返回 [`PublishError::PollTimeout`]（回执已带状态流水
//! 落盘，重跑只续轮询）。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use poly_core::CredentialRef;

use crate::archive::{Archive, AuditState, PollEntry, Receipt, receipt_key};
use crate::error::PublishError;
use crate::package::Package;
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
/// 渠道取自 `publisher.channel()`（单一事实源）；`providers` 为凭据后端链（引用模型）。
pub fn publish(
    publisher: &dyn Publisher,
    request: &PublishRequest,
    providers: &[Box<dyn CredentialProvider>],
    archive: &Archive,
) -> Result<PublishOutcome, PublishError> {
    let channel = publisher.channel().id();
    let package = Package::load(&request.package)?;
    let key = receipt_key(&request.appid, &request.version, package.digest());

    let (mut receipt, resumed_from) = match archive.load(channel, &key)? {
        Some(mut receipt) if receipt.state.is_terminal() => {
            // 终态重跑 = no-op：不触网，只如实记一次执行。
            receipt.runs += 1;
            receipt.last_unix = unix_now();
            let state = receipt.state;
            let path = archive.save(&receipt)?;
            return Ok(PublishOutcome {
                receipt,
                path,
                resumed_from: Some(state),
            });
        }
        Some(mut receipt) => {
            receipt.runs += 1;
            receipt.last_unix = unix_now();
            let state = receipt.state;
            (receipt, Some(state))
        }
        None => (
            Receipt::new(channel, &request.appid, &request.version, package.digest()),
            None,
        ),
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
        receipt.upload = Some(publisher.upload(&job)?.raw);
        receipt.state = AuditState::Uploaded;
        receipt.last_unix = unix_now();
        archive.save(&receipt)?;
    }

    // 就绪等待（默认 no-op，幂等可重入：上传后与续走时都会经过）。
    publisher.await_ready(&job)?;

    // 阶段 2：提审（全新，或停在 uploaded）。
    if matches!(resumed_from, None | Some(AuditState::Uploaded)) {
        receipt.submit = Some(publisher.submit(&job)?.raw);
        receipt.state = AuditState::Submitted;
        receipt.last_unix = unix_now();
        archive.save(&receipt)?;
    }

    // 阶段 3：轮询（到终态，或预算耗尽——每轮流水都先落回执再判断）。
    let started = Instant::now();
    loop {
        let outcome = publisher.poll(&job)?;
        receipt.state = outcome.state;
        receipt.last_unix = unix_now();
        receipt.polls.push(PollEntry {
            at_unix: receipt.last_unix,
            state: outcome.state,
            reason: outcome.reason,
            raw: outcome.raw,
        });
        let path = archive.save(&receipt)?;
        if outcome.state.is_terminal() {
            return Ok(PublishOutcome {
                receipt,
                path,
                resumed_from,
            });
        }
        if started.elapsed() >= request.poll.timeout {
            return Err(PublishError::PollTimeout {
                channel,
                last: outcome.state.to_string(),
            });
        }
        std::thread::sleep(request.poll.interval);
    }
}
