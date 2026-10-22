//! T-M1-02 真跑证据：宿主 triple 与 rustc 一致；最小 bin crate 的 release 编译产物
//! 落盘、可执行、输出含版本；compile 阶段的目标筛选与事件上报。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use common::spec_with;
use polypack_core::compile::{CompileStage, compile_core, host_triple};
use polypack_core::plan::{Project, Stage};
use polypack_core::{
    BuildEvent, EventSink, JobSpec, Plan, PolypackConfig, Profile, StageContext, StageOutcome,
};
use polypack_targets::{Bridge, Host, Runtime, TargetSpec};

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

/// 测试临时树：Drop 时整树删除（含 `compile_core` 在 crate 内生成的 `target/`）。
struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "polypack-{tag}-{}-{}",
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

/// 最小 bin crate（零依赖、离线可编译；独立 workspace 以免被上层目录的 Cargo.toml 吸附）。
fn write_fixture_crate(root: &Path) {
    let crate_dir = root.join("core");
    std::fs::create_dir_all(crate_dir.join("src")).expect("创建 fixture 目录");
    std::fs::write(
        crate_dir.join("Cargo.toml"),
        "[workspace]\n\n[package]\nname = \"polypack-compile-fixture\"\nversion = \"9.8.7\"\nedition = \"2021\"\n",
    )
    .expect("写 fixture Cargo.toml");
    std::fs::write(
        crate_dir.join("src/main.rs"),
        "fn main() {\n    println!(\"{}\", env!(\"CARGO_PKG_VERSION\"));\n}\n",
    )
    .expect("写 fixture main.rs");
}

fn project(root: &Path) -> Project {
    let config = PolypackConfig::parse(
        "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("配置合法");
    Project::new(root.to_path_buf(), config)
}

fn job(targets: Vec<TargetSpec>) -> JobSpec {
    JobSpec {
        id: "compile-test".to_string(),
        app_id: None,
        profile: Profile::Local,
        targets,
        excluded: Vec::new(),
        secrets: Vec::new(),
        budget: polypack_core::Budget::default(),
        plugins: Vec::new(),
    }
}

/// 宿主原生桌面目标（runtime=Native + bridge=TauriIpc）。
fn native_desktop_target() -> TargetSpec {
    let mut target = spec_with("linux", 1, vec![Host::Linux], None);
    target.runtime = Runtime::Native;
    target.bridge = Bridge::TauriIpc;
    target
}

#[test]
fn host_triple_matches_rustc_host_line() {
    let triple = host_triple().expect("rustc 可用");
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .expect("执行 rustc -vV");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected = stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::trim)
        .expect("rustc -vV 输出含 host 行");
    assert_eq!(triple, expected, "host_triple() 必须与 rustc -vV 一致");
}

#[test]
fn compile_core_builds_runnable_binary_and_stage_emits_compiled() {
    let tree = TempTree::new("compile");
    write_fixture_crate(tree.path());
    let project = project(tree.path());
    let target = native_desktop_target();

    let result = compile_core(&project, &target).expect("最小 crate 必须编译成功");
    assert_eq!(result.crate_name, "polypack-compile-fixture");
    let triple = host_triple().expect("rustc 可用");
    assert_eq!(result.triple, triple);
    assert!(
        result.artifact.is_file(),
        "产物必须落盘: {}",
        result.artifact.display()
    );
    assert!(
        result
            .artifact
            .starts_with(tree.path().join("core").join("target")),
        "产物应位于 crate 目录的 target/ 下: {}",
        result.artifact.display()
    );
    assert!(
        result.duration.as_secs() < 600,
        "编译耗时须被记录（实得 {:?}）",
        result.duration
    );

    let output = Command::new(&result.artifact)
        .output()
        .expect("产物必须可执行");
    assert!(output.status.success(), "产物运行应正常退出");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().contains("9.8.7"),
        "运行输出应含 crate 版本，实得: {stdout}"
    );

    // compile 阶段端到端：同一临时项目（增量、近乎零耗时）应发 Compiled 事件
    let mut sink = CaptureSink::default();
    let job = job(vec![target]);
    let mut ctx = StageContext {
        job: &job,
        events: &mut sink,
        project: Some(&project),
    };
    let outcome = CompileStage.run(&mut ctx).expect("compile 阶段不应失败");
    assert_eq!(outcome, StageOutcome::Done);
    assert_eq!(
        sink.events,
        vec![BuildEvent::Compiled {
            crate_name: "polypack-compile-fixture".to_string(),
            triple,
        }],
        "compile 阶段必须发 Compiled 事件"
    );
}

/// 多个宿主原生桌面目标只编译一次（同一 crate 同一 triple 复用），事件只发一个。
#[test]
fn compile_stage_compiles_once_for_multiple_native_desktop_targets() {
    let tree = TempTree::new("compile-once");
    write_fixture_crate(tree.path());
    let project = project(tree.path());

    let mut second = native_desktop_target();
    second.id = "linux-second".to_string();

    let mut sink = CaptureSink::default();
    let job = job(vec![native_desktop_target(), second]);
    let mut ctx = StageContext {
        job: &job,
        events: &mut sink,
        project: Some(&project),
    };
    let outcome = CompileStage.run(&mut ctx).expect("compile 阶段不应失败");
    assert_eq!(outcome, StageOutcome::Done);
    assert_eq!(sink.events.len(), 1, "多个原生桌面目标只发一个 Compiled");
}

/// 无项目上下文（如 `Plan::run` / `polypack doctor` 形态）必须 Skipped，绝不失败。
#[test]
fn compile_stage_without_project_is_skipped() {
    let mut sink = CaptureSink::default();
    let job = job(vec![native_desktop_target()]);
    let mut ctx = StageContext {
        job: &job,
        events: &mut sink,
        project: None,
    };
    let outcome = CompileStage.run(&mut ctx).expect("缺项目上下文绝不失败");
    assert_eq!(
        outcome,
        StageOutcome::Skipped {
            reason: "缺少项目上下文".to_string()
        }
    );
    assert!(sink.events.is_empty(), "跳过不发事件");
}

/// 无宿主原生桌面目标（runtime=none / wasm）→ 整体 Skipped，不触发任何编译。
#[test]
fn compile_stage_skips_non_native_desktop_targets() {
    let tree = TempTree::new("skip"); // 路径无需真实存在：跳过发生在任何 IO 之前
    let project = project(tree.path());

    let android = spec_with("android", 1, vec![Host::Linux], None); // runtime=None
    let mut web = spec_with("web", 1, vec![Host::Linux], None);
    web.runtime = Runtime::Wasm;
    web.bridge = Bridge::WasmBindgen;

    let mut sink = CaptureSink::default();
    let job = job(vec![android, web]);
    let mut ctx = StageContext {
        job: &job,
        events: &mut sink,
        project: Some(&project),
    };
    let outcome = CompileStage.run(&mut ctx).expect("非原生目标不得失败");
    assert!(
        matches!(outcome, StageOutcome::Skipped { .. }),
        "实得 {outcome:?}"
    );
    assert!(sink.events.is_empty(), "跳过不发事件");
}

/// 标准 DAG 中 compile 阶段带项目上下文真跑：Skipped-less 事件序列含 Compiled。
#[test]
fn standard_plan_with_project_emits_compiled_for_native_target() {
    let tree = TempTree::new("plan-compile");
    write_fixture_crate(tree.path());
    let project = project(tree.path());
    let target = native_desktop_target();

    let mut sink = CaptureSink::default();
    Plan::standard()
        .run_with(Some(&project), &job(vec![target]), &mut sink)
        .expect("标准 DAG 带项目上下文必须跑通");

    assert!(
        sink.events.iter().any(|event| matches!(
            event,
            BuildEvent::Compiled { crate_name, .. } if crate_name == "polypack-compile-fixture"
        )),
        "事件序列必须含 Compiled，实得 {:?}",
        sink.events
    );
    assert!(
        !sink
            .events
            .iter()
            .any(|event| matches!(event, BuildEvent::Failed { .. })),
        "成功路径不得出现 Failed 事件"
    );
}
