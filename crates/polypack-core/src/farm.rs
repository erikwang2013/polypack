//! 远程农场协议 v1（T-M8-06；v0 帧格式见 T-M2-13）：帧格式、worker 注册、拉取-执行-回传、
//! 并发调度与跨机缓存。
//!
//! - **线格式**：JSON Lines（每帧一行 UTF-8 JSON），版本串 [`PROTOCOL`]（`farm/v1`）；
//!   帧序：`hello`（注册 + 能力声明）→ `hello_ack` → `job` → `result` → `goodbye`；
//!   帧字段与 v0 完全一致（`JobSpec` 序列化未变），版本协商取双方共同版本——
//!   v1 主端接受 `farm/v0` 旧 worker 并以 v0 回 `hello_ack`（[`PROTOCOL_V0`]）；
//! - **传输抽象**：[`FarmTransport`]；默认实现 [`DirTransport`]（本地目录双信箱），
//!   v1 worker 池为每 worker 一个信箱对（`<dir>/v1/<worker_id>/`），v0 worker 沿用根信箱；
//! - **执行**：worker 与本地 `polypack build` 共用同一 DAG 实现与同一缓存键公式，共享入口
//!   即 [`execute_job`]（内部走 `crate::pipeline::plan_with_stages`）；
//! - **跨机缓存**：[`CacheBackend`] 只换 `ObjectStore` 后端（本地文件系统为默认；
//!   共享目录 = [`polypack_cache::DirSharedStore`；S3 / OSS 兼容 = [`polypack_cache::S3Store`]）；
//!   远端不可用 / 运行中掉线时经 [`FallbackStore`] 降级到工程本地 CAS，构建不失败；
//! - **凭据边界**（T-M2-11 同模型）：`JobSpec.secrets` 只传 `crate::job::CredentialRef` 引用，
//!   绝不内联明文，凭据在构建地（worker）按需拉取；
//! - **隔离**（T-M8-07）：worker 进程边界之内，每任务一个 0700 一次性工作目录，任务结束
//!   整目录删除（目录名不含网络输入，符号链接不跟随）；凭据经 [`TaskScope`] 在 CLI /
//!   polypack-sign 侧解析为内存态材料，明文不落盘、不进 argv / 日志，任务结束即清空；
//! - **失败回传**：worker 端诊断经 [`RemoteDiagnostic`] 完整回传（错误码 + 修复提示）；
//!   任务超时 / worker 失联由主端判定（[`codes::E_FARM_0004`] / [`codes::E_FARM_0005`]）
//!   并输出带 hint 的 `Failed` 事件。

mod isolation;
mod master;
mod pool;
mod worker;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use polypack_cache::{Cache, Digest, DirSharedStore, FallbackStore, LocalStore, S3Config, S3Store};
use polypack_targets::{Host, current_host};
use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, codes};
use crate::job::JobSpec;

pub use master::{FarmMaster, JobOutcome, Requirement};
pub use pool::{FarmPool, PoolJob, PoolOptions, PoolResult};
pub use worker::{
    ExecOutcome, StageFactory, TaskScope, TaskScopeFactory, WorkerExit, WorkerOptions, execute_job,
    run_pool_worker, run_worker,
};

/// 当前协议版本（`hello` / `hello_ack` 携带；新 worker / 主端默认声明）。
pub const PROTOCOL: &str = "farm/v1";

/// 旧协议版本（v0 worker 仍可接入 v1 主端；帧字段与 v1 相同，仅版本串不同）。
pub const PROTOCOL_V0: &str = "farm/v0";

/// 本端支持的协议版本（版本协商：取双方共同版本）。
pub const SUPPORTED_PROTOCOLS: [&str; 2] = [PROTOCOL_V0, PROTOCOL];

/// 目录后端的轮询间隔（拉取循环的等待粒度）。
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// 协议帧：`job.spec` 即 [`JobSpec`] 的序列化形态（本节点的「JobSpec 序列化格式」）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "frame", rename_all = "snake_case")]
pub enum FarmFrame {
    /// worker → master：注册与能力声明。
    Hello {
        /// 协议版本串。
        protocol: String,
        /// 能力声明。
        worker: WorkerCaps,
    },
    /// master → worker：注册结果。
    HelloAck {
        /// 协议版本串。
        protocol: String,
        /// 是否接受。
        accepted: bool,
        /// 拒绝原因（`accepted = false` 时给出）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// master → worker：任务投递。
    Job {
        /// 任务标识。
        job_id: String,
        /// **构建地**的工程目录（v0 假定工程已同步到 worker；协议只传路径与 JobSpec）。
        project_dir: String,
        /// 本次构建请求。
        spec: JobSpec,
    },
    /// worker → master：结果回传。
    Result {
        /// 任务标识（与 `job` 帧一致）。
        job_id: String,
        /// 是否成功。
        ok: bool,
        /// 失败诊断（`ok = false` 时携带；错误码 + 修复提示完整回传）。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<RemoteDiagnostic>,
        /// 产物清单（`ok = true` 时携带）。
        #[serde(default)]
        artifacts: Vec<ArtifactRecord>,
    },
    /// master → worker：结束会话。
    Goodbye,
}

impl FarmFrame {
    /// 编码为一行 JSON（行内无换行：JSON Lines 的帧单位）。
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("帧可序列化")
    }

    /// 解析一行 JSON；格式非法 / 版本不受支持 → `polypack::E_FARM_0001` + 修复提示。
    pub fn decode(line: &str) -> Result<Self, Diagnostic> {
        let frame: Self = serde_json::from_str(line.trim()).map_err(|err| {
            Diagnostic::error(codes::E_FARM_0001, format!("农场协议帧解析失败: {err}")).with_hint(
                "线格式 = JSON Lines（farm/v0 或 farm/v1）；核对两端 polypack 版本与字段完整性",
            )
        })?;
        if let Some(version) = frame.protocol()
            && !SUPPORTED_PROTOCOLS.contains(&version)
        {
            return Err(Diagnostic::error(
                codes::E_FARM_0001,
                format!("协议版本不受支持: {version}（本端支持 farm/v0 / farm/v1）"),
            )
            .with_hint(
                "把主端与 worker 升级到同一 polypack 版本后重试（v1 主端仍接受 v0 worker）",
            ));
        }
        Ok(frame)
    }

    /// 帧自带的协议版本（仅 `hello` / `hello_ack` 携带）。
    pub fn protocol(&self) -> Option<&str> {
        match self {
            Self::Hello { protocol, .. } | Self::HelloAck { protocol, .. } => Some(protocol),
            _ => None,
        }
    }

    /// 帧类型名（诊断文本用）。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "hello",
            Self::HelloAck { .. } => "hello_ack",
            Self::Job { .. } => "job",
            Self::Result { .. } => "result",
            Self::Goodbye => "goodbye",
        }
    }

    /// 版本协商：`peer` 与本端共同支持的版本（不支持返回 `None`，由 [`Self::decode`] 拦截）。
    pub fn negotiate(peer: &str) -> Option<&'static str> {
        [PROTOCOL, PROTOCOL_V0]
            .into_iter()
            .find(|version| *version == peer)
    }
}

/// worker 能力声明（注册载荷）：构建地 triple / 宿主 / 可构建目标。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerCaps {
    /// worker 标识（多 worker 并存时区分）。
    pub worker_id: String,
    /// 构建地 triple（如 `aarch64-apple-darwin`）。
    pub triple: String,
    /// 构建地宿主（与 `TargetSpec.hosts` 同域，注册准入判据）。
    pub host: Host,
    /// 该 worker 声明可构建的目标 id 列表。
    pub targets: Vec<String>,
    /// v1：本次会话可接任务数（0 = 不限，直到 `goodbye` / 空闲超时）。
    /// v0 worker 无此字段 → `None`，主端按 v0 语义视为 1（一次会话一个任务）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity: Option<usize>,
}

/// 本机能力声明：宿主由编译期平台判定（未知平台 → `polypack::E_FARM_0002`）；triple 优先
/// `rustc -vV`，rustc 缺失时退化为 `<arch>-<os>` 形态（只影响声明文本，不阻断注册）。
pub fn local_caps(worker_id: &str, targets: Vec<String>) -> Result<WorkerCaps, Diagnostic> {
    let Some(host) = current_host() else {
        return Err(Diagnostic::error(
            codes::E_FARM_0002,
            format!("无法识别本机宿主平台（{}）", std::env::consts::OS),
        )
        .with_hint("farm v0 只在 Windows / Linux / macOS 上提供 worker"));
    };
    let triple = crate::compile::host_triple()
        .unwrap_or_else(|_| format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS));
    Ok(WorkerCaps {
        worker_id: worker_id.to_string(),
        triple,
        host,
        targets,
        capacity: None,
    })
}

/// 缺省 worker 标识：`<os>-<pid>`（多 worker 并存时用 `--id` 人工区分）。
pub fn default_worker_id() -> String {
    format!("{}-{}", std::env::consts::OS, std::process::id())
}

/// 回传的远端诊断：错误码以字符串承载（`Code` 故意不实现 `Deserialize`，见 diagnostics）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteDiagnostic {
    /// 稳定错误码文本（如 `polypack::E_PLAN_0001`）。
    pub code: String,
    /// 人类可读描述。
    pub message: String,
    /// 修复提示。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl RemoteDiagnostic {
    /// 由本地诊断构造（worker 回传前调用）。
    pub fn from_diagnostic(diagnostic: &Diagnostic) -> Self {
        Self {
            code: diagnostic.code.to_string(),
            message: diagnostic.message.clone(),
            hint: diagnostic.hint.clone(),
        }
    }

    /// 还原为主端诊断：`polypack::E_FARM_0003`，消息内嵌远端错误码与描述，修复提示原样保留。
    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic::error(
            codes::E_FARM_0003,
            format!("worker 执行失败：[{}] {}", self.code, self.message),
        )
        .with_hint(
            self.hint.clone().unwrap_or_else(|| {
                "在 worker 端复现（构建地日志含同一错误码）后重新投递".to_string()
            }),
        )
    }
}

/// 产物记录（名称 / 内容摘要 / 字节数）：跨机取回与缓存复用的对账凭据。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRecord {
    /// 产物名（相对工程根的 `/` 分隔路径）。
    pub name: String,
    /// 内容摘要（blake3，与缓存同域）。
    pub digest: Digest,
    /// 字节数。
    pub bytes: u64,
}

/// 传输抽象：发送 / 接收一帧；v0 默认实现为 [`DirTransport`]。
pub trait FarmTransport {
    /// 发送一帧。
    fn send(&mut self, frame: &FarmFrame) -> Result<(), Diagnostic>;
    /// 接收一帧；`timeout` 内无帧返回 `Ok(None)`（调用方据此走空闲 / 超时语义）。
    fn recv(&mut self, timeout: Duration) -> Result<Option<FarmFrame>, Diagnostic>;
}

/// 本地目录后端（默认实现）：双信箱 + 序号文件名，写入经临时文件原子改名。
///
/// 布局：主端发件箱 `<dir>/to-worker`（= worker 收件箱）、worker 发件箱 `<dir>/to-master`；
/// 帧文件名 `<tag>-<8 位序号>`（定宽 → 字典序 = 发送序）；`.` 前缀文件不参与收件。
pub struct DirTransport {
    outbox: PathBuf,
    inbox: PathBuf,
    tag: &'static str,
    seq: u64,
}

impl DirTransport {
    /// 主端视角（发 `to-worker`，收 `to-master`）。
    pub fn master(dir: &Path) -> Self {
        Self {
            outbox: dir.join("to-worker"),
            inbox: dir.join("to-master"),
            tag: "master",
            seq: 0,
        }
    }

    /// worker 视角（发 `to-master`，收 `to-worker`）。
    pub fn worker(dir: &Path) -> Self {
        Self {
            outbox: dir.join("to-master"),
            inbox: dir.join("to-worker"),
            tag: "worker",
            seq: 0,
        }
    }

    /// 取走收件箱中序号最小的一帧原始行（先改名再读：并发读者不会重复取走同一帧）。
    fn take_next(&self) -> Result<Option<String>, Diagnostic> {
        let Ok(entries) = std::fs::read_dir(&self.inbox) else {
            return Ok(None); // 收件箱未创建 = 暂无一帧
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| !name.starts_with('.'))
            .collect();
        names.sort();
        let Some(name) = names.into_iter().next() else {
            return Ok(None);
        };
        let consumed = self.inbox.join(format!(".{name}.taken"));
        if std::fs::rename(self.inbox.join(&name), &consumed).is_err() {
            return Ok(None); // 竞态：对方已收走
        }
        let line = std::fs::read_to_string(&consumed)
            .map_err(|err| io_error("读取农场帧", &consumed, err))?;
        let _ = std::fs::remove_file(&consumed);
        Ok(Some(line))
    }
}

impl FarmTransport for DirTransport {
    fn send(&mut self, frame: &FarmFrame) -> Result<(), Diagnostic> {
        std::fs::create_dir_all(&self.outbox)
            .map_err(|err| io_error("创建农场信箱", &self.outbox, err))?;
        self.seq += 1;
        let name = format!("{}-{:08}", self.tag, self.seq);
        let tmp = self.outbox.join(format!(".{name}.tmp"));
        std::fs::write(&tmp, format!("{}\n", frame.encode()))
            .map_err(|err| io_error("写入农场帧", &tmp, err))?;
        std::fs::rename(&tmp, self.outbox.join(name))
            .map_err(|err| io_error("投递农场帧", &self.outbox, err))
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<FarmFrame>, Diagnostic> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(line) = self.take_next()? {
                return FarmFrame::decode(&line).map(Some);
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

/// worker 侧缓存来源（只携带位置，不暴露具体后端类型：上层只经 `ObjectStore` trait 使用）。
pub enum CacheBackend {
    /// 工程本地 CAS：`<工程根>/.polypack/cache`（默认）。
    Local,
    /// 跨机共享对象存储：多构建地共用同一后端根目录（挂载共享盘即可）。
    Shared(PathBuf),
    /// S3 / OSS 兼容对象存储（SigV4；环境值已由调用方解析进配置）。
    ///
    /// 构造期探针失败或运行中读写失败都降级到工程本地 CAS，缓存错误不使构建失败。
    S3(S3Config),
}

impl CacheBackend {
    /// 构造 [`Cache`] 门面：同一 trait 换后端即跨机复用（缓存键公式不含绝对路径）。
    pub fn cache(&self, project_root: &Path) -> Cache {
        self.cache_with(project_root, |_| {})
    }

    /// 同 [`Self::cache`]，但共享后端缺席 / 运行中不可用时先经 `warn` 告警（每类只报一次）
    /// 再降级，绝不让构建失败：
    ///
    /// - 共享目录不可用（未挂载 / 路径被占用 / 权限不足）→ 直接用工程本地 CAS；
    /// - 运行中读写失败（掉线、权限、损坏）→ [`FallbackStore`] 落到工程本地 CAS。
    pub fn cache_with(
        &self,
        project_root: &Path,
        warn: impl Fn(&str) + Send + Sync + 'static,
    ) -> Cache {
        let local = || LocalStore::new(project_root.join(".polypack").join("cache"));
        match self {
            Self::Local => Cache::new(local()),
            Self::Shared(root) => {
                let shared = DirSharedStore::new(root);
                if let Err(err) = shared.ensure_ready() {
                    warn(&format!(
                        "共享缓存目录不可用（{}），已降级为工程本地缓存（构建继续）: {err}",
                        root.display()
                    ));
                    return Cache::new(local());
                }
                Cache::new(FallbackStore::new(shared, local(), warn))
            }
            Self::S3(config) => {
                // 构造 / 探针失败 = 配置或可达性问题：告警一次后直接本地，构建继续
                let store = match S3Store::new(config) {
                    Ok(store) => store,
                    Err(err) => {
                        warn(&format!(
                            "S3 兼容对象存储不可用（{config}），已降级为工程本地缓存（构建继续）: {err}"
                        ));
                        return Cache::new(local());
                    }
                };
                if let Err(err) = store.ensure_ready() {
                    warn(&format!(
                        "S3 兼容对象存储不可用（{config}），已降级为工程本地缓存（构建继续）: {err}"
                    ));
                    return Cache::new(local());
                }
                // 运行期错误（掉线 / 权限 / 5xx）经 FallbackStore 再告警一次并落本地
                Cache::new(FallbackStore::new(store, local(), warn))
            }
        }
    }
}

/// 非预期帧 → `polypack::E_FARM_0001`（携带期望与实得帧类型）。
fn unexpected_frame(expected: &str, frame: &FarmFrame) -> Diagnostic {
    Diagnostic::error(
        codes::E_FARM_0001,
        format!("农场协议帧非预期: 期望 {expected}，实得 {}", frame.kind()),
    )
    .with_hint(
        "主端与 worker 的协议状态机必须同版本（farm/v0 / farm/v1）：核对两端 polypack 版本与投递顺序",
    )
}

/// IO 失败 → 稳定诊断。
fn io_error(action: &str, path: &Path, err: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        codes::E_IO_0001,
        format!("{action}失败（{}）: {err}", path.display()),
    )
    .with_hint("检查农场目录是否存在、权限是否足够（主端与 worker 需共享同一目录）")
}
