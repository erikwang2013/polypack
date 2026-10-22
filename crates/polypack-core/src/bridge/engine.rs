//! uniffi 引擎的定位与调用（T-M2-01）。
//!
//! 生成动作**不在本仓库实现**：绑定代码由 uniffi 官方生成器产出，本模块只负责找到它、
//! 用确定的参数调用它、把产物落点规范化。定位顺序（先显式后约定）：
//!
//! 1. `$POLYPACK_UNIFFI_BINDGEN` —— 显式指定可执行文件（CI 里 `cargo install` 后的路径）；
//! 2. `PATH` 上的 `uniffi-bindgen`（Windows 认 `.exe`）；
//! 3. 核心 crate 自带的 bin（`src/bin/uniffi-bindgen.rs`，uniffi 官方推荐的自托管形态）——
//!    经 `cargo run --quiet --bin uniffi-bindgen --` 调用，版本与核心 crate 的 uniffi 依赖锁死。
//!
//! 三处都没有 → 调用方报 `polypack::E_BRIDGE_0001` + 修复提示（既有 skip-if-missing 语义：
//! 缺工具是「跳过并说明」，绝不产出假绑定）。
//!
//! 调用参数固定为 `generate <udl> --language <lang> --out-dir <dir> --no-format`：
//! `--no-format` 关掉 ktlint / swiftformat 后处理——格式化工具的在场与否会影响产物字节，
//! 而产物字节进缓存键，故必须关掉（确定性优先于排版）。

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{BridgeLang, InterfaceSource};
use crate::diagnostics::{Diagnostic, codes};
use crate::plan::StageError;

/// uniffi 引擎的定位结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UniffiEngine {
    /// 直接可执行（`$POLYPACK_UNIFFI_BINDGEN` 或 PATH 上的 `uniffi-bindgen`）。
    Tool(PathBuf),
    /// 核心 crate 自带的 bin，经 `cargo run` 调用。
    ProjectBin {
        /// 核心 crate 目录（`cargo run` 的 cwd）。
        crate_dir: PathBuf,
    },
}

impl UniffiEngine {
    /// 按「显式 → PATH → crate 自带」顺序定位；都没有返回 `None`。
    pub fn discover(crate_dir: &Path) -> Option<Self> {
        if let Some(path) = std::env::var_os("POLYPACK_UNIFFI_BINDGEN")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
        {
            return Some(Self::Tool(path));
        }
        if let Some(path) = tool_on_path("uniffi-bindgen") {
            return Some(Self::Tool(path));
        }
        crate_dir
            .join("src")
            .join("bin")
            .join("uniffi-bindgen.rs")
            .is_file()
            .then(|| Self::ProjectBin {
                crate_dir: crate_dir.to_path_buf(),
            })
    }

    /// 引擎身份串（进缓存键：换引擎即换产物）。
    pub fn describe(&self) -> String {
        match self {
            Self::Tool(path) => format!("uniffi-bindgen@{}", path.display()),
            Self::ProjectBin { crate_dir } => {
                format!("cargo-run:uniffi-bindgen@{}", crate_dir.display())
            }
        }
    }

    /// 生成一种语言的绑定，返回产物文件（相对 `out_dir`，升序）。
    pub fn generate(
        &self,
        source: &InterfaceSource,
        lang: BridgeLang,
        out_dir: &Path,
        config: &Path,
    ) -> Result<Vec<PathBuf>, StageError> {
        std::fs::create_dir_all(out_dir).map_err(|err| io_error(out_dir, err))?;
        // 每次生成前清空落点：残留的上一版绑定会混进产物树与缓存条目
        clear_dir(out_dir)?;

        let udl = source.primary().to_string_lossy().to_string();
        let out = out_dir.to_string_lossy().to_string();
        let config = config.to_string_lossy().to_string();
        let args = [
            "generate",
            udl.as_str(),
            "--language",
            lang.id(),
            "--out-dir",
            out.as_str(),
            "--config",
            config.as_str(),
            "--no-format",
        ];
        let output = match self {
            Self::Tool(bin) => Command::new(bin).args(args).output(),
            Self::ProjectBin { crate_dir } => Command::new("cargo")
                .args(["run", "--quiet", "--bin", "uniffi-bindgen", "--"])
                .args(args)
                .current_dir(crate_dir)
                .output(),
        }
        .map_err(|err| {
            Diagnostic::error(
                codes::E_BRIDGE_0001,
                format!("无法启动 uniffi 引擎（{}）: {err}", self.describe()),
            )
            .with_hint(engine_hint())
        })?;

        if !output.status.success() {
            return Err(Diagnostic::error(
                codes::E_BRIDGE_0001,
                format!(
                    "uniffi 引擎生成 {lang} 绑定失败（{}）: {}",
                    self.describe(),
                    stderr_tail(&output.stderr)
                ),
            )
            .with_hint(engine_hint())
            .into());
        }

        let files = collect_files(out_dir)?;
        if files.is_empty() {
            return Err(Diagnostic::error(
                codes::E_BRIDGE_0001,
                format!(
                    "uniffi 引擎报告成功但 {} 下没有产物（接口定义可能是空的）",
                    out_dir.display()
                ),
            )
            .with_hint("核对 UDL 的 namespace 块内是否有函数 / 类型声明")
            .into());
        }
        Ok(files)
    }
}

/// 生成 uniffi 配置：把「库名 ↔ 绑定里的库名」钉死。
///
/// 生成的 Kotlin 用 `cdylib_name` 决定运行时 `System.loadLibrary` 的对象，Swift 用
/// `module_name` 决定 `<名>.swift` / `<名>FFI.h` / `<名>FFI.modulemap` 的文件名。二者
/// 缺省都取 namespace，与核心 crate 的 `lib<crate>.so` 未必同名——显式钉死后，
/// AAR 里的 `.so` 文件名与绑定要求的名字必然一致（否则只在设备上运行时才炸）。
///
/// 落点 `<root>/bridge/uniffi.toml`（随产物一起进缓存，可复现）。
///
/// 用 `[defaults.*]` 的**全局配置**形态：uniffi ≥0.29 的 `--config` 只认含 `[defaults]` /
/// `[crates.<名>]` / `[crate-roots]` 的全局配置，扁平的老形态（直接写 `[bindings.kotlin]`）
/// 会被**静默忽略**（只在 stderr 留一行 warning），于是库名照旧取默认值 —— 那种错配只在
/// 设备上 `System.loadLibrary` 时才炸。由 `bridge_real.rs` 断言 `findLibraryName` 兜住。
pub fn write_config(path: &Path, namespace: &str, lib_name: &str) -> Result<(), StageError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| io_error(parent, err))?;
    }
    let text = format!(
        "# 由 polypack bridge 生成（T-M2-01）：库名与绑定同名，勿手改。\n\
         [defaults.bindings.kotlin]\n\
         package_name = \"uniffi.{namespace}\"\n\
         cdylib_name = \"{lib_name}\"\n\
         \n\
         [defaults.bindings.swift]\n\
         module_name = \"{lib_name}\"\n\
         ffi_module_name = \"{lib_name}FFI\"\n"
    );
    std::fs::write(path, text).map_err(|err| io_error(path, err))
}

/// 引擎缺失 / 失败时的修复提示。
fn engine_hint() -> &'static str {
    "装一个 uniffi 生成器：①核心 crate 加 uniffi 依赖与 src/bin/uniffi-bindgen.rs\
     （fn main() { uniffi::uniffi_bindgen_main() }）；②或 cargo install uniffi --features cli \
     --bin uniffi-bindgen；③或用 $POLYPACK_UNIFFI_BINDGEN 指向已有可执行文件"
}

/// `PATH` 上查找可执行文件（Windows 认 `.exe`）。
fn tool_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let candidates = [name.to_string(), format!("{name}.exe")];
    for dir in std::env::split_paths(&path) {
        for candidate in &candidates {
            let full = dir.join(candidate);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

/// 清空目录（不存在即建空目录）。
fn clear_dir(dir: &Path) -> Result<(), StageError> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(io_error(dir, err)),
    }
    std::fs::create_dir_all(dir).map_err(|err| io_error(dir, err))
}

/// 递归收集 `dir` 下的普通文件（相对路径，升序）。
fn collect_files(dir: &Path) -> Result<Vec<PathBuf>, StageError> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).map_err(|err| io_error(&current, err))? {
            let entry = entry.map_err(|err| io_error(&current, err))?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|err| io_error(&path, err))?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                let rel = path.strip_prefix(dir).unwrap_or(&path).to_path_buf();
                out.push(rel);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// stderr 尾部关键行（≤3 行）——塞进诊断给出定位线索。
fn stderr_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let mut lines: Vec<&str> = text
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(3)
        .collect();
    lines.reverse();
    lines.join(" | ")
}

/// IO 失败 → `polypack::E_BRIDGE_0003`。
fn io_error(path: &Path, err: std::io::Error) -> StageError {
    Diagnostic::error(
        codes::E_BRIDGE_0003,
        format!("桥产物目录操作失败（{}）: {err}", path.display()),
    )
    .with_hint("确认工程目录可写；`polypack bridge` 需要 <工程根>/bridge/ 的写权限")
    .into()
}
