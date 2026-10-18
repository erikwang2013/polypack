//! AWS Signature Version 4（S3 口径）：HMAC-SHA256 签名链、规范请求与日期换算。
//!
//! 只覆盖本 crate 用到的固定形态：service = `s3`、单次请求、无分块载荷
//! （`x-amz-content-sha256` = 载荷 SHA-256）。规范 URI / query 由本模块的编码函数
//! 生成，签名与实发请求因此逐字节一致。日期换算手写（Howard Hinnant 的
//! civil-from-days / days-from-civil），不引入 chrono。

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use sha2::{Digest as _, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// 空载荷 SHA-256（GET / HEAD / DELETE 的 `x-amz-content-sha256`）。
pub(crate) const EMPTY_SHA256: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// 规范 URI 编码集：保留 `/`（按段编码），编码 RFC 3986 unreserved 之外的全部保留字符。
const URI_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b',')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// query 组件编码集：在 URI 集基础上再编码 `/`（空格编码为 `%20`，不是 `+`）。
const QUERY_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// 规范 URI：逐段百分号编码，`/` 分隔符原样保留。
pub(crate) fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|segment| utf8_percent_encode(segment, URI_SET).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// 规范 query：编码后按（名称, 值）字典序排序，`k=v` 以 `&` 连接。
pub(crate) fn canonical_query(pairs: &[(&str, &str)]) -> String {
    let mut encoded: Vec<(String, String)> = pairs
        .iter()
        .map(|(key, value)| {
            (
                utf8_percent_encode(key, QUERY_SET).to_string(),
                utf8_percent_encode(value, QUERY_SET).to_string(),
            )
        })
        .collect();
    encoded.sort();
    encoded
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// `SystemTime` → `YYYYMMDDTHHMMSSZ`（UTC；S3 的 `x-amz-date` 格式）。
pub(crate) fn amz_date(time: SystemTime) -> String {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// 距离 UNIX 纪元的天数 → (年, 月, 日)；对 1970 前后均正确。
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

/// (年, 月, 日) → 距离 UNIX 纪元的天数。
pub(crate) fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400; // [0, 399]
    let mp = if month > 2 {
        month as i64 - 3
    } else {
        month as i64 + 9
    }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + day as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// 生成 `Authorization` 头值。
///
/// `headers` 为实际随请求发送的签名头（host / 全部 `x-amz-*` / 可选
/// `x-oss-s3-compat`）；大小写不敏感，函数内按小写排序并拼 `SignedHeaders`。
#[allow(clippy::too_many_arguments)] // 签名输入天然多；拆结构只会多一层搬运
pub(crate) fn sign(
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    headers: &[(String, String)],
    payload_hash: &str,
    amz_date: &str,
    region: &str,
    access_key: &str,
    secret_key: &str,
) -> String {
    let mut headers: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| {
            (
                name.to_ascii_lowercase(),
                value.split_whitespace().collect::<Vec<_>>().join(" "),
            )
        })
        .collect();
    headers.sort_by(|left, right| left.0.cmp(&right.0));
    let canonical_headers: String = headers
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect();
    let signed_headers = headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );
    let date = amz_date.split('T').next().unwrap_or(amz_date);
    let scope = format!("{date}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical_request.as_bytes()))
    );
    let mut key = hmac(format!("AWS4{secret_key}").as_bytes(), date.as_bytes());
    key = hmac(&key, region.as_bytes());
    key = hmac(&key, b"s3");
    key = hmac(&key, b"aws4_request");
    let signature = hex(&hmac(&key, string_to_sign.as_bytes()));
    format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    )
}

/// HMAC-SHA256；`Hmac::new_from_slice` 接受任意长度密钥。
fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// 小写十六进制。
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// AWS 公开测试向量（S3 GET Object；示例密钥本身就是公开文档值）。
    #[test]
    fn aws_published_get_object_vector() {
        let headers = vec![
            (
                "Host".to_string(),
                "examplebucket.s3.amazonaws.com".to_string(),
            ),
            ("Range".to_string(), "bytes=0-9".to_string()),
            ("x-amz-content-sha256".to_string(), EMPTY_SHA256.to_string()),
            ("x-amz-date".to_string(), "20130524T000000Z".to_string()),
        ];
        let authorization = sign(
            "GET",
            "/test.txt",
            "",
            &headers,
            EMPTY_SHA256,
            "20130524T000000Z",
            "us-east-1",
            "AKIAIOSFODNN7EXAMPLE", // EXAMPLE_NOT_A_REAL_KEY（AWS 官方文档示例）
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY", // EXAMPLE_NOT_A_REAL_KEY（AWS 官方文档示例）
        );
        assert!(
            authorization.contains("SignedHeaders=host;range;x-amz-content-sha256;x-amz-date"),
            "{authorization}"
        );
        assert!(
            authorization.ends_with(
                "Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
            ),
            "{authorization}"
        );
    }

    #[test]
    fn civil_dates_roundtrip_across_leap_day() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(days_from_civil(2000, 3, 1)), (2000, 3, 1));
        assert_eq!(
            civil_from_days(days_from_civil(1969, 12, 31)),
            (1969, 12, 31)
        );
    }

    #[test]
    fn amz_date_formats_utc() {
        let time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(amz_date(time), "20231114T221320Z");
    }

    #[test]
    fn uri_and_query_encoding_is_canonical() {
        assert_eq!(encode_path("/a b/c+d"), "/a%20b/c%2Bd");
        assert_eq!(encode_path("/hex/ab/cdef"), "/hex/ab/cdef");
        assert_eq!(
            canonical_query(&[("prefix", "a/b c"), ("list-type", "2")]),
            "list-type=2&prefix=a%2Fb%20c"
        );
        assert_eq!(canonical_query(&[]), "");
    }
}
