//! 契约测试：build.sh 产物 → wasmtime 加载 → describe/package 断言 → SRI 打印。
//!
//! 本地等价命令：`cargo test -- --nocapture`（无 wasm32 target 时 SKIP-with-print）。
//! CI 先 `./build.sh`，再以 `POLYPACK_PLUGIN_WASM=dist/plugin.wasm` 复用产物。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 模板仓库根（`contract/` 的父目录）。
fn template_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("contract/ 有父目录")
}

fn target_installed() -> bool {
    Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|line| line.trim() == "wasm32-unknown-unknown")
        })
        .unwrap_or(false)
}

fn wasm_path(root: &Path) -> PathBuf {
    match std::env::var_os("POLYPACK_PLUGIN_WASM") {
        Some(path) => {
            let path = PathBuf::from(path);
            if path.is_relative() {
                root.join(path)
            } else {
                path
            }
        }
        None => root.join("dist/plugin.wasm"),
    }
}

#[test]
fn plugin_satisfies_target_spec_v1_contract() {
    if cfg!(not(unix)) {
        eprintln!("SKIP：契约测试用 build.sh，当前宿主无 bash");
        return;
    }
    let root = template_root();
    let wasm = wasm_path(root);
    if std::env::var_os("POLYPACK_PLUGIN_WASM").is_none() {
        if !target_installed() {
            eprintln!(
                "SKIP：未安装 wasm32-unknown-unknown（rustup target add wasm32-unknown-unknown）"
            );
            return;
        }
        let status = Command::new("bash")
            .arg("build.sh")
            .current_dir(root)
            // 子 cargo 用模板自带的 target 目录，避免与测试进程的 target 锁竞争。
            .env_remove("CARGO_TARGET_DIR")
            .status()
            .expect("bash 可执行");
        assert!(status.success(), "build.sh 失败");
    }
    let bytes = std::fs::read(&wasm)
        .unwrap_or_else(|err| panic!("插件产物不可读（{}）：{err}", wasm.display()));
    let report = contract::verify(&bytes).expect("契约校验通过");

    assert_eq!(report.spec_version, "target-spec/v1");
    assert_eq!(report.id, contract::PLUGIN_ID);
    assert_eq!(
        report.format,
        contract::EXPECTED_FORMAT,
        "产物格式与 EXPECTED_FORMAT 不一致；自定义 describe/package 后同步 contract/src/lib.rs"
    );
    assert_eq!(
        report.bytes,
        contract::EXPECTED_PACKAGE,
        "package 字节与 EXPECTED_PACKAGE 不一致；自定义 package 后同步 contract/src/lib.rs"
    );
    assert!(
        report.digest.starts_with("blake3-") && report.digest.len() == "blake3-".len() + 64,
        "SRI 口径不符: {}",
        report.digest
    );
    eprintln!(
        "PASS target-spec/v1 id={} artifact={}B digest={}",
        report.id,
        report.bytes.len(),
        report.digest
    );
}
