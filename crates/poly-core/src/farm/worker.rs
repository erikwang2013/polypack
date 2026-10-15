//! worker 侧：拉取-执行-回传循环，以及本地 / 远程共用的任务执行入口。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use poly_cache::{CacheStats, Digest};

use super::{
    CacheBackend, DirTransport, FarmFrame, FarmTransport, PROTOCOL, RemoteDiagnostic, WorkerCaps,
    unexpected_frame,
};
use crate::config::PolypackConfig;
use crate::diagnostics::{Diagnostic, codes};
use crate::events::{BuildEvent, EventSink};
use crate::job::{JobSpec, Profile};
use crate::pipeline;
use crate::plan::{Project, Stage, StageError};

/// 注入式阶段工厂：`package` 阶段实现在 poly-bundle / poly-cli，poly-core 不得依赖它
/// （依赖方向约束），故由调用方注入（与 `pipeline::plan_with_package` 同模型）。
pub type StageFactory = fn() -> Box<dyn Stage>;

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
}

/// worker 退出信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerExit {
    /// 已完成任务数（含失败任务：失败也已回传）。
    pub jobs: usize,
    /// 退出原因（人读文本）。
    pub reason: String,
}

/// worker 主循环：注册（`hello` → `hello_ack`）→「拉取 → 执行 → 回传」→ 直至 `goodbye` /
/// 任务用尽 / 空闲超时。
///
/// 任务失败**不**让 worker 退出：错误码与修复提示经 [`RemoteDiagnostic`] 完整回传给主端，
/// worker 继续服务（只有生命周期错误——注册被拒 / 帧非法 / IO 失败——才返回 `Err`）。
pub fn run_worker(
    opts: &WorkerOptions,
    backend: &CacheBackend,
    events: &mut dyn EventSink,
) -> Result<WorkerExit, Diagnostic> {
    let mut transport = DirTransport::worker(&opts.dir);
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
        protocol: PROTOCOL.to_string(),
        worker: opts.caps.clone(),
    })?;
    let Some(frame) = transport.recv(opts.idle)? else {
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
            "worker {} 已注册（{}，宿主 {:?}，可构建 {} 个目标）",
            opts.caps.worker_id,
            opts.caps.triple,
            opts.caps.host,
            opts.caps.targets.len()
        ),
    });

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
                let result = execute_and_report(
                    &job_id,
                    Path::new(&project_dir),
                    &spec,
                    backend,
                    opts.package,
                    events,
                );
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
    package: Option<StageFactory>,
    events: &mut dyn EventSink,
) -> FarmFrame {
    match execute_job(project_dir, spec, backend, package, events) {
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
/// （[`pipeline::plan_with_package`]）与同一缓存键公式（§5.2），差别只在缓存后端由
/// [`CacheBackend`] 决定、打包器由调用方注入。
pub fn execute_job(
    project_root: &Path,
    spec: &JobSpec,
    backend: &CacheBackend,
    package: Option<StageFactory>,
    events: &mut dyn EventSink,
) -> Result<ExecOutcome, StageError> {
    let config = PolypackConfig::load(&project_root.join("polypack.toml"))
        .map_err(|err| err.diagnostic())?;
    let project = Project {
        root: project_root.to_path_buf(),
        config,
    };
    let profile = match spec.profile {
        Profile::Ci => "ci",
        Profile::Local => "local",
    };
    let cache = Arc::new(backend.cache(project_root));
    let plan =
        pipeline::plan_with_package(cache.clone(), profile, package.map(|factory| factory()))?;
    plan.run_with(Some(&project), spec, events)?;
    Ok(ExecOutcome {
        artifacts: collect_artifacts(&project),
        cache: cache.stats(),
    })
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
