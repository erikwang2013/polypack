//! T-M6-13 验收（1/3）：无 UI（headless）L6 目标的产物体积预算门禁。
//!
//! 预算判定只有一个来源：`poly_core::budget`（判定 / 事件 / 失败诊断），CLI 的
//! `package` 阶段对**任何**打包器产物逐件套用同一门禁（`package_target`）。本文件
//! 用**真打包器 + 预置引擎静态库**（`EmbeddedInputs.prebuilt`，避免交叉工具链）把
//! L6 的四种产物形态真产出来，再逐型钉住门禁语义：
//!
//! - 静态库（VxWorks `library`，文件）
//! - 部署镜像（VxWorks `image`，zip 文件）
//! - west 包（Zephyr `west`，目录树）
//! - board 包（FreeRTOS `board`，目录树）
//!
//! 语义与 T-M3-08 同口径：未设预算不判定；恰好等于上限不算超；超限 + `warn` 事件带
//! `poly::E_BUDGET_0001` 但构建不拦；超限 + `fail` 给带修复提示的错误诊断（退出码 1）。
//! 已注册 7 个 tier-6 目标全部登记（含带 UI 的 android-automotive：其 APK 产物走同一
//! `package_target` 门禁，端到端 warn/fail 行为已由 `tests/budget.rs` 的快应用同一门禁
//! 钉住；QNX 无 SDP 时按环境槽 SKIP-with-print，不伪造产物也即无体积可判；watchos
//! 带 SwiftUI UI、且非 macOS 宿主按环境槽 SKIP，同样不入 headless 矩阵）。
//!
//! 端到端 CLI 证据在 `POLY_CONTRACT_REAL=1` 的 `autosar_build_budget_gate_end_to_end`
//! （真宿主静态库 + ARXML，真跑 `poly build --target autosar`）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use poly_bundle::embedded::{EmbeddedInputs, EmbeddedLib};
use poly_bundle::inputs::{BundleInputs, ProductInfo};
use poly_bundle::packagers::freertos::FreeRtosBoard;
use poly_bundle::packagers::qnx::QnxImage;
use poly_bundle::packagers::vxworks::{VxWorksImage, VxWorksLibrary};
use poly_bundle::packagers::zephyr::ZephyrWest;
use poly_bundle::{Artifact, BundleError, Packager};
use poly_core::budget::size_verdict;
use poly_core::diagnostics::codes;
use poly_core::events::BuildEvent;
use poly_core::job::{Budget, SizePolicy};
use poly_targets::{Registry, TargetSpec};

/// 已注册的 8 个 tier-6 目标（T-M6-01..07/11 + T-M7-09 garmin + T-M7-05/06 watchos）。
const TIER6_IDS: [&str; 8] = [
    "android-automotive",
    "autosar",
    "freertos",
    "garmin",
    "qnx",
    "vxworks",
    "watchos",
    "zephyr",
];

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "polypack-m6-budget-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
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

fn spec(id: &str) -> TargetSpec {
    Registry::builtin()
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("内建注册表缺少 {id}"))
}

fn product() -> ProductInfo {
    ProductInfo {
        name: "Pax Notes".to_string(),
        version: "1.2.3".to_string(),
        identifier: "com.polypack.paxnotes".to_string(),
        manufacturer: "PolyPack".to_string(),
        description: "Pax Notes".to_string(),
        homepage: None,
    }
}

/// 预置引擎静态库（假 `.a` 字节）：打包器的结构检查只看在场与文本元数据，
/// 不读归档内容；`prebuilt` 命中即不再起 cargo（无需交叉工具链）。
fn fake_lib(dir: &Path, arch: &str) -> EmbeddedLib {
    std::fs::create_dir_all(dir).expect("预置库目录");
    let path = dir.join(format!("libpolypack_engine_{arch}.a"));
    std::fs::write(&path, b"!<arch>\npolypack-test\n").expect("写预置库");
    EmbeddedLib {
        arch: arch.to_string(),
        path,
    }
}

/// 真实嵌入式输入：头文件由 poly-core 生成器现场产出（唯一 ABI 源），符号取 FFI_FUNCTIONS。
fn embedded(out: &Path, prebuilt: Vec<EmbeddedLib>) -> EmbeddedInputs {
    let header = poly_core::engine::header::write_header(out).expect("写引擎头文件");
    let engine_crate = Path::new(env!("CARGO_MANIFEST_DIR")).join("../poly-core");
    assert!(
        engine_crate.join("Cargo.toml").is_file(),
        "测试须在仓库内运行：{}",
        engine_crate.display()
    );
    EmbeddedInputs {
        header,
        symbols: poly_core::engine::ffi::FFI_FUNCTIONS
            .iter()
            .map(|function| function.name.to_string())
            .collect(),
        engine_crate,
        host_triple: poly_core::compile::host_triple().expect("宿主 triple"),
        qnx: None,
        qnx_triple: None,
        no_std: true,
        prebuilt,
    }
}

fn budget(size: Option<u64>, policy: SizePolicy) -> Budget {
    Budget {
        size,
        startup_ms: None,
        policy,
    }
}

/// 对一件真产物套用预算门禁，逐策略断言（与 `package_target` 的调用面一致）。
fn assert_gate(artifact: &Artifact, spec: &TargetSpec) {
    let target = &spec.id;
    let format = &artifact.format;
    assert!(
        artifact.bytes > 1,
        "{target}（{format}）产物应有可判定的体积：{}",
        artifact.bytes
    );

    // ① 未设预算：不设门禁（体积仍由 Packaged 事件报告）。
    let unset = budget(None, SizePolicy::Warn);
    assert!(
        size_verdict(artifact.bytes, &unset).is_none(),
        "{target}：未设 [budget].size 不得判定"
    );

    // ② 恰好等于上限不算超：事件不带错误码（「不超过」语义）。
    let exact = budget(Some(artifact.bytes), SizePolicy::Warn);
    let within = size_verdict(artifact.bytes, &exact).expect("有预算即判定");
    assert!(!within.exceeded, "{target}：等于上限不算超");
    let BuildEvent::Budgeted { code, .. } = within.event(target, format) else {
        panic!("应为 budgeted 事件");
    };
    assert!(code.is_none(), "{target}：在预算内不得带错误码");

    // ③ 超预算 + warn：事件带 poly::E_BUDGET_0001，构建不拦（调用方不返回 Err）。
    let over = artifact.bytes - 1;
    let warn =
        size_verdict(artifact.bytes, &budget(Some(over), SizePolicy::Warn)).expect("有预算即判定");
    assert!(warn.exceeded, "{target}：超预算必须判定为越界");
    let BuildEvent::Budgeted {
        target: event_target,
        format: event_format,
        bytes,
        limit,
        policy,
        code,
    } = warn.event(target, format)
    else {
        panic!("应为 budgeted 事件");
    };
    assert_eq!(event_target, *target);
    assert_eq!(event_format, *format);
    assert_eq!(bytes, artifact.bytes, "判定面 = 产物体积");
    assert_eq!(limit, over);
    assert_eq!(policy, "warn");
    assert_eq!(
        code,
        Some(codes::E_BUDGET_0001.to_string()),
        "{target}：warn 策略下事件必须带可定位错误码"
    );

    // ④ 超预算 + fail：错误诊断码 / 退出码 / 修复提示（package 节点据此失败）。
    let fail =
        size_verdict(artifact.bytes, &budget(Some(over), SizePolicy::Fail)).expect("有预算即判定");
    let diagnostic = fail.failure(target, format);
    assert_eq!(diagnostic.code, codes::E_BUDGET_0001);
    assert_eq!(diagnostic.severity, poly_core::Severity::Error);
    assert_eq!(diagnostic.exit_code(), 1);
    let text = diagnostic.to_string();
    for needle in [target.as_str(), format.as_str(), "超体积预算", "字节"] {
        assert!(text.contains(needle), "{target} 诊断缺 {needle:?}：{text}");
    }
    assert!(
        diagnostic
            .hint
            .as_deref()
            .unwrap_or("")
            .contains("[budget].size"),
        "{target}：修复提示应点名配置项"
    );
}

/// 已注册 tier-6 目标全覆盖：每个声明的 format 都解析到内建打包器（= package 阶段会
/// 走到同一预算门）；headless 集合恰好是 5 个，android-automotive / watchos 带 UI 不入矩阵。
#[test]
fn all_registered_tier6_targets_route_through_package_budget_gate() {
    let tier6: Vec<TargetSpec> = Registry::builtin()
        .all()
        .filter(|spec| spec.tier == 6)
        .cloned()
        .collect();
    let mut ids: Vec<&str> = tier6.iter().map(|spec| spec.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, TIER6_IDS, "tier-6 注册集合应与任务声明面一致");

    for spec in &tier6 {
        let packager = poly_bundle::find(&spec.id, &spec.package.format)
            .unwrap_or_else(|| panic!("{} 的 format 无内建打包器", spec.id));
        assert_eq!(packager.id(), spec.id, "{} 打包器归属", spec.id);
        assert_eq!(packager.format(), spec.package.format);
    }

    let headless: Vec<&str> = tier6
        .iter()
        .filter(|spec| spec.ui.is_empty())
        .map(|spec| spec.id.as_str())
        .collect();
    assert_eq!(
        headless,
        ["autosar", "freertos", "qnx", "vxworks", "zephyr"],
        "无 UI 目标 = embedded-ci 矩阵口径"
    );
}

/// 静态库 / 部署镜像 / west 包 / board 包四种 L6 产物形态的真产物体积都走同一门禁。
#[test]
fn headless_artifact_kinds_are_measured_and_gated() {
    let temp = TempDir::new("kinds");
    let root = temp.path();

    // VxWorks：预置 `vxworks` 架构静态库 → library（文件）与 image（zip）两形态。
    let out = root.join("vxworks");
    std::fs::create_dir_all(&out).expect("vxworks 输出目录");
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.embedded = Some(embedded(
        &out,
        vec![fake_lib(&root.join("libs"), "vxworks")],
    ));
    let library = VxWorksLibrary
        .package(&inputs, &spec("vxworks"))
        .expect("VxWorks library 打包");
    assert_eq!(library.format, "library");
    assert_gate(&library, &spec("vxworks"));
    let mut image_spec = spec("vxworks");
    image_spec.package.format = "image".to_string();
    let image = VxWorksImage
        .package(&inputs, &image_spec)
        .expect("VxWorks image 打包");
    assert_eq!(image.format, "image");
    assert_gate(&image, &image_spec);

    // Zephyr：x86 / arm 两枚预置库 → west 工程目录（目录树体积）。
    let out = root.join("zephyr");
    std::fs::create_dir_all(&out).expect("zephyr 输出目录");
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.embedded = Some(embedded(
        &out,
        vec![
            fake_lib(&root.join("libs"), "x86"),
            fake_lib(&root.join("libs"), "arm"),
        ],
    ));
    let west = ZephyrWest
        .package(&inputs, &spec("zephyr"))
        .expect("Zephyr west 打包");
    assert_eq!(west.format, "west");
    assert_gate(&west, &spec("zephyr"));

    // FreeRTOS：arm 预置库 → board 示例包（目录树体积）。
    let out = root.join("freertos");
    std::fs::create_dir_all(&out).expect("freertos 输出目录");
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.embedded = Some(embedded(&out, vec![fake_lib(&root.join("libs"), "arm")]));
    let board = FreeRtosBoard
        .package(&inputs, &spec("freertos"))
        .expect("FreeRTOS board 打包");
    assert_eq!(board.format, "board");
    assert_gate(&board, &spec("freertos"));
}

/// QNX 是环境槽目标：无 SDP 时打包器必须 `ToolMissing`（CLI 转 SKIP-with-print），
/// **不得伪造产物**——没有产物就没有体积可判，也就不会被门禁静默放行。有 SDP 的
/// CI 腿由 `contract_m6_rtos` 真跑；此处显式传 `qnx: None` 钉住失败闭合语义。
#[test]
fn qnx_without_sdp_fails_closed_instead_of_silently_passing() {
    let temp = TempDir::new("qnx-skip");
    let out = temp.path().join("qnx");
    std::fs::create_dir_all(&out).expect("qnx 输出目录");
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.embedded = Some(embedded(&out, Vec::new()));
    let err = QnxImage
        .package(&inputs, &spec("qnx"))
        .expect_err("无 SDP 必须报环境槽缺失，不得产假产物");
    assert!(
        matches!(err, BundleError::ToolMissing { .. }),
        "应为 ToolMissing（CLI 据此 SKIP-with-print）：{err:?}"
    );
    assert!(
        !out.join("pax-notes-image.tar.gz").exists() && !out.join("lib").exists(),
        "SKIP 后不得留下镜像或静态库"
    );
}

// ---------------------------------------------------------------- 端到端 CLI（POLY_CONTRACT_REAL=1）

fn real_mode() -> bool {
    matches!(
        std::env::var("POLY_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

fn poly(args: &[&str], cwd: &Path) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

/// 覆写 `[budget]` 段（模板其余内容不动；追加在文件尾 = 新表）。
fn write_budget(root: &Path, size: u64, policy: Option<&str>) {
    let path = root.join("polypack.toml");
    let base = std::fs::read_to_string(&path).expect("配置可读");
    let mut text = base
        .split("\n[budget]")
        .next()
        .expect("split 至少一节")
        .to_string();
    text.push_str(&format!("\n[budget]\nsize = {size}\n"));
    if let Some(policy) = policy {
        text.push_str(&format!("policy = \"{policy}\"\n"));
    }
    std::fs::write(&path, text).expect("配置可写");
}

fn json_lines(out: &std::process::Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("stdout 每行应为 JSON"))
        .collect()
}

/// 真跑端到端：`poly build --target autosar` 产物（宿主静态库 + ARXML）超预算时
/// warn 构建绿且事件带码；fail 退出 1 并给修复提示。需 POLY_CONTRACT_REAL=1（会真编译
/// poly-core 宿主静态库）。
#[test]
fn autosar_build_budget_gate_end_to_end() {
    if !real_mode() {
        eprintln!("SKIP：端到端预算门禁真跑需 POLY_CONTRACT_REAL=1（真编译宿主静态库）");
        return;
    }
    let temp = TempDir::new("autosar-e2e");
    let root = temp.path();
    let out = poly(&["init", "."], root);
    assert!(
        out.status.success(),
        "poly init 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // ① 超预算 + warn（缺省策略）：构建绿，budgeted 事件带 poly::E_BUDGET_0001。
    write_budget(root, 1, None);
    let out = poly(&["build", "--target", "autosar", "--json"], root);
    assert_eq!(
        out.status.code(),
        Some(0),
        "warn 策略不得拦构建：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let events = json_lines(&out);
    let verdict = events
        .iter()
        .find(|event| event["event"] == "budgeted")
        .unwrap_or_else(|| panic!("应有 budgeted 事件：{events:?}"));
    assert_eq!(verdict["target"], "autosar");
    assert_eq!(verdict["format"], "arxml");
    assert_eq!(verdict["policy"], "warn");
    assert_eq!(verdict["code"], "poly::E_BUDGET_0001");
    assert!(
        verdict["bytes"].as_u64().expect("产物体积") > 1,
        "真产物体积应可判定：{verdict}"
    );

    // ② 超预算 + fail：package 节点失败，退出 1，stderr 带码与修复提示。
    write_budget(root, 1, Some("fail"));
    let out = poly(&["build", "--target", "autosar", "--json"], root);
    assert_eq!(
        out.status.code(),
        Some(1),
        "fail 策略必须非零退出；stdout：{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    for needle in [
        "poly::E_BUDGET_0001",
        "autosar",
        "超体积预算",
        "[budget].size",
    ] {
        assert!(stderr.contains(needle), "stderr 缺 {needle:?}：{stderr}");
    }
    let events = json_lines(&out);
    let failed = events
        .iter()
        .find(|event| event["event"] == "failed")
        .unwrap_or_else(|| panic!("应有 failed 事件：{events:?}"));
    assert_eq!(failed["stage"], "package");
    assert_eq!(failed["code"], "poly::E_BUDGET_0001");
}
