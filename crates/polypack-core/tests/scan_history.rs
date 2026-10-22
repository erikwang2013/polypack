//! T-M2-12 验收（core 侧）·全历史面：git 对象库批量扫描（`scan.rs` 的拆分）。
//!
//! 需要 git 的用例按 skip-if-missing 语义处理（git 缺失 → 打印原因并跳过，绝不失败）；
//! 样本同样运行时拼出，源码行里不出现完整形态。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use polypack_core::scan::{history_skip_reason, scan_dir, scan_history};

/// GitHub 令牌形态：`ghp_` + 36 位字母数字（样本由片段拼出）。
fn github_token() -> String {
    format!("ghp_{}", "aB3dE7".repeat(6))
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-scan-{tag}-{}-{}",
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

/// 夹具目录（构造样本的入库位置）。
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("scan")
}

/// git 可用性：缺失即跳过（本机无 git 不是失败，见 skip-if-missing 约定）。
fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// 在 `dir` 执行 git：身份 / hooks 路径固定，不依赖机器配置。
fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.email=scan-fixture@example.invalid",
            "-c",
            "user.name=scan-fixture",
        ])
        .args(["-c", "commit.gpgsign=false"])
        .arg("-c")
        .arg(format!("core.hooksPath={}", dir.join("no-hooks").display()))
        .args(args)
        .output()
        .expect("git 可执行（可用性已判定）")
}

// ---------------------------------------------------------------- 全历史

/// 夹具入库后连**全历史**一起扫仍为绿（提交后的字节经 git 对象库回读，标记照样生效）。
#[test]
fn committed_fixture_bytes_stay_green_in_history() {
    if !git_available() {
        eprintln!("跳过：git 不可用（夹具历史扫描按「跳过 + 原因」语义处理）");
        return;
    }
    let temp = TempDir::new("fixture-history");
    let root = temp.path();
    assert!(git(root, &["init", "-q"]).status.success(), "git init");
    for name in ["whitelisted-samples.txt", "sample-keystore.jks"] {
        std::fs::copy(fixtures().join(name), root.join(name)).expect("复制夹具");
    }
    assert!(git(root, &["add", "-f", "-A"]).status.success(), "git add");
    assert!(
        git(root, &["commit", "-q", "-m", "夹具入库"])
            .status
            .success(),
        "git commit"
    );

    assert!(
        scan_dir(root).expect("目录可扫描").findings.is_empty(),
        "工作区为绿"
    );
    let history = scan_history(root).expect("历史可扫描");
    assert!(
        history.scanned >= 2,
        "两个夹具都进了对象库: {}",
        history.scanned
    );
    assert!(
        history.findings.is_empty(),
        "夹具历史必须为绿: {:?}",
        history.findings
    );
}

/// 非仓库：按「跳过 + 原因」处理，绝不失败。
#[test]
fn history_scan_skips_non_repo_with_reason() {
    let temp = TempDir::new("non-repo");
    let reason = history_skip_reason(temp.path()).expect("非仓库必须给出跳过原因（绝不失败）");
    assert!(reason.contains("全历史扫描"), "{reason}");
}

/// 工作区删掉的密钥仍在历史里：全历史扫描必须命中（提交钩子覆盖不到的场景）。
#[test]
fn history_scan_finds_secret_deleted_from_worktree() {
    if !git_available() {
        eprintln!("跳过：git 不可用（全历史扫描按「跳过 + 原因」语义处理）");
        return;
    }
    let temp = TempDir::new("history");
    let root = temp.path();
    assert!(git(root, &["init", "-q"]).status.success(), "git init");
    let file = root.join("keys.txt");
    std::fs::write(&file, format!("GITHUB_TOKEN={}\n", github_token())).expect("写样本");
    assert!(git(root, &["add", "-f", "-A"]).status.success(), "git add");
    assert!(
        git(root, &["commit", "-q", "-m", "加入构造样本"])
            .status
            .success(),
        "git commit"
    );
    std::fs::remove_file(&file).expect("删除工作区样本");
    assert!(
        git(root, &["add", "-f", "-A"]).status.success(),
        "git add（记录删除）"
    );
    assert!(
        git(root, &["commit", "-q", "-m", "移除样本文件"])
            .status
            .success(),
        "git commit"
    );

    assert!(history_skip_reason(root).is_none(), "完整仓库不该跳过");
    let dir = scan_dir(root).expect("目录可扫描");
    assert!(
        dir.findings.is_empty(),
        "工作区已无样本: {:?}",
        dir.findings
    );
    let history = scan_history(root).expect("历史可扫描");
    assert!(
        history.scanned >= 1,
        "历史 blob 应被扫到（删除提交本身不产生新 blob）: {}",
        history.scanned
    );
    assert!(
        history
            .findings
            .iter()
            .any(|finding| finding.rule == "github-token" && finding.path == "keys.txt"),
        "已删除的密钥在历史中仍须命中: {:?}",
        history.findings
    );
}

/// 干净仓库的全历史扫描为绿（同一份实现，本地与 CI 一致）。
#[test]
fn history_scan_of_clean_repo_is_green() {
    if !git_available() {
        eprintln!("跳过：git 不可用（全历史扫描按「跳过 + 原因」语义处理）");
        return;
    }
    let temp = TempDir::new("clean-history");
    let root = temp.path();
    assert!(git(root, &["init", "-q"]).status.success(), "git init");
    std::fs::write(root.join("README.md"), "# 夹具仓库\n\n无密钥材料。\n").expect("写说明");
    assert!(git(root, &["add", "-f", "-A"]).status.success(), "git add");
    assert!(
        git(root, &["commit", "-q", "-m", "初始提交"])
            .status
            .success(),
        "git commit"
    );

    assert!(history_skip_reason(root).is_none(), "完整仓库不该跳过");
    let history = scan_history(root).expect("历史可扫描");
    assert!(
        history.findings.is_empty(),
        "干净历史必须为绿: {:?}",
        history.findings
    );
    assert!(
        history.scanned >= 1,
        "至少扫到说明文件: {}",
        history.scanned
    );
}
