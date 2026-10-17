//! `poly release` 的报告输出（JSON Lines 与人读同源）。

use super::{Artifact, Outcome, Prepared};
use crate::project;

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
