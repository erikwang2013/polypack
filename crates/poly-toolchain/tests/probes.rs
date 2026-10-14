//! T-M0-09 验收：探测器集合、required_for 映射、本机探测与子进程容错。

use std::collections::BTreeSet;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use poly_toolchain::probes::run_capture;
use poly_toolchain::{Probe, default_probes};

/// 按 id 取出探测器。
fn probe(id: &str) -> Box<dyn Probe> {
    default_probes()
        .into_iter()
        .find(|probe| probe.id() == id)
        .unwrap_or_else(|| panic!("探测器集合缺少 {id}"))
}

#[test]
fn default_probes_has_six_unique_ids() {
    let probes = default_probes();
    assert_eq!(
        probes.len(),
        6,
        "六类探测器：rust / android-sdk / android-ndk / xcode / msvc / emsdk"
    );
    let ids: BTreeSet<&str> = probes.iter().map(|probe| probe.id()).collect();
    assert_eq!(ids.len(), probes.len(), "探测器 id 不得重复：{ids:?}");
}

#[test]
fn probes_run_without_panic_and_report_their_contract() {
    for probe in default_probes() {
        assert!(
            !probe.required_for().is_empty(),
            "{} 必须声明所服务的目标",
            probe.id()
        );
        let result = probe.locate();
        assert_eq!(result.id, probe.id(), "结果 id 与探测器一致");
        assert_eq!(
            result.required_for,
            probe.required_for(),
            "{} 的结果 required_for 与探测器一致",
            probe.id()
        );
        if result.found.is_none() {
            assert!(
                result.note.as_deref().is_some_and(|note| !note.is_empty()),
                "{} 未找到时必须给出 note（供 doctor 渲染）",
                probe.id()
            );
        }
    }
}

#[test]
fn required_for_matches_l1_targets() {
    let expected: [(&str, &[&str]); 6] = [
        (
            "rust",
            &[
                "android",
                "harmonyos",
                "ios",
                "linux",
                "macos",
                "web",
                "windows",
            ],
        ),
        ("android-sdk", &["android"]),
        ("android-ndk", &["android"]),
        ("xcode", &["ios", "macos"]),
        ("msvc", &["windows"]),
        ("emsdk", &["web"]),
    ];
    for (id, want) in expected {
        let mut got: Vec<&str> = probe(id).required_for().to_vec();
        got.sort_unstable();
        let mut want: Vec<&str> = want.to_vec();
        want.sort_unstable();
        assert_eq!(got, want, "{id} 的 required_for 映射");
    }
}

#[test]
fn rust_probe_found_on_this_machine() {
    let result = probe("rust").locate();
    let toolchain = result
        .found
        .expect("本机应能找到 rustc（T-M0-09 验收前提）");
    let version = toolchain.version.as_deref().unwrap_or_default();
    assert!(version.contains("rustc"), "版本含 rustc：{version}");
    assert!(version.contains("cargo"), "版本含 cargo：{version}");
}

#[test]
#[cfg(not(target_os = "macos"))]
fn xcode_probe_missing_with_note_off_macos() {
    let result = probe("xcode").locate();
    assert!(result.found.is_none(), "非 macOS 宿主不应找到 Xcode");
    let note = result.note.as_deref().unwrap_or_default();
    assert!(!note.is_empty(), "缺失必须给出原因");
    assert!(
        note.contains("仅 macOS 宿主"),
        "note 须说明平台限制：{note}"
    );
}

#[test]
fn run_capture_reports_missing_command_as_none() {
    assert!(
        run_capture("poly-nonexistent-command-xyz", &[], Duration::from_secs(1)).is_none(),
        "命令不存在必须返回 None"
    );
}

#[test]
#[cfg(unix)]
fn run_capture_returns_trimmed_stdout() {
    assert_eq!(
        run_capture("echo", &["  hello  "], Duration::from_secs(2)).as_deref(),
        Some("hello")
    );
}

#[test]
#[cfg(unix)]
fn run_capture_times_out_without_panicking() {
    let start = Instant::now();
    let out = run_capture("sleep", &["30"], Duration::from_millis(200));
    assert!(out.is_none(), "超时必须返回 None");
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(3),
        "须在 3s 内返回：{elapsed:?}"
    );
}
