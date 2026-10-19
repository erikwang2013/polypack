//! worker 侧：拉取-执行-回传循环，以及本地 / 远程共用的任务执行入口。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use poly_cache::{CacheStats, Digest};

use super::isolation::{TaskWorkspace, WorkRoot};
use super::{
    CacheBackend, DirTransport, FarmFrame, FarmTransport, PROTOCOL, PROTOCOL_V0, RemoteDiagnostic,
    WorkerCaps, unexpected_frame,
};
use crate::config::PolypackConfig;
use crate::diagnostics::{Diagnostic, codes};
use crate::events::{BuildEvent, EventSink};
use crate::job::{JobSpec, Profile};
use crate::pipeline::{self, InjectedStages};
use crate::plan::{Project, Stage, StageError};

/// 注入式阶段工厂：`package` / `sign` / `verify` 的实现在 poly-bundle / poly-sign / poly-cli，
/// poly-core 不得依赖它们（依赖方向约束），故由调用方注入（[`InjectedStages`] 的元素类型）。
pub type StageFactory = fn() -> Box<dyn Stage>;

/// 任务作用域（worker 隔离与凭据内存注入的接缝，T-M8-07）。
///
/// core 在每任务前后成对调用：`enter`（一次性工作目录已建好）→ 执行 → `exit`
/// （成功 / 失败 / `enter` 失败后都会调用）。实现方在 poly-cli：把 `JobSpec.secrets`
/// 经 poly-sign 解析成内存态材料、并把签名临时根指向任务目录；`exit` 即销毁。
/// poly-core 不依赖 poly-sign，只持有本 trait 对象（不透明句柄）。
pub trait TaskScope {
    /// 任务开始；`workspace` 为本任务独享、0700 的一次性目录。
    fn enter(
        &mut self,
        spec: &JobSpec,
        workspace: &Path,
        events: &mut dyn EventSink,
    ) -> Result<(), Diagnostic>;
    /// 任务结束（成功、失败、`enter` 失败后都会调用）：销毁内存态凭据。
    fn exit(&mut self);
}

/// 任务作用域工厂：每次任务新建（避免跨任务复用状态）；可捕获 worker 启动配置
/// （如「凭据档案名 → 环境变量名」映射——只含名字，值绝不进命令行）。
pub type TaskScopeFactory = Box<dyn Fn() -> Box<dyn TaskScope> + Send + Sync>;

/// worker 运行参数。
pub struct WorkerOptions {
    /// 农场目录（双信箱根；主端与 worker 指向同一目录）。
    pub dir: PathBuf,
    /// 能力声明（注册载荷）。
    pub caps: WorkerCaps,
    /// 处理多少任务后退出（0 = 无限，直到 `goodbye` 或空闲超时）。
    pub jobs: usize,
    /// 空闲等待上限（超过即正常退出，不视为错误——「无任务可做」不是失败）。
    pub idle: Duration,
    /// `package` 阶段工厂（缺省保留 `Skipped` 占位）。
    pub package: Option<StageFactory>,
    /// `sign` 阶段工厂（缺省保留占位；签名在构建地按需拉取凭据，T-M2-11 模型）。
    pub sign: Option<StageFactory>,
    /// `verify` 阶段工厂（缺省保留占位）。
    pub verify: Option<StageFactory>,
    /// 一次性任务工作目录的父目录（缺省系统临时目录）：worker 启动时在其下建私有
    /// 0700 工作根，每任务一个子目录，任务结束即删除。
    pub work_root: Option<PathBuf>,
    /// 任务作用域工厂（CLI 注入凭据内存通道；core 不感知凭据类型）。
    pub task_scope: Option<TaskScopeFactory>,
}

/// worker 退出信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerExit {
    /// 已完成任务数（含失败任务：失败也已回传）。
    pub jobs: usize,
    /// 退出原因（人读文本）。
    pub reason: String,
}

/// v0 worker 主循环：根信箱注册（`hello` → `hello_ack`）→「拉取 → 执行 → 回传」→ 直至
/// `goodbye` / 任务用尽 / 空闲超时。保留 v0 版本身份与根信箱布局——`farm/v0` 兼容路径的
/// 在线参照（v1 主端据此协商到 v0）。
///
/// 任务失败**不**让 worker 退出：错误码与修复提示经 [`RemoteDiagnostic`] 完整回传给主端，
/// worker 继续服务（只有生命周期错误——注册被拒 / 帧非法 / IO 失败——才返回 `Err`）。
pub fn run_worker(
    opts: &WorkerOptions,
    backend: &CacheBackend,
    events: &mut dyn EventSink,
) -> Result<WorkerExit, Diagnostic> {
    let work_root = WorkRoot::create(opts.work_root.as_deref())?;
    let mut transport = DirTransport::worker(&opts.dir);
    let caps = opts.caps.clone();
    let result = match register(&mut transport, PROTOCOL_V0, &caps, opts.idle, events) {
        Ok(()) => service_loop(&mut transport, opts, backend, &work_root, events),
        Err(err) => Err(err),
    };
    finish_work_root(work_root, events);
    result
}

/// v1 worker 主循环：每 worker 独立信箱（`<dir>/v1/<worker_id>/`）注册，供主端池并发调度。
///
/// 帧字段与 v0 相同；`hello.worker.capacity` 声明本会话可接任务数（0 = 不限）。
/// `worker_id` 会用作信箱目录名，故拒绝路径分隔符（合法性在边界处一次校验）。
pub fn run_pool_worker(
    opts: &WorkerOptions,
    backend: &CacheBackend,
    events: &mut dyn EventSink,
) -> Result<WorkerExit, Diagnostic> {
    let work_root = WorkRoot::create(opts.work_root.as_deref())?;
    let result = serve_pool(opts, backend, &work_root, events);
    finish_work_root(work_root, events);
    result
}

/// v1 worker 的注册与服务主体（工作根已就绪；生命周期错误照常上抛）。
fn serve_pool(
    opts: &WorkerOptions,
    backend: &CacheBackend,
    work_root: &WorkRoot,
    events: &mut dyn EventSink,
) -> Result<WorkerExit, Diagnostic> {
    let peer = peer_dir(&opts.dir, &opts.caps.worker_id)?;
    std::fs::create_dir_all(&peer)
        .map_err(|err| super::io_error("创建 worker 信箱", &peer, err))?;
    let mut transport = DirTransport::worker(&peer);
    let mut caps = opts.caps.clone();
    caps.capacity = Some(opts.jobs);
    register(&mut transport, PROTOCOL, &caps, opts.idle, events)?;
    service_loop(&mut transport, opts, backend, work_root, events)
}

/// 工作根显式删除；失败必须发声（`E_FARM_0006`），不能静默留下任务残留。
fn finish_work_root(work_root: WorkRoot, events: &mut dyn EventSink) {
    if let Err(err) = work_root.remove() {
        events.emit(&BuildEvent::Failed {
            stage: "farm/isolation".to_string(),
            code: err.code.to_string(),
            hint: err.hint.clone(),
        });
    }
}

/// v1 worker 信箱目录：`<dir>/v1/<worker_id>`（目录名即调度地址，禁止路径分隔符）。
fn peer_dir(dir: &Path, worker_id: &str) -> Result<PathBuf, Diagnostic> {
    if worker_id.is_empty()
        || worker_id.contains(['/', '\\'])
        || worker_id == "."
        || worker_id == ".."
    {
        return Err(Diagnostic::error(
            codes::E_FARM_0002,
            format!("worker 标识不能用作信箱目录名: {worker_id:?}"),
        )
        .with_hint("--id 只用字母、数字、-、_（不含路径分隔符）"));
    }
    Ok(dir.join("v1").join(worker_id))
}

/// 注册：作废旧会话残留帧 → `hello` → 等 `hello_ack`（拒绝 / 超时即失败）。
fn register(
    transport: &mut DirTransport,
    protocol: &str,
    caps: &WorkerCaps,
    idle: Duration,
    events: &mut dyn EventSink,
) -> Result<(), Diagnostic> {
    // 注册前作废残留帧：上一轮会话留下的 `goodbye`（主端收尾后 worker 已按任务数退出，
    // 不再取件）若被当成注册结果，会把协议状态机误判为错帧。此刻收件箱里的帧必属已结束
    // 的会话——新主端在收到 `hello` 之前不会发声，故整体作废是安全的。
    while let Ok(Some(stale)) = transport.recv(Duration::ZERO) {
        events.emit(&BuildEvent::Warning {
            stage: "farm".to_string(),
            message: format!("作废上一轮会话的残留帧（{}）", stale.kind()),
        });
    }
    transport.send(&FarmFrame::Hello {
        protocol: protocol.to_string(),
        worker: caps.clone(),
    })?;
    let Some(frame) = transport.recv(idle)? else {
        return Err(
            Diagnostic::error(codes::E_FARM_0002, "等待注册结果超时（未收到 hello_ack）")
                .with_hint("确认主端 poly farm submit 已启动，且与 worker 指向同一 --dir 农场目录"),
        );
    };
    match frame {
        FarmFrame::HelloAck { accepted: true, .. } => {}
        FarmFrame::HelloAck {
            accepted: false,
            reason,
            ..
        } => {
            let reason = reason.unwrap_or_else(|| "主端未给原因".to_string());
            return Err(Diagnostic::error(
                codes::E_FARM_0002,
                format!("worker 注册被拒: {reason}"),
            )
            .with_hint("核对主端要求（宿主 / 可构建目标）与 worker 的 --target 声明"));
        }
        other => return Err(unexpected_frame("hello_ack", &other)),
    }
    events.emit(&BuildEvent::Warning {
        stage: "farm".to_string(),
        message: format!(
            "worker {} 已注册（{}，宿主 {:?}，可构建 {} 个目标，本次最多接 {} 个任务）",
            caps.worker_id,
            caps.triple,
            caps.host,
            caps.targets.len(),
            match caps.capacity {
                Some(0) | None => "不限".to_string(),
                Some(n) => n.to_string(),
            }
        ),
    });
    Ok(())
}

/// 「拉取 → 执行 → 回传」循环（v0 / v1 worker 共用）。
fn service_loop(
    transport: &mut DirTransport,
    opts: &WorkerOptions,
    backend: &CacheBackend,
    work_root: &WorkRoot,
    events: &mut dyn EventSink,
) -> Result<WorkerExit, Diagnostic> {
    let mut jobs = 0usize;
    let mut reason = "空闲超时（无任务）".to_string();
    while opts.jobs == 0 || jobs < opts.jobs {
        let Some(frame) = transport.recv(opts.idle)? else {
            break;
        };
        match frame {
            FarmFrame::Job {
                job_id,
                project_dir,
                spec,
            } => {
                let workspace = TaskWorkspace::create(work_root.path())?;
                let mut scope = opts.task_scope.as_ref().map(|factory| factory());
                let entered = match scope.as_mut() {
                    Some(scope) => scope.enter(&spec, workspace.path(), events),
                    None => Ok(()),
                };
                let result = match entered {
                    Ok(()) => execute_and_report(
                        &job_id,
                        Path::new(&project_dir),
                        &spec,
                        backend,
                        InjectedStages {
                            package: opts.package,
                            sign: opts.sign,
                            verify: opts.verify,
                        },
                        events,
                    ),
                    // 凭据注入失败 = 该任务失败（不碰构建），错误码与 hint 照常回传
                    Err(err) => {
                        events.emit(&BuildEvent::Failed {
                            stage: format!("farm/{job_id}"),
                            code: err.code.to_string(),
                            hint: err.hint.clone(),
                        });
                        FarmFrame::Result {
                            job_id: job_id.clone(),
                            ok: false,
                            error: Some(RemoteDiagnostic::from_diagnostic(&err)),
                            artifacts: Vec::new(),
                        }
                    }
                };
                if let Some(scope) = scope.as_mut() {
                    scope.exit();
                }
                if let Err(err) = workspace.remove() {
                    events.emit(&BuildEvent::Failed {
                        stage: "farm/isolation".to_string(),
                        code: err.code.to_string(),
                        hint: err.hint,
                    });
                }
                transport.send(&result)?;
                jobs += 1;
            }
            FarmFrame::Goodbye => {
                reason = "主端结束会话（goodbye）".to_string();
                break;
            }
            other => return Err(unexpected_frame("job", &other)),
        }
    }
    if opts.jobs > 0 && jobs >= opts.jobs {
        reason = format!("已完成 {jobs} 个任务");
    }
    Ok(WorkerExit { jobs, reason })
}

/// 执行一个任务并把结果编码为回传帧（成功 / 失败都回传；失败同时落 `Failed` 事件）。
fn execute_and_report(
    job_id: &str,
    project_dir: &Path,
    spec: &JobSpec,
    backend: &CacheBackend,
    stages: InjectedStages,
    events: &mut dyn EventSink,
) -> FarmFrame {
    match execute_job(project_dir, spec, backend, stages, events) {
        Ok(outcome) => {
            events.emit(&BuildEvent::Warning {
                stage: "farm".to_string(),
                message: format!(
                    "任务 {job_id} 完成：{} 个产物（缓存命中 {}/{}）",
                    outcome.artifacts.len(),
                    outcome.cache.hits,
                    outcome.cache.hits + outcome.cache.misses
                ),
            });
            FarmFrame::Result {
                job_id: job_id.to_string(),
                ok: true,
                error: None,
                artifacts: outcome.artifacts,
            }
        }
        Err(err) => {
            let error = RemoteDiagnostic::from_diagnostic(&err.diagnostic);
            events.emit(&BuildEvent::Failed {
                stage: format!("farm/{job_id}"),
                code: error.code.clone(),
                hint: error.hint.clone(),
            });
            FarmFrame::Result {
                job_id: job_id.to_string(),
                ok: false,
                error: Some(error),
                artifacts: Vec::new(),
            }
        }
    }
}

/// 一次任务执行的产物与缓存统计。
#[derive(Debug, Clone)]
pub struct ExecOutcome {
    /// 产物清单（核心二进制 + `dist/` 文件，相对路径升序）。
    pub artifacts: Vec<super::ArtifactRecord>,
    /// 本次执行的缓存命中统计（跨机复用验证用）。
    pub cache: CacheStats,
}

/// 在 `project_root` 执行一个任务：远程 worker 与本地 `poly build` 共用同一套 `Plan` DAG
/// （[`pipeline::plan_with_stages`]）与同一缓存键公式（§5.2），差别只在缓存后端由
/// [`CacheBackend`] 决定、打包器 / 签名 / 校验由调用方注入。
pub fn execute_job(
    project_root: &Path,
    spec: &JobSpec,
    backend: &CacheBackend,
    stages: InjectedStages,
    events: &mut dyn EventSink,
) -> Result<ExecOutcome, StageError> {
    let config = PolypackConfig::load(&project_root.join("polypack.toml"))
        .map_err(|err| err.diagnostic())?;
    let project = Project::new(project_root.to_path_buf(), config);
    let profile = match spec.profile {
        Profile::Ci => "ci",
        Profile::Local => "local",
    };
    // 缓存降级告警不能直接借用 `events`（`plan.run_with` 期间独占），先缓冲再统一发出
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let cache = Arc::new(backend.cache_with(project_root, {
        let warnings = Arc::clone(&warnings);
        move |message| {
            if let Ok(mut list) = warnings.lock() {
                list.push(message.to_string());
            }
        }
    }));
    drain_cache_warnings(&warnings, events); // 构造期告警（共享目录缺席）
    let plan = pipeline::plan_with_stages(cache.clone(), profile, stages)?;
    let ran = plan.run_with(Some(&project), spec, events);
    drain_cache_warnings(&warnings, events); // 运行期告警（后端 IO 降级）
    ran?;
    Ok(ExecOutcome {
        artifacts: collect_artifacts(&project),
        cache: cache.stats(),
    })
}

/// 把缓冲的缓存降级告警逐条发成 `Warning` 事件（阶段名 `cache`）。
fn drain_cache_warnings(warnings: &Mutex<Vec<String>>, events: &mut dyn EventSink) {
    if let Ok(mut list) = warnings.lock() {
        for message in list.drain(..) {
            events.emit(&BuildEvent::Warning {
                stage: "cache".to_string(),
                message,
            });
        }
    }
}

/// 产物记录：核心二进制（存在时）+ `dist/` 下全部文件（相对路径升序）。
fn collect_artifacts(project: &Project) -> Vec<super::ArtifactRecord> {
    let mut out = Vec::new();
    if let Ok(artifact) = pipeline::artifact_path(project)
        && let Some(record) = record_file(&project.root, &artifact)
    {
        out.push(record);
    }
    let mut files = Vec::new();
    collect_files(&project.root.join("dist"), &mut files);
    files.sort();
    for file in files {
        if let Some(record) = record_file(&project.root, &file) {
            out.push(record);
        }
    }
    out
}

/// 单文件 → 产物记录（名称 = 相对 `base` 的 `/` 分隔路径）。
fn record_file(base: &Path, path: &Path) -> Option<super::ArtifactRecord> {
    let bytes = std::fs::read(path).ok()?;
    Some(super::ArtifactRecord {
        name: path
            .strip_prefix(base)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/"),
        digest: Digest::of_bytes(&bytes),
        bytes: bytes.len() as u64,
    })
}

/// 递归收集普通文件（目录不存在按空计）。
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}
