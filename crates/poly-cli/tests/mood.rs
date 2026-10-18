//! T-M1-14（CLI 半）验收：启动横幅与结束表情的人读输出契约。
//!
//! 覆盖：成功路径（欢乐）、有告警路径（撒娇 / 待确认）、失败路径（哭）、JSON 通道不混入表情行、
//! 管道（非 TTY）形态下不带 ANSI 转义序列（降级为纯文字）。表情本身的映射与降级由
//! `src/mood.rs` 的单元测试逐项断言，这里只验真实进程的输出。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------- 测试支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-{tag}-{}-{}",
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

fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

/// 末个非空 stdout 行（= 结束表情行）。
fn last_line(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .to_string()
}

// ---------------------------------------------------------------- 契约

/// 成功路径：横幅笑、结束欢乐；管道里不带 ANSI 转义序列（降级为纯文字）。
#[test]
fn success_path_prints_idle_banner_and_joy_face() {
    let project = TempDir::new("mood-ok");
    let out = poly(&["init", "."], project.path());
    assert!(
        out.status.success(),
        "poly init 失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let banner = lines.first().copied().unwrap_or_default();
    assert!(
        banner.starts_with("(^‿^)") && banner.contains("PolyPack"),
        "首行应为笑横幅: {banner}"
    );
    assert!(
        last_line(&out).starts_with("(≧▽≦) 完成"),
        "末行应为欢乐 + 完成: {}",
        last_line(&out)
    );
    assert!(
        !stdout.contains('\u{1b}'),
        "非 TTY（管道）时须降级为纯文字: {stdout:?}"
    );
}

/// 有告警路径：待确认（撒娇）；告警数写在状态行里。
///
/// 用默认 `poly build`（本机可达的全部目标）：模板声明的 android / harmonyos 在本机没有
/// 打包器，package 阶段如实告警并跳过——正是「跑完了但需要人看一眼」的形态。
#[test]
fn warnings_path_prints_coy_face() {
    let project = TempDir::new("mood-warn");
    assert!(
        poly(&["init", "."], project.path()).status.success(),
        "poly init 应成功"
    );

    let out = poly(&["build"], project.path());
    assert!(
        out.status.success(),
        "告警不得让构建失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("⚠ "),
        "本机应至少有一条告警（目标无打包器）: {stdout}"
    );
    let last = last_line(&out);
    assert!(
        last.starts_with("(｡•́︿•̀｡) 待确认"),
        "末行应为撒娇 + 待确认: {last}"
    );
    assert!(last.contains("条告警"), "状态行应带告警数: {last}");
}

/// 失败路径：哭 + 提示看上方错误；诊断仍走 stderr。
#[test]
fn failure_path_prints_crying_face() {
    let project = TempDir::new("mood-fail");
    assert!(
        poly(&["init", "."], project.path()).status.success(),
        "poly init 应成功"
    );
    // 打断核心 crate：阶段硬失败（诊断错误，非环境性缺失）
    std::fs::write(
        project.path().join("core/src/main.rs"),
        "fn main( { 这不是 Rust",
    )
    .expect("改坏 main.rs");

    // 不带 --target：显式 linux 在非 Linux 宿主是结构性不可达错误，会让本测在各宿主原地通过
    let out = poly(&["build"], project.path());
    assert!(
        !out.status.success(),
        "编译失败应非零退出: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let last = last_line(&out);
    assert!(last.starts_with("(;﹏;) 失败"), "末行应为哭 + 失败: {last}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("poly::"),
        "错误详情须留在 stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// JSON 通道纯净化：`--json` 的 stdout 全是合法 JSON 行，没有横幅 / 表情行。
#[test]
fn json_channel_has_no_banner_or_face() {
    let project = TempDir::new("mood-json");
    let out = poly(&["init", ".", "--json"], project.path());
    assert!(out.status.success(), "poly init --json 应成功");

    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        serde_json::from_str::<serde_json::Value>(line)
            .unwrap_or_else(|err| panic!("--json stdout 混入非 JSON 行：{line}（{err}）"));
    }
    assert!(
        !stdout.contains("(-_-)") && !stdout.contains("(^‿^)"),
        "JSON 通道不得输出表情: {stdout}"
    );
}
