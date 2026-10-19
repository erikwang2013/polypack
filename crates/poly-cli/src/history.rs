//! `poly history`：构建历史的追加、轮转与按目标聚合（M9-E6 / T-M9-16/17）。
//!
//! 落盘位置固定为 `<工程根>/.polypack/history/events.jsonl`，一行一条
//! [`HistoryRecord`]（schema 定义在 `poly-core`，GUI 侧将来共用同一份契约）。
//! 追加走 `O_APPEND` 单次写（无锁，并发构建不会撕裂行内容）；超限轮转采用
//! 「读全量 → 保留最新 → 临时文件 + rename」，保留最新记录，轮转失败不影响构建。

use std::collections::BTreeMap;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use poly_core::Diagnostic;
use poly_core::events::{HistoryRecord, HistoryStatus};

/// 历史文件体积上限（5 MiB）；超限轮转并保留最新记录。
pub const MAX_HISTORY_BYTES: u64 = 5 * 1024 * 1024;

/// 历史文件记录数上限（10000 条）；超限轮转并保留最新记录。
pub const MAX_HISTORY_RECORDS: usize = 10_000;

/// 历史文件相对工程根的位置（提示与测试断言共用）。
pub const HISTORY_RELATIVE_PATH: &str = ".polypack/history/events.jsonl";

/// `poly history` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// 只看指定目标。
    #[arg(long = "target", value_name = "ID")]
    pub target: Option<String>,
    /// 每个目标返回的最近记录数。
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

/// 历史文件绝对路径：`<project_root>/.polypack/history/events.jsonl`。
pub fn history_file(project_root: &Path) -> PathBuf {
    project_root
        .join(".polypack")
        .join("history")
        .join("events.jsonl")
}

/// 追加一批记录（同一次构建的目标记录一次写入），随后按体积 / 条数轮转。
///
/// 并发语义为 best-effort：`O_APPEND` + 单次 `write_all` 保证整行写入不互相撕裂；
/// 轮转使用临时文件 + rename，轮转窗口内其他进程的追加可能丢失，但不会产生
/// 半行 / 交错行。历史缺失或无法写入由调用方决定是否降级为告警。
pub fn append_history(project_root: &Path, records: &[HistoryRecord]) -> std::io::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    let path = history_file(project_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut buffer = String::new();
    for record in records {
        let line = serde_json::to_string(record).map_err(std::io::Error::other)?;
        buffer.push_str(&line);
        buffer.push('\n');
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(buffer.as_bytes())?;
    drop(file);
    rotate_history_file(&path, MAX_HISTORY_BYTES, MAX_HISTORY_RECORDS)?;
    Ok(())
}

/// 读取全部历史记录（按文件顺序）。
///
/// 文件不存在 = 空历史（不是错误）；并发追加可能留下半行，无法解析的行直接跳过。
pub fn load_history(project_root: &Path) -> std::io::Result<Vec<HistoryRecord>> {
    let path = history_file(project_root);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    Ok(text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

/// 超限轮转：保留最新的 `max_records` 条，并从最旧开始丢弃直到体积不超过
/// `max_bytes`；单条最新记录即使超过体积上限也保留。
///
/// 返回是否发生了轮转。文件不存在视为无需轮转。
pub fn rotate_history_file(
    path: &Path,
    max_bytes: u64,
    max_records: usize,
) -> std::io::Result<bool> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    let text = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let keep = max_records.max(1);
    if metadata.len() <= max_bytes && lines.len() <= keep {
        return Ok(false);
    }

    let mut first = lines.len().saturating_sub(keep);
    let mut bytes: u64 = lines[first..]
        .iter()
        .map(|line| line.len() as u64 + 1)
        .sum();
    while first + 1 < lines.len() && bytes > max_bytes {
        bytes -= lines[first].len() as u64 + 1;
        first += 1;
    }

    let mut rotated = String::with_capacity(bytes.min(usize::MAX as u64) as usize);
    for line in &lines[first..] {
        rotated.push_str(line);
        rotated.push('\n');
    }
    let temp = temp_path(path);
    std::fs::write(&temp, rotated.as_bytes())?;
    replace_file(&temp, path)?;
    Ok(true)
}

/// 临时文件名：同一进程内唯一，避免轮转与读取互相看到半成品。
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp-{}", std::process::id()));
    path.with_file_name(name)
}

/// 用临时文件替换历史文件；Windows 的 rename 不覆盖已有文件，先删目标。
fn replace_file(temp: &Path, path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    match std::fs::rename(temp, path) {
        Ok(()) => Ok(()),
        Err(err) => {
            let _ = std::fs::remove_file(temp);
            Err(err)
        }
    }
}

/// 执行 `poly history`；空历史返回 0（不是错误）。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let project = crate::project::load(&args.config)?;
    let records = load_history(&project.root).map_err(|err| {
        crate::project::io_error("读取构建历史", &history_file(&project.root), err)
    })?;
    let groups = latest_per_target(records, args.target.as_deref(), args.limit);

    if json {
        for (_, records) in &groups {
            for record in records {
                println!(
                    "{}",
                    serde_json::to_string(record).expect("历史记录可序列化")
                );
            }
        }
    } else {
        print!("{}", render_table(&groups, args.target.as_deref()));
    }
    Ok(0)
}

/// 按目标分组，组内按时间倒序（同刻按 build_id 倒序）取最近 `limit` 条。
///
/// 返回按目标 id 字典序排列的分组，保证 `--json` 输出顺序确定。
fn latest_per_target(
    records: impl IntoIterator<Item = HistoryRecord>,
    target: Option<&str>,
    limit: usize,
) -> Vec<(String, Vec<HistoryRecord>)> {
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
        .map(|(target, mut records)| {
            records.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| b.build_id.cmp(&a.build_id)));
            records.truncate(limit);
            (target, records)
        })
        .collect()
}

/// 人读表格：TARGET / STATUS / TIME / DURATION / ARTIFACT / ERROR。
fn render_table(groups: &[(String, Vec<HistoryRecord>)], target: Option<&str>) -> String {
    if groups.is_empty() {
        return match target {
            Some(id) => format!("目标 {id} 暂无构建历史\n"),
            None => {
                format!("暂无构建历史（先运行 poly build；历史文件：{HISTORY_RELATIVE_PATH}）\n")
            }
        };
    }
    let headers = [
        "TARGET".to_string(),
        "STATUS".to_string(),
        "TIME".to_string(),
        "DURATION".to_string(),
        "ARTIFACT".to_string(),
        "ERROR".to_string(),
    ];
    let rows: Vec<[String; 6]> = groups
        .iter()
        .flat_map(|(_, records)| records)
        .map(|record| {
            [
                record.target.clone(),
                status_name(record.status).to_string(),
                format_ts(record.ts),
                format_duration(record.duration_ms),
                record.artifact.clone().unwrap_or_else(|| "-".to_string()),
                record.error_code.clone().unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect();

    let mut widths = [0usize; 6];
    for row in std::iter::once(&headers).chain(rows.iter()) {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }

    let mut out = String::new();
    out.push_str(&format_row(&headers, &widths));
    for row in &rows {
        out.push_str(&format_row(row, &widths));
    }
    out
}

/// 一行表格：列间两空格，末列不留尾随空白。
fn format_row(cells: &[String; 6], widths: &[usize; 6]) -> String {
    let mut line = String::new();
    for (index, cell) in cells.iter().enumerate() {
        if index + 1 == cells.len() {
            line.push_str(cell);
        } else {
            line.push_str(&format!("{cell:<width$}  ", width = widths[index]));
        }
    }
    line.push('\n');
    line
}

/// 状态列文本。
fn status_name(status: HistoryStatus) -> &'static str {
    match status {
        HistoryStatus::Success => "success",
        HistoryStatus::Warning => "warning",
        HistoryStatus::Failed => "failed",
    }
}

/// 耗时列文本：`<1000ms` 原样毫秒，否则秒（两位小数）。
fn format_duration(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else {
        format!("{:.2}s", ms as f64 / 1_000.0)
    }
}

/// Unix 毫秒时间戳 → `YYYY-MM-DDTHH:MM:SSZ`（UTC，无时区依赖）。
fn format_ts(ts_ms: u64) -> String {
    let secs = (ts_ms / 1_000) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        (rem % 3_600) / 60,
        rem % 60
    )
}

/// 公历日期（Howard Hinnant 的 civil_from_days）：1970-01-01 起的天数 → (年, 月, 日)。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

/// 当前 Unix 毫秒时间戳（时钟回拨时按 0 处理）。
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "polypack-history-{tag}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("临时目录可创建");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn record(index: u64) -> HistoryRecord {
        HistoryRecord::build(
            format!("build-{index}"),
            "linux",
            HistoryStatus::Success,
            index,
            index,
        )
    }

    /// 空历史不是错误；追加后按文件顺序读回。
    #[test]
    fn empty_history_loads_as_empty_and_append_roundtrips() {
        let temp = TempDir::new("roundtrip");
        assert!(load_history(&temp.path).expect("空历史可读").is_empty());

        let records: Vec<HistoryRecord> = (1..=3).map(record).collect();
        append_history(&temp.path, &records).expect("追加成功");
        assert_eq!(load_history(&temp.path).expect("读取成功"), records);
    }

    /// 条数上限轮转保留最新；体积上限同样从最旧开始丢，最新一条必留。
    #[test]
    fn rotation_keeps_newest_records_by_count_and_size() {
        let temp = TempDir::new("rotation");
        let records: Vec<HistoryRecord> = (0..5).map(record).collect();
        append_history(&temp.path, &records).expect("追加成功");
        let path = history_file(&temp.path);

        assert!(rotate_history_file(&path, u64::MAX, 3).expect("按条数轮转"));
        let loaded = load_history(&temp.path).expect("读取成功");
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].build_id, "build-2");
        assert_eq!(loaded[2].build_id, "build-4");

        assert!(rotate_history_file(&path, 1, 100).expect("按体积轮转"));
        let loaded = load_history(&temp.path).expect("读取成功");
        assert_eq!(loaded.len(), 1, "最新一条即使超过体积上限也保留");
        assert_eq!(loaded[0].build_id, "build-4");
    }

    /// 体积预算：保留记录的总字节数不超过上限（轮转后）。
    #[test]
    fn rotation_respects_byte_budget_from_the_newest_end() {
        let temp = TempDir::new("bytes");
        let records: Vec<HistoryRecord> = (0..5).map(record).collect();
        append_history(&temp.path, &records).expect("追加成功");
        let path = history_file(&temp.path);
        let line = serde_json::to_string(&records[4]).expect("可序列化").len() as u64 + 1;

        assert!(rotate_history_file(&path, line * 2 + 5, 100).expect("按体积轮转"));
        let loaded = load_history(&temp.path).expect("读取成功");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].build_id, "build-3");
        assert_eq!(loaded[1].build_id, "build-4");
        let size = std::fs::metadata(&path).expect("文件存在").len();
        assert!(size <= line * 2 + 5, "轮转后体积 {size} 超限");
    }

    /// 聚合按目标分组，组内时间倒序并截断到 limit；`--target` 过滤。
    #[test]
    fn aggregation_groups_by_target_and_takes_latest() {
        let records = vec![
            HistoryRecord::build("b1", "linux", HistoryStatus::Success, 100, 1),
            HistoryRecord::build("b2", "linux", HistoryStatus::Failed, 300, 2),
            HistoryRecord::build("b3", "web", HistoryStatus::Warning, 200, 3),
            HistoryRecord::build("b4", "linux", HistoryStatus::Success, 200, 4),
        ];

        let groups = latest_per_target(records.clone(), None, 2);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "linux");
        let linux: Vec<&str> = groups[0].1.iter().map(|r| r.build_id.as_str()).collect();
        assert_eq!(linux, ["b2", "b4"]);
        assert_eq!(groups[1].0, "web");
        assert_eq!(groups[1].1.len(), 1);

        let only = latest_per_target(records, Some("web"), 20);
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].0, "web");
    }

    /// 人读表格：表头、状态、产物、错误码与 UTC 时间。
    #[test]
    fn table_renders_statuses_and_utc_timestamps() {
        let record = HistoryRecord::build(
            "build-1",
            "linux",
            HistoryStatus::Failed,
            1_700_000_000_000,
            1_250,
        )
        .with_artifact(
            Path::new("/tmp/project"),
            Path::new("/tmp/project/.polypack/packages/linux/app.AppImage"),
            Some("sha256:00".to_string()),
        )
        .with_error_code("poly::E_COMPILE_0001");
        let table = render_table(&[("linux".to_string(), vec![record])], None);
        assert!(table.contains("TARGET"), "{table}");
        assert!(table.contains("linux"), "{table}");
        assert!(table.contains("failed"), "{table}");
        assert!(
            table.contains(".polypack/packages/linux/app.AppImage"),
            "{table}"
        );
        assert!(table.contains("poly::E_COMPILE_0001"), "{table}");
        assert!(table.contains("1.25s"), "{table}");
        assert!(table.contains("2023-11-14T22:13:20Z"), "{table}");

        let empty = render_table(&[], None);
        assert!(empty.contains("暂无构建历史"), "{empty}");
    }

    /// 时间格式化边界：epoch 与毫秒截断。
    #[test]
    fn timestamp_formatting_is_utc() {
        assert_eq!(format_ts(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_ts(86_400_000), "1970-01-02T00:00:00Z");
        assert_eq!(format_ts(1_700_000_000_999), "2023-11-14T22:13:20Z");
    }
}
