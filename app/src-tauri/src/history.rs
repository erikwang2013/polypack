//! Read-only build-history aggregation for the GUI (T-M9-18).
//!
//! The GUI reuses [`poly_core::events::HistoryRecord`] and
//! [`poly_core::events::redact_path`], so the JSONL contract and privacy rules
//! stay identical to `poly history`. Nothing is written back to disk.
//!
//! Aggregation is intentionally the same shape as `poly history`: group by
//! target, sort newest first (timestamp, then build id), keep `limit` records
//! per target. The matrix row is the first record of each group.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use poly_core::events::{redact_path, HistoryRecord, REDACTED_PATH};
use serde::Serialize;

/// History file location relative to the project root (shared with the CLI).
pub const HISTORY_RELATIVE_PATH: &str = ".polypack/history/events.jsonl";

/// Default number of records kept per target.
pub const DEFAULT_LIMIT: usize = 20;

/// Upper bound accepted from the UI, to keep the IPC payload small.
pub const MAX_LIMIT: usize = 200;

/// Aggregated history for one target.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TargetHistory {
    /// Target id.
    pub target: String,
    /// Most recent record; this is the matrix row.
    pub latest: HistoryRecord,
    /// Newest-first records for the detail view.
    pub records: Vec<HistoryRecord>,
}

/// Everything the frontend needs for one render.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryView {
    /// History file path relative to the project root.
    pub source: String,
    /// Targets in lexicographic order, so renders are deterministic.
    pub targets: Vec<TargetHistory>,
    /// Non-empty JSONL lines that could not be parsed.
    pub skipped_lines: usize,
}

/// Absolute path of the history file for `root`.
pub fn history_file(root: &Path) -> PathBuf {
    root.join(HISTORY_RELATIVE_PATH)
}

/// Read and redact all parseable records; a missing file means empty history.
fn read_records(root: &Path) -> std::io::Result<(Vec<HistoryRecord>, usize)> {
    let text = match std::fs::read_to_string(history_file(root)) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(err) => return Err(err),
    };
    let mut records = Vec::new();
    let mut skipped_lines = 0;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<HistoryRecord>(line) {
            Ok(record) => records.push(redact_record(root, record)),
            Err(_) => skipped_lines += 1,
        }
    }
    Ok((records, skipped_lines))
}

/// Defensive read-time redaction for legacy or hand-edited records.
///
/// The CLI already redacts before writing, but the GUI must not display an
/// absolute host path even if one appears in the file.
fn redact_record(root: &Path, mut record: HistoryRecord) -> HistoryRecord {
    if let Some(artifact) = record.artifact.take() {
        record.artifact = Some(if artifact == REDACTED_PATH {
            artifact
        } else {
            redact_path(root, Path::new(&artifact))
        });
    }
    record
}

/// Group records by target and keep the newest `limit` per target.
///
/// `limit` is clamped to `1..=MAX_LIMIT`; target order is lexicographic.
pub fn aggregate(
    records: Vec<HistoryRecord>,
    target: Option<&str>,
    limit: usize,
) -> Vec<TargetHistory> {
    let limit = limit.clamp(1, MAX_LIMIT);
    let mut grouped: BTreeMap<String, Vec<HistoryRecord>> = BTreeMap::new();
    for record in records {
        if target.is_some_and(|wanted| record.target != wanted) {
            continue;
        }
        grouped
            .entry(record.target.clone())
            .or_default()
            .push(record);
    }
    grouped
        .into_iter()
        .filter_map(|(target, mut records)| {
            records.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| b.build_id.cmp(&a.build_id)));
            records.truncate(limit);
            let latest = records.first()?.clone();
            Some(TargetHistory {
                target,
                latest,
                records,
            })
        })
        .collect()
}

/// Build a view from `root` without touching the filesystem beyond that root.
pub fn read_view(root: &Path, target: Option<&str>, limit: usize) -> Result<HistoryView, String> {
    if !root.is_dir() {
        return Err(format!("project root not found: {}", root.display()));
    }
    let root = root
        .canonicalize()
        .map_err(|err| format!("cannot resolve {}: {err}", root.display()))?;
    let (records, skipped_lines) = read_records(&root)
        .map_err(|err| format!("cannot read {}: {err}", history_file(&root).display()))?;
    Ok(HistoryView {
        source: HISTORY_RELATIVE_PATH.to_string(),
        targets: aggregate(records, target, limit),
        skipped_lines,
    })
}

/// Tauri command: read, redact, and aggregate history for the matrix/detail UI.
///
/// An empty `root` means the process working directory. Missing history is an
/// empty view (not an error), matching `poly history`.
#[tauri::command]
pub fn history_records(
    root: Option<String>,
    target: Option<String>,
    limit: Option<usize>,
) -> Result<HistoryView, String> {
    let root = match root
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        Some(value) => PathBuf::from(value),
        None => std::env::current_dir()
            .map_err(|err| format!("cannot determine current directory: {err}"))?,
    };
    let target = target
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    read_view(&root, target.as_deref(), limit.unwrap_or(DEFAULT_LIMIT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use poly_core::events::HistoryStatus;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempRoot {
        path: PathBuf,
    }

    impl TempRoot {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "polypack-gui-{tag}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("temp root created");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn write(&self, lines: &[String]) {
            let file = history_file(&self.path);
            std::fs::create_dir_all(file.parent().expect("history parent")).expect("history dir");
            std::fs::write(file, format!("{}\n", lines.join("\n"))).expect("history written");
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn record(
        build_id: &str,
        target: &str,
        status: HistoryStatus,
        ts: u64,
        duration_ms: u64,
    ) -> HistoryRecord {
        HistoryRecord::build(build_id, target, status, ts, duration_ms)
    }

    fn line(record: &HistoryRecord) -> String {
        serde_json::to_string(record).expect("record serializes")
    }

    /// Missing file = empty view (not an error), same as `poly history`.
    #[test]
    fn empty_history_yields_empty_view() {
        let temp = TempRoot::new("empty");
        let view = read_view(temp.path(), None, DEFAULT_LIMIT).expect("empty view");
        assert!(view.targets.is_empty());
        assert_eq!(view.skipped_lines, 0);
        assert_eq!(view.source, HISTORY_RELATIVE_PATH);
    }

    /// Matrix uses the latest record; details honor the per-target limit.
    #[test]
    fn aggregates_multiple_targets_and_keeps_latest_per_target() {
        let temp = TempRoot::new("matrix");
        let records = [
            record("b1", "linux", HistoryStatus::Success, 100, 10),
            record("b2", "linux", HistoryStatus::Failed, 300, 20),
            record("b3", "web", HistoryStatus::Warning, 200, 30),
            record("b4", "linux", HistoryStatus::Success, 200, 40),
            record("b5", "web", HistoryStatus::Success, 400, 50),
        ];
        temp.write(&records.iter().map(line).collect::<Vec<_>>());

        let view = read_view(temp.path(), None, 2).expect("view");
        let targets: Vec<&str> = view
            .targets
            .iter()
            .map(|item| item.target.as_str())
            .collect();
        assert_eq!(targets, ["linux", "web"]);

        let linux = &view.targets[0];
        assert_eq!(linux.latest.build_id, "b2");
        assert_eq!(linux.latest.status, HistoryStatus::Failed);
        assert_eq!(linux.latest.error_code, None);
        let ids: Vec<&str> = linux.records.iter().map(|r| r.build_id.as_str()).collect();
        assert_eq!(ids, ["b2", "b4"], "newest first, limited to 2");

        let web = &view.targets[1];
        assert_eq!(web.latest.build_id, "b5");
        assert_eq!(web.records.len(), 2);

        let only = read_view(temp.path(), Some("web"), DEFAULT_LIMIT).expect("filtered view");
        assert_eq!(only.targets.len(), 1);
        assert_eq!(only.targets[0].target, "web");
    }

    /// Absolute in-root paths become relative; outside/traversal paths are redacted.
    #[test]
    fn artifacts_are_redacted_on_read() {
        let temp = TempRoot::new("redaction");
        let inside = temp.path().join(".polypack/packages/linux/app.AppImage");
        let outside = std::env::temp_dir()
            .join(format!("polypack-gui-outside-{}", std::process::id()))
            .join("app.bin");

        let mut absolute_inside = record("b1", "linux", HistoryStatus::Success, 100, 10);
        absolute_inside.artifact = Some(inside.to_string_lossy().into_owned());
        absolute_inside.artifact_digest = Some("sha256:abc".to_string());
        absolute_inside.error_code = Some("poly::E_COMPILE_0001".to_string());

        let mut absolute_outside = record("b2", "web", HistoryStatus::Failed, 200, 20);
        absolute_outside.artifact = Some(outside.to_string_lossy().into_owned());

        let mut traversal = record("b3", "tizen", HistoryStatus::Warning, 300, 30);
        traversal.artifact = Some("../escape/app.wgt".to_string());

        let mut placeholder = record("b4", "android", HistoryStatus::Success, 400, 40);
        placeholder.artifact = Some(REDACTED_PATH.to_string());

        temp.write(&[
            line(&absolute_inside),
            line(&absolute_outside),
            line(&traversal),
            line(&placeholder),
        ]);

        let view = read_view(temp.path(), None, DEFAULT_LIMIT).expect("view");
        let artifact = |target: &str| {
            view.targets
                .iter()
                .find(|item| item.target == target)
                .and_then(|item| item.latest.artifact.clone())
                .expect("artifact present")
        };

        assert_eq!(artifact("linux"), ".polypack/packages/linux/app.AppImage");
        assert_eq!(artifact("web"), REDACTED_PATH);
        assert_eq!(artifact("tizen"), REDACTED_PATH);
        assert_eq!(artifact("android"), REDACTED_PATH);

        let linux = view
            .targets
            .iter()
            .find(|item| item.target == "linux")
            .expect("linux target");
        assert_eq!(linux.latest.artifact_digest.as_deref(), Some("sha256:abc"));
        assert_eq!(
            linux.latest.error_code.as_deref(),
            Some("poly::E_COMPILE_0001")
        );

        let json = serde_json::to_string(&view).expect("view serializes");
        assert!(
            !json.contains(&temp.path().to_string_lossy().into_owned()),
            "project root leaked into view: {json}"
        );
    }

    /// Hand-edited or torn JSONL lines are skipped and counted, not fatal.
    #[test]
    fn malformed_lines_are_skipped_and_counted() {
        let temp = TempRoot::new("malformed");
        let good = line(&record("b1", "linux", HistoryStatus::Success, 100, 10));
        temp.write(&[
            good,
            "not json".to_string(),
            "{\"schema_version\":".to_string(),
        ]);

        let view = read_view(temp.path(), None, DEFAULT_LIMIT).expect("view");
        assert_eq!(view.targets.len(), 1);
        assert_eq!(view.skipped_lines, 2);
    }

    /// The Tauri command accepts an explicit root, target filter, and limit.
    #[test]
    fn command_accepts_explicit_root_filter_and_limit() {
        let temp = TempRoot::new("command");
        let records = [
            record("b1", "web", HistoryStatus::Warning, 100, 10),
            record("b2", "web", HistoryStatus::Success, 300, 20),
            record("b3", "linux", HistoryStatus::Success, 200, 30),
        ];
        temp.write(&records.iter().map(line).collect::<Vec<_>>());

        let view = history_records(
            Some(temp.path().display().to_string()),
            Some("web".to_string()),
            Some(1),
        )
        .expect("command view");
        assert_eq!(view.targets.len(), 1);
        assert_eq!(view.targets[0].target, "web");
        assert_eq!(view.targets[0].records.len(), 1);
        assert_eq!(view.targets[0].latest.build_id, "b2");

        let missing = temp.path().join("missing");
        assert!(history_records(Some(missing.display().to_string()), None, None).is_err());
    }

    /// Static structure check: matrix, detail, and empty-state markers exist.
    #[test]
    fn frontend_declares_matrix_detail_and_empty_state() {
        let web = Path::new(env!("CARGO_MANIFEST_DIR")).join("../web");
        let html = std::fs::read_to_string(web.join("index.html")).expect("index.html");
        let js = std::fs::read_to_string(web.join("app.js")).expect("app.js");

        for id in ["matrix-view", "detail-view", "empty-state"] {
            assert!(
                html.contains(&format!("id=\"{id}\"")),
                "index.html missing #{id}"
            );
        }
        for marker in [
            "setAttribute(\"data-testid\", \"target-row\")",
            "setAttribute(\"data-testid\", \"detail-record\")",
            "history_records",
        ] {
            assert!(js.contains(marker), "app.js missing {marker}");
        }
    }
}
