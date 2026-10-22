//! T-M0-08 验收：`polypack doctor` 报告、L1 覆盖矩阵与退出码。

use std::process::{Command, Output};

/// 运行 `polypack` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_polypack`）。
fn polypack(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polypack"))
        .args(args)
        .output()
        .expect("polypack 可执行")
}

#[test]
fn doctor_exits_zero_and_reports_l1_matrix_and_rust() {
    let out = polypack(&["doctor"]);
    assert_eq!(out.status.code(), Some(0), "doctor 默认恒 0（报告不门控）");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("L1"), "人类输出含 L1 覆盖矩阵：{stdout}");
    assert!(stdout.contains("rust"), "人类输出含 rust 探测行：{stdout}");
}

#[test]
fn doctor_json_is_json_lines() {
    let out = polypack(&["doctor", "--json"]);
    assert_eq!(out.status.code(), Some(0), "doctor 默认恒 0（报告不门控）");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert!(lines.len() >= 2, "至少含报告行 + 探测器行：{stdout}");
    let values: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|e| panic!("行不是合法 JSON：{line}（{e}）"))
        })
        .collect();

    // 首行：doctor 报告对象，其 summary.probes 与随后的 ProbeResult 行数一致
    assert_eq!(values[0]["event"], "doctor", "报告对象 tag");
    let probes = values[0]["summary"]["probes"]
        .as_u64()
        .expect("summary.probes 为整数") as usize;
    assert_eq!(probes, values.len() - 1, "报告行数 = 每探测器一行");
    assert!(values[0]["targets"].is_array(), "报告对象含 L1 覆盖矩阵");

    // 其余行：ProbeResult
    for value in &values[1..] {
        assert!(value["id"].is_string(), "探测器行含 id：{value}");
        assert!(
            value["required_for"].is_array(),
            "探测器行含 required_for：{value}"
        );
    }
}
