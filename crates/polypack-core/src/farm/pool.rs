//! 主端 worker 池（协议 v1）：worker 发现 / 注册、多任务并发调度、超时重试与失联判定。
//!
//! 信箱布局：v1 worker 各用 `<dir>/v1/<worker_id>/` 一对信箱（发现 = 主端扫描该目录）；
//! v0 worker 沿用根信箱 `<dir>/to-master` / `<dir>/to-worker`（单 worker，能力缺省一次一任务）。
//!
//! 调度：任务按目标逐条投递，worker 只接自己声明可构建且宿主匹配的目标；同批任务在各自
//! worker 线程上并发执行。任务超时 / 通信失败 → 标记该 worker 失联（信箱不复用），按
//! `PoolOptions::retries` 换 worker 重试；重试耗尽输出带 hint 的 `Failed` 事件
//! （[`codes::E_FARM_0004`] 超时 / [`codes::E_FARM_0005`] 失联或无可用 worker）。

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::{
    DirTransport, FarmFrame, FarmTransport, JobOutcome, POLL_INTERVAL, RemoteDiagnostic,
    Requirement, WorkerCaps,
};
use crate::diagnostics::{Diagnostic, codes};
use crate::events::{BuildEvent, EventSink};
use crate::job::JobSpec;

/// 已收到首个注册后的补充等待窗口：收集同批 worker，避免为后续 worker 干等到上限。
const SETTLE: Duration = Duration::from_millis(250);

/// 池调度参数。
#[derive(Debug, Clone)]
pub struct PoolOptions {
    /// 单个任务等待结果的上限。
    pub timeout: Duration,
    /// 任务超时 / 失联后的重试次数（每次换一个 worker；0 = 不重试）。
    pub retries: usize,
}

/// 投递到池的单目标任务（主端按目标拆分 [`JobSpec`]，`spec.targets` 恰一条）。
#[derive(Debug, Clone)]
pub struct PoolJob {
    /// 任务标识（结果帧按此对账）。
    pub job_id: String,
    /// 目标 id（调度判据 / 报告用）。
    pub target: String,
    /// **构建地**工程目录（v0 语义：工程已在 worker 可见；各 worker 路径可不同，跨机部署
    /// 用相对路径即可让每个 worker 解析到自己的工程副本）。
    pub project_dir: PathBuf,
    /// 单目标任务。
    pub spec: JobSpec,
}

/// 一个任务的最终结果（按提交顺序返回）。
#[derive(Debug, Clone)]
pub struct PoolResult {
    /// 任务标识。
    pub job_id: String,
    /// 目标 id。
    pub target: String,
    /// 执行该任务的 worker（无可用 worker 时为空串）。
    pub worker_id: String,
    /// 成功（产物清单）或失败诊断（超时 / 失联 / worker 回传的构建失败）。
    pub outcome: Result<JobOutcome, Diagnostic>,
}

/// 池内一个已注册 worker。
struct PoolWorker {
    caps: WorkerCaps,
    transport: Option<DirTransport>,
    /// 剩余可接任务数（`usize::MAX` = 不限；v0 worker 缺省 1）。
    remaining: usize,
    busy: bool,
}

impl PoolWorker {
    fn idle(&self) -> bool {
        !self.busy && self.remaining > 0 && self.transport.is_some()
    }

    /// 该 worker 能否执行此任务：声明目标一致且构建地宿主落在目标声明内。
    fn can_take(&self, job: &PoolJob) -> bool {
        job.spec.targets.iter().all(|target| {
            target.hosts.contains(&self.caps.host)
                && self.caps.targets.iter().any(|id| id == &target.id)
        })
    }
}

/// 主端 worker 池。
pub struct FarmPool {
    dir: PathBuf,
    workers: Vec<PoolWorker>,
    /// 已处理过的 v1 信箱目录名（被拒的也记下，避免反复取残留帧）。
    handled: Vec<String>,
    /// 根信箱（v0 worker）是否已处理过注册帧（v0 语义：单 worker）。
    legacy_handled: bool,
}

impl FarmPool {
    /// 以农场目录构造。
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            workers: Vec::new(),
            handled: Vec::new(),
            legacy_handled: false,
        }
    }

    /// 等待 worker 注册：凑齐 `min` 个（至少 1）后进入 [`SETTLE`] 补充窗口收集同批 worker。
    ///
    /// 准入与 v0 单 worker 一致（宿主 + 目标校验），但池语义放宽为「至少一个目标匹配」——
    /// 具体任务按 [`PoolWorker::can_take`] 逐条派发。一个都没等到 → `polypack::E_FARM_0002`；
    /// `wait` 内凑不齐 `min` 个时按已注册的返回（缺工由派发侧以 `E_FARM_0005` 暴露）。
    pub fn accept_workers(
        &mut self,
        require: &Requirement,
        wait: Duration,
        min: usize,
    ) -> Result<Vec<WorkerCaps>, Diagnostic> {
        let deadline = Instant::now() + wait;
        let min = min.max(1);
        let mut settle: Option<Instant> = None;
        loop {
            self.poll(require);
            let now = Instant::now();
            if self.workers.is_empty() {
                if now >= deadline {
                    return Err(Diagnostic::error(
                        codes::E_FARM_0002,
                        "等待 worker 注册超时（未收到 hello）",
                    )
                    .with_hint(
                        "先启动 worker（polypack farm worker --dir <目录>，可多个），再投递任务",
                    ));
                }
            } else if self.workers.len() >= min {
                let settle_deadline = *settle.get_or_insert(now + SETTLE);
                if now >= settle_deadline.min(deadline) {
                    return Ok(self
                        .workers
                        .iter()
                        .map(|worker| worker.caps.clone())
                        .collect());
                }
            } else if now >= deadline {
                return Ok(self
                    .workers
                    .iter()
                    .map(|worker| worker.caps.clone())
                    .collect());
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// 并发执行一批任务；返回顺序与 `jobs` 一致。超时 / 失联的终态会输出 `Failed` 事件。
    pub fn submit_jobs(
        &mut self,
        jobs: Vec<PoolJob>,
        opts: &PoolOptions,
        events: &mut dyn EventSink,
    ) -> Vec<PoolResult> {
        let total = jobs.len();
        let mut results: Vec<Option<PoolResult>> = (0..total).map(|_| None).collect();
        let mut queue: VecDeque<(usize, PoolJob, usize)> = jobs
            .into_iter()
            .enumerate()
            .map(|(index, job)| (index, job, opts.retries))
            .collect();
        let mut last_error: Vec<Option<Diagnostic>> = (0..total).map(|_| None).collect();
        let mut in_flight: Vec<Option<(PoolJob, usize)>> = (0..total).map(|_| None).collect();
        let (sender, receiver) = mpsc::channel::<(
            usize,
            usize,
            Option<DirTransport>,
            Result<JobOutcome, Diagnostic>,
        )>();

        loop {
            // 派发：每个空闲 worker 取一条它可执行的任务（同批任务因此在多个 worker 上并发）。
            while let Some((worker_index, queue_pos)) = self.pick(&queue) {
                let (job_index, job, attempts) =
                    queue.remove(queue_pos).expect("队列项刚由 pick 定位");
                let transport = self.workers[worker_index]
                    .transport
                    .take()
                    .expect("空闲 worker 必有信箱");
                self.workers[worker_index].busy = true;
                self.workers[worker_index].remaining =
                    self.workers[worker_index].remaining.saturating_sub(1);
                let worker_id = self.workers[worker_index].caps.worker_id.clone();
                let sender = sender.clone();
                let timeout = opts.timeout;
                let thread_job = job.clone();
                std::thread::spawn(move || {
                    let (transport, outcome) =
                        submit_on(transport, &thread_job, &worker_id, timeout);
                    let _ = sender.send((worker_index, job_index, transport, outcome));
                });
                in_flight[job_index] = Some((job, attempts));
            }

            if !self.workers.iter().any(|worker| worker.busy) {
                break; // 无在飞任务：剩余队列已无可派发的 worker
            }

            let (worker_index, job_index, transport, outcome) =
                receiver.recv().expect("任务线程存活");
            self.workers[worker_index].busy = false;
            let (job, attempts) = in_flight[job_index].take().expect("在飞任务可回收");
            let worker_id = self.workers[worker_index].caps.worker_id.clone();
            match outcome {
                Ok(outcome) => {
                    self.workers[worker_index].transport = transport;
                    results[job_index] = Some(PoolResult {
                        job_id: job.job_id,
                        target: job.target,
                        worker_id,
                        outcome: Ok(outcome),
                    });
                }
                Err(err) if is_transport_error(&err) => {
                    // 超时 / 通信失败 = worker 失联：信箱不复用
                    if attempts > 0 {
                        events.emit(&BuildEvent::Warning {
                            stage: "farm".to_string(),
                            message: format!(
                                "任务 {} 在 worker {worker_id} 上失败（{}），换 worker 重试（剩余 {} 次）",
                                job.job_id,
                                err.message,
                                attempts - 1
                            ),
                        });
                        last_error[job_index] = Some(err);
                        queue.push_back((job_index, job, attempts - 1));
                    } else {
                        emit_failed(&job, &err, events);
                        results[job_index] = Some(PoolResult {
                            job_id: job.job_id,
                            target: job.target,
                            worker_id,
                            outcome: Err(err),
                        });
                    }
                }
                Err(err) => {
                    // worker 回传的构建失败（确定性错误，不重试）
                    self.workers[worker_index].transport = transport;
                    emit_failed(&job, &err, events);
                    results[job_index] = Some(PoolResult {
                        job_id: job.job_id,
                        target: job.target,
                        worker_id,
                        outcome: Err(err),
                    });
                }
            }
        }

        // 队列剩余 = 无 worker 可接（能力不匹配 / 全部失联）：终态失败，带 hint 上抛。
        while let Some((job_index, job, _)) = queue.pop_front() {
            let err = last_error[job_index].take().unwrap_or_else(|| {
                Diagnostic::error(
                    codes::E_FARM_0005,
                    format!(
                        "任务 {} 无可用 worker（目标 {}）：注册的 worker 均未声明可构建该目标或已失联",
                        job.job_id, job.target
                    ),
                )
                .with_hint(
                    "启动声明该目标的 worker（polypack farm worker --target <id>），或去掉不适用的目标后重试",
                )
            });
            emit_failed(&job, &err, events);
            results[job_index] = Some(PoolResult {
                job_id: job.job_id,
                target: job.target,
                worker_id: String::new(),
                outcome: Err(err),
            });
        }
        results
            .into_iter()
            .map(|result| result.expect("每个任务都有终态"))
            .collect()
    }

    /// 结束会话：通知所有仍持有信箱的 worker 退出循环（失联 worker 已无信箱可发）。
    pub fn farewell(&mut self) -> Result<(), Diagnostic> {
        let mut first_error = None;
        for worker in &mut self.workers {
            if let Some(transport) = worker.transport.as_mut()
                && let Err(err) = transport.send(&FarmFrame::Goodbye)
                && first_error.is_none()
            {
                first_error = Some(err);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// 找一对 (空闲 worker, 队列中可派发任务) 的下标。
    fn pick(&self, queue: &VecDeque<(usize, PoolJob, usize)>) -> Option<(usize, usize)> {
        for (worker_index, worker) in self.workers.iter().enumerate() {
            if !worker.idle() {
                continue;
            }
            if let Some(job_index) = queue.iter().position(|(_, job, _)| worker.can_take(job)) {
                return Some((worker_index, job_index));
            }
        }
        None
    }

    /// 扫描 v1 信箱目录与 v0 根信箱，处理待注册帧。
    fn poll(&mut self, require: &Requirement) {
        let root = self.dir.join("v1");
        let mut peers: Vec<String> = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        peers.sort(); // 派发顺序可复现（文件系统序无关）
        for id in peers {
            if self.handled.contains(&id) {
                continue;
            }
            let mut transport = DirTransport::master(&root.join(&id));
            let Ok(Some(frame)) = transport.recv(Duration::ZERO) else {
                continue;
            };
            self.handled.push(id);
            self.admit(require, transport, frame);
        }
        if !self.legacy_handled {
            // v0 worker 的根信箱：只能容纳一个 worker（共享双信箱的固有语义）
            let mut transport = DirTransport::master(&self.dir);
            if let Ok(Some(frame)) = transport.recv(Duration::ZERO) {
                self.legacy_handled = true;
                self.admit(require, transport, frame);
            }
        }
    }

    /// 注册准入：帧必须是 `hello`；能力与需求至少一个目标匹配即接受。
    fn admit(&mut self, require: &Requirement, mut transport: DirTransport, frame: FarmFrame) {
        let FarmFrame::Hello { protocol, worker } = frame else {
            return; // 无协议版本可回；丢弃非注册帧
        };
        let Some(version) = FarmFrame::negotiate(&protocol) else {
            return; // decode 已拦不支持版本；防御性忽略
        };
        let host_ok = require.hosts.is_empty() || require.hosts.contains(&worker.host);
        let target_ok = require.targets.is_empty()
            || worker.targets.iter().any(|id| require.targets.contains(id));
        if !host_ok || !target_ok {
            let reason = if !host_ok {
                format!(
                    "宿主不匹配：任务需要 {:?}，worker 声明 {:?}",
                    require.hosts, worker.host
                )
            } else {
                format!("worker {} 未声明任务需要的任一目标", worker.worker_id)
            };
            let _ = transport.send(&FarmFrame::HelloAck {
                protocol: version.to_string(),
                accepted: false,
                reason: Some(reason),
            });
            return;
        }
        if transport
            .send(&FarmFrame::HelloAck {
                protocol: version.to_string(),
                accepted: true,
                reason: None,
            })
            .is_err()
        {
            return;
        }
        self.workers.push(PoolWorker {
            remaining: match worker.capacity {
                Some(0) => usize::MAX,
                Some(n) => n,
                None => 1, // v0 worker：一次会话一个任务
            },
            caps: worker,
            transport: Some(transport),
            busy: false,
        });
    }
}

/// 向一个 worker 投递任务并等结果；返回信箱（成功 / 失败可复用时）或失联（`None`）。
fn submit_on(
    mut transport: DirTransport,
    job: &PoolJob,
    worker_id: &str,
    timeout: Duration,
) -> (Option<DirTransport>, Result<JobOutcome, Diagnostic>) {
    let sent = transport.send(&FarmFrame::Job {
        job_id: job.job_id.clone(),
        project_dir: job.project_dir.to_string_lossy().into_owned(),
        spec: job.spec.clone(),
    });
    if let Err(err) = sent {
        return (None, Err(lost(&job.job_id, worker_id, &err.message)));
    }
    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return (None, Err(timed_out(&job.job_id, worker_id, timeout)));
        }
        match transport.recv(deadline - now) {
            Ok(None) => return (None, Err(timed_out(&job.job_id, worker_id, timeout))),
            Err(err) => return (None, Err(lost(&job.job_id, worker_id, &err.message))),
            Ok(Some(FarmFrame::Result {
                job_id,
                ok,
                error,
                artifacts,
            })) if job_id == job.job_id => {
                let outcome = if ok {
                    JobOutcome::Done { artifacts }
                } else {
                    JobOutcome::Failed {
                        error: error.unwrap_or_else(|| RemoteDiagnostic {
                            code: codes::E_FARM_0003.to_string(),
                            message: "worker 报告失败但未附诊断".to_string(),
                            hint: None,
                        }),
                    }
                };
                return (Some(transport), Ok(outcome));
            }
            Ok(Some(_)) => continue, // 过期 / 无关帧：跳过继续等本任务
        }
    }
}

/// 超时 / 通信失败都按「worker 失联」处理（可重试）。
fn is_transport_error(err: &Diagnostic) -> bool {
    err.code == codes::E_FARM_0004 || err.code == codes::E_FARM_0005
}

/// 任务超时诊断（`polypack::E_FARM_0004`）。
fn timed_out(job_id: &str, worker_id: &str, timeout: Duration) -> Diagnostic {
    Diagnostic::error(
        codes::E_FARM_0004,
        format!(
            "任务 {job_id} 等待结果超时（{}s）：worker {worker_id} 未回传",
            timeout.as_secs()
        ),
    )
    .with_hint("确认 worker 仍在运行且未失联；提高 --wait，或增加 worker 后重试（--retries）")
}

/// worker 通信失败诊断（`polypack::E_FARM_0005`）。
fn lost(job_id: &str, worker_id: &str, reason: &str) -> Diagnostic {
    Diagnostic::error(
        codes::E_FARM_0005,
        format!("任务 {job_id} 与 worker {worker_id} 通信失败: {reason}"),
    )
    .with_hint("检查农场目录权限与 worker 进程是否存活；修复后重试（--retries 会换 worker）")
}

/// 终态失败：输出带错误码与 hint 的 `Failed` 事件（主端事件流，与 worker 侧同形）。
fn emit_failed(job: &PoolJob, err: &Diagnostic, events: &mut dyn EventSink) {
    events.emit(&BuildEvent::Failed {
        stage: format!("farm/{}", job.job_id),
        code: err.code.to_string(),
        hint: err.hint.clone(),
    });
}
