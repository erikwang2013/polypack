use super::*;

#[test]
fn selfcheck_contract_matches_shared_steps() {
    let source = qnx_selfcheck_source();
    for step in SMOKE_STEPS {
        assert!(
            source.contains(&format!("[POLYPACK-STEP {step}]")),
            "QNX 自检源缺步骤 {step}"
        );
    }
    assert!(source.contains("SMOKE PASS"));
    assert!(source.contains("poly_engine version="));
}

#[test]
fn qnx_buildfile_runs_selfcheck_then_shutdown() {
    let buildfile = qnx_buildfile("libpolypack_engine_qnx.a");
    assert!(buildfile.contains("[+script] startup-script"));
    assert!(buildfile.contains("/polypack_engine_selfcheck"));
    assert!(buildfile.contains("shutdown -f -S"));
    assert!(buildfile.contains("lib/libpolypack_engine_qnx.a"));
}

#[test]
fn zephyr_exit_codes_follow_board_conventions() {
    assert_eq!(zephyr_expected_exit("qemu_cortex_m3"), Some(0));
    // isa-debug-exit: (0 << 1) | 1
    assert_eq!(zephyr_expected_exit("qemu_x86"), Some(1));
    assert_eq!(zephyr_expected_exit("unknown"), None);
}
