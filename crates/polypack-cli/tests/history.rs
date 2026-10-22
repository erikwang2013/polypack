//! T-M9-16 / T-M9-17 验收（CLI 侧）：构建历史落盘、路径脱敏与 `polypack history` 查询。
//!
//! 覆盖：空历史不报错、`--target` / `--limit` 过滤、JSON Lines 与人读表格、
//! 真实构建后每目标追加一条记录且文件内不含宿主绝对路径、失败构建也留记录。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时工程目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-cli-history-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可创建");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// 运行 `polypack` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_polypack`）。
fn polypack(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polypack"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("polypack 可执行")
}

/// stdout + stderr（失败断言时打印全量现场）。
fn combined(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// JSON Lines 逐行解析（混入非 JSON 行即失败）。
fn parse_lines(text: &str) -> Vec<serde_json::Value> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|err| panic!("JSON Lines 混入非 JSON 行：{line}（{err}）"))
        })
        .collect()
}

/// `--json` 的 stdout 逐行解析。
fn json_lines(output: &Output) -> Vec<serde_json::Value> {
    parse_lines(&String::from_utf8_lossy(&output.stdout))
}

/// 当前宿主可构建的桌面目标（真实构建测试不能硬编码 linux）。
fn host_target() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn init_project(tag: &str) -> TempDir {
    let project = TempDir::new(tag);
    let init = polypack(&["init"], project.path());
    assert!(init.status.success(), "polypack init 失败：{}", combined(&init));
    // Windows 主 rust 作业只预装 NSIS（WiX 在 windows-msi-real 作业），
    // 这里把模板已有的 windows 格式从 msi 换成 nsis，避免真实构建测试因缺
    // candle/light 而误红。不能追加新的 [targets.windows] 表（重复键）。
    if cfg!(target_os = "windows") {
        let config = project.path().join("polypack.toml");
        let text = std::fs::read_to_string(&config).expect("读 polypack.toml");
        let text = text.replace("format = \"msi\"", "format = \"nsis\"");
        std::fs::write(&config, text).expect("写 polypack.toml");
    }
    project
}

fn history_path(root: &Path) -> PathBuf {
    root.join(".polypack").join("history").join("events.jsonl")
}

/// 手写一条历史记录（schema 与 `polypack-core` 的 `HistoryRecord` 一致）。
fn fixture_record(target: &str, ts: u64, status: &str, error: Option<&str>) -> String {
    let mut value = serde_json::json!({
        "schema_version": 1,
        "build_id": format!("build-{ts}"),
        "ts": ts,
        "target": target,
        "event": "build",
        "status": status,
        "duration_ms": 42,
        "artifact": format!(".polypack/packages/{target}/app.bin"),
        "artifact_digest": "sha256:00",
    });
    if let Some(code) = error {
        value["error_code"] = serde_json::Value::String(code.to_string());
    }
    serde_json::to_string(&value).expect("夹具记录可序列化")
}

fn write_history(root: &Path, records: &[String]) {
    let path = history_path(root);
    std::fs::create_dir_all(path.parent().expect("历史目录")).expect("历史目录可建");
    std::fs::write(&path, format!("{}\n", records.join("\n"))).expect("历史夹具可写");
}

// ---------------------------------------------------------------- 契约

/// 空历史不是错误：人读给出引导，`--json` 输出零行。
#[test]
fn empty_history_is_not_an_error() {
    let project = init_project("empty");

    let human = polypack(&["history"], project.path());
    assert!(
        human.status.success(),
        "空历史必须成功：{}",
        combined(&human)
    );
    let text = String::from_utf8_lossy(&human.stdout);
    assert!(text.contains("暂无构建历史"), "{text}");

    let json = polypack(&["history", "--json"], project.path());
    assert!(json.status.success(), "{}", combined(&json));
    assert!(
        String::from_utf8_lossy(&json.stdout).trim().is_empty(),
        "空历史 --json 不输出记录：{}",
        combined(&json)
    );
}

/// `--target` 过滤、`--limit` 每目标截断、JSON Lines 与人读表格。
#[test]
fn history_filters_limits_and_renders_both_channels() {
    let project = init_project("query");
    write_history(
        project.path(),
        &[
            fixture_record("linux", 100, "success", None),
            fixture_record("linux", 300, "failed", Some("polypack::E_COMPILE_0001")),
            fixture_record("web", 200, "warning", None),
            fixture_record("linux", 200, "success", None),
        ],
    );

    let filtered = polypack(
        &["history", "--target", "linux", "--limit", "2", "--json"],
        project.path(),
    );
    assert!(filtered.status.success(), "{}", combined(&filtered));
    let lines = json_lines(&filtered);
    assert_eq!(lines.len(), 2, "每目标最近 2 条");
    assert_eq!(lines[0]["target"], "linux");
    assert_eq!(lines[0]["status"], "failed");
    assert_eq!(lines[0]["ts"], 300, "最新记录在前");
    assert_eq!(lines[0]["error_code"], "polypack::E_COMPILE_0001");
    assert_eq!(lines[1]["ts"], 200);

    let all = polypack(&["history", "--json"], project.path());
    assert_eq!(json_lines(&all).len(), 4, "缺省 limit 20 覆盖全部记录");

    let limited = polypack(&["history", "--limit", "1", "--json"], project.path());
    let lines = json_lines(&limited);
    assert_eq!(lines.len(), 2, "linux 与 web 各 1 条");
    assert_eq!(lines[0]["target"], "linux", "目标按字典序输出");
    assert_eq!(lines[1]["target"], "web");

    let human = polypack(&["history"], project.path());
    assert!(human.status.success(), "{}", combined(&human));
    let text = String::from_utf8_lossy(&human.stdout);
    assert!(text.contains("TARGET"), "{text}");
    assert!(text.contains("STATUS"), "{text}");
    assert!(text.contains("linux"), "{text}");
    assert!(text.contains("failed"), "{text}");
    assert!(text.contains("polypack::E_COMPILE_0001"), "{text}");
    assert!(
        text.contains(".polypack/packages/linux/app.bin"),
        "人读表格显示产物相对路径：{text}"
    );
}

/// 真实构建：每目标追加一条记录；文件内不得出现工程绝对路径。
#[test]
fn build_appends_redacted_history_records() {
    let project = init_project("build");
    let build = polypack(
        &["build", "--json", "--target", host_target()],
        project.path(),
    );
    assert!(
        build.status.success(),
        "polypack build 失败：{}",
        combined(&build)
    );

    let raw = std::fs::read_to_string(history_path(project.path()))
        .expect("polypack build 必须写入 .polypack/history/events.jsonl");
    let root = project.path().to_string_lossy().into_owned();
    assert!(
        !raw.contains(&root),
        "历史文件不得包含宿主绝对路径 {root}：{raw}"
    );

    let records = parse_lines(&raw);
    assert!(!records.is_empty(), "至少一条目标记录：{raw}");
    for record in &records {
        assert_eq!(record["schema_version"], 1, "{record}");
        assert_eq!(record["event"], "build", "{record}");
        assert!(
            record["build_id"]
                .as_str()
                .is_some_and(|id| id.starts_with("build-"))
        );
        assert!(record["ts"].as_u64().is_some_and(|ts| ts > 0), "{record}");
        assert!(record["duration_ms"].as_u64().is_some(), "{record}");
        let status = record["status"].as_str().expect("status 字符串");
        assert!(
            ["success", "warning", "failed"].contains(&status),
            "{record}"
        );
        if let Some(artifact) = record["artifact"].as_str() {
            assert!(!artifact.starts_with('/'), "产物必须是相对路径：{artifact}");
            assert!(!artifact.contains('\\'), "统一使用 / 分隔：{artifact}");
            assert!(artifact.starts_with(".polypack/"), "{artifact}");
        }
    }

    let query = polypack(&["history", "--json"], project.path());
    assert_eq!(
        json_lines(&query).len(),
        records.len(),
        "查询按目标返回与落盘一致的记录数"
    );
}

/// 失败构建也留记录：状态 failed，错误码为稳定 `polypack::E_*`。
#[test]
fn failed_build_is_recorded_with_error_code() {
    let project = init_project("failed");
    std::fs::write(
        project.path().join("core").join("src").join("main.rs"),
        "fn main( {",
    )
    .expect("损坏 core 源码可写");

    let build = polypack(
        &["build", "--json", "--target", host_target()],
        project.path(),
    );
    assert!(
        !build.status.success(),
        "损坏的 core 必须构建失败：{}",
        combined(&build)
    );

    let raw =
        std::fs::read_to_string(history_path(project.path())).expect("失败的构建也必须写历史");
    let records = parse_lines(&raw);
    assert!(!records.is_empty(), "{raw}");
    for record in &records {
        assert_eq!(record["status"], "failed", "{record}");
        let code = record["error_code"].as_str().expect("失败记录带错误码");
        assert!(code.starts_with("polypack::E_"), "稳定错误码：{code}");
    }
}
