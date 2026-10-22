//! M9-E2：`[workspace]` 发现、选择、稳定诊断与跨应用缓存语义。

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use common::spec_with;
use polypack_core::pipeline::{bundle_key, compile_key};
use polypack_core::web::UiEngine;
use polypack_core::{PolypackConfig, Profile, Project, codes, workspace};
use polypack_targets::{Host, TargetRequest};

/// 测试临时树：Drop 时整树删除。
struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "polypack-workspace-{tag}-{}-{}",
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

/// 工作区根配置。
fn write_root(root: &Path, members: &str) {
    write(
        &root.join("polypack.toml"),
        &format!("schema = 1\n\n[workspace]\nmembers = [{members}]\n"),
    );
}

/// 成员应用配置（共享 `../../core`）。
fn app_config(name: &str) -> String {
    format!(
        "schema = 1\n\n[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n\
         [ui]\nframework = \"vanilla\"\nentry = \"ui/index.html\"\n\n\
         [core]\ncrate = \"../../core\"\n"
    )
}

fn write_app(root: &Path, dir: &str, name: &str) {
    let app = root.join(dir);
    write(&app.join("polypack.toml"), &app_config(name));
    write(
        &app.join("ui/index.html"),
        "<!doctype html><html><body></body></html>\n",
    );
    write(&app.join("ui/public/icon.svg"), "<svg></svg>\n");
}

fn write_core(root: &Path) {
    write(
        &root.join("core/Cargo.toml"),
        "[workspace]\n\n[package]\nname = \"shared-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write(&root.join("core/src/main.rs"), "fn main() {}\n");
    write(&root.join("core/Cargo.lock"), "# lock\n");
}

#[test]
fn single_app_config_parses_and_job_json_stays_backward_compatible() {
    let config =
        PolypackConfig::parse(&common::fixture("polypack.min.toml")).expect("单应用夹具必须可解析");
    let project = Project::new(PathBuf::from("/tmp/single-app"), config);
    assert!(project.workspace_root.is_none());
    assert!(project.app_id.is_none());

    let registry =
        polypack_targets::Registry::from_specs(vec![spec_with("web", 1, vec![Host::Linux], None)])
            .expect("测试目标合法");
    let job = project
        .config
        .to_job(
            &registry,
            &TargetRequest::Explicit(vec!["web".to_string()]),
            Profile::Local,
            &[Host::Linux],
        )
        .expect("to_job 应成功");
    assert!(job.app_id.is_none());
    let json = serde_json::to_value(&job).expect("JobSpec 可序列化");
    assert!(
        json.get("app_id").is_none(),
        "单应用 JobSpec JSON 不应出现 app_id"
    );

    // 旧 JobSpec JSON（无 app_id）仍可反序列化。
    let mut legacy = json;
    legacy.as_object_mut().expect("对象").remove("app_id");
    let parsed: polypack_core::JobSpec = serde_json::from_value(legacy).expect("旧 JSON 必须兼容");
    assert!(parsed.app_id.is_none());
}

#[test]
fn two_apps_parse_and_select_by_id() {
    let tree = TempTree::new("two-apps");
    let root = tree.path();
    write_root(root, "\"apps/a\", \"apps/b\"");
    write_app(root, "apps/a", "app-a");
    write_app(root, "apps/b", "app-b");

    let discovered = workspace::discover(&root.join("polypack.toml")).expect("工作区应发现成功");
    let ids: Vec<&str> = discovered.apps.iter().map(|app| app.id.as_str()).collect();
    assert_eq!(ids, ["a", "b"], "成员 id 取目录名且保持声明顺序");
    assert_eq!(
        discovered.apps[0].project.root,
        std::fs::canonicalize(root.join("apps/a")).expect("成员目录存在")
    );
    assert_eq!(
        discovered.apps[0].project.workspace_root.as_deref(),
        Some(discovered.root.as_path())
    );
    assert_eq!(discovered.apps[0].project.app_id.as_deref(), Some("a"));

    let only_b = discovered.select(Some("b")).expect("成员 b 存在");
    assert_eq!(only_b.len(), 1);
    assert_eq!(only_b[0].id, "b");
    assert_eq!(discovered.select(None).expect("全部成员").len(), 2);
}

#[test]
fn shared_core_compile_key_is_identical_and_bundle_key_is_per_app() {
    let tree = TempTree::new("cache-keys");
    let root = tree.path();
    write_root(root, "\"apps/a\", \"apps/b\"");
    write_app(root, "apps/a", "same-name");
    write_app(root, "apps/b", "same-name");
    write_core(root);

    let discovered = workspace::discover(&root.join("polypack.toml")).expect("工作区应发现成功");
    let target = spec_with("web", 1, vec![Host::Linux], None);
    let key_a = compile_key(&discovered.apps[0].project, &target, "local").expect("编译键");
    let key_b = compile_key(&discovered.apps[1].project, &target, "local").expect("编译键");
    assert_eq!(
        key_a, key_b,
        "共享 core 同输入必须得到同一 compile 键（缓存可跨应用复用）"
    );

    let bundle_a =
        bundle_key(&discovered.apps[0].project, &UiEngine::Static, "local").expect("bundle 键");
    let bundle_b =
        bundle_key(&discovered.apps[1].project, &UiEngine::Static, "local").expect("bundle 键");
    assert_ne!(
        bundle_a, bundle_b,
        "bundle 键混入 app id：UI 产物缓存按应用隔离"
    );
}

#[test]
fn duplicate_member_ids_fail_with_stable_code() {
    let tree = TempTree::new("duplicate");
    let root = tree.path();
    write_root(root, "\"apps/a\", \"tools/a\"");
    write_app(root, "apps/a", "app-a");
    write_app(root, "tools/a", "app-a");

    let err = workspace::discover(&root.join("polypack.toml"))
        .err()
        .expect("重复 id 必须失败");
    assert_eq!(err.code, codes::E_WS_0001, "{err}");
    assert!(err.message.contains("id 重复"), "{err}");
}

#[test]
fn missing_member_or_config_fails_with_stable_code() {
    let tree = TempTree::new("missing");
    let root = tree.path();
    write_root(root, "\"apps/missing\"");
    let err = workspace::discover(&root.join("polypack.toml"))
        .err()
        .expect("缺目录必须失败");
    assert_eq!(err.code, codes::E_WS_0002, "{err}");

    // 目录存在但没有 polypack.toml。
    std::fs::create_dir_all(root.join("apps/no-config")).expect("建目录");
    write_root(root, "\"apps/no-config\"");
    let err = workspace::discover(&root.join("polypack.toml"))
        .err()
        .expect("缺成员配置必须失败");
    assert_eq!(err.code, codes::E_WS_0002, "{err}");
    assert!(err.message.contains("polypack.toml"), "{err}");
}

#[test]
fn escaping_member_path_fails_with_stable_code() {
    let tree = TempTree::new("escape");
    let root = tree.path();
    write_root(root, "\"../outside\"");

    let err = workspace::discover(&root.join("polypack.toml"))
        .err()
        .expect("越出根必须失败");
    assert_eq!(err.code, codes::E_WS_0003, "{err}");
}

#[test]
fn unknown_app_selection_fails_with_stable_code() {
    let tree = TempTree::new("unknown-app");
    let root = tree.path();
    write_root(root, "\"apps/a\"");
    write_app(root, "apps/a", "app-a");

    let discovered = workspace::discover(&root.join("polypack.toml")).expect("工作区应发现成功");
    let err = discovered
        .select(Some("nope"))
        .err()
        .expect("未知成员必须失败");
    assert_eq!(err.code, codes::E_WS_0004, "{err}");
}
