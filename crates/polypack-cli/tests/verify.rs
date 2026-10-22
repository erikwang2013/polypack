//! T-M2-06 验收（CLI 侧）：`polypack verify --reproducible` 的二跑比对与如实报告。
//!
//! 用 web 静态站点目标做真跑基准：静态站点打包器不依赖任何外部工具，三宿主都能真比；
//! 缺打包输入的路径（无 `dist/`）在三宿主都确定性触发「告警跳过 → 全部跳过即报错」，
//! 断言的是「不假绿」而不是本机装了什么。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 最小可托管页面（内建 HTML 结构断言要求 doctype / html / body 配对）。
const INDEX_HTML: &str =
    "<!doctype html>\n<html lang=\"zh\">\n<body><h1>fixture</h1></body>\n</html>\n";

/// PWA manifest（图标引用必须真在站点里）。
const MANIFEST_JSON: &str = r#"{"name":"Verify Fixture","start_url":"/","display":"standalone","icons":[{"src":"icon.png","sizes":"192x192"}]}"#;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-cli-verify-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可创建");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 最小工程配置；`with_dist` 决定 UI 产物是否就绪。
fn fixture(root: &Path, with_dist: bool) -> PathBuf {
    let config = root.join("polypack.toml");
    std::fs::write(
        &config,
        "[package]\nname = \"Verify Fixture\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("写配置");
    if with_dist {
        let dist = root.join("dist");
        std::fs::create_dir_all(&dist).expect("建 dist");
        std::fs::write(dist.join("index.html"), INDEX_HTML).expect("写页面");
        std::fs::write(dist.join("manifest.webmanifest"), MANIFEST_JSON).expect("写 manifest");
        std::fs::write(dist.join("icon.png"), b"not-a-real-png").expect("写图标占位");
    }
    config
}

/// 运行 `polypack` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_polypack`）。
fn polypack(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polypack"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("polypack 可执行")
}

/// 按 JSON Lines 解析 stdout。
fn json_lines(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("stdout 每行应为合法 JSON"))
        .collect()
}

#[test]
fn verify_reproducible_without_build_inputs_reports_honestly() {
    let temp = TempDir::new("no-inputs");
    fixture(temp.path(), false);

    let out = polypack(
        &["verify", "--reproducible", "--target", "web", "--json"],
        temp.path(),
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "什么都不比必须非零退出（不假绿）"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("polypack::E_VERIFY_0002"), "{stderr}");
    assert!(
        stderr.contains("polypack build"),
        "提示应指向先把输入就绪的那条路：{stderr}"
    );

    // 跳过原因要如实进事件流（json 模式单行一条），而不是悄悄放过
    let lines = json_lines(&out);
    assert!(
        lines.iter().any(|line| {
            line["event"] == "warning"
                && line["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("web") && message.contains("ui_dist"))
        }),
        "跳过原因须标明目标与缺失输入：{lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line["event"] == "verify-reproducible"),
        "没有比对结果就不得产出比对报告：{lines:?}"
    );
}

/// T-M6-08：`--allow-skipped` 只让 CI 在全环境槽跳过时不硬红，跳过原因仍逐条打印。
#[test]
fn allow_skipped_keeps_ci_green_but_lists_every_skip() {
    let temp = TempDir::new("allow-skipped");
    fixture(temp.path(), false);

    let out = polypack(
        &["verify", "--target", "qnx", "--allow-skipped", "--json"],
        temp.path(),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "全跳过 + --allow-skipped 应以 0 退出（原因仍须打印）：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("polypack::E_VERIFY_0002"),
        "--allow-skipped 不应再报全跳过失败：{stderr}"
    );
    let lines = json_lines(&out);
    let warnings: Vec<&str> = lines
        .iter()
        .filter(|line| line["event"] == "warning")
        .filter_map(|line| line["message"].as_str())
        .collect();
    assert!(
        warnings
            .iter()
            .any(|message| message.contains("qnx") && message.contains("未校验")),
        "逐腿跳过原因须如实出现在事件流：{warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|message| message.contains("--allow-skipped")),
        "全跳过注记须说明 CI 开关语义：{warnings:?}"
    );
}

#[test]
fn verify_reproducible_web_static_site_is_byte_identical() {
    let temp = TempDir::new("happy");
    fixture(temp.path(), true);

    let out = polypack(
        &["verify", "--reproducible", "--target", "web", "--json"],
        temp.path(),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "静态站点二跑应字节一致：stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let lines = json_lines(&out);
    let report = lines
        .iter()
        .find(|line| line["event"] == "verify-reproducible")
        .unwrap_or_else(|| panic!("缺 verify-reproducible 报告：{lines:?}"));
    assert_eq!(report["target"], "web");
    assert_eq!(report["kind"], "directory", "静态站点是目录产物");
    assert_eq!(report["match"], true);
    assert!(
        report["digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha384-")),
        "摘要须是 SRI 形态：{report}"
    );

    // 二跑的落点必须是两个不同的绝对目录（证据留档）
    let repro = temp.path().join(".polypack/repro/web");
    assert!(repro.join("a/site/index.html").is_file(), "a 跑产物留档");
    assert!(repro.join("b/site/index.html").is_file(), "b 跑产物留档");
}
