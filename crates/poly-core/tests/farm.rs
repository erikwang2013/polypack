//! T-M2-13 验收（core 侧）：协议帧、目录传输后端、worker 拉取-执行-回传、注册准入。
//!
//! 跨机缓存与 iOS mock 契约见同目录 `farm_remote.rs`（同一套夹具；拆文件以守住行数上限）。
//! 夹具工程只有 `web` 目标（无 native 桌面目标 → `compile` 支线跳过、UI 走内建静态打包），
//! 因此全部用例都在毫秒级、且不依赖 node / 打包器 / 网络。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use poly_cache::Digest;
use poly_core::farm::{
    ArtifactRecord, CacheBackend, DirTransport, FarmFrame, FarmMaster, FarmTransport, JobOutcome,
    PROTOCOL, Requirement, WorkerCaps, WorkerOptions, run_worker,
};
use poly_core::{BuildEvent, EventSink, JobSpec, PolypackConfig, Profile, codes};
use poly_targets::{Host, Registry, TargetRequest, current_host};

// ---------------------------------------------------------------- 测试支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-farm-{tag}-{}-{}",
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

/// 夹具配置：web 目标（`react-dom` 适配器 → 静态打包），无 native 桌面目标。
const CONFIG_TEXT: &str = "schema = 1

[package]
name = \"farm-fixture\"
version = \"0.1.0\"

[ui]
framework = \"react\"
entry = \"ui/index.html\"

[core]
crate = \"core\"
";

/// 写夹具工程（`ui/index.html` + `ui/public/icon.svg`：静态打包后产物含入口与 PWA 图标）。
fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join("ui").join("public")).expect("创建工程目录");
    std::fs::write(root.join("polypack.toml"), CONFIG_TEXT).expect("写 polypack.toml");
    std::fs::write(
        root.join("ui").join("index.html"),
        "<!doctype html>\n<html>\n  <head>\n    <title>farm fixture</title>\n  </head>\n  <body></body>\n</html>\n",
    )
    .expect("写 index.html");
    std::fs::write(
        root.join("ui").join("public").join("icon.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n",
    )
    .expect("写 icon.svg");
}

/// 由夹具配置构造 `JobSpec`（`hosts` = 「可发起构建的宿主」）。
fn job_spec(hosts: &[Host], targets: &[&str]) -> JobSpec {
    let config = PolypackConfig::parse(CONFIG_TEXT).expect("夹具配置合法");
    let request = TargetRequest::Explicit(targets.iter().map(|id| id.to_string()).collect());
    config
        .to_job(&Registry::builtin(), &request, Profile::Ci, hosts)
        .expect("夹具目标可解析")
}

/// worker 侧接收端：只记录事件文本，不输出（保持测试 stdout 干净）。
#[derive(Default)]
struct Recorder {
    events: Vec<String>,
}

impl EventSink for Recorder {
    fn emit(&mut self, event: &BuildEvent) {
        self.events.push(format!("{event:?}"));
    }
}

/// 本机能力声明（triple 探测失败不阻断：声明文本不影响协议面断言）。
fn local_worker(id: &str, targets: &[&str]) -> WorkerCaps {
    WorkerCaps {
        worker_id: id.to_string(),
        triple: poly_core::host_triple().unwrap_or_else(|_| "unknown-triple".to_string()),
        host: current_host().expect("测试宿主必被识别"),
        targets: targets.iter().map(|id| id.to_string()).collect(),
    }
}

/// 本机宿主（测试运行地必被识别）。
fn host() -> Host {
    current_host().expect("测试宿主必被识别")
}

// ---------------------------------------------------------------- 协议面

#[test]
fn frame_roundtrip_is_single_line_and_version_guarded() {
    let caps = local_worker("w1", &["web"]);
    let frames = vec![
        FarmFrame::Hello {
            protocol: PROTOCOL.to_string(),
            worker: caps.clone(),
        },
        FarmFrame::HelloAck {
            protocol: PROTOCOL.to_string(),
            accepted: false,
            reason: Some("宿主不匹配".to_string()),
        },
        FarmFrame::Job {
            job_id: "job-1".to_string(),
            project_dir: "/tmp/project".to_string(),
            spec: job_spec(&[host()], &["web"]),
        },
        FarmFrame::Result {
            job_id: "job-1".to_string(),
            ok: true,
            error: None,
            artifacts: vec![ArtifactRecord {
                name: "dist/index.html".to_string(),
                digest: Digest::of_bytes(b"<html/>"),
                bytes: 7,
            }],
        },
        FarmFrame::Goodbye,
    ];
    for frame in &frames {
        let line = frame.encode();
        assert!(!line.contains('\n'), "JSON Lines 帧必须单行: {line}");
        assert_eq!(
            &FarmFrame::decode(&line).expect("帧可解析"),
            frame,
            "往返保真"
        );
    }

    let bad_version = FarmFrame::Hello {
        protocol: "farm/v1".to_string(),
        worker: caps,
    };
    let err = FarmFrame::decode(&bad_version.encode()).expect_err("版本不符必须失败");
    assert_eq!(err.code, codes::E_FARM_0001);
    assert!(err.hint.is_some(), "失败必须带修复提示");

    let err = FarmFrame::decode("{ 不是 JSON").expect_err("非法帧必须失败");
    assert_eq!(err.code, codes::E_FARM_0001);
}

#[test]
fn dir_transport_moves_frames_between_mailboxes() {
    let temp = TempDir::new("transport");
    let mut master = DirTransport::master(temp.path());
    let mut worker = DirTransport::worker(temp.path());

    assert!(
        worker
            .recv(Duration::from_millis(30))
            .expect("空信箱不是错误")
            .is_none(),
        "空信箱超时应返回 None（不失败）"
    );
    master.send(&FarmFrame::Goodbye).expect("投递");
    assert_eq!(
        worker.recv(Duration::from_secs(5)).expect("接收"),
        Some(FarmFrame::Goodbye)
    );
    assert!(
        worker
            .recv(Duration::from_millis(30))
            .expect("二次接收")
            .is_none(),
        "帧只能被取走一次"
    );
}

// ---------------------------------------------------------------- 执行与回传

#[test]
fn worker_loopback_runs_shared_dag_and_returns_artifacts() {
    let temp = TempDir::new("loopback");
    let project = temp.path().join("app");
    write_project(&project);
    let farm = temp.path().join("farm");
    let spec = job_spec(&[host()], &["web"]);

    let opts = WorkerOptions {
        dir: farm.clone(),
        caps: local_worker("loopback-worker", &["web"]),
        jobs: 1,
        idle: Duration::from_secs(30),
        package: None,
    };
    let worker = std::thread::spawn(move || {
        let mut events = Recorder::default();
        (
            run_worker(&opts, &CacheBackend::Local, &mut events),
            events.events,
        )
    });

    let mut master = FarmMaster::new(&farm);
    let require = Requirement {
        hosts: vec![host()],
        targets: vec!["web".to_string()],
    };
    let caps = master
        .accept_worker(&require, Duration::from_secs(10))
        .expect("注册应被接受");
    assert_eq!(caps.worker_id, "loopback-worker");
    let outcome = master
        .submit("job-1", &project, &spec, Duration::from_secs(60))
        .expect("结果回传");
    master.farewell().expect("收尾");

    let (exit, events) = worker.join().expect("worker 线程");
    let exit = exit.expect("worker 正常退出");
    assert_eq!(exit.jobs, 1, "退出信息: {exit:?}");
    assert!(
        events.iter().any(|event| event.contains("已注册")),
        "注册事件: {events:?}"
    );

    let JobOutcome::Done { artifacts } = outcome else {
        panic!("loopback 应成功: {outcome:?}")
    };
    let names: Vec<&str> = artifacts
        .iter()
        .map(|artifact| artifact.name.as_str())
        .collect();
    assert!(
        names.contains(&"dist/index.html"),
        "真实产物应回传: {names:?}"
    );
    assert!(
        names.contains(&"dist/manifest.webmanifest"),
        "PWA manifest 应回传: {names:?}"
    );
    assert!(
        artifacts.iter().all(|artifact| artifact.bytes > 0),
        "产物不得为空: {artifacts:?}"
    );
    assert!(
        project.join("dist").join("index.html").is_file(),
        "产物确实落盘（构建地）"
    );
}

#[test]
fn task_failure_returns_remote_code_and_hint() {
    // 构建地工程目录不可装载（无 polypack.toml）→ worker 端失败，错误码与修复提示完整回传
    let temp = TempDir::new("failure");
    let project = temp.path().join("broken");
    std::fs::create_dir_all(&project).expect("创建空工程目录");
    let farm = temp.path().join("farm");
    let spec = job_spec(&[host()], &["web"]);

    let opts = WorkerOptions {
        dir: farm.clone(),
        caps: local_worker("failing-worker", &["web"]),
        jobs: 1,
        idle: Duration::from_secs(30),
        package: None,
    };
    let worker = std::thread::spawn(move || {
        let mut events = Recorder::default();
        (
            run_worker(&opts, &CacheBackend::Local, &mut events),
            events.events,
        )
    });

    let mut master = FarmMaster::new(&farm);
    let require = Requirement {
        hosts: vec![host()],
        targets: vec!["web".to_string()],
    };
    master
        .accept_worker(&require, Duration::from_secs(10))
        .expect("注册应被接受");
    let outcome = master
        .submit("job-broken", &project, &spec, Duration::from_secs(60))
        .expect("失败也要回传");
    let _ = master.farewell();

    let (exit, events) = worker.join().expect("worker 线程");
    assert_eq!(
        exit.expect("worker 生命周期正常（任务失败不杀 worker）")
            .jobs,
        1
    );
    assert!(
        events.iter().any(|event| event.contains("poly::E_IO_0001")),
        "失败事件须带远端错误码: {events:?}"
    );

    let JobOutcome::Failed { error } = outcome else {
        panic!("应回传失败: {outcome:?}")
    };
    assert_eq!(error.code, "poly::E_IO_0001");
    assert!(error.hint.is_some(), "修复提示必须回传: {error:?}");
    // 主端诊断：远端码内嵌，退出码 1
    let diagnostic = error.to_diagnostic();
    assert_eq!(diagnostic.code, codes::E_FARM_0003);
    assert!(
        diagnostic.message.contains("poly::E_IO_0001"),
        "远端码须内嵌: {}",
        diagnostic.message
    );
    assert_eq!(diagnostic.exit_code(), 1);
}

// ---------------------------------------------------------------- 注册准入

#[test]
fn host_mismatch_rejects_registration_with_reason() {
    // 宿主阻断：iOS 目标声明 hosts=[Macos]，Linux worker 注册必须被拒（原因回传两端）
    let temp = TempDir::new("host-mismatch");
    let farm = temp.path().join("farm");
    let worker_farm = farm.clone();
    let worker = std::thread::spawn(move || {
        let mut transport = DirTransport::worker(&worker_farm);
        transport
            .send(&FarmFrame::Hello {
                protocol: PROTOCOL.to_string(),
                worker: WorkerCaps {
                    worker_id: "linux-worker".to_string(),
                    triple: "x86_64-unknown-linux-gnu".to_string(),
                    host: Host::Linux,
                    targets: vec!["ios".to_string()],
                },
            })
            .expect("发送注册帧");
        transport.recv(Duration::from_secs(10)).expect("收注册结果")
    });

    let mut master = FarmMaster::new(&farm);
    let require = Requirement {
        hosts: vec![Host::Macos],
        targets: vec!["ios".to_string()],
    };
    let err = master
        .accept_worker(&require, Duration::from_secs(10))
        .expect_err("宿主不匹配必须拒绝");
    assert_eq!(err.code, codes::E_FARM_0002);
    assert!(
        err.message.contains("Macos") && err.message.contains("Linux"),
        "{}",
        err.message
    );
    assert!(err.hint.is_some(), "拒绝必须带修复提示");

    match worker
        .join()
        .expect("worker 线程")
        .expect("worker 应收到注册结果")
    {
        FarmFrame::HelloAck {
            accepted: false,
            reason: Some(reason),
            ..
        } => {
            assert!(
                reason.contains("宿主不匹配"),
                "worker 应收到同一原因: {reason}"
            );
        }
        other => panic!("应收到拒绝的 hello_ack: {other:?}"),
    }
}

#[test]
fn worker_surfaces_rejected_registration_as_error() {
    let temp = TempDir::new("rejected");
    let farm = temp.path().join("farm");
    let worker_farm = farm.clone();
    let master = std::thread::spawn(move || {
        let mut transport = DirTransport::master(&worker_farm);
        transport
            .recv(Duration::from_secs(10))
            .expect("收注册帧")
            .expect("应有 hello");
        transport
            .send(&FarmFrame::HelloAck {
                protocol: PROTOCOL.to_string(),
                accepted: false,
                reason: Some("worker 未声明可构建目标 web".to_string()),
            })
            .expect("回拒绝");
    });

    let opts = WorkerOptions {
        dir: farm,
        caps: local_worker("rejected-worker", &[]),
        jobs: 1,
        idle: Duration::from_secs(30),
        package: None,
    };
    let mut events = Recorder::default();
    let err = run_worker(&opts, &CacheBackend::Local, &mut events).expect_err("注册被拒必须失败");
    master.join().expect("主端线程");
    assert_eq!(err.code, codes::E_FARM_0002);
    assert!(
        err.message.contains("worker 未声明可构建目标 web"),
        "{}",
        err.message
    );
    assert!(err.hint.is_some());
}
