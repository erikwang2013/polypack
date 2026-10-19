//! T-M1-01 验收：DAG 构图校验、拓扑分层、层内真并行、事件确定性、失败汇总。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use poly_core::plan::{Project, StageNode};
use poly_core::{
    BuildEvent, Code, Diagnostic, EventSink, JobSpec, Plan, PolypackConfig, Profile, Stage,
    StageContext, StageError, StageOutcome, codes,
};

/// 捕获事件用于断言。
#[derive(Default)]
struct CaptureSink {
    events: Vec<BuildEvent>,
}

impl EventSink for CaptureSink {
    fn emit(&mut self, event: &BuildEvent) {
        self.events.push(event.clone());
    }
}

fn job() -> JobSpec {
    JobSpec {
        id: "dag-test".to_string(),
        app_id: None,
        profile: Profile::Local,
        targets: Vec::new(),
        excluded: Vec::new(),
        secrets: Vec::new(),
        budget: poly_core::Budget::default(),
        plugins: Vec::new(),
    }
}

/// 可编程 mock 阶段：发一个 `Cached{key}` 事件、可选失败、记录被调次数。
struct MockStage {
    name: &'static str,
    fail_code: Option<Code>,
    runs: Arc<AtomicUsize>,
}

impl MockStage {
    fn new(name: &'static str, fail_code: Option<Code>) -> (Self, Arc<AtomicUsize>) {
        let runs = Arc::new(AtomicUsize::new(0));
        (
            Self {
                name,
                fail_code,
                runs: Arc::clone(&runs),
            },
            runs,
        )
    }
}

impl Stage for MockStage {
    fn name(&self) -> &'static str {
        self.name
    }

    fn plan(&self, _job: &JobSpec) -> Result<(), StageError> {
        Ok(())
    }

    fn run(&self, ctx: &mut StageContext<'_>) -> Result<StageOutcome, StageError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        ctx.events.emit(&BuildEvent::Cached {
            key: self.name.to_string(),
            hit: true,
        });
        if let Some(code) = self.fail_code {
            return Err(
                Diagnostic::error(code, format!("mock 阶段 {} 失败", self.name))
                    .with_hint("mock 修复提示")
                    .into(),
            );
        }
        Ok(StageOutcome::Done)
    }
}

/// 握手阶段：先向对端发信号、再等对端信号——两者都到齐才继续（证明层内真并行），
/// 超时即失败而非挂起。
struct HandshakeStage {
    name: &'static str,
    tx: Mutex<Sender<()>>,
    rx: Mutex<Receiver<()>>,
    settle: Duration,
}

impl Stage for HandshakeStage {
    fn name(&self) -> &'static str {
        self.name
    }

    fn plan(&self, _job: &JobSpec) -> Result<(), StageError> {
        Ok(())
    }

    fn run(&self, ctx: &mut StageContext<'_>) -> Result<StageOutcome, StageError> {
        self.tx
            .lock()
            .expect("发送端锁")
            .send(())
            .expect("投递握手信号");
        self.rx
            .lock()
            .expect("接收端锁")
            .recv_timeout(Duration::from_secs(10))
            .expect("层内应并行执行：超时未收到对端握手信号");
        std::thread::sleep(self.settle);
        ctx.events.emit(&BuildEvent::Cached {
            key: self.name.to_string(),
            hit: true,
        });
        Ok(StageOutcome::Done)
    }
}

fn cached(key: &str) -> BuildEvent {
    BuildEvent::Cached {
        key: key.to_string(),
        hit: true,
    }
}

fn failed(stage: &str, code: &str) -> BuildEvent {
    BuildEvent::Failed {
        stage: stage.to_string(),
        code: code.to_string(),
        hint: Some("mock 修复提示".to_string()),
    }
}

#[test]
fn duplicate_node_names_rejected_with_e_plan_0003() {
    let (a, _) = MockStage::new("dup", None);
    let (b, _) = MockStage::new("dup", None);
    let err = Plan::from_nodes(vec![StageNode::new("dup", a), StageNode::new("dup", b)])
        .expect_err("重名节点必须被拒绝");
    assert_eq!(err.diagnostic.code, codes::E_PLAN_0003);
    assert!(err.diagnostic.hint.is_some(), "构图违规必须带修复提示");
}

#[test]
fn unknown_dependency_rejected_with_e_plan_0003() {
    let (a, _) = MockStage::new("a", None);
    let err = Plan::from_nodes(vec![StageNode::new("a", a).after(&["ghost"])])
        .expect_err("依赖未定义节点必须被拒绝");
    assert_eq!(err.diagnostic.code, codes::E_PLAN_0003);
    assert!(err.diagnostic.message.contains("ghost"));
}

#[test]
fn dependency_cycle_rejected_with_e_plan_0003() {
    let (a, _) = MockStage::new("a", None);
    let (b, _) = MockStage::new("b", None);
    let err = Plan::from_nodes(vec![
        StageNode::new("cycle-a", a).after(&["cycle-b"]),
        StageNode::new("cycle-b", b).after(&["cycle-a"]),
    ])
    .expect_err("成环依赖必须被拒绝");
    assert_eq!(err.diagnostic.code, codes::E_PLAN_0003);
    assert!(err.diagnostic.hint.is_some(), "成环必须带修复提示");

    let (c, _) = MockStage::new("c", None);
    let err =
        Plan::from_nodes(vec![StageNode::new("c", c).after(&["c"])]).expect_err("自依赖亦为环");
    assert_eq!(err.diagnostic.code, codes::E_PLAN_0003);
}

#[test]
fn names_returns_topological_order_for_out_of_order_declaration() {
    let (publish, _) = MockStage::new("publish", None);
    let (sign, _) = MockStage::new("sign", None);
    let (plan, _) = MockStage::new("plan", None);
    let plan = Plan::from_nodes(vec![
        StageNode::new("publish", publish).after(&["sign"]),
        StageNode::new("sign", sign).after(&["plan"]),
        StageNode::new("plan", plan),
    ])
    .expect("合法 DAG");
    assert_eq!(
        plan.names(),
        ["plan", "sign", "publish"],
        "names() 必须返回拓扑序"
    );
}

#[test]
fn parallel_branches_run_concurrently_and_events_are_deterministic() {
    let (tx_a, rx_a) = channel();
    let (tx_b, rx_b) = channel();
    let a = HandshakeStage {
        name: "a",
        tx: Mutex::new(tx_a),
        rx: Mutex::new(rx_b),
        // a 全层最后完成：事件仍须按声明序（a 在 b 前）
        settle: Duration::from_millis(50),
    };
    let b = HandshakeStage {
        name: "b",
        tx: Mutex::new(tx_b),
        rx: Mutex::new(rx_a),
        settle: Duration::ZERO,
    };
    let (root, _) = MockStage::new("root", None);
    let (converge, converge_runs) = MockStage::new("converge", None);
    let plan = Plan::from_nodes(vec![
        StageNode::new("root", root),
        StageNode::new("a", a).after(&["root"]),
        StageNode::new("b", b).after(&["root"]),
        StageNode::new("converge", converge).after(&["a", "b"]),
    ])
    .expect("合法 DAG");

    let expected = vec![cached("root"), cached("a"), cached("b"), cached("converge")];
    for round in 0..2 {
        let mut sink = CaptureSink::default();
        plan.run(&job(), &mut sink)
            .unwrap_or_else(|err| panic!("第 {round} 轮执行失败: {err}"));
        assert_eq!(
            sink.events, expected,
            "第 {round} 轮事件序列必须确定（按声明序）"
        );
    }
    assert_eq!(
        converge_runs.load(Ordering::SeqCst),
        2,
        "汇聚节点每轮执行一次"
    );
}

#[test]
fn failing_layer_reports_every_failure_and_flushes_sibling_events() {
    let (root, _) = MockStage::new("root", None);
    let (ok, ok_runs) = MockStage::new("ok", None);
    let (bad1, _) = MockStage::new("bad1", Some(codes::E_PLAN_0001));
    let (bad2, _) = MockStage::new("bad2", Some(codes::E_PLAN_0002));
    let (converge, converge_runs) = MockStage::new("converge", None);
    let plan = Plan::from_nodes(vec![
        StageNode::new("root", root),
        StageNode::new("ok", ok).after(&["root"]),
        StageNode::new("bad1", bad1).after(&["root"]),
        StageNode::new("bad2", bad2).after(&["root"]),
        StageNode::new("converge", converge).after(&["ok", "bad1", "bad2"]),
    ])
    .expect("合法 DAG");

    let mut sink = CaptureSink::default();
    let err = plan
        .run(&job(), &mut sink)
        .expect_err("层内失败必须中止构建");

    assert_eq!(
        err.diagnostic.code,
        codes::E_PLAN_0001,
        "必须返回节点声明序中的第一个失败"
    );
    assert_eq!(
        sink.events,
        vec![
            cached("root"),
            cached("ok"),
            cached("bad1"),
            failed("bad1", "poly::E_PLAN_0001"),
            cached("bad2"),
            failed("bad2", "poly::E_PLAN_0002"),
        ],
        "成功支线事件照常上报，每个失败节点各发一个 Failed（按声明序）"
    );
    assert_eq!(ok_runs.load(Ordering::SeqCst), 1, "同层成功节点必须已执行");
    assert_eq!(
        converge_runs.load(Ordering::SeqCst),
        0,
        "依赖失败节点的后继不执行也不再发事件"
    );
}

/// 后继链上的失败：上游失败后，下游不再执行（也不再发事件）。
#[test]
fn successor_of_failed_node_does_not_execute() {
    let (fail, _) = MockStage::new("fail", Some(codes::E_IO_0001));
    let (after, after_runs) = MockStage::new("after", None);
    let plan = Plan::from_nodes(vec![
        StageNode::new("fail", fail),
        StageNode::new("after", after).after(&["fail"]),
    ])
    .expect("合法 DAG");

    let mut sink = CaptureSink::default();
    let err = plan.run(&job(), &mut sink).expect_err("上游失败必须中止");
    assert_eq!(err.diagnostic.code, codes::E_IO_0001);
    assert_eq!(
        sink.events,
        vec![cached("fail"), failed("fail", "poly::E_IO_0001")]
    );
    assert_eq!(after_runs.load(Ordering::SeqCst), 0);
}

/// `run_with(Some(project), ..)` 必须把项目上下文透传给阶段。
#[test]
fn run_with_exposes_project_context() {
    struct ProjectProbe {
        seen_root: Arc<Mutex<Option<std::path::PathBuf>>>,
    }

    impl Stage for ProjectProbe {
        fn name(&self) -> &'static str {
            "probe-project"
        }

        fn plan(&self, _job: &JobSpec) -> Result<(), StageError> {
            Ok(())
        }

        fn run(&self, ctx: &mut StageContext<'_>) -> Result<StageOutcome, StageError> {
            let project = ctx.project.expect("run_with(Some(..)) 必须透传项目上下文");
            *self.seen_root.lock().expect("锁") = Some(project.root.clone());
            Ok(StageOutcome::Done)
        }
    }

    let seen_root = Arc::new(Mutex::new(None));
    let plan = Plan::from_nodes(vec![StageNode::new(
        "probe-project",
        ProjectProbe {
            seen_root: Arc::clone(&seen_root),
        },
    )])
    .expect("合法 DAG");
    let config = PolypackConfig::parse(
        "[package]\nname = \"t\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("配置合法");
    let project = Project::new(std::path::PathBuf::from("/tmp/polypack-dag-test"), config);

    let mut sink = CaptureSink::default();
    plan.run_with(Some(&project), &job(), &mut sink)
        .expect("阶段不得失败");
    assert_eq!(
        seen_root.lock().expect("锁").as_deref(),
        Some(std::path::Path::new("/tmp/polypack-dag-test"))
    );
}
