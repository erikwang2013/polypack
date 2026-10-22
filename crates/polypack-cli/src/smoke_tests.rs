use super::*;

const OK_TEXT: &str = "polypack_engine version=0.0.0 abi=1\n\
    [POLYPACK-STEP version] PASS\n\
    [POLYPACK-STEP validate-target] PASS\n\
    [POLYPACK-STEP cache-key] PASS\n\
    SMOKE PASS\n";

fn steps() -> Vec<String> {
    vec![
        "version".to_string(),
        "validate-target".to_string(),
        "cache-key".to_string(),
    ]
}

fn log() -> PathBuf {
    PathBuf::from("/tmp/smoke-test.log")
}

#[test]
fn judge_accepts_full_contract() {
    let (version, states) = judge(OK_TEXT, Some(0), 0, &steps(), &log()).expect("应通过");
    assert_eq!(version, "polypack_engine version=0.0.0 abi=1");
    assert!(states.iter().all(|(_, state)| *state == Some(true)));
}

#[test]
fn judge_maps_failed_step_to_stage() {
    let text = OK_TEXT.replace(
        "[POLYPACK-STEP validate-target] PASS",
        "[POLYPACK-STEP validate-target] FAIL",
    );
    let failure = judge(&text, Some(0), 0, &steps(), &log()).expect_err("步骤 FAIL 必须失败");
    assert_eq!(failure.stage, "validate-target");
    assert_eq!(failure.code, codes::E_SMOKE_0002);
    let rendered = failure.to_string();
    assert!(
        rendered.starts_with("Failed{stage=validate-target, code=polypack::E_SMOKE_0002, hint=")
    );
    assert!(rendered.ends_with('}'));
}

#[test]
fn judge_maps_missing_step_to_that_step() {
    let text = OK_TEXT.replace("[POLYPACK-STEP cache-key] PASS\n", "");
    let failure = judge(&text, Some(0), 0, &steps(), &log()).expect_err("缺步骤必须失败");
    assert_eq!(failure.stage, "cache-key");
    assert_eq!(failure.code, codes::E_SMOKE_0004);
}

#[test]
fn judge_maps_exit_code_mismatch_to_qemu_run() {
    let failure = judge(OK_TEXT, Some(2), 1, &steps(), &log()).expect_err("退出码不符必须失败");
    assert_eq!(failure.stage, "qemu-run");
    assert_eq!(failure.code, codes::E_SMOKE_0003);
}

#[test]
fn judge_requires_final_marker() {
    let text = OK_TEXT.replace("SMOKE PASS\n", "");
    let failure = judge(&text, Some(0), 0, &steps(), &log()).expect_err("缺终标记必须失败");
    assert_eq!(failure.stage, "smoke");
    assert_eq!(failure.code, codes::E_SMOKE_0004);
}

#[test]
fn safe_case_keeps_board_names_and_sanitizes_rest() {
    assert_eq!(safe_case("qemu_cortex_m3"), "qemu_cortex_m3");
    assert_eq!(safe_case("pax notes/image"), "pax-notes-image");
    assert_eq!(safe_case("///"), "case");
}

#[cfg(unix)]
#[test]
fn execute_archives_success_and_timeout() {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let root = std::env::temp_dir().join(format!(
        "polypack-smoke-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);

    let ok = SmokeRun {
        target: "selftest".to_string(),
        case: "board-a".to_string(),
        program: PathBuf::from("/bin/sh"),
        args: vec![
            "-c".to_string(),
            format!("printf '{}'; exit 0", OK_TEXT.replace('\'', "'\\''")),
        ],
        cwd: None,
        env: Vec::new(),
        expected_exit: 0,
        steps: steps(),
        log_root: root.clone(),
        timeout: Duration::from_secs(5),
    };
    let report = ok.execute().expect("成功冒烟应通过");
    assert_eq!(report.exit_code, 0);
    assert!(report.log.is_file(), "串口日志必须归档");
    let archived: serde_json::Value =
        serde_json::from_slice(&fs::read(&report.result).expect("结果 JSON")).expect("合法 JSON");
    assert_eq!(archived["status"], "passed");
    assert_eq!(archived["steps"].as_array().map(Vec::len), Some(3));

    let hang = SmokeRun {
        target: "selftest".to_string(),
        case: "board-b".to_string(),
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_string(), "sleep 30".to_string()],
        cwd: None,
        env: Vec::new(),
        expected_exit: 0,
        steps: steps(),
        log_root: root.clone(),
        timeout: Duration::from_millis(200),
    };
    let failure = hang.execute().expect_err("超时必须失败");
    assert_eq!(failure.stage, "qemu-run");
    assert_eq!(failure.code, codes::E_SMOKE_0001);
    let result = root.join("selftest/board-b.result.json");
    let archived: serde_json::Value =
        serde_json::from_slice(&fs::read(&result).expect("超时也要归档结果")).expect("合法 JSON");
    assert_eq!(archived["status"], "timeout");

    let _ = fs::remove_dir_all(&root);
}
