//! M9-E5 / T-M9-14 验收（CLI 侧）：`poly publish --check-metadata` 只校验不发布；
//! `--metadata-dir` 生成确定性材料；旧 `--materials` 与目录二选一（互斥）。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-publish-metadata-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("时钟")
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可建");
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

const WECHAT_METADATA: &str = r#"schema = 1
title = "PolyPack Tool"
short_description = "Build and ship mini apps with one command."
full_description = "PolyPack builds, packages, and submits mini programs from one configuration file."
keywords = ["build", "publish"]
screenshots = ["screenshots/home.png"]
privacy_url = "https://example.com/privacy"
support_url = "https://example.com/support"
"#;

fn project(tag: &str) -> TempDir {
    let temp = TempDir::new(tag);
    std::fs::write(
        temp.path().join("polypack.toml"),
        "[package]\nname = \"metadata-fixture\"\nversion = \"0.1.0\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("写工程配置");
    temp
}

fn write_metadata(temp: &TempDir, toml: &str) {
    let dir = temp.path().join("store/metadata/wechat");
    std::fs::create_dir_all(dir.join("screenshots")).expect("metadata 目录可建");
    std::fs::write(dir.join("default.toml"), toml).expect("写 metadata");
    std::fs::write(dir.join("screenshots/home.png"), b"").expect("截图占位（空文件）");
}

fn poly(args: &[&str], cwd: &Path, env: Option<(&str, &str)>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_poly"));
    command.args(args).current_dir(cwd);
    if let Some((key, value)) = env {
        command.env(key, value);
    }
    command.output().expect("poly 可执行")
}

fn json_line(out: &Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 1, "--json 恰好一行：{stdout}");
    serde_json::from_str(lines[0]).expect("报告是合法 JSON 行")
}

fn check_args() -> [&'static str; 7] {
    [
        "publish",
        "--check-metadata",
        "--metadata-dir",
        "store/metadata",
        "--channel",
        "wechat-open-platform",
        "--json",
    ]
}

/// `--check-metadata` 不要求 --appid / --version / --credential；不落发布回执。
#[test]
fn check_metadata_passes_without_publish_inputs_and_writes_no_receipt() {
    let temp = project("check-ok");
    write_metadata(&temp, WECHAT_METADATA);

    let out = poly(&check_args(), temp.path(), None);
    assert!(
        out.status.success(),
        "校验应通过：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report = json_line(&out);
    assert_eq!(report["event"], "publish-metadata-check");
    assert_eq!(report["platform"], "wechat");
    assert_eq!(report["ok"], true);
    assert_eq!(report["locales"], serde_json::json!(["default"]));
    assert_eq!(report["violations"], serde_json::json!([]));
    assert!(
        !temp.path().join(".polypack/publish").exists(),
        "--check-metadata 不得创建发布回执"
    );
}

/// 违规 → 非零退出、稳定码、一次报告全部；且绝不生成发布请求（无回执目录）。
#[test]
fn check_metadata_failure_exits_nonzero_and_creates_no_publish_request() {
    let temp = project("check-fail");
    let invalid = WECHAT_METADATA
        .replace("https://example.com/privacy", "not-a-valid-url")
        .replace(
            "title = \"PolyPack Tool\"",
            &format!("title = \"{}\"", "a".repeat(31)),
        );
    write_metadata(&temp, &invalid);

    let out = poly(&check_args(), temp.path(), None);
    assert_eq!(out.status.code(), Some(1), "违规须非零退出");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("poly::E_PUBLISH_0001"),
        "须走发布域输入码：{stderr}"
    );
    assert!(
        stderr.contains("metadata.url_invalid") && stderr.contains("metadata.field_too_long"),
        "一次报告全部违规：{stderr}"
    );
    let report = json_line(&out);
    assert_eq!(report["ok"], false);
    assert!(
        report["violations"]
            .as_array()
            .is_some_and(|items| items.len() >= 2),
        "JSON 报告须列出全部违规：{report}"
    );
    assert!(
        !temp.path().join(".polypack/publish").exists(),
        "校验失败不得创建发布回执 / 发布请求"
    );
}

/// 旧 `--materials` 与 `--metadata-dir` 互斥（clap 在解析期拒绝，退出码 2）。
#[test]
fn materials_and_metadata_dir_are_mutually_exclusive() {
    let temp = project("exclusive");
    let out = poly(
        &[
            "publish",
            "--materials",
            "materials.json",
            "--metadata-dir",
            "store/metadata",
            "--appid",
            "wx1",
            "--version",
            "1.0.0",
            "--credential",
            "env:POLY_METADATA_TEST_CRED",
        ],
        temp.path(),
        None,
    );
    assert_eq!(out.status.code(), Some(2), "clap 须拒绝二选一同时给出");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "须明确互斥规则：{stderr}"
    );
}

/// `--metadata-dir` 在 dry-run 中构建材料：locale / 摘要稳定，二跑字节一致，不落回执。
#[test]
fn metadata_dir_dry_run_is_deterministic_and_stays_offline() {
    let temp = project("dry-run");
    write_metadata(&temp, WECHAT_METADATA);
    let dist = temp.path().join("dist");
    std::fs::create_dir_all(&dist).expect("dist 可建");
    std::fs::write(dist.join("app.json"), r#"{"pages":["pages/index/index"]}"#).expect("写");
    std::fs::write(dist.join("app.js"), "App({})").expect("写");
    let credential = "eyJ0b2tlbiI6InJlZGFjdGVkIn0="; // base64({"token":"redacted"})

    let args = [
        "publish",
        "--dry-run",
        "--json",
        "--channel",
        "wechat-open-platform",
        "--metadata-dir",
        "store/metadata",
        "--appid",
        "wx1",
        "--version",
        "1.0.0",
        "--credential",
        "env:POLY_METADATA_TEST_CRED",
        "--package",
        "dist",
    ];
    let first = poly(
        &args,
        temp.path(),
        Some(("POLY_METADATA_TEST_CRED", credential)),
    );
    let second = poly(
        &args,
        temp.path(),
        Some(("POLY_METADATA_TEST_CRED", credential)),
    );
    for out in [&first, &second] {
        assert!(
            out.status.success(),
            "dry-run 应成功：{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let first = json_line(&first);
    let second = json_line(&second);
    assert_eq!(first["event"], "publish-dry-run");
    assert_eq!(first["materials"], true);
    assert_eq!(first["materials_locale"], "default");
    assert_eq!(
        first["materials_digest"], second["materials_digest"],
        "同输入材料摘要必须一致"
    );
    assert_eq!(
        first["materials_digest"].as_str().map(str::len),
        Some(64),
        "blake3 hex"
    );
    assert!(
        !temp.path().join(".polypack/publish").exists(),
        "dry-run 不得落回执"
    );
}
