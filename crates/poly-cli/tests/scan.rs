//! T-M2-12 验收（CLI 侧）：`poly scan` 的命中退出码 / 白名单 / 跳过语义 / 提交钩子安装。
//!
//! 构造样本一律运行时拼出（源码行不出现完整形态），与 `poly-core/tests/scan.rs` 同法；
//! 需要 git 的用例按 skip-if-missing 语义处理（git 缺失 → 打印原因并跳过，绝不失败）。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// GitHub 令牌形态：`ghp_` + 36 位字母数字（样本由片段拼出）。
fn github_token() -> String {
    format!("ghp_{}", "aB3dE7".repeat(6))
}

/// 运行 `poly` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_poly`）。
fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

/// 按 JSON Lines 解析 stdout（逐行必须合法 JSON）。
fn json_lines(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("stdout 每行应为合法 JSON"))
        .collect()
}

/// git 可用性：缺失即跳过（本机无 git 不是失败）。
fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// 在 `dir` 执行 git：身份 / hooks 路径固定，不依赖机器配置。
fn git(dir: &Path, args: &[&str]) -> Output {
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

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-cli-scan-{tag}-{}-{}",
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

/// 写样本文件：`marker` 为真时同行加白名单标记。
fn write_sample(root: &Path, marker: bool) {
    let token = github_token();
    let line = match marker {
        true => format!("GITHUB_TOKEN={token}  # POLYPACK_TEST_FIXTURE"),
        false => format!("GITHUB_TOKEN={token}"),
    };
    std::fs::write(root.join("keys.txt"), format!("{line}\n")).expect("写样本");
}

/// 建一个提交过样本的仓库；返回（临时目录, 样本内容）。
fn git_repo_with_sample(tag: &str) -> TempDir {
    let temp = TempDir::new(tag);
    let root = temp.path();
    assert!(git(root, &["init", "-q"]).status.success(), "git init");
    write_sample(root, false);
    assert!(git(root, &["add", "-f", "-A"]).status.success(), "git add");
    assert!(
        git(root, &["commit", "-q", "-m", "加入构造样本"])
            .status
            .success(),
        "git commit"
    );
    temp
}

// ---------------------------------------------------------------- 扫描面

/// 真实形态命中：退出码 1、单行 JSON 报告、摘要不回显内容、修复提示指向凭据引用。
#[test]
fn scan_hits_constructed_key_and_redacts_excerpt() {
    let temp = TempDir::new("hit");
    write_sample(temp.path(), false);

    let out = poly(&["scan", "--dir", ".", "--json"], temp.path());
    assert_eq!(
        out.status.code(),
        Some(1),
        "命中必须退出码 1（门控 CI 与提交钩子）"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("poly::E_SCAN_0001"),
        "stderr 带稳定错误码：{stderr}"
    );

    let lines = json_lines(&out);
    assert_eq!(lines.len(), 1, "stdout 为单行 JSON 报告");
    let report = &lines[0];
    assert_eq!(report["event"], "scan");
    assert_eq!(report["history"], false);
    assert_eq!(report["hits"][0]["rule"], "github-token");
    assert_eq!(report["hits"][0]["path"], "keys.txt");
    assert_eq!(report["hits"][0]["line"], 1);
    let excerpt = report["hits"][0]["excerpt"].as_str().expect("摘要为字符串");
    assert!(
        !excerpt.contains(&github_token()),
        "摘要不得回显命中内容：{excerpt}"
    );
    let hint = report["hits"][0]["hint"].as_str().expect("提示为字符串");
    assert!(
        hint.contains("CredentialRef"),
        "修复提示指向凭据引用：{hint}"
    );
}

/// 白名单：去掉标记必须命中，加回标记必须不命中（双断言，防「规则失效被标记掩盖」）。
#[test]
fn scan_whitelist_marker_decides_between_hit_and_green() {
    let temp = TempDir::new("whitelist");
    write_sample(temp.path(), true);

    let out = poly(&["scan", "--dir", ".", "--json"], temp.path());
    assert_eq!(out.status.code(), Some(0), "带标记的构造样本必须为绿");
    let lines = json_lines(&out);
    assert_eq!(
        lines[0]["hits"].as_array().map(Vec::len),
        Some(0),
        "无命中: {:?}",
        lines[0]
    );
    assert_eq!(
        lines[0]["scanned"], 1,
        "样本文件确实被扫过（绿不是因为跳过）"
    );

    write_sample(temp.path(), false);
    let out = poly(&["scan", "--dir", ".", "--json"], temp.path());
    assert_eq!(out.status.code(), Some(1), "同一份内容去掉标记后必须命中");
}

/// `--history` 的跳过语义：非仓库按「跳过 + 原因」处理，退出码仍由命中决定（这里为绿）。
#[test]
fn scan_history_skips_non_repo_with_reason() {
    let temp = TempDir::new("non-repo");
    let out = poly(&["scan", "--dir", ".", "--history", "--json"], temp.path());
    assert_eq!(
        out.status.code(),
        Some(0),
        "非仓库不是失败（skip-if-missing）"
    );
    let lines = json_lines(&out);
    assert_eq!(lines[0]["history"], true);
    let skipped = lines[0]["skipped"].as_array().expect("跳过清单为数组");
    assert!(
        skipped
            .iter()
            .any(|reason| reason.as_str().unwrap_or("").contains("全历史扫描")),
        "跳过须带原因: {skipped:?}"
    );
}

/// 工作区删掉的密钥：`--history` 必须补上（提交钩子覆盖不到的场景），退出码 1。
#[test]
fn scan_history_finds_key_deleted_from_worktree() {
    if !git_available() {
        eprintln!("跳过：git 不可用（全历史扫描按「跳过 + 原因」语义处理）");
        return;
    }
    let temp = git_repo_with_sample("history");
    let root = temp.path();
    std::fs::remove_file(root.join("keys.txt")).expect("删除工作区样本");
    assert!(
        git(root, &["add", "-f", "-A"]).status.success(),
        "git add（记录删除）"
    );
    assert!(
        git(root, &["commit", "-q", "-m", "移除样本"])
            .status
            .success(),
        "git commit"
    );

    assert_eq!(
        poly(&["scan", "--dir", "."], root).status.code(),
        Some(0),
        "工作区已为绿"
    );
    let out = poly(&["scan", "--dir", ".", "--history", "--json"], root);
    assert_eq!(out.status.code(), Some(1), "历史里的密钥必须命中");
    let lines = json_lines(&out);
    let hits = lines[0]["hits"].as_array().expect("命中清单为数组");
    assert!(
        hits.iter()
            .any(|hit| hit["rule"] == "github-token" && hit["path"] == "keys.txt"),
        "命中来自历史 blob: {hits:?}"
    );
    assert!(
        lines[0]["skipped"].as_array().is_some_and(Vec::is_empty),
        "完整仓库不该跳过"
    );
}

// ---------------------------------------------------------------- 提交钩子

/// 安装钩子：fail-open 脚本 + 标记 + 可重复安装（幂等）；JSON 报告单行。
#[test]
fn install_hook_writes_marked_fail_open_script() {
    if !git_available() {
        eprintln!("跳过：git 不可用（钩子安装需要 .git/hooks 目录）");
        return;
    }
    let temp = git_repo_with_sample("hook");
    let root = temp.path();
    let hook = root.join(".git").join("hooks").join("pre-commit");

    let out = poly(&["scan", "--install-hook", "--dir", ".", "--json"], root);
    assert_eq!(
        out.status.code(),
        Some(0),
        "安装成功: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines = json_lines(&out);
    assert_eq!(lines[0]["event"], "scan-hook");
    assert!(
        lines[0]["installed"]
            .as_str()
            .unwrap_or("")
            .contains("pre-commit"),
        "报告安装路径: {:?}",
        lines[0]["installed"]
    );

    let script = std::fs::read_to_string(&hook).expect("钩子文件已写入");
    assert!(
        script.contains("poly scan --install-hook"),
        "带本工具标记（拒写外部钩子的判据）"
    );
    assert!(
        script.contains("fail-open"),
        "poly 缺席时跳过检查（不阻断提交）"
    );
    assert!(script.contains("scan --dir"), "钩子跑同一份扫描实现");
    assert_eq!(
        script.lines().next(),
        Some("#!/bin/sh"),
        "git 钩子按 sh 执行"
    );

    let again = poly(&["scan", "--install-hook", "--dir", "."], root);
    assert_eq!(
        again.status.code(),
        Some(0),
        "重复安装幂等（自己的钩子可覆盖）"
    );
    assert_eq!(
        std::fs::read_to_string(&hook).expect("钩子文件仍在"),
        script,
        "幂等安装内容一致"
    );
}

/// 拒写外部钩子与不存在的仓库：`poly::E_SCAN_0002` + 修复提示，绝不覆盖别人的钩子。
#[test]
fn install_hook_refuses_foreign_hook_and_non_repo() {
    let temp = TempDir::new("no-repo");
    let out = poly(&["scan", "--install-hook", "--dir", "."], temp.path());
    assert_eq!(out.status.code(), Some(1), "非仓库装钩子必须失败");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("poly::E_SCAN_0002"),
        "stderr 带稳定错误码：{stderr}"
    );
    assert!(stderr.contains("修复提示"), "失败必须带修复提示：{stderr}");

    if !git_available() {
        eprintln!("跳过：git 不可用（外部钩子拒写分支需要 .git/hooks 目录）");
        return;
    }
    let temp = TempDir::new("foreign-hook");
    let root = temp.path();
    assert!(git(root, &["init", "-q"]).status.success(), "git init");
    let hook = root.join(".git").join("hooks").join("pre-commit");
    let foreign = "#!/bin/sh\necho 别人的钩子\n";
    std::fs::write(&hook, foreign).expect("写外部钩子");

    let out = poly(&["scan", "--install-hook", "--dir", "."], root);
    assert_eq!(out.status.code(), Some(1), "外部钩子必须拒写");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("poly::E_SCAN_0002") && stderr.contains("不覆盖"),
        "{stderr}"
    );
    assert_eq!(
        std::fs::read_to_string(&hook).expect("外部钩子未被破坏"),
        foreign
    );
}
