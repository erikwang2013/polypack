//! 主端（投递侧）：worker 注册准入、任务投递与结果回收。

use std::path::Path;
use std::time::Duration;

use poly_targets::Host;

use super::{
    ArtifactRecord, DirTransport, FarmFrame, FarmTransport, PROTOCOL, RemoteDiagnostic, WorkerCaps,
    unexpected_frame,
};
use crate::diagnostics::{Diagnostic, codes};
use crate::job::JobSpec;

/// 主端对 worker 的能力要求（注册准入判据）。
#[derive(Debug, Clone, Default)]
pub struct Requirement {
    /// 可接受的宿主集合（空 = 不限；构建地宿主须落在任务的目标声明内）。
    pub hosts: Vec<Host>,
    /// 必须声明的可构建目标 id（空 = 不限）。
    pub targets: Vec<String>,
}

/// 任务结果。
#[derive(Debug, Clone, PartialEq)]
pub enum JobOutcome {
    /// 成功：worker 回传的产物清单。
    Done {
        /// 产物记录。
        artifacts: Vec<ArtifactRecord>,
    },
    /// 失败：worker 端诊断完整回传（经 [`RemoteDiagnostic::to_diagnostic`] 还原为诊断）。
    Failed {
        /// worker 端诊断。
        error: RemoteDiagnostic,
    },
}

/// 主端（投递侧）：注册准入 → 投递任务 → 结果回收。
pub struct FarmMaster<T: FarmTransport> {
    transport: T,
}

impl FarmMaster<DirTransport> {
    /// 以农场目录构造（本地目录后端）。
    pub fn new(dir: &Path) -> Self {
        Self {
            transport: DirTransport::master(dir),
        }
    }
}

impl<T: FarmTransport> FarmMaster<T> {
    /// 以自定义传输后端构造（M3+ 远端后端接入点）。
    pub fn with_transport(transport: T) -> Self {
        Self { transport }
    }

    /// 接受一个 worker 注册：帧合法性由 [`FarmFrame::decode`] 把关，本方法校验能力声明。
    ///
    /// 不匹配 → `poly::E_FARM_0002`（同时给 worker 回 `accepted = false` 与同一原因）。
    pub fn accept_worker(
        &mut self,
        require: &Requirement,
        timeout: Duration,
    ) -> Result<WorkerCaps, Diagnostic> {
        let Some(frame) = self.transport.recv(timeout)? else {
            return Err(Diagnostic::error(
                codes::E_FARM_0002,
                "等待 worker 注册超时（未收到 hello）",
            )
            .with_hint("先启动 worker（poly farm worker --dir <目录>），再投递任务"));
        };
        let FarmFrame::Hello { worker, .. } = frame else {
            return Err(unexpected_frame("hello", &frame));
        };
        let mismatch = if !require.hosts.is_empty() && !require.hosts.contains(&worker.host) {
            Some((
                format!(
                    "宿主不匹配：任务需要 {:?}，worker 声明 {:?}",
                    require.hosts, worker.host
                ),
                "在匹配宿主的机器上起 worker（如 iOS 目标需 macOS worker），或改投其他目标",
            ))
        } else {
            require
                .targets
                .iter()
                .find(|id| !worker.targets.contains(id))
                .map(|missing| {
                    (
                        format!("worker {} 未声明可构建目标 {missing}", worker.worker_id),
                        "在 worker 的 --target 中声明该目标，或去掉主端的显式 --target",
                    )
                })
        };
        if let Some((reason, fix)) = mismatch {
            let _ = self.transport.send(&FarmFrame::HelloAck {
                protocol: PROTOCOL.to_string(),
                accepted: false,
                reason: Some(reason.clone()),
            });
            return Err(Diagnostic::error(codes::E_FARM_0002, reason).with_hint(fix));
        }
        self.transport.send(&FarmFrame::HelloAck {
            protocol: PROTOCOL.to_string(),
            accepted: true,
            reason: None,
        })?;
        Ok(worker)
    }

    /// 投递任务并等待该 `job_id` 的结果（v0：一次会话投递一个任务）。
    ///
    /// 超时 → `poly::E_FARM_0003`；任务失败经 [`JobOutcome::Failed`] 完整回传。
    pub fn submit(
        &mut self,
        job_id: &str,
        project_dir: &Path,
        spec: &JobSpec,
        timeout: Duration,
    ) -> Result<JobOutcome, Diagnostic> {
        self.transport.send(&FarmFrame::Job {
            job_id: job_id.to_string(),
            project_dir: project_dir.to_string_lossy().into_owned(),
            spec: spec.clone(),
        })?;
        let Some(frame) = self.transport.recv(timeout)? else {
            return Err(Diagnostic::error(
                codes::E_FARM_0003,
                format!("等待任务 {job_id} 的结果超时（worker 可能已退出或负载过高）"),
            )
            .with_hint("确认 worker 仍在运行；必要时提高 --wait 秒数"));
        };
        match frame {
            FarmFrame::Result {
                job_id: id,
                ok: true,
                artifacts,
                ..
            } if id == job_id => Ok(JobOutcome::Done { artifacts }),
            FarmFrame::Result {
                job_id: id,
                ok: false,
                error,
                ..
            } if id == job_id => Ok(JobOutcome::Failed {
                error: error.unwrap_or_else(|| RemoteDiagnostic {
                    code: codes::E_FARM_0003.to_string(),
                    message: "worker 报告失败但未附诊断".to_string(),
                    hint: None,
                }),
            }),
            other => Err(unexpected_frame("result", &other)),
        }
    }

    /// 结束会话：通知 worker 退出循环（v0：一次会话一个任务）。
    pub fn farewell(&mut self) -> Result<(), Diagnostic> {
        self.transport.send(&FarmFrame::Goodbye)
    }
}
