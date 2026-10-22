//! 车厂 / OEM 渠道占位客户端（T-M6-11）：无统一公开 API，只接线不提交。
//!
//! 每个车厂有自己的凭据体系与上架流程，不存在可复用的公开提交端点。本占位客户端让目标
//! 声明（`TargetSpec.publish = ["oem-channel"]`）/ CLI 派发 / 凭据引用链先接通：四步中任何
//! 一步都返回 [`PublishError::OemCredentialsRequired`]（稳定码 `polypack::E_PUB_0006`），绝不
//! 伪造上传 / 提审 / 审核回执。
//!
//! 凭据只以 [`polypack_core::CredentialRef`] 形式随 [`Job`] 进入（值经后端链现取）；错误文本
//! 只带档案名，不带值。

use crate::error::PublishError;
use crate::{Channel, Job, PollOutcome, Publisher, Submission, Uploaded};

/// 车厂 / OEM 渠道占位客户端。
pub struct OemPublisher;

impl OemPublisher {
    /// 占位客户端对所有步骤的统一拒绝（需 OEM 凭据引用；不提交）。
    fn required(job: &Job<'_>) -> PublishError {
        PublishError::OemCredentialsRequired {
            channel: Channel::Oem.id(),
            credential: job.credential.name.clone(),
        }
    }
}

impl Publisher for OemPublisher {
    fn channel(&self) -> Channel {
        Channel::Oem
    }

    fn upload(&self, job: &Job<'_>) -> Result<Uploaded, PublishError> {
        Err(Self::required(job))
    }

    fn await_ready(&self, job: &Job<'_>) -> Result<(), PublishError> {
        Err(Self::required(job))
    }

    fn submit(&self, job: &Job<'_>) -> Result<Submission, PublishError> {
        Err(Self::required(job))
    }

    fn poll(&self, job: &Job<'_>) -> Result<PollOutcome, PublishError> {
        Err(Self::required(job))
    }
}
