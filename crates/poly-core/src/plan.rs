//! 流水线 DAG：`Stage` 契约、拓扑分层调度与标准阶段序（ROADMAP §5.1）。
//!
//! 调度语义（T-M1-01）：
//! - 阶段按依赖分层（自实现 Kahn 分层，不引外部图库），层内以 `std::thread::scope` 真并行；
//! - 每节点事件先写入自有缓冲，join 后按节点声明顺序刷入真实 sink —— 事件序列与线程
//!   完成顺序无关（确定性）；
//! - 某层任一节点失败：该层全部节点的状态照常上报（失败节点追加 `Failed` 事件），随后
//!   整体中止 —— 依赖失败节点的后继不执行也不再发事件。
//!
//! 阶段实现：M0 交付 `plan` / `probe` 骨架，`compile` 见 [`crate::compile`]（T-M1-02），
//! 其余阶段以 `Skipped` 占位并在 M1+ 逐阶段替换。

use std::path::PathBuf;

use crate::config::PolypackConfig;
use crate::diagnostics::{Diagnostic, codes};
use crate::events::{BuildEvent, EventSink};
use crate::job::{JobError, JobSpec};

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

impl From<StageError> for Diagnostic {
    fn from(err: StageError) -> Self {
        err.diagnostic
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

/// 项目上下文：`polypack.toml` 的装载结果与项目根目录。
pub struct Project {
    /// 项目根目录（`polypack.toml` 所在目录）。
    pub root: PathBuf,
    /// 已装载并校验的配置。
    pub config: PolypackConfig,
}

/// 阶段执行上下文。
pub struct StageContext<'a> {
    /// 本次构建请求。
    pub job: &'a JobSpec,
    /// 事件接收端。
    pub events: &'a mut dyn EventSink,
    /// 项目上下文；`Plan::run`（= `run_with(None, ..)`）下为 `None`，此时需要项目上下文的
    /// 阶段必须 `Skipped { reason: "缺少项目上下文" }`，绝不失败。
    pub project: Option<&'a Project>,
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

/// DAG 中的一个阶段节点。
pub struct StageNode {
    /// 阶段名（同图内唯一；即事件与诊断中的 stage 标识）。
    pub name: &'static str,
    /// 前置阶段名（须为同图内已声明的节点）。
    pub deps: Vec<&'static str>,
    /// 阶段实现。
    pub stage: Box<dyn Stage>,
}

impl StageNode {
    /// 新建无依赖节点。
    pub fn new(name: &'static str, stage: impl Stage + 'static) -> Self {
        Self {
            name,
            deps: Vec::new(),
            stage: Box::new(stage),
        }
    }

    /// 链式声明前置依赖。
    #[must_use]
    pub fn after(mut self, deps: &[&'static str]) -> Self {
        self.deps.extend_from_slice(deps);
        self
    }
}

/// 标准阶段序（§5.1）：plan → probe → {compile, bundle} 双支线 → package → sign → verify → publish。
pub struct Plan {
    nodes: Vec<StageNode>,
    /// 拓扑分层：每层为 `nodes` 下标（层内按声明序）。
    layers: Vec<Vec<usize>>,
}

impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plan")
            .field("stages", &self.names())
            .finish()
    }
}

impl Plan {
    /// 标准阶段序：`compile` 为真实阶段（T-M1-02），`bundle`/`package`/`sign`/`verify`/`publish`
    /// 暂以 `NotYetStage` 占位，集成时逐个替换。
    pub fn standard() -> Self {
        Self::from_nodes(vec![
            StageNode::new("plan", PlanStage),
            StageNode::new("probe", ProbeStage).after(&["plan"]),
            StageNode::new("compile", crate::compile::CompileStage).after(&["probe"]),
            StageNode::new(
                "bundle",
                NotYetStage {
                    name: "bundle",
                    since: "M1",
                },
            )
            .after(&["probe"]),
            StageNode::new(
                "package",
                NotYetStage {
                    name: "package",
                    since: "M1",
                },
            )
            .after(&["compile", "bundle"]),
            StageNode::new(
                "sign",
                NotYetStage {
                    name: "sign",
                    since: "M2",
                },
            )
            .after(&["package"]),
            StageNode::new(
                "verify",
                NotYetStage {
                    name: "verify",
                    since: "M2",
                },
            )
            .after(&["sign"]),
            StageNode::new(
                "publish",
                NotYetStage {
                    name: "publish",
                    since: "M2",
                },
            )
            .after(&["verify"]),
        ])
        .expect("内置标准 DAG 合法（常量声明，构造即校验）")
    }

    /// 由图节点构造 Plan：校验重名 / 依赖缺失 / 成环（违规 → `poly::E_PLAN_0003`），
    /// 并预计算拓扑分层。
    pub fn from_nodes(nodes: Vec<StageNode>) -> Result<Self, StageError> {
        let mut names = std::collections::BTreeSet::new();
        for node in &nodes {
            if !names.insert(node.name) {
                return Err(Diagnostic::error(
                    codes::E_PLAN_0003,
                    format!("DAG 节点名重复: {}", node.name),
                )
                .with_hint("每个节点名必须唯一；阶段名即事件与诊断中的 stage 标识")
                .into());
            }
        }
        for node in &nodes {
            for dep in &node.deps {
                if !names.contains(dep) {
                    return Err(Diagnostic::error(
                        codes::E_PLAN_0003,
                        format!("DAG 节点 {} 依赖未定义的节点 {dep}", node.name),
                    )
                    .with_hint("把依赖改为已声明的节点名，或补上缺失的节点")
                    .into());
                }
            }
        }

        // Kahn 分层：每轮取「依赖全部就绪」的节点为一层（按声明序）；轮空即成环。
        let mut pending: Vec<usize> = (0..nodes.len()).collect();
        let mut done = std::collections::BTreeSet::new();
        let mut layers: Vec<Vec<usize>> = Vec::new();
        while !pending.is_empty() {
            let (layer, rest): (Vec<usize>, Vec<usize>) = pending
                .iter()
                .copied()
                .partition(|&index| nodes[index].deps.iter().all(|dep| done.contains(dep)));
            if layer.is_empty() {
                let cycle: Vec<&str> = pending.iter().map(|&index| nodes[index].name).collect();
                return Err(Diagnostic::error(
                    codes::E_PLAN_0003,
                    format!("DAG 存在依赖环: {}", cycle.join(" → ")),
                )
                .with_hint("检查 .after([...]) 声明，去掉成环的依赖")
                .into());
            }
            done.extend(layer.iter().map(|&index| nodes[index].name));
            layers.push(layer);
            pending = rest;
        }
        Ok(Self { nodes, layers })
    }

    /// 本 Plan 的阶段名序列（拓扑序 = 执行序：层内按节点声明序）。
    pub fn names(&self) -> Vec<&'static str> {
        self.layers
            .iter()
            .flatten()
            .map(|&index| self.nodes[index].name)
            .collect()
    }

    /// 阶段名序列（顺序即执行顺序）。
    pub fn stage_names() -> Vec<&'static str> {
        Self::standard().names()
    }

    /// 以无项目上下文执行（`run_with(None, ..)` 的快捷形态）。
    pub fn run(&self, job: &JobSpec, events: &mut dyn EventSink) -> Result<(), StageError> {
        self.run_with(None, job, events)
    }

    /// 带项目上下文执行：按拓扑分层推进，层内真并行，事件按声明序刷入（确定性）。
    /// 任一层出现失败即返回该层声明序中的第一个失败，其后各层不再执行。
    pub fn run_with(
        &self,
        project: Option<&Project>,
        job: &JobSpec,
        events: &mut dyn EventSink,
    ) -> Result<(), StageError> {
        for layer in &self.layers {
            let results = std::thread::scope(|scope| {
                let handles: Vec<_> = layer
                    .iter()
                    .map(|&index| {
                        let node = &self.nodes[index];
                        scope.spawn(move || {
                            let mut buffer = BufferSink::default();
                            let mut ctx = StageContext {
                                job,
                                events: &mut buffer,
                                project,
                            };
                            let outcome =
                                node.stage.plan(job).and_then(|()| node.stage.run(&mut ctx));
                            (buffer.events, outcome)
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| match handle.join() {
                        Ok(result) => result,
                        // 阶段 panic 属实现缺陷：原样向上抛，不吞掉现场
                        Err(payload) => std::panic::resume_unwind(payload),
                    })
                    .collect::<Vec<_>>()
            });

            // 按节点声明顺序刷入：事件序列与线程完成顺序无关（确定性）。
            let mut first_error = None;
            for ((buffered, outcome), &index) in results.into_iter().zip(layer) {
                for event in &buffered {
                    events.emit(event);
                }
                if let Err(err) = outcome {
                    events.emit(&BuildEvent::Failed {
                        stage: self.nodes[index].name.to_string(),
                        code: err.diagnostic.code.to_string(),
                        hint: err.diagnostic.hint.clone(),
                    });
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
            if let Some(err) = first_error {
                return Err(err);
            }
        }
        Ok(())
    }
}

/// 节点私有事件缓冲：join 后统一按声明序刷入真实 sink，保证跨线程事件序列确定性。
#[derive(Default)]
struct BufferSink {
    events: Vec<BuildEvent>,
}

impl EventSink for BufferSink {
    fn emit(&mut self, event: &BuildEvent) {
        self.events.push(event.clone());
    }
}

/// plan 阶段：校验 JobSpec（目标非空、id 不重复、目标声明合法、凭据引用齐备）。
pub(crate) struct PlanStage;

impl Stage for PlanStage {
    fn name(&self) -> &'static str {
        "plan"
    }

    fn plan(&self, job: &JobSpec) -> Result<(), StageError> {
        // 校验实现在引擎层（`JobSpec::validate`，no_std 可用）；这里只做诊断映射。
        job.validate().map_err(|err| {
            match err {
                JobError::EmptyTargets => {
                    Diagnostic::error(codes::E_PLAN_0001, "JobSpec 未包含任何目标")
                        .with_hint("检查 polypack.toml 的 [targets.*] 段或 --target 参数")
                }
                JobError::DuplicateTarget(id) => {
                    Diagnostic::error(codes::E_PLAN_0001, format!("目标重复: {id}"))
                        .with_hint("每个目标在 JobSpec.targets 中只能出现一次")
                }
                JobError::InvalidTarget { id, reason } => {
                    Diagnostic::error(codes::E_TGT_0003, format!("目标 {id} 声明非法: {reason}"))
                        .with_hint("核对 polypack.toml 的 [targets.*] 段与目标注册表声明")
                }
                JobError::MissingCredential { target, name } => Diagnostic::error(
                    codes::E_PLAN_0002,
                    format!("目标 {target} 引用凭据档案 {name}，但 JobSpec 缺少对应凭据引用"),
                )
                .with_hint("检查配置的 signing 档案与凭据来源（密钥库 / Vault / CI 注入）"),
            }
            .into()
        })
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
pub(crate) struct ProbeStage;

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
pub(crate) struct NotYetStage {
    pub(crate) name: &'static str,
    pub(crate) since: &'static str,
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
            project: None,
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

    /// 内置标准 DAG 必须自校验通过，且分层符合 §5.1（compile 与 bundle 同层）。
    #[test]
    fn standard_plan_layers_follow_double_branch() {
        let plan = Plan::standard();
        assert_eq!(
            plan.layers,
            vec![
                vec![0],
                vec![1],
                vec![2, 3],
                vec![4],
                vec![5],
                vec![6],
                vec![7]
            ]
        );
    }
}
