//! T-M6-08 声明侧：`poly targets --tier 6 --headless --json` 是 CI 矩阵的唯一来源。
//!
//! 矩阵条目必须与 [`Registry::builtin`] 同源：新增 tier 6 无 UI `TargetSpec` 后
//! 命令自动多一行，workflow 不需要改。这里逐字段交叉核对，防止 CLI 另写一份清单。

use std::process::Command;

use poly_targets::{Registry, TargetSpec};
use poly_toolchain::default_probes;
use serde_json::Value;

fn run(args: &[&str]) -> Vec<Value> {
    let out = Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(std::env::temp_dir())
        .output()
        .expect("poly 可执行");
    assert!(
        out.status.success(),
        "poly {args:?} 失败：{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("targets JSON 行"))
        .collect()
}

fn rows(values: &[Value]) -> Vec<&Value> {
    values
        .iter()
        .filter(|value| value["event"] == "target")
        .collect()
}

fn expected_ids(headless: bool) -> Vec<String> {
    let mut ids: Vec<String> = Registry::builtin()
        .all()
        .filter(|spec| spec.tier == 6)
        .filter(|spec| !headless || spec.ui.is_empty())
        .map(|spec| spec.id.clone())
        .collect();
    ids.sort();
    ids
}

#[test]
fn tier6_headless_matrix_is_derived_from_registry() {
    let values = run(&["targets", "--tier", "6", "--headless", "--json"]);
    let summary = values.first().expect("首行摘要");
    assert_eq!(summary["event"], "targets");
    assert_eq!(summary["tier"], 6);
    assert_eq!(summary["headless"], true);
    let ids = expected_ids(true);
    assert_eq!(ids, ["autosar", "freertos", "qnx", "vxworks", "zephyr"]);
    assert_eq!(summary["count"], ids.len());

    let rows = rows(&values);
    assert_eq!(
        rows.iter()
            .map(|row| row["id"].as_str().expect("目标 id"))
            .collect::<Vec<_>>(),
        ids
    );

    // 逐字段与内建注册表交叉核对（矩阵 job 直接消费这些字段）。
    let registry = Registry::builtin();
    for (row, id) in rows.iter().zip(&ids) {
        let spec: &TargetSpec = registry
            .get(id)
            .unwrap_or_else(|| panic!("注册表缺少 {id}"));
        assert_eq!(row["tier"], spec.tier);
        assert_eq!(row["hosts"], serde_json::json!(spec.hosts));
        assert_eq!(row["runtime"], serde_json::json!(spec.runtime));
        assert_eq!(row["bridge"], serde_json::json!(spec.bridge));
        assert_eq!(row["ui"], serde_json::json!(spec.ui));
        assert_eq!(row["format"], spec.package.format);
        assert_eq!(row["builder"], spec.package.builder);
        assert_eq!(row["verify"], serde_json::json!(spec.verify));
        assert_eq!(row["publish"], serde_json::json!(spec.publish));
        assert_eq!(row["experimental"], spec.experimental);
        assert!(
            row["ui"].as_array().is_some_and(Vec::is_empty),
            "headless 矩阵不得含 UI 目标：{row}"
        );
        let probes: Vec<&str> = default_probes()
            .iter()
            .filter(|probe| probe.required_for().contains(&id.as_str()))
            .map(|probe| probe.id())
            .collect();
        assert_eq!(row["required_probes"], serde_json::json!(probes));
    }
}

#[test]
fn headless_filter_only_drops_ui_targets() {
    let with_ui_values = run(&["targets", "--tier", "6", "--json"]);
    let headless_values = run(&["targets", "--tier", "6", "--headless", "--json"]);
    let with_ui = rows(&with_ui_values);
    let headless = rows(&headless_values);
    assert!(
        with_ui.iter().any(|row| row["id"] == "android-automotive"),
        "未加 --headless 时 L6 车机目标应在清单里"
    );
    assert!(
        !headless.iter().any(|row| row["id"] == "android-automotive"),
        "android-automotive 有 UI，不应进无 UI 编译矩阵"
    );
    assert!(
        with_ui.iter().any(|row| row["id"] == "garmin"),
        "未加 --headless 时 Garmin 实验目标应在清单里"
    );
    assert!(
        !headless.iter().any(|row| row["id"] == "garmin"),
        "garmin 有 UI（monkey-c-subset），不应进无 UI 编译矩阵"
    );
    assert_eq!(
        with_ui.len(),
        headless.len() + 2,
        "tier 6 两个 UI 目标：android-automotive / garmin"
    );
    assert_eq!(
        expected_ids(false).len(),
        with_ui.len(),
        "普通 tier 过滤与注册表同源"
    );
}

#[test]
fn unknown_tier_yields_empty_matrix_and_exit_zero() {
    let values = run(&["targets", "--tier", "9", "--headless", "--json"]);
    assert_eq!(values.len(), 1, "只有摘要行：{values:?}");
    assert_eq!(values[0]["count"], 0);
}
