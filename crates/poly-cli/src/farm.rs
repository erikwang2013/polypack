//! `poly farm`：远程农场协议 v1 的命令行入口（T-M8-06；v0 命令面见 T-M2-13）。
//!
//! - `poly farm worker --dir <目录>`：本机作为构建地注册（能力声明 = 宿主 / triple /
//!   `--target` 声明的可构建目标 / 本次可接任务数），信箱为 `<dir>/v1/<id>/`；
//!   同一农场目录可起多个 worker 组成池；
//! - `poly farm submit --dir <目录> --config <polypack.toml>`：主端按目标拆分任务，
//!   等 worker 池注册后并发投递、回收结果并合并产物清单；超时 / 失联按 `--retries`
//!   换 worker 重试，终态输出带 hint 的 `Failed` 事件；
//! - 两侧共用 `poly_core::farm` 的同一套执行实现（同一 `Plan` DAG、同一缓存键公式），
//!   缓存后端由 `--cache` 决定：`local`（工程本地）或共享目录（跨机复用）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use poly_core::farm::{
    ArtifactRecord, CacheBackend, FarmPool, JobOutcome, PoolJob, PoolOptions, Requirement,
    WorkerOptions, default_worker_id, local_caps, run_pool_worker,
};
use poly_core::{Diagnostic, JobSpec, Profile};
use poly_targets::{Host, Registry, TargetRequest, current_host};

/// `poly farm` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 子命令。
    #[command(subcommand)]
    pub command: FarmCommand,
}

/// farm 子命令。
#[derive(Debug, clap::Subcommand)]
pub enum FarmCommand {
    /// 以本机为构建地运行 worker（注册 → 拉取 → 执行 → 回传）。
    Worker(WorkerArgs),
    /// 向 worker 投递一次任务并回收结果与产物清单。
    Submit(SubmitArgs),
}

/// `poly farm worker` 的参数。
#[derive(Debug, clap::Args)]
pub struct WorkerArgs {
    /// 农场目录（双信箱根；主端与 worker 指向同一目录）。
    #[arg(long, default_value = ".polypack-farm")]
    pub dir: PathBuf,
    /// worker 标识（缺省 `<os>-<pid>`）。
    #[arg(long)]
    pub id: Option<String>,
    /// 本 worker 声明可构建的目标 id（可重复；缺省 = 不限，按本机宿主接受任务）。
    #[arg(long = "target", value_name = "ID")]
    pub targets: Vec<String>,
    /// 本次会话最多接多少个任务（0 = 直到主端 `goodbye` 或空闲超时）。
    #[arg(long, default_value_t = 0)]
    pub jobs: usize,
    /// 空闲等待上限（秒）：超时即正常退出（无任务可做不是失败）。
    #[arg(long = "idle", default_value_t = 60)]
    pub idle_secs: u64,
    /// 缓存后端：`local`（工程本地）或共享目录路径（多构建地共用，跨机复用）。
    #[arg(long, default_value = "local")]
    pub cache: String,
}

/// `poly farm submit` 的参数。
#[derive(Debug, clap::Args)]
pub struct SubmitArgs {
    /// 工程配置路径（**构建地**的 `polypack.toml`）。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// 农场目录（与 worker 一致）。
    #[arg(long, default_value = ".polypack-farm")]
    pub dir: PathBuf,
    /// 目标 id（可重复；缺省 = 当前宿主可达的全部已声明目标。显式目标不受发起地宿主过滤，
    /// 宿主可达性由 worker 准入把关）。
    #[arg(long = "target", value_name = "ID")]
    pub targets: Vec<String>,
    /// 构建 profile。
    #[arg(long, value_enum, default_value_t = crate::build::ProfileArg::Local)]
    pub profile: crate::build::ProfileArg,
    /// 等待 worker 注册 / 等待每个任务结果的上限（秒）。
    #[arg(long = "wait", default_value_t = 120)]
    pub wait_secs: u64,
    /// 任务超时 / worker 失联后的重试次数（每次换一个 worker；0 = 不重试）。
    #[arg(long, default_value_t = 1)]
    pub retries: usize,
    /// 构建地工程路径（缺省 = 本端工程根；跨机布局不同时显式给出。相对路径按各 worker
    /// 自己的当前目录解析，因此本机多 worker 各持一份工程副本时用 `.` 即可）。
    #[arg(long)]
    pub project_dir: Option<PathBuf>,
    /// 缓存后端：`local` 或共享目录路径（与 worker 用同一共享目录即跨机命中）。
    #[arg(long, default_value = "local")]
    pub cache: String,
}

/// 执行 `poly farm`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    match &args.command {
        FarmCommand::Worker(worker) => serve(worker, json),
        FarmCommand::Submit(job) => submit(job, json),
    }
}

/// worker：以本机为构建地注册并服务（能力声明见 `local_caps`）。
fn serve(args: &WorkerArgs, json: bool) -> Result<i32, Diagnostic> {
    let id = args.id.clone().unwrap_or_else(default_worker_id);
    let opts = WorkerOptions {
        dir: args.dir.clone(),
        // `--target` 缺省 = 本机宿主可达的全部已声明目标（该参数帮助文本的「不限」语义）
        caps: local_caps(&id, declared_targets(&args.targets))?,
        jobs: args.jobs,
        idle: Duration::from_secs(args.idle_secs),
        // package / sign / verify 由 CLI 注入（打包器矩阵在 poly-bundle、签名链与校验器在
        // poly-sign，poly-core 不得依赖它们）；签名在构建地按需拉取凭据（T-M2-11 模型）
        package: Some(|| Box::new(crate::package::PackageStage)),
        sign: Some(|| Box::new(crate::sign::SignStage)),
        verify: Some(|| Box::new(crate::verify::VerifyStage)),
    };
    let mut events = crate::project::sink(json);
    let exit = run_pool_worker(&opts, &cache_backend(&args.cache), events.as_mut())?;
    if !json {
        println!(
            "poly farm worker 退出：{}（已完成 {} 个任务）",
            exit.reason, exit.jobs
        );
    }
    Ok(0)
}

/// 主端：等 worker 池注册 → 按目标并发投递 → 合并结果（失败时错误码与修复提示完整上抛）。
fn submit(args: &SubmitArgs, json: bool) -> Result<i32, Diagnostic> {
    let project = crate::project::load(&args.config)?;
    let registry = Registry::builtin();
    let request = if args.targets.is_empty() || args.targets.iter().any(|id| id == "all") {
        TargetRequest::All
    } else {
        TargetRequest::Explicit(args.targets.clone())
    };
    // 农场投递的「可用宿主」：**显式**目标不做发起地过滤——任务在构建地执行，宿主准入交给
    // master 对 worker 的注册判据（下面的 `Requirement.hosts`），于是 Linux 也能发起
    // `--target ios` 去等 macOS worker；`--target all`（缺省）仍按本机宿主取可达子集
    // （与 `poly build` 一致，见该参数帮助）。本机过滤由 build 侧自行处理。
    let hosts = match &request {
        TargetRequest::Explicit(_) => declared_hosts(&registry),
        TargetRequest::All => current_host().into_iter().collect(),
    };
    let spec = project
        .config
        .to_job(&registry, &request, profile(args.profile), &hosts)
        .map_err(|err| err.diagnostic)?;

    // 注册准入判据（池语义：至少匹配一个目标即可入池；逐任务派发时再按目标复核）
    let mut hosts = Vec::new();
    for target in &spec.targets {
        for host in &target.hosts {
            if !hosts.contains(host) {
                hosts.push(*host);
            }
        }
    }
    let requirement = Requirement {
        hosts,
        targets: spec
            .targets
            .iter()
            .map(|target| target.id.clone())
            .collect(),
    };
    let wait = Duration::from_secs(args.wait_secs);
    let project_dir = args
        .project_dir
        .clone()
        .unwrap_or_else(|| project.root.clone());
    let jobs = split_jobs(&spec, &project_dir);
    let mut pool = FarmPool::new(&args.dir);
    let workers = pool.accept_workers(&requirement, wait, 1)?;
    if !json {
        for worker in &workers {
            println!(
                "worker 已注册：{}（{}，宿主 {:?}）",
                worker.worker_id, worker.triple, worker.host
            );
        }
    }
    let mut events = crate::project::sink(json);
    let results = pool.submit_jobs(
        jobs,
        &PoolOptions {
            timeout: wait,
            retries: args.retries,
        },
        events.as_mut(),
    );
    // 无论成败都礼貌收尾（worker 侧据此退出循环）
    let _ = pool.farewell();

    // 合并产物（按名称去重：各目标任务的 UI 产物同源）并定位首个失败任务
    let mut artifacts: BTreeMap<String, ArtifactRecord> = BTreeMap::new();
    let mut first_error = None;
    let mut rows = Vec::new();
    for result in &results {
        let failed = match &result.outcome {
            Ok(JobOutcome::Done { artifacts: list }) => {
                for artifact in list {
                    artifacts
                        .entry(artifact.name.clone())
                        .or_insert_with(|| artifact.clone());
                }
                None
            }
            // worker 端失败：错误码 + 修复提示完整回传（不吞错），退出码 1
            Ok(JobOutcome::Failed { error }) => Some(error.to_diagnostic()),
            Err(err) => Some(err.clone()),
        };
        if first_error.is_none() {
            first_error = failed.clone();
        }
        rows.push(serde_json::json!({
            "job_id": result.job_id,
            "target": result.target,
            "worker_id": result.worker_id,
            "ok": failed.is_none(),
            "code": failed.map(|err| err.code.to_string()),
        }));
    }
    if let Some(err) = first_error {
        return Err(err);
    }

    let job_id = format!("{}-{}", spec.id, std::process::id());
    let artifacts: Vec<ArtifactRecord> = artifacts.into_values().collect();
    if json {
        let out = serde_json::json!({
            "event": "farm-result",
            "job_id": job_id,
            "cache": args.cache,
            "workers": workers.iter().map(|worker| worker.worker_id.clone()).collect::<Vec<_>>(),
            "jobs": rows,
            "artifacts": artifacts.iter().map(|artifact| serde_json::json!({
                "name": artifact.name,
                "bytes": artifact.bytes,
                "digest": artifact.digest.as_str(),
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string(&out).expect("任务结果可序列化"));
    } else {
        println!(
            "农场任务完成：{} 个任务 / {} 个 worker，合并 {} 个产物",
            results.len(),
            workers.len(),
            artifacts.len()
        );
        for artifact in &artifacts {
            println!(
                "  {}（{} 字节，{}）",
                artifact.name, artifact.bytes, artifact.digest
            );
        }
    }
    Ok(0)
}

/// 按目标拆分任务：每个目标一条 [`PoolJob`]（池据此并发派发到不同 worker）。
///
/// 凭据引用按目标裁剪——只把该目标签名档案的引用随任务走（明文本就不在车道上）。
fn split_jobs(spec: &JobSpec, project_dir: &Path) -> Vec<PoolJob> {
    spec.targets
        .iter()
        .map(|target| {
            let mut single = spec.clone();
            single.targets = vec![target.clone()];
            single.secrets = spec
                .secrets
                .iter()
                .filter(|secret| target.signing.as_deref() == Some(secret.name.as_str()))
                .cloned()
                .collect();
            PoolJob {
                job_id: format!("{}-{}-{}", spec.id, target.id, std::process::id()),
                target: target.id.clone(),
                project_dir: project_dir.to_path_buf(),
                spec: single,
            }
        })
        .collect()
}

/// worker 的能力声明清单：`--target` 显式给出时照用；缺省 = 本机宿主可达的全部已声明目标
/// （于是「缺省 worker + 缺省 submit」在同机农场上正好互补，宿主准入判据不变）。
fn declared_targets(explicit: &[String]) -> Vec<String> {
    if !explicit.is_empty() {
        return explicit.to_vec();
    }
    let Some(host) = current_host() else {
        return Vec::new(); // local_caps 会先以 `poly::E_FARM_0002` 拒绝未知平台
    };
    Registry::builtin()
        .all()
        .filter(|spec| spec.hosts.contains(&host))
        .map(|spec| spec.id.clone())
        .collect()
}

/// 注册表中各目标已声明宿主的并集（显式目标的「可用宿主」：不按发起地过滤）。
fn declared_hosts(registry: &Registry) -> Vec<Host> {
    let mut hosts = Vec::new();
    for host in registry.all().flat_map(|spec| spec.hosts.iter()) {
        if !hosts.contains(host) {
            hosts.push(*host);
        }
    }
    hosts
}

/// `--cache` 解析：`local` → 工程本地 CAS；其余按共享目录（跨机复用验证用）。
fn cache_backend(value: &str) -> CacheBackend {
    match value {
        "local" => CacheBackend::Local,
        other => CacheBackend::Shared(PathBuf::from(other)),
    }
}

/// CLI profile → `JobSpec.profile`（`build::ProfileArg` 的同名映射是模块私有，故在此重列）。
fn profile(arg: crate::build::ProfileArg) -> Profile {
    match arg {
        crate::build::ProfileArg::Local => Profile::Local,
        crate::build::ProfileArg::Ci => Profile::Ci,
    }
}
