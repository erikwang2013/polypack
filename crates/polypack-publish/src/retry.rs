//! 发布阶段重试（T-M8-10）：指数退避、可恢复错误分类与结构化失败。
//!
//! 只重试**可恢复**错误（[`PublishError::is_retryable`]）：网络超时 / 连接失败、HTTP
//! 5xx / 429、平台限流 / 系统繁忙码。凭据、材料、平台业务拒绝与磁盘错误一律不重试——
//! 重试改变不了它们，只会拖长失败。
//!
//! shortcut: 退避无抖动（发布是单进程串行；同键多跑本就幂等），需要多客户端错峰时再
//! 加 jitter。

use std::fmt;
use std::time::Duration;

use crate::error::PublishError;

/// 指数退避重试策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    /// 总尝试次数（含首次；< 1 按 1 处理）。
    pub max_attempts: u32,
    /// 第一次重试前的等待。
    pub initial_backoff: Duration,
    /// 单次等待上限。
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(8),
        }
    }
}

impl RetryPolicy {
    /// 第 `retry` 次重试（从 1 起）的等待：`initial * 2^(retry-1)`，封顶 `max_backoff`。
    pub fn delay(&self, retry: u32) -> Duration {
        let exponent = retry.saturating_sub(1).min(16);
        self.initial_backoff
            .saturating_mul(1u32 << exponent)
            .min(self.max_backoff)
    }
}

/// 发布阶段（`Failed{stage, …}` 的 stage 口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// 代码包载具读取。
    Package,
    /// 回执 / 去重状态读写。
    Receipt,
    /// 代码包上传。
    Upload,
    /// 平台侧就绪 / 构建等待。
    Ready,
    /// 提审。
    Submit,
    /// 审核状态轮询。
    Poll,
}

impl Stage {
    /// 稳定文本（错误 / JSON 事件同口径）。
    pub fn label(&self) -> &'static str {
        match self {
            Self::Package => "package",
            Self::Receipt => "receipt",
            Self::Upload => "upload",
            Self::Ready => "ready",
            Self::Submit => "submit",
            Self::Poll => "poll",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// 不可恢复的发布失败：阶段 + 底层稳定错误码 / 修复提示（验收口径
/// `Failed{stage, code, hint}`）。
#[derive(Debug)]
pub struct PublishFailure {
    /// 出事阶段。
    pub stage: Stage,
    /// 底层错误（码 / 提示仍是既有 `polypack::E_PUB_*` 口径，不重新编号）。
    pub error: PublishError,
}

impl PublishFailure {
    /// 稳定错误码（原样透传底层码）。
    pub fn code(&self) -> &'static str {
        self.error.code()
    }

    /// 修复提示（原样透传底层提示）。
    pub fn hint(&self) -> Option<String> {
        self.error.hint()
    }

    /// 验收口径的人读形态：`Failed{stage=…, code=…, hint=…}`。
    pub fn render(&self) -> String {
        format!(
            "Failed{{stage={}, code={}, hint={}}}",
            self.stage,
            self.code(),
            self.hint().unwrap_or_default()
        )
    }
}

impl fmt::Display for PublishFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

impl std::error::Error for PublishFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// 执行一次可重试调用；失败归因到 `stage`。
pub(crate) fn attempt<T>(
    stage: Stage,
    policy: &RetryPolicy,
    mut call: impl FnMut() -> Result<T, PublishError>,
) -> Result<T, PublishFailure> {
    let max_attempts = policy.max_attempts.max(1);
    let mut attempt = 1;
    loop {
        match call() {
            Ok(value) => return Ok(value),
            Err(error) if attempt < max_attempts && error.is_retryable() => {
                std::thread::sleep(policy.delay(attempt));
                attempt += 1;
            }
            Err(error) => return Err(PublishFailure { stage, error }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::PublishError;

    fn http(status: Option<u16>) -> PublishError {
        PublishError::Http {
            endpoint: "https://example.invalid/x".to_string(),
            status,
            reason: "测试".to_string(),
        }
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let policy = RetryPolicy {
            max_attempts: 6,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_millis(500),
        };
        assert_eq!(policy.delay(1), Duration::from_millis(100));
        assert_eq!(policy.delay(2), Duration::from_millis(200));
        assert_eq!(policy.delay(3), Duration::from_millis(400));
        assert_eq!(policy.delay(4), Duration::from_millis(500), "封顶");
        assert_eq!(policy.delay(99), Duration::from_millis(500), "上限恒定");
        assert_eq!(policy.delay(0), Duration::from_millis(100), "下界按 1");
    }

    #[test]
    fn retryable_classification_covers_5xx_timeout_429_and_platform_limits() {
        assert!(http(Some(500)).is_retryable());
        assert!(http(Some(503)).is_retryable());
        assert!(http(Some(429)).is_retryable(), "平台限流");
        assert!(http(None).is_retryable(), "网络超时 / 连接失败");
        assert!(!http(Some(400)).is_retryable());
        assert!(!http(Some(404)).is_retryable());
        let platform = |channel: &'static str, code: &str| PublishError::Platform {
            channel,
            code: code.to_string(),
            message: "测试".to_string(),
            hint: None,
        };
        assert!(platform("wechat-open-platform", "45009").is_retryable());
        assert!(platform("douyin-open-platform", "11301").is_retryable());
        assert!(platform("alipay-open-platform", "SYSTEM_ERROR").is_retryable());
        assert!(!platform("wechat-open-platform", "85008").is_retryable());
        assert!(!platform("douyin-open-platform", "11303").is_retryable());
    }

    #[test]
    fn attempt_stops_after_max_attempts_and_reports_stage() {
        let policy = RetryPolicy {
            max_attempts: 3,
            initial_backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
        };
        let mut calls = 0;
        let failure = attempt(Stage::Upload, &policy, || {
            calls += 1;
            Err::<(), _>(http(Some(503)))
        })
        .expect_err("重试耗尽须失败");
        assert_eq!(calls, 3, "max_attempts 含首次");
        assert_eq!(failure.stage, Stage::Upload);
        assert_eq!(failure.code(), crate::codes::E_PUB_0002);
        assert!(
            failure
                .render()
                .starts_with("Failed{stage=upload, code=polypack::E_PUB_0002")
        );

        // 不可恢复错误只调一次。
        calls = 0;
        let failure = attempt(Stage::Submit, &policy, || {
            calls += 1;
            Err::<(), _>(platform_error())
        })
        .expect_err("业务拒绝不重试");
        assert_eq!(calls, 1);
        assert_eq!(failure.stage, Stage::Submit);
    }

    fn platform_error() -> PublishError {
        PublishError::Platform {
            channel: "wechat-open-platform",
            code: "85008".to_string(),
            message: "类目非法".to_string(),
            hint: Some("核对类目".to_string()),
        }
    }
}
