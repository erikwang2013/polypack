//! T-M1-09 验收（core 侧）：缓存键的稳定性与敏感性，compile / bundle 支线的命中复用。

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use common::{spec_with, write_min_crate};
use poly_cache::Cache;
use poly_core::pipeline::{self, BundleStage, CompileStage};
use poly_core::web::UiEngine;
use poly_core::{
    Budget, BuildEvent, EventSink, JobSpec, PolypackConfig, Profile, Project, Stage, StageContext,
    StageOutcome,
};
use poly_targets::{Bridge, Host, Runtime, TargetSpec};

/// 捕获事件用于断言。
#[derive(Default)]
struct CaptureSink {
    events: Vec<BuildEvent>,
}

impl EventSink for CaptureSink {
    fn emit(&mut self, event: &BuildEvent) {
        self.events.push(event.clone());
    }
}

/// 测试临时树：Drop 时整树删除（含编译产物）。
struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "polypack-pipeline-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
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

fn project(root: &Path) -> Project {
    let config = PolypackConfig::parse(
        "[package]\nname = \"pipeline-fixture\"\nversion = \"0.1.0\"\n\
         [ui]\nframework = \"vanilla\"\nentry = \"ui/index.html\"\n\
         [core]\ncrate = \"core\"\n",
    )
    .expect("配置合法");
    Project {
        root: root.to_path_buf(),
        config,
    }
}

/// 最小 UI（静态打包即可产出）。
fn write_ui(root: &Path) {
    let ui = root.join("ui");
    std::fs::create_dir_all(ui.join("public")).expect("创建 ui 目录");
    // manifest 必须有图标资产，否则 write_manifest 报 E_WEB_0002
    std::fs::write(ui.join("public/icon.svg"), "<svg></svg>\n").expect("写 icon.svg");
    std::fs::write(
        ui.join("index.html"),
        "<!doctype html>\n<html>\n  <head><title>fixture</title></head>\n  <body></body>\n</html>\n",
    )
    .expect("写 index.html");
    std::fs::write(ui.join("main.js"), "console.log(\"fixture\");\n").expect("写 main.js");
}

/// 宿主原生桌面目标（runtime=Native + bridge=TauriIpc，ui=react-dom）。
fn native_desktop_target() -> TargetSpec {
    let mut target = spec_with("linux", 1, vec![Host::Linux], None);
    target.runtime = Runtime::Native;
    target.bridge = Bridge::TauriIpc;
    target
}

fn job(targets: Vec<TargetSpec>) -> JobSpec {
    JobSpec {
        id: "pipeline-test".to_string(),
        profile: Profile::Local,
        targets,
        excluded: Vec::new(),
        secrets: Vec::new(),
        budget: Budget::default(),
    }
}

fn cache_at(root: &Path) -> Arc<Cache> {
    Arc::new(Cache::from_root(root.join(".polypack/cache")))
}

/// 缓存键：同输入必须同键；源树 / 锁文件 / profile / 目标声明任一变化必须换键（级联失效）。
#[test]
fn compile_key_is_stable_and_sensitive_to_every_input() {
    let tree = TempTree::new("compile-key");
    write_min_crate(tree.path(), "core", "polypack-key-fixture");
    let project = project(tree.path());
    let target = native_desktop_target();

    let baseline = pipeline::compile_key(&project, &target, "local").expect("算键成功");
    assert_eq!(
        pipeline::compile_key(&project, &target, "local").expect("算键成功"),
        baseline,
        "同输入必须算出同键（否则永远无法命中）"
    );
    assert_ne!(
        pipeline::compile_key(&project, &target, "ci").expect("算键成功"),
        baseline,
        "profile 是键输入"
    );
    assert_ne!(
        pipeline::compile_key(&project, &native_desktop_target_with_id("linux-2"), "local")
            .expect("算键成功"),
        baseline,
        "目标声明是键输入"
    );

    // 锁文件是键输入
    std::fs::write(tree.path().join("core/Cargo.lock"), "version = 4\n").expect("写锁文件");
    let with_lock = pipeline::compile_key(&project, &target, "local").expect("算键成功");
    assert_ne!(with_lock, baseline, "补上锁文件必须换键");

    // 源树是键输入
    std::fs::write(
        tree.path().join("core/src/main.rs"),
        "fn main() { println!(\"changed\"); }\n",
    )
    .expect("改写 main.rs");
    assert_ne!(
        pipeline::compile_key(&project, &target, "local").expect("算键成功"),
        with_lock,
        "源码变化必须换键"
    );
}

fn native_desktop_target_with_id(id: &str) -> TargetSpec {
    let mut target = native_desktop_target();
    target.id = id.to_string();
    target
}

/// bundle 缓存键：源树 / 引擎 / profile 任一变化必须换键。
#[test]
fn bundle_key_tracks_ui_source_and_engine() {
    let tree = TempTree::new("bundle-key");
    write_ui(tree.path());
    let project = project(tree.path());

    let static_key = pipeline::bundle_key(&project, &UiEngine::Static, "local").expect("算键成功");
    assert_eq!(
        pipeline::bundle_key(&project, &UiEngine::Static, "local").expect("算键成功"),
        static_key,
        "同输入必须算出同键"
    );
    assert_ne!(
        pipeline::bundle_key(
            &project,
            &UiEngine::Vite {
                version: Some("7.3.7".to_string())
            },
            "local"
        )
        .expect("算键成功"),
        static_key,
        "引擎是键输入（vite 与静态打包产物不同）"
    );
    assert_ne!(
        pipeline::bundle_key(&project, &UiEngine::Static, "ci").expect("算键成功"),
        static_key,
        "profile 是键输入"
    );

    std::fs::write(
        tree.path().join("ui/main.js"),
        "console.log(\"changed\");\n",
    )
    .expect("改写 UI");
    assert_ne!(
        pipeline::bundle_key(&project, &UiEngine::Static, "local").expect("算键成功"),
        static_key,
        "UI 源码变化必须换键"
    );
}

/// compile 支线：首跑真编译并写缓存，二跑直接命中并从 CAS 还原产物（含首跑自建锁文件后键不变）。
#[test]
fn compile_stage_second_run_hits_cache_and_restores_artifact() {
    let tree = TempTree::new("compile-hit");
    write_min_crate(tree.path(), "core", "polypack-pipe-fixture");
    let project = project(tree.path());
    let stage = CompileStage::new(cache_at(tree.path()), "local");
    let job = job(vec![native_desktop_target()]);

    let mut first = CaptureSink::default();
    let mut ctx = StageContext {
        job: &job,
        events: &mut first,
        project: Some(&project),
    };
    assert_eq!(
        stage.run(&mut ctx).expect("首跑不应失败"),
        StageOutcome::Done
    );
    assert!(
        matches!(first.events[0], BuildEvent::Cached { hit: false, .. }),
        "首跑必须未命中：{:?}",
        first.events
    );
    assert!(
        first
            .events
            .iter()
            .any(|event| matches!(event, BuildEvent::Compiled { .. })),
        "首跑必须真编译并发 Compiled：{:?}",
        first.events
    );

    let artifact = pipeline::artifact_path(&project).expect("推导产物路径");
    assert!(artifact.is_file(), "产物必须落盘: {}", artifact.display());
    let bytes = std::fs::read(&artifact).expect("读产物");
    assert!(
        tree.path().join("core/Cargo.lock").is_file(),
        "首跑应已补上锁文件（ensure_lockfile）"
    );

    // 删掉产物：二跑若命中，必须从 CAS 还原出逐字节相同的产物
    std::fs::remove_file(&artifact).expect("删除产物");

    let mut second = CaptureSink::default();
    let mut ctx = StageContext {
        job: &job,
        events: &mut second,
        project: Some(&project),
    };
    assert_eq!(
        stage.run(&mut ctx).expect("二跑不应失败"),
        StageOutcome::Done
    );
    match &second.events[0] {
        BuildEvent::Cached { key, hit } => {
            assert!(*hit, "二跑必须命中（键须与首跑相同）");
            assert_eq!(
                key,
                match &first.events[0] {
                    BuildEvent::Cached { key, .. } => key,
                    other => panic!("首跑首事件应为 Cached，实得 {other:?}"),
                },
                "首跑自建锁文件不得改变缓存键"
            );
        }
        other => panic!("二跑首事件应为 Cached，实得 {other:?}"),
    }
    assert_eq!(
        std::fs::read(&artifact).expect("还原后的产物"),
        bytes,
        "命中还原的产物必须与首跑逐字节相同"
    );
}

/// bundle 支线：首跑打包，二跑从 CAS 还原（且不残留上一次的 dist 内容）。
#[test]
fn bundle_stage_second_run_restores_dist_from_cache() {
    let tree = TempTree::new("bundle-hit");
    write_ui(tree.path());
    let project = project(tree.path());
    let stage = BundleStage::new(cache_at(tree.path()), "local");
    let job = job(vec![native_desktop_target()]);
    let dist = tree.path().join("dist");

    let mut first = CaptureSink::default();
    let mut ctx = StageContext {
        job: &job,
        events: &mut first,
        project: Some(&project),
    };
    assert_eq!(
        stage.run(&mut ctx).expect("首跑不应失败"),
        StageOutcome::Done
    );
    assert!(
        matches!(first.events[0], BuildEvent::Cached { hit: false, .. }),
        "首跑必须未命中：{:?}",
        first.events
    );
    assert!(
        first.events.iter().any(
            |event| matches!(event, BuildEvent::UiBundled { adapter } if adapter == "react-dom")
        ),
        "bundle 必须报 react-dom 适配器：{:?}",
        first.events
    );
    let html = std::fs::read(dist.join("index.html")).expect("读产物 HTML");

    // 模拟「上一次构建的残留」：删掉 dist 再放一个哨兵文件
    std::fs::remove_dir_all(&dist).expect("删除 dist");
    std::fs::create_dir_all(&dist).expect("重建 dist");
    std::fs::write(dist.join("stale.js"), "stale\n").expect("写哨兵");

    let mut second = CaptureSink::default();
    let mut ctx = StageContext {
        job: &job,
        events: &mut second,
        project: Some(&project),
    };
    assert_eq!(
        stage.run(&mut ctx).expect("二跑不应失败"),
        StageOutcome::Done
    );
    assert!(
        matches!(second.events[0], BuildEvent::Cached { hit: true, .. }),
        "二跑必须命中：{:?}",
        second.events
    );
    assert_eq!(
        std::fs::read(dist.join("index.html")).expect("还原后的产物"),
        html,
        "命中还原的产物必须与首跑逐字节相同"
    );
    assert!(
        dist.join("manifest.webmanifest").is_file(),
        "manifest 必须还原"
    );
    assert!(
        !dist.join("stale.js").exists(),
        "命中还原不得残留上一次的 dist 内容"
    );
}

/// 入选目标无 react-dom 适配器时如实跳过（react-native 系 UI 打包归 M2）。
#[test]
fn bundle_stage_skips_when_no_react_dom_adapter() {
    let tree = TempTree::new("bundle-skip");
    write_ui(tree.path());
    let project = project(tree.path());
    let stage = BundleStage::new(cache_at(tree.path()), "local");
    let mut target = native_desktop_target();
    target.ui = vec!["react-native".to_string()];
    let job = job(vec![target]);

    let mut sink = CaptureSink::default();
    let mut ctx = StageContext {
        job: &job,
        events: &mut sink,
        project: Some(&project),
    };
    match stage.run(&mut ctx).expect("跳过不是失败") {
        StageOutcome::Skipped { reason } => assert!(
            reason.contains("react-dom") && reason.contains("M2"),
            "跳过原因须说明 DOM 适配器缺失且归 M2：{reason}"
        ),
        other => panic!("无 DOM 适配器时必须 Skipped，实得 {other:?}"),
    }
    assert!(sink.events.is_empty(), "跳过不得发事件：{:?}", sink.events);
}
