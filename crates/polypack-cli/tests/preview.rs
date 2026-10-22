//! T-M9-20 验收（CLI 侧）：`polypack preview --dry-run` 的确定性计划、错误码与无网络保证。
//!
//! 全部用例只调用二进制并解析 stdout，不启动设备、不读凭据、不访问网络。

use std::process::{Command, Output};

/// 运行 `polypack` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_polypack`）。
fn polypack(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polypack"))
        .args(args)
        .output()
        .expect("polypack 可执行")
}

/// `--json` 模式下 stdout 为单行 JSON（启动横幅已抑制）。
fn json_stdout(output: &Output) -> serde_json::Value {
    let text = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(text.trim()).unwrap_or_else(|err| {
        panic!(
            "stdout 不是 JSON（{err}）：stdout={text:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn combined(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn dry_run_plan_is_deterministic_and_has_leases() {
    let args = [
        "--json",
        "preview",
        "--dry-run",
        "--at",
        "0",
        "--timeout",
        "900",
        "--device",
        "firebase-test-lab/Pixel 7/android",
        "--device",
        "self-hosted/tv-lab-01/android/android-tv",
    ];
    let first = polypack(&args);
    let second = polypack(&args);
    assert!(first.status.success(), "dry-run 失败：{}", combined(&first));
    assert_eq!(
        first.stdout, second.stdout,
        "同一输入与起点的计划必须逐字节相同"
    );

    let plan = json_stdout(&first);
    assert_eq!(plan["protocol"], "preview/v1");
    assert_eq!(plan["farm_protocol"], "farm/v1");
    assert_eq!(plan["job_kind"], "preview");
    assert_eq!(plan["dry_run"], true);
    let leases = plan["leases"].as_array().expect("leases 是数组");
    assert_eq!(leases.len(), 2);
    assert_eq!(leases[0]["lease_id"], "lease-0001");
    assert_eq!(leases[0]["provider"], "firebase-test-lab");
    assert_eq!(leases[0]["device_id"], "Pixel 7");
    assert_eq!(leases[0]["os"], "android");
    assert_eq!(leases[0]["target"], "android");
    assert_eq!(leases[0]["acquired_at"], 0);
    assert_eq!(leases[0]["expires_at"], 900);
    assert_eq!(leases[0]["timeout_secs"], 900);
    assert_eq!(leases[0]["cleanup"]["on_success"], "release");
    assert_eq!(leases[0]["cleanup"]["on_failure"], "release_and_archive");
    assert_eq!(leases[0]["cleanup"]["on_timeout"], "force_release");
    assert_eq!(leases[0]["cleanup"]["uninstall"], true);
    assert_eq!(
        leases[0]["cleanup"]["evidence_dir"],
        ".polypack/preview/android/Pixel 7/"
    );
    assert_eq!(leases[1]["lease_id"], "lease-0002");
    assert_eq!(leases[1]["target"], "android-tv");
}

#[test]
fn invalid_provider_is_rejected_with_stable_code() {
    let output = polypack(&[
        "--json",
        "preview",
        "--dry-run",
        "--device",
        "azure/pixel/android",
    ]);
    assert!(!output.status.success());
    let text = combined(&output);
    assert!(text.contains("polypack::E_FARM_0002"), "{text}");
    assert!(text.contains("azure"), "{text}");
}

#[test]
fn empty_device_list_is_rejected() {
    let output = polypack(&["--json", "preview", "--dry-run"]);
    assert!(!output.status.success());
    assert!(
        combined(&output).contains("polypack::E_FARM_0002"),
        "{}",
        combined(&output)
    );
}

#[test]
fn zero_timeout_is_rejected() {
    let output = polypack(&[
        "--json",
        "preview",
        "--dry-run",
        "--timeout",
        "0",
        "--device",
        "self-hosted/tv-lab-01/android",
    ]);
    assert!(!output.status.success());
    assert!(
        combined(&output).contains("polypack::E_PLAN_0001"),
        "{}",
        combined(&output)
    );
}

#[test]
fn dry_run_needs_no_network_or_credentials() {
    // 网络客户端若被误调用：代理指向关闭端口、云凭据为垃圾值，命令仍必须成功。
    let output = Command::new(env!("CARGO_BIN_EXE_polypack"))
        .args([
            "--json",
            "preview",
            "--dry-run",
            "--at",
            "0",
            "--device",
            "self-hosted/tv-lab-01/android/android-tv",
        ])
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .env("FIREBASE_TOKEN", "invalid")
        .env("BROWSERSTACK_ACCESS_KEY", "invalid")
        .env("AWS_ACCESS_KEY_ID", "invalid")
        .env("AWS_SECRET_ACCESS_KEY", "invalid")
        .output()
        .expect("polypack 可执行");
    assert!(output.status.success(), "{}", combined(&output));
    let plan = json_stdout(&output);
    assert_eq!(plan["leases"][0]["provider"], "self-hosted");
}

#[test]
fn real_execution_without_dry_run_is_not_faked() {
    let output = polypack(&[
        "--json",
        "preview",
        "--device",
        "self-hosted/tv-lab-01/android/android-tv",
    ]);
    assert!(!output.status.success());
    let text = combined(&output);
    assert!(text.contains("polypack::E_FARM_0005"), "{text}");
    assert!(text.contains("T-M9-21"), "{text}");
    assert!(output.stdout.is_empty(), "不得输出伪造的计划或结果");
}
