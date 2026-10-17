//! T-M1-11 验收：`poly init` 模板落盘、幂等保护与 JSON Lines 报告。
//!
//! 覆盖：完整文件集（含 `ui/public/icon.svg` 与 `docs/assets/pax-mark.svg` 逐字节一致）、
//! 无 `--force` 时拒绝覆盖且不触碰既有文件、`--force` 覆盖、`--json` 单行报告。
//! `init` 产物能否构建见 tests/build_cache.rs，此处不重复。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 模板文件清单（单源为 src/init.rs 的 `FILES`，按字节序排列以便整集比对）。
const TEMPLATE_FILES: [&str; 12] = [
    ".gitignore",
    "README.md",
    "core/Cargo.toml",
    "core/src/main.rs",
    "polypack.toml",
    "ui/index.html",
    "ui/main.jsx",
    "ui/package.json",
    "ui/public/icon.svg",
    "ui/public/sw.js",
    "ui/tokens.css",
    "ui/vite.config.js",
];

// ---------------------------------------------------------------- 测试支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        // pid + 进程内计数：并发测试与并发 agent 都不撞名
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

/// 运行 `poly` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_poly`）。
fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

/// 模板目录（`include_str!` 的磁盘源；内容比对的事实来源）。
fn templates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/app")
}

/// 目录下全部文件的（相对路径 → 内容）快照；用于「未被触碰」整树比对。
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

/// stdout 的非空行。
fn stdout_lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------- 契约

/// 落盘完整模板集，且 `ui/public/icon.svg` 与 `docs/assets/pax-mark.svg` 逐字节一致。
#[test]
fn init_writes_full_template_set_and_pax_icon() {
    let project = TempDir::new("init-full");
    let out = poly(&["init"], project.path());
    assert!(
        out.status.success(),
        "poly init 失败（{}）：{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );

    let files = files_under(project.path());
    let paths: Vec<&str> = files.keys().map(String::as_str).collect();
    assert_eq!(
        paths,
        TEMPLATE_FILES.to_vec(),
        "init 落盘文件集与模板清单不一致"
    );

    // 图标单一品牌源：生成的 icon.svg 必须与仓库 Pax 徽章逐字节相同
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/poly-cli 上溯两级为 workspace 根");
    let brand = std::fs::read(workspace.join("docs/assets/pax-mark.svg")).expect("读 pax-mark.svg");
    let icon = &files["ui/public/icon.svg"];
    assert_eq!(
        icon.len(),
        brand.len(),
        "ui/public/icon.svg 长度 {} ≠ docs/assets/pax-mark.svg 长度 {}",
        icon.len(),
        brand.len()
    );
    assert!(
        icon == &brand,
        "ui/public/icon.svg 与 docs/assets/pax-mark.svg 非逐字节一致"
    );
}

/// 无 `--force` 的二次 init：非零退出、stderr 含 `poly::E_INIT_0001`、既有文件一字节不改。
#[test]
fn init_without_force_refuses_and_leaves_files_untouched() {
    let project = TempDir::new("init-refuse");
    assert!(
        poly(&["init"], project.path()).status.success(),
        "首次 poly init 应成功"
    );
    let before = files_under(project.path());

    let second = poly(&["init"], project.path());
    assert_ne!(second.status.code(), Some(0), "二次 init 应非零退出");
    let stderr = String::from_utf8_lossy(&second.stderr);
    // 码值见 src/init.rs 的 codes::E_INIT_0001（diagnostics.rs 的 Display：[poly::E_INIT_0001] …）
    assert!(
        stderr.contains("poly::E_INIT_0001"),
        "拒绝覆盖须报 poly::E_INIT_0001：{stderr}"
    );

    assert!(
        files_under(project.path()) == before,
        "拒绝覆盖时不得改动既有文件"
    );
}

/// `--force`：覆盖被改坏的模板文件并补回被删者（幂等回到出厂内容）。
#[test]
fn init_force_overwrites_existing_files() {
    let project = TempDir::new("init-force");
    assert!(
        poly(&["init"], project.path()).status.success(),
        "首次 init 应成功"
    );

    std::fs::write(project.path().join("README.md"), "被改坏的内容").expect("改坏 README.md");
    std::fs::remove_file(project.path().join("ui/main.jsx")).expect("删除 ui/main.jsx");

    let out = poly(&["init", "--force"], project.path());
    assert!(
        out.status.success(),
        "poly init --force 失败（{}）：{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        files_under(project.path()) == files_under(&templates_dir()),
        "--force 后应与模板目录内容一致"
    );
}

/// `--json`：stdout 恰好一行合法 JSON（附文件清单），无人读噪声。
#[test]
fn init_json_prints_single_line_report() {
    let project = TempDir::new("init-json");
    let out = poly(&["init", "--json"], project.path());
    assert!(out.status.success(), "poly init --json 应成功");

    let lines = stdout_lines(&out);
    assert_eq!(lines.len(), 1, "JSON 模式 stdout 恰好一行：{lines:?}");
    let report: serde_json::Value =
        serde_json::from_str(&lines[0]).expect("init 报告是合法 JSON 行");
    assert_eq!(report["event"], "init", "报告 tag：{report}");
    let mut files: Vec<&str> = report["files"]
        .as_array()
        .expect("报告含 files 数组")
        .iter()
        .map(|item| item.as_str().expect("files 元素为字符串"))
        .collect();
    files.sort_unstable(); // 报告按声明序排，比对前归一为字节序
    assert_eq!(
        files,
        TEMPLATE_FILES.to_vec(),
        "报告的文件清单与模板清单不一致"
    );
    assert!(
        report["root"].as_str().is_some_and(
            |root| root.ends_with(project.path().file_name().unwrap().to_str().unwrap())
        ),
        "root 指向生成目录：{report}"
    );
}
