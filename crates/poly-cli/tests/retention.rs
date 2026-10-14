//! T-M1-15 验收（产物侧）：`[retention].artifact_builds` 超限后按 LRU 自动淘汰产物快照。
//!
//! 整份文件是 **Linux 宿主专属**（口径同 tests/package.rs）：用例用 `poly build --target linux`
//! 造快照，而显式目标在非 Linux 宿主上结构性不可达（`Registry::resolve_all` → `Unreachable`）。
//! 保留 / 淘汰机制本身与宿主无关，由 ubuntu 腿完整覆盖。
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

/// 测试临时树：Drop 时整树删除（含编译产物）。
struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "polypack-retention-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("创建临时目录");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn poly(root: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("执行 poly");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// JSON Lines 输出 → 事件列表。
fn events(stdout: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|e| panic!("非 JSON 行：{line}（{e}）"))
        })
        .collect()
}

/// 每目标只保留 `artifact_builds` 次快照：连跑三次后只剩一份，且淘汰有告警不阻断。
#[test]
fn artifact_snapshots_are_pruned_by_retention_policy() {
    let tree = TempTree::new("artifacts");
    let (code, _, stderr) = poly(tree.path(), &["init", ".", "--json"]);
    assert_eq!(code, 0, "poly init 失败：{stderr}");

    // 把保留次数压到 1，避免测试真跑 11 次构建
    let config = tree.path().join("polypack.toml");
    let toml = std::fs::read_to_string(&config).expect("读 polypack.toml");
    let patched = toml.replace("artifact_builds = 10", "artifact_builds = 1");
    assert_ne!(
        patched, toml,
        "模板应含 artifact_builds = 10（本测试依赖其字面量）"
    );
    std::fs::write(&config, patched).expect("写回 polypack.toml");

    let mut warnings = Vec::new();
    for run in 1..=3 {
        let (code, stdout, stderr) = poly(tree.path(), &["build", "--target", "linux", "--json"]);
        assert_eq!(code, 0, "第 {run} 次构建失败：{stderr}");
        let events = events(&stdout);
        // 第 1 跑未命中并真编译；其后应 100% 命中（缓存键稳定）
        let cached: Vec<&serde_json::Value> = events
            .iter()
            .filter(|event| event["event"] == "cached")
            .collect();
        assert_eq!(
            cached.len(),
            2,
            "每跑应有 compile / bundle 两个 cached 事件：{events:?}"
        );
        let hits = cached.iter().filter(|event| event["hit"] == true).count();
        if run == 1 {
            assert_eq!(hits, 0, "首跑必须未命中：{events:?}");
        } else {
            assert_eq!(hits, 2, "第 {run} 跑必须全部命中：{events:?}");
        }
        warnings.extend(
            events
                .iter()
                .filter(|event| event["stage"] == "retention")
                .map(|event| event["message"].as_str().unwrap_or_default().to_string()),
        );
    }

    let snapshots: Vec<PathBuf> = std::fs::read_dir(tree.path().join(".polypack/artifacts/linux"))
        .expect("产物目录应存在")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    assert_eq!(
        snapshots.len(),
        1,
        "artifact_builds = 1 时只应保留最近一次快照，实得 {snapshots:?}"
    );
    assert!(
        snapshots[0].join("core").is_dir() && snapshots[0].join("dist").is_dir(),
        "保留的快照应含 core 与 dist：{}",
        snapshots[0].display()
    );

    assert!(
        !warnings.is_empty(),
        "淘汰产物快照必须发 retention 告警（不阻断构建）"
    );
    assert!(
        warnings.iter().all(|message| !message.contains("失败")),
        "淘汰告警不得是失败：{warnings:?}"
    );
}
