//! T-M6-03/04/07 验收：QNX 静态库 + 镜像、AUTOSAR Adaptive 静态库 + ARXML、
//! FreeRTOS QEMU 示例板。
//!
//! 默认（mock）只跑结构与 SKIP 口径：注册表 / 打包器解析、无 SDP 时 SKIP-with-print
//! 且不落任何伪造产物、doctor 的 qnx-sdp 探测行。真实静态库构建、`polypack build --target
//! qnx` 的端到端 SKIP 与 FreeRTOS 板级包 + QEMU 冒烟由 `POLYPACK_CONTRACT_REAL=1` 开启
//! （本机/CI 手动验收）。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::{combined, command_available, init_project, polypack};
use polypack_bundle::embedded::{EmbeddedInputs, QnxEnv};
use polypack_bundle::inputs::{BundleInputs, ProductInfo};
use polypack_bundle::packagers::freertos::FreeRtosBoard;
use polypack_bundle::{
    AutosarAdaptive, Packager, QnxImage, builtin, check_arxml, check_freertos_structure, find,
};
use polypack_targets::{Host, PackageSpec, Registry, TargetSpec};

fn real_mode() -> bool {
    matches!(
        std::env::var("POLYPACK_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
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

fn spec(id: &str) -> TargetSpec {
    Registry::builtin()
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("内建注册表缺少 {id}"))
}

fn ffi_symbols() -> Vec<String> {
    let mut symbols: Vec<String> = polypack_core::engine::ffi::FFI_FUNCTIONS
        .iter()
        .map(|function| function.name.to_string())
        .collect();
    symbols.sort();
    symbols
}

/// 真实引擎输入：头文件由 polypack-core 生成器现场产出，符号取 FFI_FUNCTIONS。
fn embedded(out: &Path, qnx: Option<QnxEnv>, no_std: bool) -> EmbeddedInputs {
    let header = polypack_core::engine::header::write_header(out).expect("写引擎头文件");
    let engine_crate = Path::new(env!("CARGO_MANIFEST_DIR")).join("../polypack-core");
    assert!(
        engine_crate.join("Cargo.toml").is_file(),
        "测试须在仓库内运行：{}",
        engine_crate.display()
    );
    EmbeddedInputs {
        header,
        symbols: polypack_core::engine::ffi::FFI_FUNCTIONS
            .iter()
            .map(|function| function.name.to_string())
            .collect(),
        engine_crate,
        host_triple: polypack_core::compile::host_triple().expect("宿主 triple"),
        qnx,
        qnx_triple: None,
        no_std,
        prebuilt: Vec::new(),
    }
}

#[test]
fn qnx_and_autosar_targets_and_packagers_are_wired() {
    for id in ["qnx", "autosar"] {
        let spec = spec(id);
        assert_eq!(spec.tier, 6, "{id} 应为 L6");
        assert!(spec.hosts.contains(&Host::Linux), "{id} 应 Linux 可达");
        assert!(spec.ui.is_empty(), "{id} 无 UI");
        assert_eq!(spec.runtime, polypack_targets::Runtime::Staticlib);
        assert_eq!(spec.bridge, polypack_targets::Bridge::Cffi);
    }
    let qnx = spec("qnx");
    assert_eq!(qnx.package.format, "image");
    assert_eq!(qnx.package.builder, "qnx-sdp");
    assert_eq!(
        qnx.verify,
        vec![
            "qnx-image-structure".to_string(),
            "c-abi-symbols".to_string(),
            "qnx-qemu-smoke".to_string()
        ]
    );

    let autosar = spec("autosar");
    assert_eq!(autosar.package.format, "arxml");
    assert_eq!(autosar.package.builder, "autosar-adaptive");
    assert_eq!(
        autosar.verify,
        vec!["arxml-schema".to_string(), "c-abi-symbols".to_string()]
    );

    // 两个 QNX 格式（image 缺省 / library 配置覆盖）与 AUTOSAR 各解析到同一实现。
    assert_eq!(find("qnx", "image").map(|p| p.id()), Some("qnx"));
    assert_eq!(find("qnx", "library").map(|p| p.id()), Some("qnx"));
    assert_eq!(find("autosar", "arxml").map(|p| p.id()), Some("autosar"));
    let pairs: Vec<(String, String)> = builtin()
        .iter()
        .map(|p| (p.id().to_string(), p.format().to_string()))
        .collect();
    let mut unique = pairs.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(pairs.len(), unique.len(), "打包器 (id, format) 不得重复");
}

/// T-M6-10 声明侧：Zephyr 冒烟腿挂在 TargetSpec.verify，`polypack retry --from verify` 可独立重跑。
#[test]
fn zephyr_qemu_smoke_leg_is_declared() {
    let zephyr = spec("zephyr");
    assert_eq!(zephyr.tier, 6);
    assert!(zephyr.ui.is_empty());
    assert_eq!(zephyr.package.format, "west");
    assert_eq!(zephyr.package.builder, "zephyr-west");
    assert_eq!(
        zephyr.verify,
        vec![
            "c-abi-symbols".to_string(),
            "zephyr-west-build".to_string(),
            "zephyr-qemu-smoke".to_string()
        ]
    );
    assert_eq!(
        find("zephyr", "west").map(|packager| packager.id()),
        Some("zephyr")
    );
}

/// T-M6-07 声明侧：目标 / 打包器 / verify id 全部接线。
#[test]
fn freertos_target_and_packager_are_wired() {
    let freertos = spec("freertos");
    assert_eq!(freertos.tier, 6, "FreeRTOS 应为 L6");
    assert!(
        freertos.hosts.contains(&Host::Linux),
        "FreeRTOS 应 Linux 可达"
    );
    assert!(freertos.ui.is_empty(), "FreeRTOS 无 UI");
    assert_eq!(freertos.runtime, polypack_targets::Runtime::Staticlib);
    assert_eq!(freertos.bridge, polypack_targets::Bridge::Cffi);
    assert_eq!(freertos.package.format, "board");
    assert_eq!(freertos.package.builder, "freertos-qemu");
    assert_eq!(
        freertos.verify,
        vec![
            "c-abi-symbols".to_string(),
            "freertos-qemu-smoke".to_string()
        ]
    );
    assert_eq!(
        find("freertos", "board").map(|packager| packager.id()),
        Some("freertos")
    );
}

/// 真实 FreeRTOS 板级包：引擎静态库 + 模板真产 + 结构断言 + QEMU 冒烟。
#[test]
fn freertos_package_builds_board_and_qemu_smoke() {
    if !real_mode() {
        eprintln!("SKIP：真实 FreeRTOS 板级包与 QEMU 冒烟需 POLYPACK_CONTRACT_REAL=1");
        return;
    }
    let guard = TempDir::new("polypack-freertos-real");
    let out = guard.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.embedded = Some(embedded(&out, None, true));
    let artifact = FreeRtosBoard
        .package(&inputs, &spec("freertos"))
        .expect("FreeRTOS 板级包应真跑通过");
    assert_eq!(artifact.format, "board");
    let board = out.join("board");
    let structure = check_freertos_structure(&board).expect("板级包结构检查");
    assert_eq!(structure.board, "mps2-an385");
    assert_eq!(structure.triple, "thumbv7m-none-eabi");
    assert_eq!(structure.symbols, ffi_symbols());

    if !command_available("arm-none-eabi-gcc", &["--version"])
        || !command_available("qemu-system-arm", &["--version"])
    {
        eprintln!("SKIP QEMU：本机缺 arm-none-eabi-gcc / qemu-system-arm");
        return;
    }
    let sample = board.join("sample");
    let kernel_dir = std::env::var_os("POLYPACK_FREERTOS_KERNEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| guard.path().join("kernel"));
    let smoke = Command::new("make")
        .arg("smoke")
        .arg(format!("KERNEL_DIR={}", kernel_dir.display()))
        .arg(format!(
            "BUILD_DIR={}",
            guard.path().join("build").display()
        ))
        .current_dir(&sample)
        .output()
        .expect("make 可执行");
    let text = combined(&smoke);
    eprintln!("FreeRTOS smoke 输出：{text}");
    assert!(smoke.status.success(), "QEMU 冒烟失败：{text}");
    assert!(
        text.contains("polypack_engine version=") && text.contains("SMOKE PASS"),
        "须打印引擎版本行且 QEMU 退出码 0：{text}"
    );
}

#[test]
fn doctor_reports_qnx_sdp_probe_with_install_hint() {
    let out = polypack(&["doctor", "--json"], &std::env::temp_dir());
    assert_eq!(out.status.code(), Some(0), "doctor 默认恒 0");
    let values: Vec<serde_json::Value> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("doctor JSON 行"))
        .collect();
    let qnx = values
        .iter()
        .find(|value| value["id"] == "qnx-sdp")
        .expect("doctor 输出应含 qnx-sdp 探测行");
    if qnx["found"].is_null() {
        let note = qnx["note"].as_str().unwrap_or_default();
        assert!(
            note.contains("QNX SDP") && note.contains("QNX_HOST"),
            "缺失提示须说明 SDP 安装与环境变量：{note}"
        );
    }
}

#[test]
fn qnx_without_sdp_skips_with_print_and_fakes_nothing() {
    let guard = TempDir::new("polypack-qnx-skip");
    let out = guard.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.embedded = Some(embedded(&out, None, true));
    let image = QnxImage;
    let plan = image
        .plan(&inputs, &spec("qnx"))
        .expect("无 SDP 的 plan 仍可预览");
    assert!(plan.commands.is_empty(), "无 SDP 不得发起交叉编译");
    assert!(
        plan.skipped
            .iter()
            .any(|skip| skip.reason.contains("QNX SDP")),
        "SKIP 注记须点名 QNX SDP：{:?}",
        plan.skipped
    );
    let err = image
        .package(&inputs, &spec("qnx"))
        .expect_err("无 SDP 必须返回环境槽错误而不是伪造产物");
    assert!(
        matches!(err, polypack_bundle::BundleError::ToolMissing { .. }),
        "{err:?}"
    );
    assert!(
        !out.join("qnx").exists(),
        "SKIP 后不得留下 buildfile / manifest"
    );
    assert!(!out.join("lib").exists(), "SKIP 后不得留下静态库");
    assert!(
        !out.join("pax-notes-image.tar.gz").exists(),
        "SKIP 后不得留下镜像"
    );
}

#[test]
fn qnx_library_format_plan_targets_cross_triple() {
    let guard = TempDir::new("polypack-qnx-library");
    let out = guard.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.embedded = Some(embedded(
        &out,
        Some(QnxEnv {
            root: PathBuf::from("/opt/qnx710"),
            host: PathBuf::from("/opt/qnx710/host/linux/x86_64"),
            target: PathBuf::from("/opt/qnx710/target/qnx7"),
        }),
        true,
    ));
    let mut library = spec("qnx");
    library.package = PackageSpec {
        format: "library".to_string(),
        builder: "qnx-sdp".to_string(),
    };
    let plan = find("qnx", "library")
        .expect("library 格式打包器")
        .plan(&inputs, &library)
        .expect("有 SDP 时规划交叉构建");
    let command = plan.commands.first().expect("cargo 命令");
    assert!(
        command
            .args
            .iter()
            .any(|arg| arg == "aarch64-unknown-nto-qnx710"),
        "缺省交叉 triple：{command:?}"
    );
    assert!(command.env.iter().any(|(key, _)| key == "QNX_HOST"));
    assert!(
        plan.scripts
            .iter()
            .any(|script| script.path.ends_with("pax-notes.build"))
    );
}

/// 真实 AUTOSAR 打包：cargo rustc 真产静态库 + ARXML 结构 / 符号 / xmllint 真跑。
#[test]
fn autosar_package_builds_staticlib_and_arxml() {
    if !real_mode() {
        eprintln!("SKIP：真实静态库构建需 POLYPACK_CONTRACT_REAL=1");
        return;
    }
    let guard = TempDir::new("polypack-autosar-real");
    let out = guard.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let mut inputs = BundleInputs::new(product(), &out);
    inputs.embedded = Some(embedded(&out, None, false));
    let artifact = AutosarAdaptive
        .package(&inputs, &spec("autosar"))
        .expect("AUTOSAR 打包应真跑通过");
    assert_eq!(artifact.format, "arxml");
    assert!(artifact.path.extension().is_some_and(|ext| ext == "arxml"));
    let structure = check_arxml(&out).expect("ARXML 结构检查");
    assert_eq!(structure.services, ffi_symbols());
    assert_eq!(structure.ports.len(), ffi_symbols().len());
    let library = out.join("lib/libpolypack_engine_autosar.a");
    assert!(library.is_file(), "静态库应产出：{}", library.display());
    if polypack_bundle::exec::tool_present("nm") {
        let found = polypack_bundle::embedded::nm_engine_symbols(&library).expect("nm 真跑");
        assert_eq!(found, ffi_symbols());
    }
    if polypack_bundle::xmllint_tool().is_some() {
        assert!(
            artifact
                .notes
                .iter()
                .any(|note| note.contains("xmllint 真跑")),
            "xmllint 在场时必须真跑并注记：{:?}",
            artifact.notes
        );
    }
}

/// 端到端 `polypack build --target qnx`：无 SDP 时告警 SKIP-with-print、无包目录。
#[test]
fn qnx_build_without_sdp_skips_with_print_end_to_end() {
    if !real_mode() {
        eprintln!("SKIP：端到端构建需 POLYPACK_CONTRACT_REAL=1");
        return;
    }
    if !cfg!(target_os = "linux") {
        eprintln!("SKIP：端到端构建契约在 Linux 宿主执行");
        return;
    }
    let project = init_project("m6-qnx");
    let out = Command::new(env!("CARGO_BIN_EXE_polypack"))
        .args(["build", "--target", "qnx", "--json"])
        .current_dir(&project)
        .env_remove("QNX_HOST")
        .env_remove("QNX_TARGET")
        .env_remove("POLYPACK_QNX_SDP")
        .env_remove("QNX_SDP_ROOT")
        .env_remove("QNX_SDP")
        .output()
        .expect("polypack 可执行");
    let text = combined(&out);
    eprintln!("polypack build --target qnx 输出：{text}");
    assert!(out.status.success(), "无 SDP 不应让构建失败：{text}");
    assert!(
        text.contains("QNX SDP") && text.contains("未打包"),
        "须打印环境槽 SKIP 原因：{text}"
    );
    let packages = project.join(".polypack/packages/qnx");
    assert!(
        !packages.join("qnx").exists() && !packages.join("lib").exists(),
        "SKIP 后不得留下伪造的 QNX 产物：{}",
        packages.display()
    );
}

/// 测试用临时目录（唯一名；避免引入新依赖）。
struct TempDir(PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}
