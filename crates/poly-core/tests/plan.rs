//! T-M0-06：plan 阶段校验、标准阶段序与全序执行。

mod common;

use common::spec;
use poly_core::{
    BuildEvent, CredentialRef, CredentialSource, EventSink, HumanSink, JobSpec, Plan, Profile,
    codes,
};
use poly_targets::TargetSpec;

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

fn job(targets: Vec<TargetSpec>) -> JobSpec {
    JobSpec {
        id: "test-job".to_string(),
        profile: Profile::Local,
        targets,
        excluded: Vec::new(),
        secrets: Vec::new(),
        budget: poly_core::Budget::default(),
        plugins: Vec::new(),
    }
}

fn failed_event_code(sink: &CaptureSink, stage: &str) -> Option<String> {
    sink.events.iter().find_map(|event| match event {
        BuildEvent::Failed { stage: s, code, .. } if s == stage => Some(code.clone()),
        _ => None,
    })
}

#[test]
fn standard_stage_order_is_plan_to_publish() {
    let expected = [
        "plan", "probe", "compile", "bundle", "package", "sign", "verify", "publish",
    ];
    assert_eq!(Plan::stage_names(), expected, "§5.1 阶段序");
    assert_eq!(
        Plan::standard().names(),
        expected,
        "standard() 实际构造的阶段序必须与 stage_names() 一致"
    );
}

/// 注入三阶段（package / sign / verify）后 DAG 形状不变，且都真在图层里按依赖序执行。
///
/// 这是 T-M2-10 接线的核心不变量：`sign` 依赖 `package`、`verify` 依赖 `sign`，三者不在同一
/// 层（否则顺序无保证）；注入与否只换实现（`NotYetStage` → 真实阶段），不改图。
#[test]
fn injected_stages_keep_the_dag_shape_and_run_in_dependency_order() {
    use poly_core::farm::StageFactory;
    use poly_core::pipeline::{self, InjectedStages};
    use poly_core::plan::{Stage, StageContext, StageOutcome};
    use std::sync::{Arc, Mutex};

    /// 记录「我跑过」的测试替身（真实实现由 poly-cli 注入，poly-core 不得依赖它们）。
    struct MarkerStage(&'static str);

    /// 记录序（每测试独占本二进制内的这一个静态量）。
    static LOG: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

    impl Stage for MarkerStage {
        fn name(&self) -> &'static str {
            self.0
        }
        fn plan(&self, _job: &JobSpec) -> Result<(), poly_core::StageError> {
            Ok(())
        }
        fn run(&self, _ctx: &mut StageContext<'_>) -> Result<StageOutcome, poly_core::StageError> {
            LOG.lock().expect("锁可用").push(self.0);
            Ok(StageOutcome::Done)
        }
    }

    fn package_stage() -> Box<dyn Stage> {
        Box::new(MarkerStage("package"))
    }
    fn sign_stage() -> Box<dyn Stage> {
        Box::new(MarkerStage("sign"))
    }
    fn verify_stage() -> Box<dyn Stage> {
        Box::new(MarkerStage("verify"))
    }

    let root = std::env::temp_dir().join(format!("polypack-plan-inject-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("临时目录可建");
    let cache = Arc::new(poly_cache::Cache::from_root(root.clone()));
    let stages = InjectedStages {
        package: Some(package_stage as StageFactory),
        sign: Some(sign_stage as StageFactory),
        verify: Some(verify_stage as StageFactory),
    };
    let expected = [
        "plan", "probe", "compile", "bundle", "package", "sign", "verify", "publish",
    ];
    let plan = pipeline::plan_with_stages(cache.clone(), "local", stages).expect("DAG 合法");
    assert_eq!(
        plan.names(),
        expected,
        "注入不改阶段序（同一张图，只换实现）"
    );
    assert_eq!(
        pipeline::plan(cache, "local").expect("默认注入").names(),
        expected,
        "缺省注入（骨架占位）也必须是同一张图"
    );

    LOG.lock().expect("锁可用").clear();
    let mut sink = CaptureSink::default();
    // 无项目上下文：compile / bundle 如实跳过，三个注入阶段照跑（依赖序即执行序）
    plan.run(&job(vec![spec("linux")]), &mut sink)
        .expect("骨架阶段不该失败");
    assert_eq!(
        *LOG.lock().expect("锁可用"),
        vec!["package", "sign", "verify"],
        "sign 必须在 package 之后、verify 在 sign 之后"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn empty_job_fails_and_emits_failed_event() {
    let mut sink = CaptureSink::default();
    let err = Plan::standard()
        .run(&job(Vec::new()), &mut sink)
        .expect_err("空目标 JobSpec 必须快速失败");

    assert_eq!(err.diagnostic.code, codes::E_PLAN_0001);
    assert_eq!(
        sink.events,
        vec![BuildEvent::Failed {
            stage: "plan".to_string(),
            code: "poly::E_PLAN_0001".to_string(),
            hint: err.diagnostic.hint.clone(),
        }],
        "校验失败必须发出 Failed 事件，且此前的阶段不得产出事件"
    );
}

#[test]
fn invalid_target_spec_fails_with_e_tgt_0003() {
    let mut invalid = spec("android");
    invalid.tier = 9; // 越界，validate() 必失败

    let mut sink = CaptureSink::default();
    let err = Plan::standard()
        .run(&job(vec![invalid]), &mut sink)
        .expect_err("非法目标声明必须快速失败");

    assert_eq!(err.diagnostic.code, codes::E_TGT_0003);
    assert!(err.diagnostic.hint.is_some(), "错误必须带修复提示");
    assert_eq!(
        failed_event_code(&sink, "plan").as_deref(),
        Some("poly::E_TGT_0003")
    );
}

#[test]
fn duplicate_targets_fail_with_e_plan_0001() {
    let mut sink = CaptureSink::default();
    let err = Plan::standard()
        .run(&job(vec![spec("android"), spec("android")]), &mut sink)
        .expect_err("重复目标必须快速失败");

    assert_eq!(err.diagnostic.code, codes::E_PLAN_0001);
    assert_eq!(
        failed_event_code(&sink, "plan").as_deref(),
        Some("poly::E_PLAN_0001")
    );
}

#[test]
fn missing_credential_ref_fails_with_e_plan_0002() {
    let mut signing = spec("android");
    signing.signing = Some("android-keystore".to_string());

    let mut sink = CaptureSink::default();
    let err = Plan::standard()
        .run(&job(vec![signing]), &mut sink)
        .expect_err("签名目标缺凭据引用必须快速失败");

    assert_eq!(err.diagnostic.code, codes::E_PLAN_0002);
    assert_eq!(
        failed_event_code(&sink, "plan").as_deref(),
        Some("poly::E_PLAN_0002")
    );
}

#[test]
fn two_targets_run_full_sequence_and_emit_planned_then_probed() {
    let mut signing = spec("android");
    signing.signing = Some("android-keystore".to_string());

    let mut job = job(vec![signing, spec("web")]);
    job.secrets = vec![CredentialRef {
        name: "android-keystore".to_string(),
        source: CredentialSource::Keyring,
    }];

    let mut sink = CaptureSink::default();
    Plan::standard()
        .run(&job, &mut sink)
        .expect("合法 JobSpec 必须全序执行完成（骨架阶段以 Skipped 让路）");

    assert!(
        matches!(
            sink.events.first(),
            Some(BuildEvent::Planned { targets, excluded })
                if targets == &["android", "web"] && excluded.is_empty()
        ),
        "首个事件必须是含两目标的 Planned，实得 {:?}",
        sink.events.first()
    );
    assert!(
        matches!(sink.events.get(1), Some(BuildEvent::Probed { .. })),
        "第二个事件必须是 Probed，实得 {:?}",
        sink.events.get(1)
    );
    assert!(
        !sink
            .events
            .iter()
            .any(|e| matches!(e, BuildEvent::Failed { .. })),
        "成功路径不得出现 Failed 事件"
    );
}

/// L1 目标所需工具链全缺失时仅告警不阻断（M0 语义，门控归 poly doctor）。
#[test]
fn probe_never_blocks_on_missing_toolchains() {
    let mut sink = HumanSink { out: Vec::new() };
    Plan::standard()
        .run(&job(vec![spec("android")]), &mut sink)
        .expect("工具链缺失不得阻断构建计划");

    let out = String::from_utf8(sink.out).expect("人读输出必须为 UTF-8");
    assert!(out.contains("探测"), "必须报告探测结果，实得: {out}");
}
