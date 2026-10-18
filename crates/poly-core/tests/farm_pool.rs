//! T-M8-06 验收（core 侧）：worker 池并发调度、v0 版本协商、超时重试与失联负向用例。
//!
//! 本文件的 mock worker 不执行真实构建（只验协议 / 调度面）；真实多目标构建经农场完成
//! 由 `poly-cli/tests/farm_pool.rs` 的两个真 worker 子进程覆盖。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use poly_cache::Digest;
use poly_core::farm::{
    ArtifactRecord, DirTransport, FarmFrame, FarmPool, FarmTransport, JobOutcome, PROTOCOL,
    PROTOCOL_V0, PoolJob, PoolOptions, Requirement, WorkerCaps,
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
            "polypack-farm-pool-{tag}-{}-{}",
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

/// 夹具配置：web / chromeos-pwa 均为静态站点链，毫秒级、不依赖 node / 网络。
const CONFIG_TEXT: &str = "schema = 1

[package]
name = \"farm-pool-fixture\"
version = \"0.1.0\"

[ui]
framework = \"react\"
entry = \"ui/index.html\"

[core]
crate = \"core\"
";

/// 由夹具配置构造单目标 `JobSpec`。
fn job_spec(hosts: &[Host], target: &str) -> JobSpec {
    let config = PolypackConfig::parse(CONFIG_TEXT).expect("夹具配置合法");
    let request = TargetRequest::Explicit(vec![target.to_string()]);
    config
        .to_job(&Registry::builtin(), &request, Profile::Ci, hosts)
        .expect("夹具目标可解析")
}

/// 本机宿主（测试运行地必被识别）。
fn host() -> Host {
    current_host().expect("测试宿主必被识别")
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

/// 成功结果帧（产物名区分 worker / 目标）。
fn done(job_id: &str, name: &str) -> FarmFrame {
    let bytes = name.as_bytes();
    FarmFrame::Result {
        job_id: job_id.to_string(),
        ok: true,
        error: None,
        artifacts: vec![ArtifactRecord {
            name: name.to_string(),
            digest: Digest::of_bytes(bytes),
            bytes: bytes.len() as u64,
        }],
    }
}

/// v1 mock worker：注册 → 收一条任务 → 由 `on_job` 决定回不回结果 → 等 goodbye。
/// 返回线程句柄（值为收到 `hello_ack` 的协议版本）。
fn start_v1_worker(
    farm: &Path,
    id: &str,
    targets: &[&str],
    on_job: impl FnOnce(&str, &JobSpec) -> Option<FarmFrame> + Send + 'static,
) -> std::thread::JoinHandle<String> {
    let farm = farm.to_path_buf();
    let id = id.to_string();
    let targets: Vec<String> = targets.iter().map(|id| id.to_string()).collect();
    std::thread::spawn(move || {
        let peer = farm.join("v1").join(&id);
        std::fs::create_dir_all(&peer).expect("worker 信箱目录");
        let mut transport = DirTransport::worker(&peer);
        transport
            .send(&FarmFrame::Hello {
                protocol: PROTOCOL.to_string(),
                worker: WorkerCaps {
                    worker_id: id.clone(),
                    triple: "test-triple".to_string(),
                    host: host(),
                    targets,
                    capacity: Some(0), // 不限：池可连续派发
                },
            })
            .expect("发送注册帧");
        let ack = transport
            .recv(Duration::from_secs(10))
            .expect("收注册结果")
            .expect("应有 hello_ack");
        let protocol = match ack {
            FarmFrame::HelloAck {
                protocol,
                accepted: true,
                ..
            } => protocol,
            other => panic!("{id} 注册应被接受: {other:?}"),
        };
        match transport.recv(Duration::from_secs(10)).expect("收任务帧") {
            Some(FarmFrame::Job { job_id, spec, .. }) => {
                if let Some(result) = on_job(&job_id, &spec) {
                    transport.send(&result).expect("回传结果");
                }
            }
            Some(FarmFrame::Goodbye) => return protocol, // 池里没有它能接的任务
            other => panic!("{id} 应收到 job 或 goodbye: {other:?}"),
        }
        let _ = transport.recv(Duration::from_millis(500)); // 收尾 goodbye（失联 worker 收不到）
        protocol
    })
}

/// 并发屏障：两个 worker 都写下 started 标记后才继续——调度若退化串行，这里会超时。
fn barrier(dir: &Path, id: &str, peers: usize) {
    std::fs::write(dir.join(format!("{id}.started")), b"1").expect("写屏障标记");
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        let started = std::fs::read_dir(dir)
            .expect("屏障目录可读")
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".started"))
            .count();
        if started >= peers {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("并发屏障超时：另一个 worker 未同时开始执行（调度退化为串行）");
}

/// 池任务（单目标）。
fn pool_job(id: &str, target: &str, root: &Path) -> PoolJob {
    PoolJob {
        job_id: id.to_string(),
        target: target.to_string(),
        project_dir: root.join(id),
        spec: job_spec(&[host()], target),
    }
}

// ---------------------------------------------------------------- 并发调度

#[test]
fn pool_runs_multi_target_jobs_concurrently_on_two_workers() {
    let temp = TempDir::new("concurrent");
    let farm = temp.path().join("farm");
    let markers = temp.path().join("markers");
    std::fs::create_dir_all(&markers).expect("屏障目录");
    let jobs = vec![
        pool_job("job-web", "web", temp.path()),
        pool_job("job-pwa", "chromeos-pwa", temp.path()),
    ];

    let targets = ["web", "chromeos-pwa"];
    let worker_a = start_v1_worker(&farm, "a", &targets, {
        let markers = markers.clone();
        move |job_id, spec| {
            barrier(&markers, "a", 2);
            Some(done(job_id, &format!("dist/{}.html", spec.targets[0].id)))
        }
    });
    let worker_b = start_v1_worker(&farm, "b", &targets, {
        let markers = markers.clone();
        move |job_id, spec| {
            barrier(&markers, "b", 2);
            Some(done(job_id, &format!("dist/{}.html", spec.targets[0].id)))
        }
    });

    let mut pool = FarmPool::new(&farm);
    let require = Requirement {
        hosts: vec![host()],
        targets: targets.iter().map(|id| id.to_string()).collect(),
    };
    let caps = pool
        .accept_workers(&require, Duration::from_secs(10), 2)
        .expect("两个 worker 应注册成功");
    assert_eq!(caps.len(), 2, "池应收集到两个 worker: {caps:?}");

    let mut events = Recorder::default();
    let results = pool.submit_jobs(
        jobs,
        &PoolOptions {
            timeout: Duration::from_secs(5),
            retries: 0,
        },
        &mut events,
    );
    pool.farewell().expect("收尾");

    for result in &results {
        assert!(
            matches!(result.outcome, Ok(JobOutcome::Done { .. })),
            "并发任务应全部成功: {result:?}"
        );
    }
    let used: HashSet<&str> = results
        .iter()
        .map(|result| result.worker_id.as_str())
        .collect();
    assert_eq!(used.len(), 2, "两个任务应落在两个 worker 上: {results:?}");
    assert_eq!(worker_a.join().expect("worker a"), "farm/v1");
    assert_eq!(worker_b.join().expect("worker b"), "farm/v1");
    assert!(
        !events.events.iter().any(|event| event.contains("Failed")),
        "成功路径不该有 Failed 事件: {:?}",
        events.events
    );
}

// ---------------------------------------------------------------- v0 兼容

#[test]
fn v0_worker_negotiates_v0_and_completes_a_job() {
    let temp = TempDir::new("v0-compat");
    let farm = temp.path().join("farm");
    let worker_farm = farm.clone();
    let worker = std::thread::spawn(move || {
        let mut transport = DirTransport::worker(&worker_farm);
        // v0 worker：根信箱 + farm/v0 + 不声明 capacity（旧版 WorkerCaps 序列化形态）
        transport
            .send(&FarmFrame::Hello {
                protocol: PROTOCOL_V0.to_string(),
                worker: WorkerCaps {
                    worker_id: "v0-worker".to_string(),
                    triple: "x86_64-unknown-linux-gnu".to_string(),
                    host: host(),
                    targets: vec!["web".to_string()],
                    capacity: None,
                },
            })
            .expect("v0 注册帧");
        let protocol = match transport
            .recv(Duration::from_secs(10))
            .expect("收注册结果")
            .expect("应有 hello_ack")
        {
            FarmFrame::HelloAck {
                protocol,
                accepted: true,
                ..
            } => protocol,
            other => panic!("v0 注册应被接受: {other:?}"),
        };
        let job_id = match transport
            .recv(Duration::from_secs(10))
            .expect("收任务帧")
            .expect("应有 job")
        {
            FarmFrame::Job { job_id, spec, .. } => {
                assert_eq!(spec.targets[0].id, "web", "JobSpec 对 v0 worker 保持兼容");
                job_id
            }
            other => panic!("v0 worker 应收到 job: {other:?}"),
        };
        transport
            .send(&done(&job_id, "dist/v0.html"))
            .expect("回传结果");
        let _ = transport.recv(Duration::from_secs(5)); // goodbye
        protocol
    });

    let mut pool = FarmPool::new(&farm);
    let require = Requirement {
        hosts: vec![host()],
        targets: vec!["web".to_string()],
    };
    let caps = pool
        .accept_workers(&require, Duration::from_secs(10), 1)
        .expect("v0 worker 应接入");
    assert_eq!(caps.len(), 1);
    assert_eq!(caps[0].capacity, None, "v0 worker 未声明容量");

    let mut events = Recorder::default();
    let results = pool.submit_jobs(
        vec![pool_job("job-v0", "web", temp.path())],
        &PoolOptions {
            timeout: Duration::from_secs(10),
            retries: 0,
        },
        &mut events,
    );
    pool.farewell().expect("收尾");

    match &results[0].outcome {
        Ok(JobOutcome::Done { artifacts }) => {
            assert_eq!(artifacts[0].name, "dist/v0.html");
        }
        other => panic!("v0 worker 任务应成功: {other:?}"),
    }
    assert_eq!(
        worker.join().expect("v0 worker 线程"),
        "farm/v0",
        "版本协商：v1 主端以 farm/v0 回 hello_ack"
    );
}

// ---------------------------------------------------------------- 超时 / 失联负向

#[test]
fn timed_out_job_emits_failed_event_with_code_and_hint() {
    let temp = TempDir::new("timeout");
    let farm = temp.path().join("farm");
    // 注册后收任务但永不回传 = 失联 / 负载过高
    let worker = start_v1_worker(&farm, "slow", &["web"], |_, _| None);

    let mut pool = FarmPool::new(&farm);
    let require = Requirement {
        hosts: vec![host()],
        targets: vec!["web".to_string()],
    };
    pool.accept_workers(&require, Duration::from_secs(10), 1)
        .expect("worker 应注册");

    let mut events = Recorder::default();
    let results = pool.submit_jobs(
        vec![pool_job("job-timeout", "web", temp.path())],
        &PoolOptions {
            timeout: Duration::from_millis(120),
            retries: 0,
        },
        &mut events,
    );
    let err = results[0]
        .outcome
        .as_ref()
        .expect_err("未回传结果必须超时失败");
    assert_eq!(err.code, codes::E_FARM_0004, "稳定超时错误码: {err:?}");
    assert!(err.hint.is_some(), "超时诊断必须带修复提示: {err:?}");
    assert_eq!(results[0].worker_id, "slow");
    assert!(
        events
            .events
            .iter()
            .any(|event| event.contains("poly::E_FARM_0004") && event.contains("hint: Some")),
        "主端必须输出带 hint 的 Failed 事件: {:?}",
        events.events
    );
    let _ = pool.farewell();
    worker.join().expect("mock worker 线程");
}

#[test]
fn retry_after_timeout_succeeds_on_another_worker() {
    let temp = TempDir::new("retry");
    let farm = temp.path().join("farm");
    // 目录名排序决定首派发顺序：a-lost 拿到任务后失联，重试落到 b-good
    let lost = start_v1_worker(&farm, "a-lost", &["web"], |_, _| None);
    let good = start_v1_worker(&farm, "b-good", &["web"], |job_id, _| {
        Some(done(job_id, "dist/retried.html"))
    });

    let mut pool = FarmPool::new(&farm);
    let require = Requirement {
        hosts: vec![host()],
        targets: vec!["web".to_string()],
    };
    pool.accept_workers(&require, Duration::from_secs(10), 2)
        .expect("两个 worker 应注册");

    let mut events = Recorder::default();
    let results = pool.submit_jobs(
        vec![pool_job("job-retry", "web", temp.path())],
        &PoolOptions {
            timeout: Duration::from_millis(120),
            retries: 1,
        },
        &mut events,
    );
    pool.farewell().expect("收尾");

    match &results[0].outcome {
        Ok(JobOutcome::Done { artifacts }) => assert_eq!(artifacts[0].name, "dist/retried.html"),
        other => panic!("重试应成功: {other:?}"),
    }
    assert_eq!(results[0].worker_id, "b-good", "重试应换到另一 worker");
    assert!(
        events.events.iter().any(|event| event.contains("重试")),
        "重试应有告警事件: {:?}",
        events.events
    );
    assert!(
        !events.events.iter().any(|event| event.contains("Failed")),
        "重试成功不该有 Failed 事件: {:?}",
        events.events
    );
    lost.join().expect("失联 worker 线程");
    good.join().expect("正常 worker 线程");
}

#[test]
fn job_without_capable_worker_fails_with_lost_code_and_hint() {
    let temp = TempDir::new("no-worker");
    let farm = temp.path().join("farm");
    // worker 只声明 web；任务要 chromeos-pwa → 池能收它但派不出去
    let worker = start_v1_worker(&farm, "web-only", &["web"], |_, _| None);

    let mut pool = FarmPool::new(&farm);
    let require = Requirement {
        hosts: vec![host()],
        targets: vec!["web".to_string(), "chromeos-pwa".to_string()],
    };
    pool.accept_workers(&require, Duration::from_secs(10), 1)
        .expect("web worker 应注册");

    let mut events = Recorder::default();
    let results = pool.submit_jobs(
        vec![pool_job("job-pwa", "chromeos-pwa", temp.path())],
        &PoolOptions {
            timeout: Duration::from_millis(120),
            retries: 0,
        },
        &mut events,
    );
    let err = results[0]
        .outcome
        .as_ref()
        .expect_err("无可用 worker 必须失败");
    assert_eq!(err.code, codes::E_FARM_0005, "稳定失联错误码: {err:?}");
    assert!(err.hint.is_some(), "失联诊断必须带修复提示: {err:?}");
    assert!(
        err.message.contains("chromeos-pwa"),
        "错误信息应点名目标: {}",
        err.message
    );
    assert!(
        events
            .events
            .iter()
            .any(|event| event.contains("poly::E_FARM_0005") && event.contains("hint: Some")),
        "主端必须输出带 hint 的 Failed 事件: {:?}",
        events.events
    );
    pool.farewell().expect("收尾");
    worker.join().expect("mock worker 线程");
}
