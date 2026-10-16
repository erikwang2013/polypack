//! T-M2-01 / T-M2-02 / T-M3-01 验收（CLI 侧）：`poly bridge generate|aar|ohos` 的参数面、
//! 诊断码与「缺工具就说清楚、绝不产假产物」。
//!
//! 这一份不依赖 uniffi 引擎 / NDK / kotlinc（缺工具的分支本身就是断言对象），三宿主都跑；
//! 真生成 / 真交叉编译在 `poly-core/tests/bridge_real.rs`（`POLY_CONTRACT_REAL=1` 门控）。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 示例接口（与 `poly-core/tests/bridge.rs` 同形）。
const UDL: &str = "namespace pax {\n  string greet(string name);\n};\n";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-cli-bridge-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可创建");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 最小工程：`polypack.toml` + 带 UDL 的核心 crate。
fn fixture(root: &Path) -> PathBuf {
    let core = root.join("core");
    std::fs::create_dir_all(core.join("src")).expect("建核心 crate");
    std::fs::write(core.join("src/pax.udl"), UDL).expect("写 UDL");
    std::fs::write(
        core.join("Cargo.toml"),
        "[workspace]\n\n[package]\nname = \"pax_core\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .expect("写核心 Cargo.toml");
    let config = root.join("polypack.toml");
    std::fs::write(
        &config,
        "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("写配置");
    config
}

/// 运行 `poly` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_poly`）。
fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

/// 按 JSON Lines 解析 stdout。
fn json_lines(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("stdout 每行应为合法 JSON"))
        .collect()
}

/// PATH 上是否有 uniffi-bindgen（判据与生产逻辑同口径）。
fn uniffi_on_path() -> bool {
    std::env::var_os("POLYPACK_UNIFFI_BINDGEN").is_some()
        || std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path)
                .flat_map(|dir| ["uniffi-bindgen", "uniffi-bindgen.exe"].map(|name| dir.join(name)))
                .any(|candidate| candidate.is_file())
        })
}

#[test]
fn bridge_dry_run_reports_plan_and_writes_nothing() {
    let temp = TempDir::new("dry-run");
    fixture(temp.path());

    let out = poly(&["bridge", "generate", "--dry-run", "--json"], temp.path());
    assert_eq!(out.status.code(), Some(0), "dry-run 不碰引擎，必须为绿");
    let lines = json_lines(&out);
    assert_eq!(lines.len(), 1, "stdout 为单行 JSON 报告");
    let report = &lines[0];
    assert_eq!(report["event"], "bridge-plan");
    assert_eq!(report["schema"], "bridge/v1", "接口按 bridge/v1 版本化");
    assert_eq!(
        report["interface"].as_str().expect("摘要").len(),
        64,
        "接口摘要 = blake3 hex（进库级缓存键的那一个）"
    );
    assert_eq!(report["langs"][0], "kotlin");
    assert_eq!(report["langs"][1], "swift");
    assert_eq!(report["langs"][2], "napi", "all = 全部已接入出口（含 T-M3-01 的 napi）");
    assert!(!temp.path().join("bridge").exists(), "dry-run 不落任何产物");
}

#[test]
fn bridge_rejects_unknown_language_by_name() {
    let temp = TempDir::new("bad-lang");
    fixture(temp.path());

    let out = poly(
        &["bridge", "generate", "--dry-run", "--lang", "rust"],
        temp.path(),
    );
    assert_eq!(out.status.code(), Some(1), "非法语言必须非零退出");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_BRIDGE_0002"), "{stderr}");
    assert!(
        stderr.contains("wasm-bindgen"),
        "提示应列出可选出口：{stderr}"
    );
}

#[test]
fn bridge_rejects_reserved_exit_until_m3() {
    let temp = TempDir::new("reserved");
    fixture(temp.path());

    // M3 之后的出口已登记但未接入：调用必须明确报「未接入」，而不是静默产出别的东西
    let out = poly(&["bridge", "generate", "--lang", "cffi"], temp.path());
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("poly::E_BRIDGE_0002") && stderr.contains("M3"),
        "{stderr}"
    );
    assert!(
        !temp.path().join("bridge/cffi").exists(),
        "未接入出口不落目录"
    );
}

/// napi 出口（T-M3-01）：不依赖 uniffi 引擎在场，胶水落盘且含两个装载入口。
#[test]
fn bridge_generate_napi_needs_no_uniffi_engine() {
    let temp = TempDir::new("napi");
    fixture(temp.path());

    let out = poly(
        &["bridge", "generate", "--lang", "napi", "--json"],
        temp.path(),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "napi 出口不需要 uniffi 引擎：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let reports = json_lines(&out);
    let generated = reports
        .iter()
        .find(|report| report["event"] == "bridge-generated")
        .expect("应有 bridge-generated 报告");
    assert_eq!(generated["outputs"][0]["lang"], "napi");
    assert_eq!(generated["outputs"][0]["files"][0], "pax.c");

    let glue = temp.path().join("bridge/napi/pax.c");
    let text = std::fs::read_to_string(&glue).expect("胶水落盘");
    assert!(text.contains("napi_register_module_v1"), "{text}");
    assert!(
        text.contains("uniffi_pax_core_fn_func_greet("),
        "符号前缀 = 归一化 crate 名（与 UDL namespace 不同域）：{text}"
    );
}

/// OHOS SDK 缺失 → `poly::E_BRIDGE_0003` + 环境变量修复提示，且不产半份模块。
#[test]
fn bridge_ohos_without_sdk_reports_hint() {
    let temp = TempDir::new("no-ohos-sdk");
    fixture(temp.path());

    // 把四个 SDK 定位环境变量显式指到不存在的路径：本机装没装 SDK 都走「缺 SDK」分支
    let out = Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(["bridge", "ohos", "--abi", "arm64-v8a"])
        .current_dir(temp.path())
        .env("POLYPACK_OHOS_SDK", temp.path().join("no-sdk"))
        .env("DEVECO_SDK_HOME", temp.path().join("no-sdk"))
        .env("HARMONYOS_TOOLS", temp.path().join("no-sdk"))
        .env("OHOS_SDK_HOME", temp.path().join("no-sdk"))
        .output()
        .expect("poly 可执行");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_BRIDGE_0003"), "{stderr}");
    assert!(
        stderr.contains("HARMONYOS_TOOLS"),
        "提示应给出可执行的环境变量修法：{stderr}"
    );
    assert!(
        !temp.path().join("bridge").exists(),
        "缺 SDK 时不产半份模块、不留空目录"
    );
}

#[test]
fn bridge_without_engine_reports_missing_engine() {
    if uniffi_on_path() {
        eprintln!("SKIP：本机已装 uniffi-bindgen（缺引擎分支无从触发）");
        return;
    }
    let temp = TempDir::new("no-engine");
    fixture(temp.path());

    let out = poly(&["bridge", "generate"], temp.path());
    assert_eq!(out.status.code(), Some(1), "缺引擎必须非零退出");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_BRIDGE_0001"), "{stderr}");
    assert!(
        stderr.contains("uniffi"),
        "提示应给出装引擎的三条路：{stderr}"
    );
    assert!(
        !temp.path().join("bridge").exists(),
        "缺引擎时不产假绑定、不留空目录"
    );
}

#[test]
fn bridge_rejects_unknown_abi_by_name() {
    let temp = TempDir::new("bad-abi");
    fixture(temp.path());

    let out = poly(&["bridge", "aar", "--abi", "riscv64"], temp.path());
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_BRIDGE_0003"), "{stderr}");
    assert!(
        stderr.contains("arm64-v8a") && stderr.contains("x86_64"),
        "提示应列出四个 ABI 目录名：{stderr}"
    );
}

#[test]
fn bridge_aar_without_ndk_reports_hint() {
    let temp = TempDir::new("no-ndk");
    fixture(temp.path());

    // 显式把 NDK / SDK 环境指到不存在的路径：本机装没装 NDK 都走「缺 NDK」分支
    let out = Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(["bridge", "aar", "--abi", "arm64-v8a"])
        .current_dir(temp.path())
        .env("POLYPACK_NDK_HOME", temp.path().join("no-ndk"))
        .env("ANDROID_NDK_HOME", temp.path().join("no-ndk"))
        .env("POLYPACK_ANDROID_SDK", temp.path().join("no-sdk"))
        .env("ANDROID_HOME", temp.path().join("no-sdk"))
        .env("ANDROID_SDK_ROOT", temp.path().join("no-sdk"))
        .env("HOME", temp.path())
        .output()
        .expect("poly 可执行");
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_BRIDGE_0003"), "{stderr}");
    assert!(
        stderr.contains("ndk;27.0.12077973"),
        "提示应给出 sdkmanager 的装法：{stderr}"
    );
}
