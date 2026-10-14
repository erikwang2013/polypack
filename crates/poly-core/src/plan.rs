//! 流水线 DAG：`Stage` 契约与标准阶段序（ROADMAP §5.1）。
//!
//! M0 交付阶段骨架：`plan` / `probe` 为真实实现，后续阶段以 `Skipped` 占位并在
//! M1+ 逐阶段替换（T-M1-01 及以后）。

use crate::diagnostics::{Diagnostic, codes};
use crate::events::{BuildEvent, EventSink};
use crate::job::JobSpec;

/// 阶段执行失败（携带稳定诊断）。
#[derive(Debug, Clone, PartialEq)]
pub struct StageError {
    /// 根因诊断。
    pub diagnostic: Diagnostic,
}

impl From<Diagnostic> for StageError {
    fn from(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

impl std::fmt::Display for StageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.diagnostic)
    }
}

/// 阶段执行结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageOutcome {
    /// 正常完成。
    Done,
    /// 跳过（附原因，如缓存命中或骨架占位）。
    Skipped {
        /// 跳过原因。
        reason: String,
    },
}

/// 阶段执行上下文。
pub struct StageContext<'a> {
    /// 本次构建请求。
    pub job: &'a JobSpec,
    /// 事件接收端。
    pub events: &'a mut dyn EventSink,
}

/// 流水线阶段的统一契约。
pub trait Stage: Send + Sync {
    /// 阶段名（与 §5.1 阶段序一致）。
    fn name(&self) -> &'static str;
    /// 仅做计划校验（不产生副作用）。
    fn plan(&self, job: &JobSpec) -> Result<(), StageError>;
    /// 执行本阶段。
    fn run(&self, ctx: &mut StageContext<'_>) -> Result<StageOutcome, StageError>;
}

/// 标准阶段序（§5.1）：plan → probe → compile → bundle → package → sign → verify → publish。
pub struct Plan {
    stages: Vec<Box<dyn Stage>>,
}

impl Plan {
    /// 标准阶段序（M0：前两阶段真实，其余为骨架占位）。
    pub fn standard() -> Self {
        Self {
            stages: vec![
                Box::new(PlanStage),
                Box::new(ProbeStage),
                Box::new(NotYetStage {
                    name: "compile",
                    since: "M1",
                }),
                Box::new(NotYetStage {
                    name: "bundle",
                    since: "M1",
                }),
                Box::new(NotYetStage {
                    name: "package",
                    since: "M1",
                }),
                Box::new(NotYetStage {
                    name: "sign",
                    since: "M2",
                }),
                Box::new(NotYetStage {
                    name: "verify",
                    since: "M2",
                }),
                Box::new(NotYetStage {
                    name: "publish",
                    since: "M2",
                }),
            ],
        }
    }

    /// 本 Plan 的阶段名序列（顺序即执行顺序）。
    pub fn names(&self) -> Vec<&'static str> {
        self.stages.iter().map(|stage| stage.name()).collect()
    }

    /// 阶段名序列（顺序即执行顺序）。
    pub fn stage_names() -> Vec<&'static str> {
        Self::standard().names()
    }

    /// 顺序执行全部阶段；任一阶段失败（含前置校验）即中止并发出 `Failed` 事件。
    pub fn run(&self, job: &JobSpec, events: &mut dyn EventSink) -> Result<(), StageError> {
        let mut ctx = StageContext { job, events };
        for stage in &self.stages {
            let outcome = stage.plan(ctx.job).and_then(|()| stage.run(&mut ctx));
            if let Err(err) = outcome {
                ctx.events.emit(&BuildEvent::Failed {
                    stage: stage.name().to_string(),
                    code: err.diagnostic.code.to_string(),
                    hint: err.diagnostic.hint.clone(),
                });
                return Err(err);
            }
        }
        Ok(())
    }
}

/// plan 阶段：校验 JobSpec（目标非空、id 不重复、目标声明合法、凭据引用齐备）。
struct PlanStage;

impl Stage for PlanStage {
    fn name(&self) -> &'static str {
        "plan"
    }

    fn plan(&self, job: &JobSpec) -> Result<(), StageError> {
        if job.targets.is_empty() {
            return Err(
                Diagnostic::error(codes::E_PLAN_0001, "JobSpec 未包含任何目标")
                    .with_hint("检查 polypack.toml 的 [targets.*] 段或 --target 参数")
                    .into(),
            );
        }
        let mut seen = std::collections::BTreeSet::new();
        for spec in &job.targets {
            if !seen.insert(spec.id.as_str()) {
                return Err(Diagnostic::error(
                    codes::E_PLAN_0001,
                    format!("目标重复: {}", spec.id),
                )
                .with_hint("每个目标在 JobSpec.targets 中只能出现一次")
                .into());
            }
            if let Err(err) = spec.validate() {
                return Err(Diagnostic::error(
                    codes::E_TGT_0003,
                    format!("目标 {} 声明非法: {err}", spec.id),
                )
                .with_hint("核对 polypack.toml 的 [targets.*] 段与目标注册表声明")
                .into());
            }
            if let Some(name) = &spec.signing
                && !job.secrets.iter().any(|secret| &secret.name == name)
            {
                return Err(Diagnostic::error(
                    codes::E_PLAN_0002,
                    format!(
                        "目标 {} 引用凭据档案 {name}，但 JobSpec 缺少对应凭据引用",
                        spec.id
                    ),
                )
                .with_hint("检查配置的 signing 档案与凭据来源（密钥库 / Vault / CI 注入）")
                .into());
            }
        }
        Ok(())
    }

    fn run(&self, ctx: &mut StageContext<'_>) -> Result<StageOutcome, StageError> {
        ctx.events.emit(&BuildEvent::Planned {
            targets: ctx.job.targets.iter().map(|t| t.id.clone()).collect(),
            excluded: ctx
                .job
                .excluded
                .iter()
                .map(|e| format!("{}（{}）", e.id, e.reason))
                .collect(),
        });
        Ok(StageOutcome::Done)
    }
}

/// probe 阶段：探测工具链并发布 `Probed` 事件。
///
/// M0 语义：工具链缺失**只告警不阻断**（门控归 `poly doctor`）——即便某 L1 目标
/// 所需工具链全部缺失，本阶段仍返回 `Done`（见 `tests/plan.rs` 的探针测试）。
/// 逐目标告警文本需要 `Probed` 之外的告警通道，留待 M1 扩展事件（本阶段不新增字段）。
struct ProbeStage;

impl Stage for ProbeStage {
    fn name(&self) -> &'static str {
        "probe"
    }

    fn plan(&self, _job: &JobSpec) -> Result<(), StageError> {
        Ok(())
    }

    fn run(&self, ctx: &mut StageContext<'_>) -> Result<StageOutcome, StageError> {
        let mut found = Vec::new();
        let mut missing = Vec::new();
        for probe in poly_toolchain::default_probes() {
            let result = probe.locate();
            if result.found.is_some() {
                found.push(result.id.to_string());
            } else {
                missing.push(result.id.to_string());
            }
        }
        ctx.events.emit(&BuildEvent::Probed { found, missing });
        Ok(StageOutcome::Done)
    }
}

/// M1+ 待实现的阶段占位（如实报告跳过原因，绝不伪装完成）。
struct NotYetStage {
    name: &'static str,
    since: &'static str,
}

impl Stage for NotYetStage {
    fn name(&self) -> &'static str {
        self.name
    }

    fn plan(&self, _job: &JobSpec) -> Result<(), StageError> {
        Ok(())
    }

    fn run(&self, _ctx: &mut StageContext<'_>) -> Result<StageOutcome, StageError> {
        Ok(StageOutcome::Skipped {
            reason: format!("阶段骨架：{} 在 {} 实现", self.name, self.since),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::HumanSink;
    use crate::job::{Budget, Profile};

    /// 骨架阶段必须如实报告 `Skipped`，绝不伪装完成。
    #[test]
    fn not_yet_stage_reports_skipped() {
        let job = JobSpec {
            id: "t".to_string(),
            profile: Profile::Local,
            targets: Vec::new(),
            excluded: Vec::new(),
            secrets: Vec::new(),
            budget: Budget::default(),
        };
        let mut sink = HumanSink { out: Vec::new() };
        let mut ctx = StageContext {
            job: &job,
            events: &mut sink,
        };
        let outcome = NotYetStage {
            name: "compile",
            since: "M1",
        }
        .run(&mut ctx)
        .expect("骨架阶段不应失败");
        assert!(
            matches!(outcome, StageOutcome::Skipped { .. }),
            "骨架阶段必须报告 Skipped，实得 {outcome:?}"
        );
    }
}
