//! FreeRTOS 示例板打包器（T-M6-07）。
//!
//! 产物是一棵可复现的板级包：
//! - `include/polypack_engine.h`：T-M6-02 生成器产出的唯一头文件；
//! - `lib/arm/libpolypack_engine.a`：应用侧 wrapper staticlib（no_std 引擎 +
//!   FreeRTOS `pvPortMalloc` / `vPortFree` 分配器 + panic 钩子 + 六个 C ABI 保留锚点）；
//! - `sample/`：mps2-an385（Cortex-M3）工程模板（启动 / 中断向量 / UART /
//!   FreeRTOSConfig / 链接脚本 / Makefile），内核按 tag + sha256 现拉，不入库；
//! - `poly-template.json`：模板版本 / 板型 / triple / 内核 tag + sha256 / 符号清单。
//!
//! 真跑由 verify 腿 `freertos-qemu-smoke` 执行：`make smoke` 构建并在 QEMU 中启动，
//! 引擎任务调用 `poly_engine_*`（版本 / 目标校验 / 缓存键）后以 semihosting
//! `SYS_EXIT_EXTENDED` 退出码 0 结束；工具链 / QEMU / 网络缺席按环境槽 SKIP-with-print。
//! 预置库（`EmbeddedInputs.prebuilt` 的 `arm` 项）在场时不跑 cargo。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::embedded::EmbeddedInputs;
use crate::error::{BundleError, io_err};
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, ArtifactKind, BundlePlan, Packager};
use crate::packagers::embedded::{self, EngineBuild, RETENTION_ANCHORS, TEMPLATE_VERSION};

/// 目标 id 与产物格式（与 `TargetSpec.package.{format,builder}` 一致）。
pub const ID: &str = "freertos";
/// 板级包格式。
pub const FORMAT: &str = "board";
/// QEMU 板型（Cortex-M3，CMSDK APB UART）。
pub const BOARD: &str = "mps2-an385";
/// Rust triple（内核 ARM_CM3 端口 / 裸机 no_std）。
pub const TRIPLE: &str = "thumbv7m-none-eabi";
/// 预置库架构键。
const ARCH: &str = "arm";
/// 固定内核版本与校验和（`fetch_kernel.sh` 现拉时校验，不入库）。
pub const KERNEL_TAG: &str = "V11.1.0";
/// 固定内核 tarball 的 sha256（下载后校验，不匹配即失败）。
pub const KERNEL_SHA256: &str = "0e21928b3bcc4f9bcaf7333fb1c8c0299d97e2ec9e13e3faa2c5a7ac8a3bc573";

const ROOT: &str = "board";
const HEADER_FILE: &str = "include/polypack_engine.h";
const LIB_FILE: &str = "lib/arm/libpolypack_engine.a";
const MANIFEST_FILE: &str = "poly-template.json";

/// 示例板模板文件（路径相对于 `board/`；打包器落盘与结构检查共用）。
const SAMPLE_FILES: [(&str, &str); 8] = [
    (
        "sample/Makefile",
        include_str!("freertos_template/Makefile"),
    ),
    (
        "sample/fetch_kernel.sh",
        include_str!("freertos_template/fetch_kernel.sh"),
    ),
    (
        "sample/FreeRTOSConfig.h",
        include_str!("freertos_template/FreeRTOSConfig.h"),
    ),
    (
        "sample/linker.ld",
        include_str!("freertos_template/linker.ld"),
    ),
    (
        "sample/src/board.h",
        include_str!("freertos_template/src/board.h"),
    ),
    (
        "sample/src/main.c",
        include_str!("freertos_template/src/main.c"),
    ),
    (
        "sample/src/uart.c",
        include_str!("freertos_template/src/uart.c"),
    ),
    (
        "sample/src/startup.c",
        include_str!("freertos_template/src/startup.c"),
    ),
];

/// FreeRTOS 示例板打包器。
pub struct FreeRtosBoard;

impl Packager for FreeRtosBoard {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec)?;
        inputs.product.validate(ID)?;
        let embedded = embedded_inputs(inputs)?;
        let header = embedded::read_header(&embedded.header)?;
        let mut plan = BundlePlan::new().with_script(HEADER_FILE, header);
        for (relative, contents) in SAMPLE_FILES {
            plan = plan.with_script(relative, contents);
        }
        Ok(plan.with_script(MANIFEST_FILE, manifest(inputs, spec)))
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let embedded = embedded_inputs(inputs)?;
        let root = inputs.out_dir.join(ROOT);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).map_err(io_err(&root))?;

        for script in &plan.scripts {
            fsutil::write_text(&root.join(&script.path), &script.contents)?;
        }
        let lib = engine_lib(inputs, embedded)?;
        fsutil::copy_file(&lib, &root.join(LIB_FILE), false)?;
        // wrapper 构建中间物不属于板级包；留着会污染 §5.3 扫描。
        let _ = std::fs::remove_dir_all(inputs.out_dir.join("engine").join(ARCH));

        check_freertos_structure(&root)?;
        embedded::check_no_absolute_paths(ID, &root)?;
        let sample = root.join("sample");
        Ok(Artifact {
            target: spec.id.clone(),
            format: FORMAT.to_string(),
            path: root.clone(),
            bytes: fsutil::tree_bytes(&root)?,
            kind: ArtifactKind::Directory,
            notes: vec![
                format!(
                    "FreeRTOS 板级包 v{TEMPLATE_VERSION}：{BOARD} / QEMU，内核 {KERNEL_TAG} \
                     按 sha256 现拉（不入库）；复跑：make -C {} fetch && make -C {} smoke",
                    sample.display(),
                    sample.display()
                ),
                format!(
                    "头文件 {HEADER_FILE}；静态库 {LIB_FILE}；引擎调用版本 / 目标校验 / \
                     缓存键后以 semihosting 退出码 0 结束"
                ),
            ],
        })
    }
}

/// 板级包结构检查结果（打包器自检与 `poly verify` 共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeRtosStructure {
    /// 模板版本。
    pub template_version: String,
    /// QEMU 板型。
    pub board: String,
    /// Rust triple。
    pub triple: String,
    /// 固定内核 tag。
    pub kernel_tag: String,
    /// 固定内核 sha256。
    pub kernel_sha256: String,
    /// 头文件声明的 `poly_engine_*` 符号。
    pub symbols: Vec<String>,
}

/// 校验板级包：模板文件 / 引擎库 / 头文件 / manifest 与固定内核钉扎。
pub fn check_freertos_structure(root: &Path) -> Result<FreeRtosStructure, BundleError> {
    for (label, relative) in [("C 头文件", HEADER_FILE), ("引擎静态库", LIB_FILE)] {
        if !root.join(relative).is_file() {
            return Err(bad(root, format!("缺{label}（{relative}）")));
        }
    }
    for (relative, _) in SAMPLE_FILES {
        if !root.join(relative).is_file() {
            return Err(bad(root, format!("缺模板文件（{relative}）")));
        }
    }
    let header_path = root.join(HEADER_FILE);
    let header = embedded::read_header(&header_path)?;
    let symbols = embedded::parse_c_abi(&header);
    if symbols.is_empty() {
        return Err(bad(root, "头文件没有 poly_engine_* 声明".to_string()));
    }
    let manifest_path = root.join(MANIFEST_FILE);
    let manifest_text = std::fs::read_to_string(&manifest_path).map_err(io_err(&manifest_path))?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .map_err(|err| bad(root, format!("poly-template.json 非法：{err}")))?;
    let version = field(root, &manifest, "template_version")?;
    if version != TEMPLATE_VERSION {
        return Err(bad(
            root,
            format!("manifest 模板版本 {version} ≠ {TEMPLATE_VERSION}"),
        ));
    }
    let board = field(root, &manifest, "board")?;
    let triple = field(root, &manifest, "triple")?;
    if board != BOARD || triple != TRIPLE {
        return Err(bad(
            root,
            format!("manifest board/triple = {board}/{triple}，期望 {BOARD}/{TRIPLE}"),
        ));
    }
    let kernel_tag = field(root, &manifest, "kernel_tag")?;
    let kernel_sha256 = field(root, &manifest, "kernel_sha256")?;
    if kernel_tag != KERNEL_TAG || kernel_sha256 != KERNEL_SHA256 {
        return Err(bad(
            root,
            format!(
                "manifest 内核钉扎 = {kernel_tag}/{kernel_sha256}，期望 {KERNEL_TAG}/{KERNEL_SHA256}"
            ),
        ));
    }
    let recorded: Vec<String> = manifest["symbols"]
        .as_array()
        .ok_or_else(|| bad(root, "poly-template.json 缺 symbols".to_string()))?
        .iter()
        .filter_map(|symbol| symbol.as_str().map(str::to_string))
        .collect();
    if recorded != symbols {
        return Err(bad(
            root,
            format!("manifest symbols = {recorded:?} ≠ 头文件 {symbols:?}"),
        ));
    }
    let makefile = std::fs::read_to_string(root.join("sample/Makefile"))
        .map_err(io_err(&root.join("sample/Makefile")))?;
    for needle in ["mps2-an385", "smoke", "semihosting-config enable=on"] {
        if !makefile.contains(needle) {
            return Err(bad(root, format!("Makefile 缺 {needle}（QEMU 冒烟腿）")));
        }
    }
    Ok(FreeRtosStructure {
        template_version: version.to_string(),
        board: board.to_string(),
        triple: triple.to_string(),
        kernel_tag: kernel_tag.to_string(),
        kernel_sha256: kernel_sha256.to_string(),
        symbols,
    })
}

fn field(root: &Path, manifest: &serde_json::Value, key: &str) -> Result<String, BundleError> {
    manifest[key]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| bad(root, format!("poly-template.json 缺 {key}")))
}

fn engine_lib(inputs: &BundleInputs, embedded: &EmbeddedInputs) -> Result<PathBuf, BundleError> {
    if let Some(lib) = embedded::prebuilt_lib(embedded, ARCH) {
        return Ok(lib.to_path_buf());
    }
    let staging = inputs.out_dir.join("engine").join(ARCH);
    embedded::build_staticlib(&EngineBuild {
        core_crate: embedded.engine_crate.as_path(),
        staging: &staging,
        target: TRIPLE,
        wrapper_rs: &wrapper_rs(),
        nightly: true,
        static_reloc: false,
    })
}

/// `poly-template.json` 内容（模板版本 / 板型 / triple / 内核钉扎 / 符号清单）。
fn manifest(inputs: &BundleInputs, spec: &TargetSpec) -> String {
    let symbols = inputs
        .embedded
        .as_ref()
        .and_then(|embedded| std::fs::read_to_string(&embedded.header).ok())
        .map(|header| embedded::parse_c_abi(&header))
        .unwrap_or_default();
    let manifest = serde_json::json!({
        "template": "polypack-freertos-board",
        "template_version": TEMPLATE_VERSION,
        "target": spec.id,
        "board": BOARD,
        "triple": TRIPLE,
        "kernel_tag": KERNEL_TAG,
        "kernel_sha256": KERNEL_SHA256,
        "abi": 1,
        "symbols": symbols,
        "commands": {
            "fetch": "make fetch",
            "smoke": "make smoke",
        },
        "no_ui": true,
    });
    format!(
        "{}\n",
        serde_json::to_string_pretty(&manifest).expect("FreeRTOS manifest 序列化")
    )
}

/// 应用侧 wrapper：六个 C ABI 保留锚点 + FreeRTOS heap_4 分配器 + 板级 panic 钩子。
///
/// `pvPortMalloc` 的对齐只保证 `portBYTE_ALIGNMENT`（Cortex-M3 为 8 字节），
/// `GlobalAlloc` 的 Layout 可能要求更大对齐，所以按 Layout 额外留头 + 对齐余量。
fn wrapper_rs() -> String {
    format!(
        r#"{RETENTION_ANCHORS}
// FreeRTOS 运行时：heap_4 分配器与板级 panic 钩子（板级包以 semihosting 退出）。
unsafe extern "C" {{
    fn pvPortMalloc(size: usize) -> *mut c_void;
    fn vPortFree(ptr: *mut c_void);
    fn poly_engine_rt_panic() -> !;
}}

struct FreeRtosAlloc;

unsafe impl core::alloc::GlobalAlloc for FreeRtosAlloc {{
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {{
        let align = layout.align().max(core::mem::size_of::<usize>());
        let size = layout.size().max(1);
        // SAFETY: 额外 size_of::<usize>() + align 给原始指针头与对齐余量。
        let raw = unsafe {{ pvPortMalloc(size + align + core::mem::size_of::<usize>()) }};
        if raw.is_null() {{
            return core::ptr::null_mut();
        }}
        let start = raw as usize + core::mem::size_of::<usize>();
        let aligned = (start + align - 1) & !(align - 1);
        let header = (aligned - core::mem::size_of::<usize>()) as *mut usize;
        // SAFETY: header 落在 raw..raw+size+align 内且按 usize 对齐。
        unsafe {{ *header = raw as usize }};
        aligned as *mut u8
    }}

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: core::alloc::Layout) {{
        // SAFETY: ptr 来自 alloc，头部保存原始 pvPortMalloc 指针。
        let header = (ptr as usize - core::mem::size_of::<usize>()) as *const usize;
        let raw = unsafe {{ *header }};
        unsafe {{ vPortFree(raw as *mut c_void) }};
    }}
}}

#[global_allocator]
static ALLOC: FreeRtosAlloc = FreeRtosAlloc;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {{
    // SAFETY: 板级钩子打印后以 semihosting 非零码退出，永不返回。
    unsafe {{ poly_engine_rt_panic() }}
}}
"#
    )
}

fn embedded_inputs(inputs: &BundleInputs) -> Result<&EmbeddedInputs, BundleError> {
    inputs.embedded.as_ref().ok_or(BundleError::MissingInput {
        packager: ID,
        field: "embedded",
    })
}

fn check_format(spec: &TargetSpec) -> Result<(), BundleError> {
    if spec.package.format == FORMAT {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: ID,
        field: "package.format",
        value: spec.package.format.clone(),
        reason: format!("FreeRTOS 打包器只服务 `{FORMAT}`（QEMU 示例板工程）"),
    })
}

fn bad(root: &Path, reason: String) -> BundleError {
    BundleError::InvalidInput {
        packager: ID,
        field: "freertos.board",
        value: root.display().to_string(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> BundleInputs {
        BundleInputs::new(
            crate::inputs::ProductInfo {
                name: "Pax Notes".to_string(),
                version: "1.0.0".to_string(),
                identifier: "com.polypack.pax".to_string(),
                manufacturer: "PolyPack".to_string(),
                description: "notes".to_string(),
                homepage: None,
            },
            std::env::temp_dir(),
        )
    }

    #[test]
    fn manifest_pins_board_triple_and_kernel() {
        let spec = TargetSpec::new_stub(ID, 6, &[poly_targets::Host::Linux]);
        let manifest: serde_json::Value =
            serde_json::from_str(&manifest(&inputs(), &spec)).expect("json");
        assert_eq!(manifest["board"], BOARD);
        assert_eq!(manifest["triple"], TRIPLE);
        assert_eq!(manifest["kernel_tag"], KERNEL_TAG);
        assert_eq!(manifest["kernel_sha256"], KERNEL_SHA256);
        assert_eq!(manifest["template_version"], TEMPLATE_VERSION);
    }

    #[test]
    fn template_pins_kernel_and_qemu_board() {
        let fetch = SAMPLE_FILES
            .iter()
            .find(|(path, _)| *path == "sample/fetch_kernel.sh")
            .map(|(_, contents)| *contents)
            .expect("fetch script");
        assert!(fetch.contains(KERNEL_TAG));
        assert!(fetch.contains(KERNEL_SHA256));
        let makefile = SAMPLE_FILES
            .iter()
            .find(|(path, _)| *path == "sample/Makefile")
            .map(|(_, contents)| *contents)
            .expect("Makefile");
        assert!(makefile.contains(BOARD));
        assert!(makefile.contains("smoke"));
        let linker = SAMPLE_FILES
            .iter()
            .find(|(path, _)| *path == "sample/linker.ld")
            .map(|(_, contents)| *contents)
            .expect("linker script");
        assert!(linker.contains("__exidx_start"));
    }

    #[test]
    fn structure_check_rejects_empty_dir() {
        let dir = std::env::temp_dir().join(format!("poly-freertos-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let err = check_freertos_structure(&dir).expect_err("空目录必须报错");
        assert!(matches!(err, BundleError::InvalidInput { .. }), "{err:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
