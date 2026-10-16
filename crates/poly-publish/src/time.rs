//! 最小时间工具：回执一律用 unix 秒——格式固定、无时区歧义、排序即时间序；
//! 网关类平台（支付宝）要求的 `yyyy-MM-dd HH:mm:ss`（GMT+8）由本模块换算。

use std::time::{SystemTime, UNIX_EPOCH};

/// 当前 unix 秒（回执 `at_unix` / `first_unix` / `last_unix`）。
pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 支付宝网关 `timestamp` 参数（`yyyy-MM-dd HH:mm:ss`，**GMT+8**，官方公共参数口径）。
pub(crate) fn gateway_timestamp(unix_secs: u64) -> String {
    format_gateway_timestamp(unix_secs)
}

/// unix 秒 → GMT+8 墙上时间（Hinnant `civil_from_days`；不引日历库）。
fn format_gateway_timestamp(unix_secs: u64) -> String {
    const GMT8_OFFSET: u64 = 8 * 3600;
    let shifted = unix_secs + GMT8_OFFSET;
    let days = (shifted / 86_400) as i64;
    let secs_of_day = shifted % 86_400;
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    );
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

/// 自 1970-01-01 起的天数 → (年, 月, 日)（Howard Hinnant 的 `civil_from_days` 算法，
/// 对 1970 前后的日期都成立；本 crate 只用到 1970 之后）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_timestamp_is_gmt8_wall_clock() {
        // unix 0 = 1970-01-01 00:00:00 UTC = GMT+8 的 08:00:00。
        assert_eq!(format_gateway_timestamp(0), "1970-01-01 08:00:00");
        // 闰日：2024-02-29 00:00:00 UTC → GMT+8 08:00:00。
        assert_eq!(
            format_gateway_timestamp(1_709_164_800),
            "2024-02-29 08:00:00"
        );
        // 跨日回卷：2026-10-16 23:30:00 UTC → GMT+8 次日 07:30:00。
        assert_eq!(
            format_gateway_timestamp(1_792_193_400),
            "2026-10-17 07:30:00"
        );
        // 年末：2025-12-31 16:00:00 UTC → GMT+8 2026-01-01 00:00:00。
        assert_eq!(
            format_gateway_timestamp(1_767_196_800),
            "2026-01-01 00:00:00"
        );
    }

    #[test]
    fn civil_from_days_matches_unix_epoch_anchors() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }
}
