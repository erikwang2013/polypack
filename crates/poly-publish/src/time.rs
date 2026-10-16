//! 最小时间工具：回执一律用 unix 秒——格式固定、无时区歧义、排序即时间序。

use std::time::{SystemTime, UNIX_EPOCH};

/// 当前 unix 秒（回执 `at_unix` / `first_unix` / `last_unix`）。
pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
