//! `polypack release` 的报告输出与回执存取（JSON Lines 与人读同源）。

use std::path::Path;

use polypack_core::codes;
use polypack_core::diagnostics::Diagnostic;
use serde::{Deserialize, Serialize};

use super::{Artifact, KeySource, Outcome, Prepared, Receipt, SCHEMA_VERSION};
use crate::project;

/// 仓库产物格式（本命令只编排有仓库语义的格式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepoKind {
    /// Debian / Ubuntu 系（`.deb` + Packages / Release / InRelease）。
    Deb,
    /// Fedora / RHEL 系（`.rpm` + repodata）。
    Rpm,
    /// FreeBSD pkg 仓库（`.pkg` + `packagesite`）。
    Pkg,
    /// NetBSD / DragonFly pkgsrc 仓库（`.tgz` + `pkg_summary.gz` + 逐包 GPG 签名）。
    Pkgsrc,
}

impl RepoKind {
    /// 目标声明的产物格式 → 仓库格式；其它格式不归本命令。
    pub(super) fn from_format(format: &str) -> Option<Self> {
        match format {
            "deb" => Some(Self::Deb),
            "rpm" => Some(Self::Rpm),
            "pkg" => Some(Self::Pkg),
            "pkgsrc" => Some(Self::Pkgsrc),
            _ => None,
        }
    }

    /// 产物文件扩展名（也是回执里的稳定文本）。
    pub(super) fn extension(self) -> &'static str {
        match self {
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::Pkg => "pkg",
            Self::Pkgsrc => "tgz",
        }
    }
}

/// dry-run 报告：产物清单、清单摘要、回执锚点与命中预测（不落盘）。
pub(super) fn report_dry_run(
    prepared: &Prepared,
    cache_hit: bool,
    credential_given: bool,
    json: bool,
) {
    project::report(
        "release",
        json,
        serde_json::json!({
            "event": "release-dry-run",
            "target": &prepared.target,
            "version": &prepared.version,
            "repo": prepared.repo.display().to_string(),
            "repo_kind": prepared.kind.extension(),
            "manifest_digest": &prepared.manifest_digest,
            "receipt": prepared.receipt.display().to_string(),
            "artifact_count": prepared.artifacts.len(),
            "artifacts": artifacts_json(&prepared.artifacts),
            "cache_hit": cache_hit,
            "credential_given": credential_given,
            "note": "dry-run：只解析产物与幂等锚点，未写仓库、未落回执",
        }),
    );
}

/// 真腿报告：Published 事件口径 + 回执路径（验收要求的可追溯锚点）。
pub(super) fn report_published(prepared: &Prepared, outcome: &Outcome, json: bool) {
    project::report(
        "release",
        json,
        serde_json::json!({
            "event": "published",
            "target": &prepared.target,
            "version": &prepared.version,
            "repo": prepared.repo.display().to_string(),
            "repo_kind": prepared.kind.extension(),
            "manifest_digest": &prepared.manifest_digest,
            "receipt": prepared.receipt.display().to_string(),
            "artifact_count": prepared.artifacts.len(),
            "runs": outcome.runs,
            "cache_hit": outcome.cache_hit,
            "note": if outcome.cache_hit {
                "产物集合与仓库未变：仓库与回执 no-op（runs+1）"
            } else {
                "仓库元数据已刷新并签名，回执已归档"
            },
        }),
    );
}

/// `--changed` skipped 事件：与 published 严格区分，回执零重写、runs 不递增。
pub(super) fn report_skipped(
    prepared: &Prepared,
    receipt: &Receipt,
    key_source: KeySource,
    json: bool,
) {
    project::report(
        "release",
        json,
        skipped_event(prepared, receipt, key_source),
    );
}

/// skipped 事件的 JSON 契约（单测直接断言，不依赖 stdout 捕获）。
pub(super) fn skipped_event(
    prepared: &Prepared,
    receipt: &Receipt,
    key_source: KeySource,
) -> serde_json::Value {
    let note = match key_source {
        KeySource::CacheKey => {
            "产物集合与仓库未变：--changed 跳过，仓库与回执均未改写（runs 不递增）"
        }
        KeySource::ManifestDigest => {
            "旧回执无 cache_key，按 manifest_digest 降级判定：--changed 跳过，回执未改写"
        }
    };
    serde_json::json!({
        "event": "release-skipped",
        "target": &prepared.target,
        "version": &prepared.version,
        "repo": prepared.repo.display().to_string(),
        "repo_kind": prepared.kind.extension(),
        "manifest_digest": &prepared.manifest_digest,
        "cache_key": &prepared.cache_key,
        "receipt": prepared.receipt.display().to_string(),
        "artifact_count": prepared.artifacts.len(),
        "runs": receipt.runs,
        "key_source": key_source.as_str(),
        "reason": "unchanged",
        "note": note,
    })
}

/// 产物清单的 JSON 形态（不含本地绝对路径）。
fn artifacts_json(artifacts: &[Artifact]) -> Vec<serde_json::Value> {
    artifacts
        .iter()
        .map(|artifact| {
            serde_json::json!({
                "name": &artifact.name,
                "bytes": artifact.bytes,
                "digest": &artifact.digest,
            })
        })
        .collect()
}

/// 回执命中判定：变更键 + 仓库根 + 仓库格式三者一致才 no-op；旧回执无
/// `cache_key` 时降级比较 `manifest_digest`。
pub(super) fn is_noop(receipt: &Receipt, prepared: &Prepared) -> bool {
    let key_matches = match receipt.cache_key.as_deref() {
        Some(key) => key == prepared.cache_key,
        None => receipt.manifest_digest == prepared.manifest_digest,
    };
    key_matches
        && receipt.repo == prepared.repo.display().to_string()
        && receipt.repo_kind == prepared.kind
}

/// 读回执并判命中（dry-run 的命中预测；读失败照常报错）。
pub(super) fn no_op_receipt(prepared: &Prepared) -> Result<Option<Receipt>, Diagnostic> {
    Ok(load_receipt(&prepared.receipt, prepared)?.filter(|receipt| is_noop(receipt, prepared)))
}

/// 读回执：不存在 → `Ok(None)`；形态非法 / 版本陌生 / 身份不符 → `E_RELEASE_0003`。
pub(super) fn load_receipt(
    path: &Path,
    prepared: &Prepared,
) -> Result<Option<Receipt>, Diagnostic> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(receipt_error(path, format!("回执不可读：{err}"))),
    };
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .map_err(|err| receipt_error(path, format!("回执 JSON 非法：{err}")))?;
    if receipt.schema_version != SCHEMA_VERSION {
        return Err(receipt_error(
            path,
            format!(
                "回执 schema 版本 {} 与本期 {SCHEMA_VERSION} 不符（格式固定；迁移后重跑）",
                receipt.schema_version
            ),
        ));
    }
    if receipt.target != prepared.target || receipt.version != prepared.version {
        return Err(receipt_error(
            path,
            format!(
                "回执身份（{} {}）与路径不符",
                receipt.target, receipt.version
            ),
        ));
    }
    Ok(Some(receipt))
}

/// 回执错误统一口径（永久保留，不静默重建）。
fn receipt_error(path: &Path, message: String) -> Diagnostic {
    Diagnostic::error(
        codes::E_RELEASE_0003,
        format!("{}：{message}", path.display()),
    )
    .with_hint("回执永久保留：修复或迁移后重跑；勿手改 schema 版本")
}

/// 原子落盘（同目录临时文件 + rename）。
pub(super) fn save_receipt(path: &Path, receipt: &Receipt) -> Result<(), Diagnostic> {
    let dir = path.parent().expect("回执路径必有父目录");
    std::fs::create_dir_all(dir)
        .map_err(|err| receipt_error(dir, format!("回执目录不可建：{err}")))?;
    let json = serde_json::to_vec_pretty(receipt).expect("回执序列化不会失败");
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|err| receipt_error(&tmp, format!("回执不可写：{err}")))?;
    std::fs::rename(&tmp, path).map_err(|err| receipt_error(path, format!("回执落盘失败：{err}")))
}
