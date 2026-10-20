//! T-M9-27 验收（CLI 侧）：`poly deploy line-liff` 的校验与 dry-run 契约。
//!
//! 覆盖：合法配置输出 Console 人工步骤且不触网；非 HTTPS / 含 fragment / 缺字段 /
//! 未知字段 / 非数字 channel_id / 目录不存在均以稳定错误码 `E_DEPLOY_0001` 失败；
//! `--json` 事件声明 `dry_run=true`、`network=false`、`target_spec=false`。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-cli-deploy-{tag}-{}-{}",
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

/// 运行 `poly` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_poly`）。
fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

fn combined(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

const VALID: &str = r#"schema = 1
liff_id = "1657038298-AbCdEfgh"
endpoint = "https://example.com/liff/"
channel_id = "1657038298"
scopes = ["profile", "openid"]
"#;

/// 建一个含 `dist/` 与 `line-liff.toml` 的工程目录。
fn fixture(tag: &str, config: &str) -> TempDir {
    let dir = TempDir::new(tag);
    std::fs::create_dir_all(dir.path().join("dist")).expect("dist 可建");
    std::fs::write(dir.path().join("line-liff.toml"), config).expect("配置可写");
    dir
}

/// 以固定参数运行 deploy；返回输出。
fn deploy(dir: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["deploy", "line-liff", "--dir", "dist"];
    args.extend_from_slice(extra);
    poly(&args, dir)
}

#[test]
fn valid_config_prints_console_steps() {
    let project = fixture("valid", VALID);
    let output = deploy(project.path(), &[]);
    let text = combined(&output);
    assert!(output.status.success(), "合法配置应通过：{text}");
    assert!(text.contains("dry-run"), "应声明 dry-run：{text}");
    assert!(text.contains("https://example.com/liff/"), "{text}");
    assert!(text.contains("LINE Developers Console"), "{text}");
    assert!(text.contains("不调用 LINE API"), "{text}");
}

#[test]
fn json_event_declares_offline_non_targetspec() {
    let project = fixture("json", VALID);
    let output = deploy(project.path(), &["--json"]);
    assert!(output.status.success(), "{}", combined(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let event: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout 应是单个 JSON 对象");
    assert_eq!(event["event"], "deploy_dry_run");
    assert_eq!(event["mode"], "line-liff");
    assert_eq!(event["dry_run"], true);
    assert_eq!(event["network"], false);
    assert_eq!(event["target_spec"], false);
    assert_eq!(event["endpoint"], "https://example.com/liff/");
    assert_eq!(event["steps"].as_array().map(Vec::len), Some(5));
}

#[test]
fn http_endpoint_is_rejected() {
    let project = fixture("http", &VALID.replace("https://", "http://"));
    let output = deploy(project.path(), &[]);
    assert!(!output.status.success(), "http 应失败");
    assert!(
        combined(&output).contains("poly::E_DEPLOY_0001"),
        "{}",
        combined(&output)
    );
}

#[test]
fn fragment_endpoint_is_rejected() {
    let project = fixture(
        "fragment",
        &VALID.replace("https://example.com/liff/", "https://example.com/liff/#app"),
    );
    let output = deploy(project.path(), &[]);
    assert!(!output.status.success(), "fragment 应失败");
    assert!(combined(&output).contains("fragment"));
}

#[test]
fn missing_required_field_is_rejected() {
    let project = fixture(
        "missing",
        "schema = 1\nliff_id = \"1-AbCdEfgh\"\nendpoint = \"https://example.com/\"\nchannel_id = \"1\"\n",
    );
    let output = deploy(project.path(), &[]);
    assert!(!output.status.success(), "缺 scopes 应失败");
    assert!(combined(&output).contains("scopes"));
}

#[test]
fn unknown_field_is_rejected() {
    let project = fixture("unknown", &format!("{VALID}extra = true\n"));
    let output = deploy(project.path(), &[]);
    assert!(!output.status.success(), "未知字段应失败");
    assert!(combined(&output).contains("unknown field"));
}

#[test]
fn non_numeric_channel_id_is_rejected() {
    let project = fixture(
        "channel",
        &VALID.replace(
            "channel_id = \"1657038298\"",
            "channel_id = \"not-a-number\"",
        ),
    );
    let output = deploy(project.path(), &[]);
    assert!(!output.status.success(), "非数字 channel_id 应失败");
    assert!(combined(&output).contains("channel_id 必须是数字"));
}

#[test]
fn missing_dir_is_rejected() {
    let project = fixture("missing-dir", VALID);
    let output = poly(
        &["deploy", "line-liff", "--dir", "no-such-dist"],
        project.path(),
    );
    assert!(!output.status.success(), "目录不存在应失败");
    let text = combined(&output);
    assert!(text.contains("poly::E_DEPLOY_0001"), "{text}");
    assert!(text.contains("--dir 不是目录"), "{text}");
}
