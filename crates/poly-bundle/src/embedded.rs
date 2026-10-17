//! 嵌入式 / 车机目标的共享输入与引擎构建辅助（T-M6-03/04）。
//!
//! C ABI 的唯一事实源在 `poly-core`：头文件由 `poly_core::engine::header` 生成，符号清单
//! 来自 `poly_core::engine::ffi::FFI_FUNCTIONS`（两者同源）。本模块只消费，不复制 ABI：
//! 打包器拿到的是 CLI 递入的 `EmbeddedInputs`（头文件路径 + 符号清单 + 引擎源码路径）。
//!
//! 引擎构建形态（与 `poly-core/src/engine/ffi.rs` 的文档一致）：
//! - 四端（QNX 等）以 **rlib 链入应用 crate**（应用提供 global_allocator / panic_handler）；
//! - 需要 `.a` 时生成一个应用侧 `staticlib` 包装 crate（path 依赖 poly-core），不修改
//!   poly-core 的 crate-type；
//! - 宿主自包含静态库走 `cargo rustc -p poly-core --lib --crate-type staticlib`。

use std::path::{Path, PathBuf};

use crate::error::BundleError;

/// QNX 缺省交叉 triple（QNX 7.1 aarch64le）；可用 `$POLYPACK_QNX_TRIPLE` 覆盖。
pub const QNX_DEFAULT_TRIPLE: &str = "aarch64-unknown-nto-qnx710";

/// QNX SDP 已解析的三个路径（根 / host / target）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QnxEnv {
    /// SDP 根（含 `host/` 与 `target/`）。
    pub root: PathBuf,
    /// `$QNX_HOST`（`<root>/host/<os>/<arch>`）。
    pub host: PathBuf,
    /// `$QNX_TARGET`（`<root>/target/<ver>`）。
    pub target: PathBuf,
}

/// 嵌入式打包输入（QNX / AUTOSAR）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedInputs {
    /// 引擎 C 头文件（T-M6-02 生成器产出，唯一来源）。
    pub header: PathBuf,
    /// C ABI 函数名清单（`FFI_FUNCTIONS` 单一来源）。
    pub symbols: Vec<String>,
    /// poly-core 源码目录（包装 crate 的 path 依赖 / 宿主静态库构建）。
    pub engine_crate: PathBuf,
    /// 宿主 triple（AUTOSAR 静态库）。
    pub host_triple: String,
    /// QNX SDP（`None` = 无 SDP，按环境槽 SKIP-with-print）。
    pub qnx: Option<QnxEnv>,
    /// QNX 交叉 triple（缺省 [`QNX_DEFAULT_TRIPLE`]）。
    pub qnx_triple: Option<String>,
    /// 引擎层 no_std（AUTOSAR 可选；QNX 恒为 no_std）。
    pub no_std: bool,
}

impl EmbeddedInputs {
    /// 打包器取输入：产品元数据校验 + 头文件必须在场。
    pub fn require(&self, packager: &'static str) -> Result<(), BundleError> {
        if !self.header.is_file() {
            return Err(BundleError::MissingInput {
                packager,
                field: "embedded.header",
            });
        }
        if !self.engine_crate.join("Cargo.toml").is_file() {
            return Err(BundleError::MissingInput {
                packager,
                field: "embedded.engine_crate",
            });
        }
        Ok(())
    }

    /// QNX 交叉 triple（显式覆盖优先）。
    pub fn qnx_triple(&self) -> &str {
        self.qnx_triple.as_deref().unwrap_or(QNX_DEFAULT_TRIPLE)
    }
}

/// 打包器入口：`BundleInputs.embedded` 在场且头文件 / 引擎源码在场。
pub fn require<'a>(
    inputs: &'a crate::inputs::BundleInputs,
    packager: &'static str,
) -> Result<&'a EmbeddedInputs, BundleError> {
    let embedded = inputs.embedded.as_ref().ok_or(BundleError::MissingInput {
        packager,
        field: "embedded",
    })?;
    embedded.require(packager)?;
    Ok(embedded)
}

/// 生成应用侧 `staticlib` 包装 crate（path 依赖 poly-core；不修改 poly-core crate-type）。
///
/// 返回 crate 目录。QNX 交叉构建用 `cargo build --target <triple>`；宿主（AUTOSAR）
/// 也可用同一包装 crate，但宿主自包含产物优先走 [`host_staticlib_command`]。
pub fn write_wrapper_crate(
    dir: &Path,
    engine_crate: &Path,
    crate_name: &str,
    no_std: bool,
) -> Result<PathBuf, BundleError> {
    std::fs::create_dir_all(dir.join("src")).map_err(|err| crate::error::io_err(dir)(err))?;
    std::fs::write(
        dir.join("Cargo.toml"),
        wrapper_manifest_text(crate_name, engine_crate, no_std),
    )
    .map_err(|err| crate::error::io_err(dir)(err))?;
    std::fs::write(dir.join("src/lib.rs"), wrapper_source_text(no_std))
        .map_err(|err| crate::error::io_err(dir)(err))?;
    Ok(dir.to_path_buf())
}

/// 包装 crate 的 `Cargo.toml` 文本（plan 与落盘同源）。
pub(crate) fn wrapper_manifest_text(crate_name: &str, engine_crate: &Path, no_std: bool) -> String {
    format!(
        "[package]\nname = \"{crate_name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\
         publish = false\n\n[lib]\ncrate-type = [\"staticlib\"]\n\n\
         # 独立 workspace：工程根可能属于别的 Cargo workspace\n[workspace]\n\n\
         [dependencies]\npoly-core = {{ path = \"{}\", default-features = {} }}\n",
        toml_path(engine_crate),
        !no_std
    )
}

/// 包装 crate 的 `src/lib.rs` 文本（plan 与落盘同源）。
pub(crate) fn wrapper_source_text(no_std: bool) -> String {
    let mut lib = String::new();
    if no_std {
        lib.push_str("#![no_std]\n");
    }
    lib.push_str(
        "//! 应用侧 staticlib 包装：把 poly-core 的 C ABI 打进归档；ABI 定义在 poly-core，\n\
         //! 本 crate 不新增任何导出符号。\n\
         pub use poly_core::engine::ffi;\n",
    );
    lib
}

/// 宿主自包含静态库命令：`cargo rustc --manifest-path <poly-core>/Cargo.toml --lib
/// --crate-type staticlib`（poly-core 文档给出的宿主最小面）。
pub fn host_staticlib_command(
    engine_crate: &Path,
    host_triple: &str,
    target_dir: &Path,
    no_std: bool,
) -> crate::Command {
    let mut args = vec![
        "rustc".to_string(),
        "--manifest-path".to_string(),
        engine_crate.join("Cargo.toml").display().to_string(),
        "--lib".to_string(),
        "--crate-type".to_string(),
        "staticlib".to_string(),
        "--release".to_string(),
        "--target".to_string(),
        host_triple.to_string(),
        "--target-dir".to_string(),
        target_dir.display().to_string(),
    ];
    if no_std {
        args.push("--no-default-features".to_string());
    }
    crate::Command::new("cargo", args)
}

/// 宿主静态库落点：`<target-dir>/<triple>/release/libpoly_core.a`。
pub fn host_staticlib_path(target_dir: &Path, host_triple: &str) -> PathBuf {
    target_dir
        .join(host_triple)
        .join("release")
        .join("libpoly_core.a")
}

/// `nm -g --defined-only <archive>` 的 `poly_engine_*` 导出符号集合（排序）。
///
/// 工具缺席 / 命令失败 → `None`（调用方按 SKIP-with-print 处理，绝不假绿）。
pub fn nm_engine_symbols(archive: &Path) -> Option<Vec<String>> {
    let output = std::process::Command::new("nm")
        .args(["-g", "--defined-only"])
        .arg(archive)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut symbols: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .filter(|name| name.starts_with("poly_engine_"))
        .map(str::to_string)
        .collect();
    symbols.sort();
    symbols.dedup();
    Some(symbols)
}

/// TOML 字符串里的路径：正斜杠 + 引号 / 反斜杠转义（跨平台生成）。
fn toml_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .replace('"', "\\\"")
}
