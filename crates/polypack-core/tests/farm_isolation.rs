//! T-M8-07 验收（core 侧）：worker 任务隔离——一次性私有工作目录、任务结束即销毁、
//! 跨任务无残留、网络输入不参与目录命名（路径逃逸）、清理不跟随符号链接。
//!
//! 进程边界本身由「worker 作为独立进程」承担（CLI 侧 `farm_isolation.rs` 用真 worker
//! 子进程 + 磁盘 / 日志扫描覆盖）；本文件用真实 worker 线程 + 夹具工程验证目录生命周期。
//! 容器边界未接入：无 docker 的宿主机按进程边界 + 私有临时目录真跑（见 CLI 用例的 SKIP 说明）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use polypack_core::farm::{
    CacheBackend, FarmMaster, JobOutcome, Requirement, TaskScope, TaskScopeFactory, WorkerCaps,
    WorkerOptions, run_worker,
};
use polypack_core::{BuildEvent, EventSink, JobSpec, PolypackConfig, Profile};
use polypack_targets::{Registry, TargetRequest, current_host};

// ---------------------------------------------------------------- 测试支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-farm-isolation-{tag}-{}-{}",
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

/// 夹具配置：web 目标（静态打包，毫秒级、无网络）。
const CONFIG_TEXT: &str = "schema = 1

[package]
name = \"farm-isolation-fixture\"
version = \"0.1.0\"

[ui]
framework = \"react\"
entry = \"ui/index.html\"

[core]
crate = \"core\"
";

fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("ui").join("public")).expect("创建工程目录");
    std::fs::write(root.join("polypack.toml"), CONFIG_TEXT).expect("写 polypack.toml");
    std::fs::write(
        root.join("ui").join("index.html"),
        "<!doctype html>\n<html>\n  <head>\n    <title>isolation fixture</title>\n  </head>\n  <body></body>\n</html>\n",
    )
    .expect("写 index.html");
    // 静态 Web 打包要求 PWA 图标输入（与 farm.rs 夹具同口径）
    std::fs::write(
        root.join("ui").join("public").join("icon.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n",
    )
    .expect("写 icon.svg");
}

fn job_spec() -> JobSpec {
    let config = PolypackConfig::parse(CONFIG_TEXT).expect("夹具配置合法");
    let request = TargetRequest::Explicit(vec!["web".to_string()]);
    let host = current_host().expect("测试宿主必被识别");
    config
        .to_job(&Registry::builtin(), &request, Profile::Ci, &[host])
        .expect("夹具目标可解析")
}

fn local_worker(id: &str) -> WorkerCaps {
    WorkerCaps {
        worker_id: id.to_string(),
        triple: polypack_core::host_triple().unwrap_or_else(|_| "unknown-triple".to_string()),
        host: current_host().expect("测试宿主必被识别"),
        targets: vec!["web".to_string()],
        capacity: None,
    }
}

/// 只记录事件文本（保持测试 stdout 干净）。
#[derive(Default)]
struct Recorder {
    events: Vec<String>,
}

impl EventSink for Recorder {
    fn emit(&mut self, event: &BuildEvent) {
        self.events.push(format!("{event:?}"));
    }
}

// ---------------------------------------------------------------- 记录型任务作用域

/// 每个任务的观测结果（目录是否一次性、退出前后残留、权限）。
#[derive(Debug)]
struct Observed {
    workspace: PathBuf,
    residue_existed_before_task: bool,
    residue_existed_at_exit: bool,
    #[cfg_attr(not(unix), allow(dead_code))]
    mode: u32,
}

/// 任务作用域：记录目录生命周期，并在任务内写入残留文件 + 指向目录外的符号链接，
/// 用于验证「任务结束整目录删除」与「清理不跟随符号链接」。
struct RecordingScope {
    observed: Arc<Mutex<Vec<Observed>>>,
    /// 目录外的「受害者」目录（符号链接目标）。
    // 仅 unix 用例读取（Windows 无 symlink 分支），避免 -D warnings 下的 dead_code
    #[cfg_attr(not(unix), allow(dead_code))]
    outside: PathBuf,
    residue: bool,
}

impl TaskScope for RecordingScope {
    fn enter(
        &mut self,
        spec: &JobSpec,
        workspace: &Path,
        _events: &mut dyn EventSink,
    ) -> Result<(), polypack_core::Diagnostic> {
        let residue = workspace.join("residue.txt");
        let existed_before = residue.exists();
        std::fs::write(&residue, spec.id.as_bytes()).expect("任务内写残留文件");
        // 符号链接指向目录外：清理必须只删链接本身，绝不跟随
        #[cfg(unix)]
        std::os::unix::fs::symlink(&self.outside, workspace.join("escape")).expect("建符号链接");
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(workspace)
                .expect("工作目录存在")
                .permissions()
                .mode()
                & 0o777
        };
        #[cfg(not(unix))]
        let mode = 0;
        self.observed.lock().expect("观测锁").push(Observed {
            workspace: workspace.to_path_buf(),
            residue_existed_before_task: existed_before,
            residue_existed_at_exit: false,
            mode,
        });
        self.residue = true;
        Ok(())
    }

    fn exit(&mut self) {
        if !self.residue {
            return;
        }
        let mut observed = self.observed.lock().expect("观测锁");
        let last = observed.last_mut().expect("enter 必已记录");
        last.residue_existed_at_exit = last.workspace.join("residue.txt").exists();
    }
}

/// 作用域工厂：每次任务新建（与 CLI 的 `--credential` 映射同一形态）。
fn recording_factory(observed: Arc<Mutex<Vec<Observed>>>, outside: PathBuf) -> TaskScopeFactory {
    Box::new(move || {
        Box::new(RecordingScope {
            observed: observed.clone(),
            outside: outside.clone(),
            residue: false,
        }) as Box<dyn TaskScope>
    })
}

// ---------------------------------------------------------------- 用例

/// 两个任务走真实 worker：工作目录一次性（第二任务看不到第一任务的残留）、0700、
/// 任务结束即删除、符号链接目标不受清理影响、工作根随 worker 退出删除。
#[test]
fn task_workspace_is_one_shot_private_and_cleanup_does_not_follow_symlinks() {
    let temp = TempDir::new("lifecycle");
    let project = temp.path().join("app");
    write_project(&project);
    let farm = temp.path().join("farm");
    let work_base = temp.path().join("work");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).expect("创建符号链接目标目录");
    std::fs::write(outside.join("keep.txt"), b"keep").expect("写受害者文件");

    let observed = Arc::new(Mutex::new(Vec::new()));
    let opts = WorkerOptions {
        dir: farm.clone(),
        caps: local_worker("isolation-worker"),
        jobs: 2,
        idle: Duration::from_secs(30),
        package: None,
        sign: None,
        verify: None,
        work_root: Some(work_base.clone()),
        task_scope: Some(recording_factory(observed.clone(), outside.clone())),
    };
    let worker = std::thread::spawn(move || {
        let mut events = Recorder::default();
        (
            run_worker(&opts, &CacheBackend::Local, &mut events),
            events.events,
        )
    });

    let host = current_host().expect("测试宿主必被识别");
    let require = Requirement {
        hosts: vec![host],
        targets: vec!["web".to_string()],
    };
    let spec = job_spec();
    let mut master = FarmMaster::new(&farm);
    master
        .accept_worker(&require, Duration::from_secs(10))
        .expect("注册应被接受");
    // 任务标识来自网络：含路径穿越成分也不得参与工作目录命名
    let malicious = format!("../../polypack-escape-{}", std::process::id());
    let first = master
        .submit(&malicious, &project, &spec, Duration::from_secs(60))
        .expect("第一任务回传");
    let second = master
        .submit("job-2", &project, &spec, Duration::from_secs(60))
        .expect("第二任务回传");
    master.farewell().expect("收尾");

    let (exit, _events) = worker.join().expect("worker 线程");
    assert_eq!(exit.expect("worker 正常退出").jobs, 2);
    assert!(matches!(first, JobOutcome::Done { .. }), "第一任务应成功");
    assert!(matches!(second, JobOutcome::Done { .. }), "第二任务应成功");

    let observed = observed.lock().expect("观测锁");
    assert_eq!(observed.len(), 2, "每个任务一个作用域: {observed:?}");
    let (first, second) = (&observed[0], &observed[1]);
    assert_ne!(first.workspace, second.workspace, "工作目录跨任务不复用");
    assert!(
        !first.residue_existed_before_task,
        "一次性目录在任务开始时必须为空（跨任务无残留）"
    );
    assert!(
        first.residue_existed_at_exit && second.residue_existed_at_exit,
        "残留文件在任务结束前应存在（删除发生在 exit 之后）"
    );
    for entry in observed.iter() {
        assert!(
            !entry.workspace.exists(),
            "任务结束即删除工作目录: {}",
            entry.workspace.display()
        );
        assert!(
            entry.workspace.starts_with(&work_base),
            "工作目录必须在 worker 私有根内: {}",
            entry.workspace.display()
        );
        #[cfg(unix)]
        assert_eq!(entry.mode, 0o700, "工作目录必须为 0700（仅 worker 可读）");
    }
    // 工作根随 worker 退出删除（work_base 为空）
    assert_eq!(
        std::fs::read_dir(&work_base)
            .expect("工作父目录存在")
            .count(),
        0,
        "worker 退出后工作根不得残留"
    );
    // 符号链接目标完好（remove_dir_all 只删链接本身）
    assert_eq!(
        std::fs::read_to_string(outside.join("keep.txt")).expect("受害者文件仍在"),
        "keep"
    );
    assert_eq!(
        std::fs::read_dir(&outside).expect("受害者目录可读").count(),
        1,
        "清理不得向符号链接目标写入 / 删除任何东西"
    );
    // 网络任务标识不参与路径：穿越名不得在父目录留下任何东西
    assert!(
        !temp
            .path()
            .join(format!("polypack-escape-{}", std::process::id()))
            .exists(),
        "任务标识中的 .. 不得造成路径逃逸"
    );
}

/// 工作目录创建失败（父路径被同名文件占住）→ worker 以 `E_FARM_0006` 拒绝启动，
/// 不留半成品目录（任务结束即销毁的边界条件）。
#[test]
fn unusable_work_root_fails_worker_startup_with_isolation_code() {
    let temp = TempDir::new("bad-root");
    let blocker = temp.path().join("blocked");
    std::fs::write(&blocker, b"not a directory").expect("写占位文件");
    let opts = WorkerOptions {
        dir: temp.path().join("farm"),
        caps: local_worker("bad-root-worker"),
        jobs: 1,
        idle: Duration::from_secs(5),
        package: None,
        sign: None,
        verify: None,
        work_root: Some(blocker.join("nested")),
        task_scope: None,
    };
    let mut events = Recorder::default();
    let err = run_worker(&opts, &CacheBackend::Local, &mut events)
        .expect_err("不可用的工作根必须让 worker 启动失败");
    assert_eq!(err.code, polypack_core::codes::E_FARM_0006);
    assert!(err.hint.is_some(), "隔离失败必须带修复提示");
}
