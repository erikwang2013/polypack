//! T-M1-15 验收：`poly clean` 的 dry-run 只读与 `--yes` 实清（§5.5）。
//!
//! 同一份真实构建产物上顺序断言：`--cache`（无 `--yes`）报告可回收项且 `.polypack` 整树
//! 逐字节不变；随后 `--all --yes` 清空缓存对象与产物快照（`.polypack/cache` 与
//! `.polypack/artifacts` 下不再有文件，空目录骨架允许保留）。
//! 缓存命中语义见 tests/build_cache.rs，此处不重复。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// 临时工程目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("polypack-{tag}-{}", std::process::id()));
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

/// 运行 `poly` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_poly`）；cwd 即工程根（`clean` 读当前目录）。
fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

/// 目录下全部文件的（相对路径 → 内容）快照；用于「未写入磁盘」整树比对。
fn files_under(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("目录可读").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .expect("子路径前缀成立")
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(rel, std::fs::read(&path).expect("文件可读"));
        }
    }
    files
}

/// 单行 JSON 报告（`--json` 一份 report 一行）。
fn json_line(out: &Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 1, "--json 恰好一行：{stdout}");
    serde_json::from_str(lines[0]).expect("清理报告是合法 JSON 行")
}

// ---------------------------------------------------------------- 契约

/// dry-run 只读（`.polypack` 逐字节不变）且如实报告可回收量；`--all --yes` 清空缓存与产物快照。
#[test]
fn dry_run_leaves_disk_untouched_then_yes_empties_cache_and_artifacts() {
    let project = TempDir::new("clean");
    assert!(
        poly(&["init"], project.path()).status.success(),
        "poly init 应成功"
    );
    let build = poly(&["build"], project.path());
    assert!(
        build.status.success(),
        "poly build 失败：{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let polypack = project.path().join(".polypack");
    let cache = polypack.join("cache");
    let artifacts = polypack.join("artifacts");
    let before = files_under(&polypack);
    assert!(
        before.keys().any(|rel| rel.starts_with("cache/")),
        "构建后应有缓存对象：{}",
        before.len()
    );
    assert!(
        before.keys().any(|rel| rel.starts_with("artifacts/")),
        "构建后应有产物快照"
    );

    // dry-run（scope = cache）：报告可回收项，但一个字节都不落盘
    let dry = poly(&["clean", "--cache", "--json"], project.path());
    assert!(
        dry.status.success(),
        "poly clean --cache 失败：{}",
        String::from_utf8_lossy(&dry.stderr)
    );
    let report = json_line(&dry);
    assert_eq!(report["event"], "clean", "报告 tag：{report}");
    assert_eq!(
        report["dry_run"],
        serde_json::json!(true),
        "无 --yes 即 dry-run：{report}"
    );
    assert!(
        report["cache_entries"].as_u64().unwrap_or(0) >= 1,
        "dry-run 须报告可回收的缓存条目数：{report}"
    );
    assert_eq!(
        report["artifact_snapshots"],
        serde_json::json!(0),
        "scope = cache 时不得统计产物快照：{report}"
    );
    assert!(
        report["bytes"].as_u64().unwrap_or(0) > 0,
        "dry-run 须报告可回收字节数：{report}"
    );
    assert!(
        files_under(&polypack) == before,
        "dry-run 必须零写入（.polypack 整树应逐字节不变）"
    );

    // 实清：缓存对象与全部产物快照内容清空
    let cleared = poly(&["clean", "--all", "--yes", "--json"], project.path());
    assert!(
        cleared.status.success(),
        "poly clean --all --yes 失败：{}",
        String::from_utf8_lossy(&cleared.stderr)
    );
    let report = json_line(&cleared);
    assert_eq!(
        report["dry_run"],
        serde_json::json!(false),
        "带 --yes 即实清：{report}"
    );
    assert!(
        report["artifact_snapshots"].as_u64().unwrap_or(0) >= 1,
        "--all 应统计到产物快照：{report}"
    );
    for dir in [&cache, &artifacts] {
        let left = files_under(dir);
        assert!(left.is_empty(), "{} 仍有残留文件：{left:?}", dir.display());
    }
}
