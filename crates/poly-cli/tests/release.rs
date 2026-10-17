//! T-M5-11 准备面：`poly release` 参数面与稳定错误码（真实接线待 T-M5-05/06/07）。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// 最小工程骨架（只需配置可装载）。
fn fixture(tag: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("polypack-release-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("工程目录可建");
    std::fs::write(
        root.join("polypack.toml"),
        "[package]\nname = \"fixture-app\"\nversion = \"0.1.0\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("写配置");
    root
}

fn poly(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("poly 可执行")
}

#[test]
fn help_lists_release_surface() {
    let root = fixture("help");
    let out = poly(&root, &["release", "--help"]);
    assert_eq!(out.status.code(), Some(0), "--help 恒 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    for flag in ["--target", "--repo", "--credential", "--dry-run"] {
        assert!(stdout.contains(flag), "help 须含 {flag}：{stdout}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn comma_separated_targets_are_split_and_rejected_by_format() {
    let root = fixture("comma");
    let out = poly(&root, &["release", "--target", "linux,web", "--dry-run"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("poly::E_RELEASE_0002"),
        "非仓库格式报稳定码：{stderr}"
    );
    assert!(
        stderr.contains("appimage"),
        "逗号拆分后首个目标 = linux（appimage）：{stderr}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn repeated_target_flag_is_accepted() {
    let root = fixture("repeat");
    let out = poly(
        &root,
        &[
            "release",
            "--target",
            "linux",
            "--target",
            "web",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("poly::E_RELEASE_0002"),
        "重复 --target 被接受并走到格式校验：{stderr}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unknown_target_uses_target_code_and_hint() {
    let root = fixture("unknown");
    let out = poly(&root, &["release", "--target", "no-such", "--dry-run"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_TGT_0001"), "{stderr}");
    assert!(
        stderr.contains("poly doctor"),
        "修复提示指向目标矩阵：{stderr}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 用既有 `linux` 目标 + `[targets.linux].format` 覆盖模拟 rpm 目标（真实 fedora 目标
/// 待 T-M5-05/06/07 入 main；本测试只验 poly-cli 面，不依赖其实现）。
fn fixture_rpm(tag: &str) -> PathBuf {
    let root = fixture(tag);
    let mut config = std::fs::read_to_string(root.join("polypack.toml")).expect("读配置");
    config.push_str("[targets.linux]\nformat = \"rpm\"\n");
    std::fs::write(root.join("polypack.toml"), config).expect("写配置");
    let dir = root.join(".polypack").join("packages").join("linux");
    std::fs::create_dir_all(&dir).expect("产物目录可建");
    std::fs::write(dir.join("fixture-app-0.1.0-1.x86_64.rpm"), b"rpm-bytes").expect("写产物");
    root
}

/// `--json` 输出恰一行，便于整行断言（stdout 必须纯 JSON Lines）。
fn only_json_line(stdout: &[u8]) -> serde_json::Value {
    let text = String::from_utf8_lossy(stdout);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 1, "stdout 应恰一行 JSON：{text}");
    serde_json::from_str(lines[0])
        .unwrap_or_else(|err| panic!("行不是合法 JSON：{}（{err}）", lines[0]))
}

#[test]
fn dry_run_reports_manifest_and_receipt_anchor() {
    let root = fixture_rpm("dry");
    let out = poly(
        &root,
        &["release", "--target", "linux", "--dry-run", "--json"],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "dry-run 须成功：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value = only_json_line(&out.stdout);
    assert_eq!(value["event"], "release-dry-run");
    assert_eq!(value["target"], "linux");
    assert_eq!(value["repo_kind"], "rpm", "配置 format 覆盖生效");
    assert_eq!(value["artifact_count"], 1);
    assert_eq!(value["cache_hit"].as_bool(), Some(false));
    let receipt = Path::new(value["receipt"].as_str().expect("回执路径"));
    assert_eq!(
        receipt.parent(),
        Some(
            root.join(".polypack")
                .join("release")
                .join("linux")
                .as_path()
        ),
        "回执落点"
    );
    assert!(
        receipt
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("0.1.0-") && name.ends_with(".json")),
        "回执文件名 = 版本-摘要前 16：{}",
        receipt.display()
    );
    assert!(!receipt.exists(), "dry-run 不落回执");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unwired_backend_reports_stable_code_and_leaves_no_receipt() {
    let root = fixture_rpm("pending");
    let out = poly(&root, &["release", "--target", "linux"]);
    assert_eq!(out.status.code(), Some(1), "后端未接线须失败");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_RELEASE_0004"), "{stderr}");
    assert!(
        stderr.contains("--dry-run"),
        "修复提示给出先行预演：{stderr}"
    );
    assert!(
        !root.join(".polypack").join("release").exists(),
        "失败不落回执"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rerun_with_matching_receipt_is_published_noop() {
    let root = fixture_rpm("noop");
    let dry = poly(
        &root,
        &["release", "--target", "linux", "--dry-run", "--json"],
    );
    assert_eq!(dry.status.code(), Some(0));
    let value = only_json_line(&dry.stdout);
    let receipt_path = PathBuf::from(value["receipt"].as_str().expect("回执路径"));
    // 模拟前一次成功发布留下的回执（真实首跑要等仓库后端接线）。
    let receipt = serde_json::json!({
        "schema_version": 1,
        "target": "linux",
        "version": "0.1.0",
        "repo": value["repo"].clone(),
        "repo_kind": "rpm",
        "manifest_digest": value["manifest_digest"].clone(),
        "artifacts": value["artifacts"].clone(),
        "runs": 1,
        "first_unix": 100,
        "last_unix": 100,
    });
    std::fs::create_dir_all(receipt_path.parent().expect("父目录")).expect("建目录");
    std::fs::write(
        &receipt_path,
        serde_json::to_vec_pretty(&receipt).expect("序列化"),
    )
    .expect("写回执");

    let out = poly(&root, &["release", "--target", "linux", "--json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "命中回执即 no-op：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value = only_json_line(&out.stdout);
    assert_eq!(value["event"], "published");
    assert_eq!(value["cache_hit"].as_bool(), Some(true));
    assert_eq!(value["runs"], 2, "no-op 也如实 runs+1");
    assert_eq!(value["receipt"], receipt_path.display().to_string());
    let stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&receipt_path).expect("回执存在"))
            .expect("回执可解析");
    assert_eq!(stored["runs"], 2);
    assert_eq!(stored["first_unix"], 100, "首跑时刻保留");
    let _ = std::fs::remove_dir_all(&root);
}
