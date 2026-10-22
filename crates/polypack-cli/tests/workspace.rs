//! M9-E2 验收（CLI 侧）：`--app` / `--workspace` 选择器、按应用输出隔离与二跑缓存。
//!
//! 夹具只声明 `web` 目标（runtime=wasm → compile 支线跳过），因此测试无需真编 Rust，
//! 但仍走完整 bundle → package → verify 流水线。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

/// 临时工作区；drop 即递归删除。
struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-workspace-cli-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
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

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().expect("路径含父目录")).expect("创建父目录");
    std::fs::write(path, content).expect("写文件");
}

/// 两应用工作区：`apps/a` 与 `apps/b`，各自带可区分的 UI 载荷。
fn fixture() -> TempTree {
    let tree = TempTree::new("two-apps");
    let root = tree.path();
    write(
        &root.join("polypack.toml"),
        "schema = 1\n\n[workspace]\nmembers = [\"apps/a\", \"apps/b\"]\n",
    );
    for (app, marker) in [("a", "MARKER-A"), ("b", "MARKER-B")] {
        let dir = root.join("apps").join(app);
        write(
            &dir.join("polypack.toml"),
            &format!(
                "schema = 1\n\n[package]\nname = \"workspace-app-{app}\"\nversion = \"0.1.0\"\n\n\
                 [ui]\nframework = \"vanilla\"\nentry = \"ui/index.html\"\n\n\
                 [core]\ncrate = \"../../core\"\n\n[targets.web]\nformat = \"static-site\"\n"
            ),
        );
        write(
            &dir.join("ui/index.html"),
            &format!(
                "<!doctype html>\n<html><head><title>{app}</title></head><body>{marker}</body></html>\n"
            ),
        );
        write(&dir.join("ui/public/icon.svg"), "<svg></svg>\n");
    }
    write(
        &root.join("core/Cargo.toml"),
        "[workspace]\n\n[package]\nname = \"workspace-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(&root.join("core/src/main.rs"), "fn main() {}\n");
    tree
}

fn polypack(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polypack"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("polypack 可执行")
}

fn json_lines(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|err| panic!("--json 的 stdout 混入非 JSON 行：{line}（{err}）"))
        })
        .collect()
}

fn cached_events(events: &[serde_json::Value]) -> Vec<&serde_json::Value> {
    events
        .iter()
        .filter(|event| event["event"] == "cached")
        .collect()
}

#[test]
fn app_selector_builds_only_that_member_and_workspace_builds_both() {
    let tree = fixture();
    let root = tree.path();
    let packages = root.join(".polypack/packages");

    let only_a = polypack(&["build", "--app", "a", "--target", "web", "--json"], root);
    assert!(
        only_a.status.success(),
        "--app a 失败: {}",
        String::from_utf8_lossy(&only_a.stderr)
    );
    assert!(
        packages.join("a/web/site/index.html").is_file(),
        "--app a 应在 .polypack/packages/a/web/site 下产出"
    );
    assert!(!packages.join("b").exists(), "--app a 不得构建成员 b");
    assert!(
        !root.join("apps/b/dist").exists(),
        "--app a 不得运行成员 b 的 bundle"
    );
    assert!(
        !root.join(".polypack/artifacts/b").exists(),
        "成员 b 不应有产物快照"
    );
    let a_html =
        std::fs::read_to_string(packages.join("a/web/site/index.html")).expect("读取成员 a 产物");
    assert!(a_html.contains("MARKER-A"), "成员 a 产物内容错: {a_html}");

    let workspace = polypack(&["build", "--workspace", "--target", "web", "--json"], root);
    assert!(
        workspace.status.success(),
        "--workspace 失败: {}",
        String::from_utf8_lossy(&workspace.stderr)
    );
    for (app, marker) in [("a", "MARKER-A"), ("b", "MARKER-B")] {
        let html = std::fs::read_to_string(packages.join(app).join("web/site/index.html"))
            .unwrap_or_else(|err| panic!("读取成员 {app} 产物失败: {err}"));
        assert!(html.contains(marker), "成员 {app} 产物内容错: {html}");
        assert!(
            root.join("apps")
                .join(app)
                .join("dist/index.html")
                .is_file(),
            "成员 {app} 应有独立 dist"
        );
    }
}

#[test]
fn workspace_second_run_hits_per_app_cache_keys() {
    let tree = fixture();
    let root = tree.path();

    let first = polypack(&["build", "--workspace", "--target", "web", "--json"], root);
    assert!(
        first.status.success(),
        "首跑失败: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_events = json_lines(&first);
    let first_cached = cached_events(&first_events);
    assert_eq!(first_cached.len(), 2, "两个成员各一条 bundle 缓存事件");
    assert!(
        first_cached.iter().all(|event| event["hit"] == false),
        "首跑不应虚报命中: {first_cached:?}"
    );
    let keys: Vec<&str> = first_cached
        .iter()
        .filter_map(|event| event["key"].as_str())
        .collect();
    assert_eq!(keys.len(), 2);
    assert_ne!(keys[0], keys[1], "bundle 缓存键必须按应用区分");

    let second = polypack(&["build", "--workspace", "--target", "web", "--json"], root);
    assert!(
        second.status.success(),
        "二跑失败: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second_events = json_lines(&second);
    let second_cached = cached_events(&second_events);
    assert_eq!(second_cached.len(), 2, "两个成员各一条 bundle 缓存事件");
    assert!(
        second_cached.iter().all(|event| event["hit"] == true),
        "二跑两个成员都应命中: {second_cached:?}"
    );
}

#[test]
fn workspace_discovery_errors_use_stable_codes() {
    let tree = TempTree::new("errors");
    let root = tree.path();
    write(
        &root.join("polypack.toml"),
        "schema = 1\n\n[workspace]\nmembers = [\"apps/missing\"]\n",
    );
    let out = polypack(&["build", "--workspace", "--target", "web", "--json"], root);
    assert!(!out.status.success(), "缺失成员必须失败");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("polypack::E_WS_0002"),
        "缺失成员应报 E_WS_0002: {stderr}"
    );

    write(
        &root.join("polypack.toml"),
        "schema = 1\n\n[workspace]\nmembers = [\"../outside\"]\n",
    );
    let out = polypack(&["build", "--workspace", "--target", "web", "--json"], root);
    assert!(!out.status.success(), "越出根必须失败");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("polypack::E_WS_0003"),
        "越出根应报 E_WS_0003: {stderr}"
    );
}
