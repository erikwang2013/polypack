//! ADR 0004 索引 / 吊销快照的语义校验、新鲜度与 rfc3339 时间处理。

use std::collections::BTreeSet;

use polypack_core::codes;
use polypack_core::diagnostics::Diagnostic;

use super::schema::{
    INDEX_SCHEMA, IndexDocument, MAX_REVOCATION_AGE_SECS, PluginEntry, REVOCATIONS_SCHEMA,
    Revocation, RevocationsDocument, index_error,
};

/// 全量 schema 校验（签名验证之后调用）。
pub fn validate_index(index: &IndexDocument) -> Result<(), Diagnostic> {
    if index.schema != INDEX_SCHEMA {
        return Err(index_error(format!(
            "不支持的索引 schema：{:?}（期望 {INDEX_SCHEMA}）",
            index.schema
        )));
    }
    if index.sequence == 0 {
        return Err(index_error("索引 sequence 必须 >= 1"));
    }
    for (field, value) in [
        ("generated_at", &index.generated_at),
        ("expires_at", &index.expires_at),
    ] {
        if parse_rfc3339_utc(value).is_none() {
            return Err(index_error(format!(
                "索引 {field} 不是 RFC 3339 UTC（YYYY-MM-DDTHH:MM:SSZ）：{value:?}"
            )));
        }
    }
    if let Some(digest) = &index.previous_digest
        && !valid_blake3(digest)
    {
        return Err(index_error(format!(
            "previous_digest 不是 blake3-<64 hex>：{digest:?}"
        )));
    }
    if !valid_blake3(&index.revocations_digest) {
        return Err(index_error(format!(
            "revocations_digest 不是 blake3-<64 hex>：{:?}",
            index.revocations_digest
        )));
    }
    if normalize_fingerprint(&index.signing_key_id).is_none() {
        return Err(index_error(format!(
            "signing_key_id 不是 GPG 指纹：{:?}",
            index.signing_key_id
        )));
    }
    let mut seen = BTreeSet::new();
    for entry in &index.plugins {
        validate_entry(entry)?;
        if !seen.insert((entry.id.as_str(), entry.version.as_str())) {
            return Err(index_error(format!(
                "索引中 id + version 重复：{}@{}",
                entry.id, entry.version
            )));
        }
    }
    Ok(())
}

/// 单条插件条目的 schema 校验。
fn validate_entry(entry: &PluginEntry) -> Result<(), Diagnostic> {
    if !valid_plugin_id(&entry.id) {
        return Err(index_error(format!("插件 id 非法：{:?}", entry.id)));
    }
    if entry.name.trim().is_empty() {
        return Err(index_error(format!("插件 {} 的 name 为空", entry.id)));
    }
    if semver::Version::parse(&entry.version).is_err() {
        return Err(index_error(format!(
            "插件 {} 的版本不是 SemVer：{:?}",
            entry.id, entry.version
        )));
    }
    if entry.wit_version.is_empty() {
        return Err(index_error(format!(
            "插件 {} 的 wit_version 为空",
            entry.id
        )));
    }
    if semver::Version::parse(&entry.min_host_version).is_err() {
        return Err(index_error(format!(
            "插件 {} 的 min_host_version 不是 SemVer：{:?}",
            entry.id, entry.min_host_version
        )));
    }
    if parse_rfc3339_utc(&entry.published_at).is_none() {
        return Err(index_error(format!(
            "插件 {} 的 published_at 不是 RFC 3339 UTC：{:?}",
            entry.id, entry.published_at
        )));
    }
    if !valid_artifact_url(&entry.artifact.url) {
        return Err(index_error(format!(
            "插件 {} 的 artifact.url 必须是 https:// 或 file:：{:?}",
            entry.id, entry.artifact.url
        )));
    }
    if !valid_blake3(&entry.artifact.sri) {
        return Err(index_error(format!(
            "插件 {} 的 artifact.sri 不是 blake3-<64 hex>：{:?}",
            entry.id, entry.artifact.sri
        )));
    }
    if entry.artifact.size == 0 {
        return Err(index_error(format!(
            "插件 {} 的 artifact.size 为 0",
            entry.id
        )));
    }
    if normalize_fingerprint(&entry.publisher.key_id).is_none() {
        return Err(index_error(format!(
            "插件 {} 的 publisher.key_id 不是 GPG 指纹：{:?}",
            entry.id, entry.publisher.key_id
        )));
    }
    if !valid_artifact_url(&entry.signature.url) {
        return Err(index_error(format!(
            "插件 {} 的 signature.url 必须是 https:// 或 file:：{:?}",
            entry.id, entry.signature.url
        )));
    }
    match entry.status.as_str() {
        "active" => {}
        "yanked" => {
            if entry
                .yank_reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
            {
                return Err(index_error(format!(
                    "插件 {}@{} 标记 yanked 但缺少 yank_reason",
                    entry.id, entry.version
                )));
            }
        }
        other => {
            return Err(index_error(format!(
                "插件 {}@{} 的 status 非法：{other:?}",
                entry.id, entry.version
            )));
        }
    }
    Ok(())
}

/// 吊销快照全量校验。
pub fn validate_revocations(document: &RevocationsDocument) -> Result<(), Diagnostic> {
    if document.schema != REVOCATIONS_SCHEMA {
        return Err(index_error(format!(
            "不支持的吊销 schema：{:?}（期望 {REVOCATIONS_SCHEMA}）",
            document.schema
        )));
    }
    if document.sequence == 0 {
        return Err(index_error("吊销快照 sequence 必须 >= 1"));
    }
    if parse_rfc3339_utc(&document.issued_at).is_none() {
        return Err(index_error(format!(
            "吊销快照 issued_at 不是 RFC 3339 UTC：{:?}",
            document.issued_at
        )));
    }
    for revocation in &document.revocations {
        validate_revocation(revocation)?;
    }
    Ok(())
}

fn validate_revocation(revocation: &Revocation) -> Result<(), Diagnostic> {
    if parse_rfc3339_utc(&revocation.revoked_at).is_none() {
        return Err(index_error(format!(
            "吊销 revoked_at 不是 RFC 3339 UTC：{:?}",
            revocation.revoked_at
        )));
    }
    if !matches!(
        revocation.reason.as_str(),
        "compromised" | "malicious" | "withdrawn" | "superseded"
    ) {
        return Err(index_error(format!(
            "吊销 reason 非法：{:?}",
            revocation.reason
        )));
    }
    let has_id = revocation.id.is_some();
    let has_version = revocation.version.is_some();
    let has_fingerprint = revocation.fingerprint.is_some();
    match revocation.kind.as_str() {
        "version" => {
            let Some(id) = &revocation.id else {
                return Err(index_error("version 吊销缺少 id"));
            };
            let Some(version) = &revocation.version else {
                return Err(index_error("version 吊销缺少 version"));
            };
            if !valid_plugin_id(id) {
                return Err(index_error(format!("吊销 id 非法：{id:?}")));
            }
            if semver::Version::parse(version).is_err() {
                return Err(index_error(format!(
                    "吊销 version 不是 SemVer：{version:?}"
                )));
            }
            if has_fingerprint {
                return Err(index_error("version 吊销不接受 fingerprint 字段"));
            }
        }
        "plugin" => {
            let Some(id) = &revocation.id else {
                return Err(index_error("plugin 吊销缺少 id"));
            };
            if !valid_plugin_id(id) {
                return Err(index_error(format!("吊销 id 非法：{id:?}")));
            }
            if has_version || has_fingerprint {
                return Err(index_error("plugin 吊销不接受 version / fingerprint 字段"));
            }
        }
        "key" => {
            let Some(fingerprint) = &revocation.fingerprint else {
                return Err(index_error("key 吊销缺少 fingerprint"));
            };
            if normalize_fingerprint(fingerprint).is_none() {
                return Err(index_error(format!(
                    "key 吊销 fingerprint 不是 GPG 指纹：{fingerprint:?}"
                )));
            }
            if has_id || has_version {
                return Err(index_error("key 吊销不接受 id / version 字段"));
            }
        }
        other => {
            return Err(index_error(format!("吊销 kind 非法：{other:?}")));
        }
    }
    if let Some(replacement) = &revocation.replacement
        && semver::Version::parse(replacement).is_err()
    {
        return Err(index_error(format!(
            "吊销 replacement 不是 SemVer：{replacement:?}"
        )));
    }
    Ok(())
}

/// 索引过期与吊销快照新鲜度（ADR 0004 §7 / §8）。
pub fn check_freshness(
    index: &IndexDocument,
    revocations: &RevocationsDocument,
    now: i64,
) -> Result<(), Diagnostic> {
    let expires = parse_rfc3339_utc(&index.expires_at)
        .ok_or_else(|| index_error("索引 expires_at 不是 RFC 3339 UTC"))?;
    if now >= expires {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0012,
            format!("插件索引已过期（expires_at {}）", index.expires_at),
        )
        .with_hint("刷新索引 / 镜像后再安装；过期索引只允许查看已安装插件"));
    }
    let issued = parse_rfc3339_utc(&revocations.issued_at)
        .ok_or_else(|| index_error("吊销快照 issued_at 不是 RFC 3339 UTC"))?;
    if now - issued > MAX_REVOCATION_AGE_SECS {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0012,
            format!(
                "吊销快照超过 30 天未刷新（issued_at {}）",
                revocations.issued_at
            ),
        )
        .with_hint("获取最新的签名吊销快照后再安装"));
    }
    Ok(())
}

/// 命中任一吊销条件时返回该条记录。
pub fn matching_revocation<'a>(
    entry: &PluginEntry,
    revocations: &'a RevocationsDocument,
) -> Option<&'a Revocation> {
    let publisher = normalize_fingerprint(&entry.publisher.key_id);
    revocations
        .revocations
        .iter()
        .find(|revocation| match revocation.kind.as_str() {
            "version" => {
                revocation.id.as_deref() == Some(entry.id.as_str())
                    && revocation.version.as_deref() == Some(entry.version.as_str())
            }
            "plugin" => revocation.id.as_deref() == Some(entry.id.as_str()),
            "key" => {
                revocation
                    .fingerprint
                    .as_deref()
                    .and_then(normalize_fingerprint)
                    == publisher
            }
            _ => false,
        })
}

/// 小写反向域名插件 id（至少两段；官方 `dev.polypack.*` 同样满足）。
pub fn valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.split('.').count() >= 2
        && id.split('.').all(|segment| {
            !segment.is_empty()
                && segment.len() <= 63
                && !segment.starts_with('-')
                && !segment.ends_with('-')
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

/// GPG 指纹规范化：40 / 64 hex → 大写；非法返回 None。
pub fn normalize_fingerprint(value: &str) -> Option<String> {
    let value = value.trim();
    (matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_uppercase())
}

/// `blake3-<64 hex>`（大小写 hex 均可）。
pub fn valid_blake3(value: &str) -> bool {
    value
        .strip_prefix("blake3-")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// 离线实现接受的 artifact / signature URL。
pub fn valid_artifact_url(value: &str) -> bool {
    value.starts_with("https://") || value.starts_with("file:")
}

/// 严格 RFC 3339 UTC（`YYYY-MM-DDTHH:MM:SSZ`）→ unix 秒。
pub fn parse_rfc3339_utc(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return None;
    }
    let digit = |range: std::ops::Range<usize>| -> Option<i64> {
        let slice = value.get(range)?;
        slice
            .bytes()
            .all(|byte| byte.is_ascii_digit())
            .then_some(())?;
        slice.parse().ok()
    };
    let year = digit(0..4)?;
    let month = digit(5..7)?;
    let day = digit(8..10)?;
    let hour = digit(11..13)?;
    let minute = digit(14..16)?;
    let second = digit(17..19)?;
    if !(1..=12).contains(&month)
        || !(1..=days_in_month(year, month)).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// 当前 unix 秒；系统时钟早于 epoch 时取 0。
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 自 1970-01-01 起的天数（Howard Hinnant `days_from_civil`）。
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_round_trips_known_epochs() {
        assert_eq!(parse_rfc3339_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339_utc("2026-10-20T12:34:56Z"),
            Some(1_792_499_696)
        );
        assert_eq!(
            parse_rfc3339_utc("2024-02-29T00:00:00Z"),
            Some(1_709_164_800)
        );
        assert_eq!(parse_rfc3339_utc("2023-02-29T00:00:00Z"), None);
        assert_eq!(parse_rfc3339_utc("2026-10-20T12:34:56+08:00"), None);
        assert_eq!(parse_rfc3339_utc("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn ids_and_fingerprints_follow_adr_shape() {
        assert!(valid_plugin_id("dev.example.pocket-notes"));
        assert!(valid_plugin_id("dev.polypack.notes"));
        for bad in ["", "notes", "Dev.Example", ".dev.x", "dev..x", "dev.x-"] {
            assert!(!valid_plugin_id(bad), "应拒绝 {bad:?}");
        }
        let fingerprint = "0123456789ABCDEF0123456789ABCDEF01234567";
        assert_eq!(
            normalize_fingerprint(fingerprint),
            Some(fingerprint.to_string())
        );
        assert_eq!(
            normalize_fingerprint(&fingerprint.to_ascii_lowercase()),
            Some(fingerprint.to_string())
        );
        assert_eq!(normalize_fingerprint("xyz"), None);
    }

    #[test]
    fn all_issuers_of_a_publisher_key_are_revoked() {
        let revocations: RevocationsDocument = serde_json::from_value(serde_json::json!({
            "schema": REVOCATIONS_SCHEMA,
            "sequence": 1,
            "issued_at": "2026-10-20T00:00:00Z",
            "revocations": [{
                "kind": "key",
                "fingerprint": "0123456789abcdef0123456789abcdef01234567",
                "reason": "compromised",
                "revoked_at": "2026-10-20T00:00:00Z"
            }]
        }))
        .expect("吊销快照可解析");
        validate_revocations(&revocations).expect("吊销快照合法");
        let entry: PluginEntry = serde_json::from_value(serde_json::json!({
            "id": "dev.example.notes",
            "name": "Notes",
            "summary": "",
            "version": "1.0.0",
            "wit_version": crate::plugin::index::schema::SUPPORTED_WIT,
            "min_host_version": "1.0.0",
            "published_at": "2026-10-20T00:00:00Z",
            "artifact": {"url": "file:notes.wasm", "sri": format!("blake3-{}", "a".repeat(64)), "size": 1},
            "publisher": {"key_id": "0123456789ABCDEF0123456789ABCDEF01234567", "display_name": "Example"},
            "signature": {"url": "file:notes.wasm.asc"},
            "status": "active"
        }))
        .expect("条目可解析");
        assert!(matching_revocation(&entry, &revocations).is_some());
    }
}
