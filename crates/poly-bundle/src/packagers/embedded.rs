//! 嵌入式引擎打包的共享实现（T-M6-05 VxWorks / T-M6-06 Zephyr）。
//!
//! 三件事只在这里做一次：
//! - **引擎静态库**：应用侧 wrapper crate 以 path 依赖链入 `poly-core`（rlib，no_std +
//!   alloc），提供 `global_allocator` / `panic_handler` 并保留六个 `poly_engine_*` 符号；
//!   T-M6-02 契约不要求改 poly-core 的 crate-type。x86 目标用 nightly `-Z build-std` +
//!   `relocation-model=static`（预置 core 是 PIC，Zephyr 的 x86 链接器脚本会丢弃 `.got.plt`）。
//! - **§5.3 路径审计**：`--remap-path-prefix` 把工具链 / cargo registry / 工作区 / 暂存目录
//!   映射成相对前缀，产物里不允许出现 `/home/`、`/tmp/`、`C:\` 等绝对路径。
//! - **C ABI 断言**：头文件是唯一符号源（T-M6-02 生成器产物），`nm` 定义集合必须覆盖声明，
//!   未解析闭包只允许各目标运行时白名单（工具链缺席时的真跑面，wr-ld / west 另腿 SKIP）。

use std::path::{Path, PathBuf};

use crate::error::{BundleError, io_err};
use crate::exec;
use crate::inputs::EmbeddedInputs;
use crate::packager::Command;

/// 模板版本（`prj.conf` / manifest 与模板同版，随模板演进递增）。
pub const TEMPLATE_VERSION: &str = "1";

/// wrapper crate 的公共头部：no_std 导入 + 六个 `poly_engine_*` 保留锚点。
///
/// staticlib 只收被引用对象；不显式引用时 poly-core 的对象（含全部 C ABI 符号）会被整包剪掉。
/// 类型逐字对应 T-M6-02 生成的头文件，不新增 / 不重声明 ABI；Zephyr / VxWorks 只在尾部
/// 追加各自的 allocator / panic_handler。
pub const RETENTION_ANCHORS: &str = r#"#![no_std]

use core::ffi::{c_char, c_void};

#[used]
static KEEP_ABI_VERSION: extern "C" fn() -> u32 =
    poly_core::engine::ffi::poly_engine_abi_version;
#[used]
static KEEP_VERSION: extern "C" fn() -> *const c_char = poly_core::engine::ffi::poly_engine_version;
#[used]
static KEEP_VALIDATE_JOB: extern "C" fn(*const u8, usize) -> i32 =
    poly_core::engine::ffi::poly_engine_validate_job_json;
#[used]
static KEEP_VALIDATE_TARGET: extern "C" fn(*const u8, usize) -> i32 =
    poly_core::engine::ffi::poly_engine_validate_target_json;
#[used]
static KEEP_CACHE_KEY: extern "C" fn(
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
    *const c_char,
) -> *mut c_char = poly_core::engine::ffi::poly_engine_cache_key;
#[used]
static KEEP_STRING_FREE: extern "C" fn(*mut c_char) =
    poly_core::engine::ffi::poly_engine_string_free;
"#;

/// 读 T-M6-02 生成的唯一头文件。
pub fn read_header(path: &Path) -> Result<String, BundleError> {
    std::fs::read_to_string(path).map_err(io_err(path))
}

/// 从头文件里取 `poly_engine_*` 符号声明（去重排序；生成器是唯一 ABI 源）。
pub fn parse_c_abi(header: &str) -> Vec<String> {
    let mut symbols: Vec<String> = header
        .match_indices("poly_engine_")
        .filter_map(|(offset, prefix)| {
            let rest = &header[offset + prefix.len()..];
            let end = rest
                .find(|ch: char| !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_'))
                .unwrap_or(rest.len());
            (end > 0).then(|| format!("{prefix}{}", &rest[..end]))
        })
        .collect();
    symbols.sort();
    symbols.dedup();
    symbols
}

/// 归档里的全部定义符号（`nm -A --defined-only`，含局部符号）。
pub fn defined_symbols(lib: &Path) -> Result<Vec<String>, BundleError> {
    let output = exec::run(&Command::new(nm()?, ["-A", "--defined-only", &path(lib)]))?;
    Ok(parse_symbols(&output.stdout, SymbolKind::Defined))
}

/// 归档的真正未解析符号闭包：全部定义集合 − 全部 undefined 集合。
pub fn unresolved_symbols(lib: &Path) -> Result<Vec<String>, BundleError> {
    let defined: std::collections::BTreeSet<String> =
        defined_symbols(lib)?.into_iter().collect();
    let output = exec::run(&Command::new(nm()?, ["-A", "-u", &path(lib)]))?;
    let mut unresolved: Vec<String> = parse_symbols(&output.stdout, SymbolKind::Undefined)
        .into_iter()
        .filter(|symbol| !defined.contains(symbol))
        .collect();
    unresolved.sort();
    unresolved.dedup();
    Ok(unresolved)
}

/// C ABI 断言：头文件声明的每个符号都必须在归档里定义；归档不得多出未声明的
/// `poly_engine_*` 符号（第二份 ABI / 手写头文件的漂移会在这里炸）。
pub fn check_c_abi(lib: &Path, header: &str) -> Result<(), BundleError> {
    let declared = parse_c_abi(header);
    let defined = defined_symbols(lib)?;
    let missing: Vec<&String> = declared
        .iter()
        .filter(|symbol| !defined.contains(symbol))
        .collect();
    let extra: Vec<&String> = defined
        .iter()
        .filter(|symbol| symbol.starts_with("poly_engine_") && !declared.contains(symbol))
        .collect();
    if missing.is_empty() && extra.is_empty() {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: "embedded",
        field: "poly_engine_*",
        value: lib.display().to_string(),
        reason: format!("头文件与静态库符号不一致：缺 {missing:?}，多 {extra:?}"),
    })
}

/// 未解析闭包白名单断言；返回越界符号（空 = 通过）。
pub fn unexpected_unresolved(lib: &Path, allowed: &[&str]) -> Result<Vec<String>, BundleError> {
    Ok(unresolved_symbols(lib)?
        .into_iter()
        .filter(|symbol| !allowed.contains(&symbol.as_str()))
        .collect())
}

/// 归档里不得出现 Rust `std` 符号（no_std 接入的可执行检查）。
pub fn check_no_std(lib: &Path) -> Result<(), BundleError> {
    let mut symbols = defined_symbols(lib)?;
    symbols.extend(unresolved_symbols(lib)?);
    let hits: Vec<&String> = symbols
        .iter()
        .filter(|symbol| symbol.starts_with("_ZN3std") || symbol.contains("3std9panicking"))
        .collect();
    if hits.is_empty() {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: "embedded",
        field: "no_std",
        value: lib.display().to_string(),
        reason: format!("发现 std 符号：{hits:?}"),
    })
}

/// 文件 / 目录树里的绝对路径样本（§5.3；空 = 干净）。
pub fn absolute_paths(path: &Path) -> Result<Vec<String>, BundleError> {
    let mut found = Vec::new();
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(io_err(path))?
            .flatten()
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for entry in entries {
            found.extend(absolute_paths(&entry)?);
        }
        return Ok(found);
    }
    let bytes = std::fs::read(path).map_err(io_err(path))?;
    const NEEDLES: [&[u8]; 9] = [
        b"/home/",
        b"/root/",
        b"/Users/",
        b"/tmp/",
        b"/private/",
        b"/mnt/",
        b"/var/",
        b"/rustc/",
        b"C:\\",
    ];
    for needle in NEEDLES {
        let mut from = 0usize;
        while let Some(offset) = bytes[from..]
            .windows(needle.len())
            .position(|window| window == needle)
        {
            let start = from + offset;
            // 只认「路径起点」：`registry/src/.../src/private/doc.rs` 里的 `/private/`
            // 是相对路径的内部段，不是绝对路径。
            let boundary = start == 0
                || !matches!(
                    bytes[start - 1],
                    b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.' | b'/' | b'-' | b'\\'
                );
            if !boundary {
                from = start + needle.len();
                continue;
            }
            let end = (start + 80).min(bytes.len());
            let sample: String = bytes[start..end]
                .iter()
                .map(|byte| {
                    if (0x20..0x7f).contains(byte) {
                        *byte as char
                    } else {
                        ' '
                    }
                })
                .collect();
            found.push(format!("{}: {}", path.display(), sample.trim_end()));
            from = start + needle.len();
            if found.len() >= 8 {
                return Ok(found);
            }
        }
    }
    Ok(found)
}

/// §5.3 硬门禁：产物（或产物目录）出现绝对路径即失败。
pub fn check_no_absolute_paths(packager: &'static str, path: &Path) -> Result<(), BundleError> {
    let found = absolute_paths(path)?;
    if found.is_empty() {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager,
        field: "artifact.paths",
        value: path.display().to_string(),
        reason: format!("产物含绝对路径（§5.3）：{}", found.join("；")),
    })
}

/// 预置库查找（`prebuilt` 非空时打包器不跑 cargo）。
pub fn prebuilt_lib<'a>(inputs: &'a EmbeddedInputs, arch: &str) -> Option<&'a Path> {
    inputs
        .prebuilt
        .iter()
        .find(|lib| lib.arch == arch)
        .map(|lib| lib.path.as_path())
}

/// 引擎静态库构建参数。
pub struct EngineBuild<'a> {
    /// poly-core crate 源目录。
    pub core_crate: &'a Path,
    /// 暂存目录（wrapper crate 与 CARGO_TARGET_DIR 落点）。
    pub staging: &'a Path,
    /// Rust triple。
    pub target: &'a str,
    /// wrapper crate 的 `src/lib.rs`（应用侧 allocator / panic / 符号保留锚点）。
    pub wrapper_rs: &'a str,
    /// 走 nightly `-Z build-std=core,alloc`（无预置 core 的目标，如 VxWorks / i686）。
    pub nightly: bool,
    /// 附加 `-C relocation-model=static`（i686 预置 core 为 PIC；Zephyr 丢弃 `.got.plt`）。
    pub static_reloc: bool,
}

/// 构建应用侧 wrapper staticlib；返回 `.a` 路径。
///
/// 只在 `prebuilt` 缺省时调用。路径重映射在这里统一注入——产物无绝对路径不是事后清洗，
/// 而是编译期就映射掉（§5.3）。
pub fn build_staticlib(build: &EngineBuild<'_>) -> Result<PathBuf, BundleError> {
    // cargo / rustc 会把路径依赖解析成规范路径；remap 必须用同一形态，否则
    // `poly-cli/../poly-core` 之类的写法只能映射到字面量，产物里仍留绝对路径。
    let core_crate = std::fs::canonicalize(build.core_crate).unwrap_or_else(|_| {
        build.core_crate.to_path_buf()
    });
    let core = format!("{:?}", core_crate);
    let manifest = format!(
        r#"[package]
name = "polypack-engine-rt"
version = "0.1.0"
edition = "2024"
publish = false

[workspace]

[lib]
crate-type = ["staticlib"]

[dependencies]
poly-core = {{ path = {core}, default-features = false }}
# x86（i686 / VxWorks）不能让 blake3 走 C/汇编实现：cc -m32 需要宿主 32 位 libc 头，
# CI 容器常见缺席；`pure` 经特性统一作用到 poly-cache 的同一 blake3 条目，零新版本。
blake3 = {{ version = "1", default-features = false, features = ["pure"] }}

[profile.release]
panic = "abort"
opt-level = "z"
lto = false
codegen-units = 1
strip = "debuginfo"
"#,
        core = core
    );
    // 每次打包从干净 staging 重建：RUSTFLAGS / remap 变化不参与 cargo 指纹，
    // 复用旧 target 会把上一轮带绝对路径的归档当成新鲜产物。
    let _ = std::fs::remove_dir_all(build.staging);
    std::fs::create_dir_all(build.staging.join("src")).map_err(io_err(build.staging))?;
    std::fs::write(build.staging.join("Cargo.toml"), manifest)
        .map_err(io_err(build.staging.join("Cargo.toml")))?;
    std::fs::write(build.staging.join("src").join("lib.rs"), build.wrapper_rs)
        .map_err(io_err(build.staging.join("src/lib.rs")))?;

    let sysroot = sysroot(build.nightly)?;
    let mut flags: Vec<String> = Vec::new();
    if build.static_reloc {
        flags.push("-C".to_string());
        flags.push("relocation-model=static".to_string());
    }
    for (from, to) in remaps(&core_crate, build.staging, Path::new(&sysroot)) {
        flags.push(format!("--remap-path-prefix={from}={to}"));
    }
    let mut cargo = Command::new("cargo", build_args(build.target, build.nightly));
    cargo = cargo
        .in_dir(build.staging)
        .with_env("CARGO_TARGET_DIR", build.staging.join("target").display().to_string())
        .with_env("CARGO_ENCODED_RUSTFLAGS", flags.join("\u{1f}"));
    exec::run(&cargo)?;

    let archive = build
        .staging
        .join("target")
        .join(build.target)
        .join("release")
        .join("libpolypack_engine_rt.a");
    if !archive.is_file() {
        return Err(BundleError::ToolFailed {
            command: cargo.line(),
            status: "missing-artifact".to_string(),
            stderr: format!("cargo 未产出 {}", archive.display()),
            hint: Some(format!(
                "核对 Rust triple {}（rustup target add / nightly rust-src）；\
                 引擎层必须能以 --no-default-features 构建",
                build.target
            )),
        });
    }
    Ok(archive)
}

/// `nm` 可执行（缺失即 [`BundleError::ToolMissing`]）。
pub fn nm() -> Result<String, BundleError> {
    exec::tool_path("nm")
        .map(|path| path.display().to_string())
        .ok_or_else(|| BundleError::ToolMissing {
            packager: "embedded",
            tool: "nm".to_string(),
            hint: "安装 binutils（nm 随 binutils / llvm 提供）——C ABI 与未解析符号断言所需"
                .to_string(),
        })
}

fn build_args(target: &str, nightly: bool) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    if nightly {
        args.push("+nightly".to_string());
        args.push("build".to_string());
        args.push("-Z".to_string());
        args.push("build-std=core,alloc".to_string());
    } else {
        args.push("build".to_string());
    }
    for arg in ["--release", "--lib", "--target", target] {
        args.push(arg.to_string());
    }
    args
}

fn sysroot(nightly: bool) -> Result<String, BundleError> {
    let args: Vec<&str> = if nightly {
        vec!["+nightly", "--print", "sysroot"]
    } else {
        vec!["--print", "sysroot"]
    };
    let command = Command::new("rustc", args);
    let output = exec::run(&command)?;
    let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sysroot.is_empty() {
        return Err(BundleError::ToolFailed {
            command: command.line(),
            status: "empty-sysroot".to_string(),
            stderr: "rustc --print sysroot 返回空".to_string(),
            hint: None,
        });
    }
    Ok(sysroot)
}

fn remaps(core_crate: &Path, staging: &Path, sysroot: &Path) -> Vec<(String, String)> {
    let mut pairs = vec![
        (sysroot.display().to_string(), "toolchain".to_string()),
        (core_crate.display().to_string(), "polypack-core".to_string()),
        (staging.display().to_string(), "template".to_string()),
    ];
    if let Some(parent) = core_crate.parent() {
        pairs.push((parent.display().to_string(), "crates".to_string()));
    }
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")));
    if let Some(registry) = cargo_home.map(|home| home.join("registry"))
        && registry.is_dir()
    {
        pairs.push((registry.display().to_string(), "registry".to_string()));
    }
    pairs
}

enum SymbolKind {
    Defined,
    Undefined,
}

fn parse_symbols(stdout: &[u8], kind: SymbolKind) -> Vec<String> {
    let text = String::from_utf8_lossy(stdout);
    let mut symbols = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let symbol = match kind {
            // `nm -A --defined-only`：`<file>:<addr> T name`（局部符号类型也可能是小写）。
            SymbolKind::Defined => fields
                .iter()
                .position(|field| field.len() == 1 && "TtDdBbRrWwVvNn".contains(*field))
                .and_then(|index| fields.get(index + 1)),
            // `nm -A -u`：`<file>: U name`。
            SymbolKind::Undefined => fields
                .iter()
                .position(|field| *field == "U")
                .and_then(|index| fields.get(index + 1)),
        };
        if let Some(symbol) = symbol {
            symbols.push((*symbol).to_string());
        }
    }
    symbols.sort();
    symbols.dedup();
    symbols
}

fn path(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_abi_parser_reads_the_generated_header() {
        let header = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../poly-core/include/polypack_engine.h"),
        )
        .expect("T-M6-02 生成头文件");
        let symbols = parse_c_abi(&header);
        assert_eq!(
            symbols,
            vec![
                "poly_engine_abi_version",
                "poly_engine_cache_key",
                "poly_engine_string_free",
                "poly_engine_validate_job_json",
                "poly_engine_validate_target_json",
                "poly_engine_version",
            ]
        );
    }

    #[test]
    fn absolute_path_scan_flags_embedded_build_paths() {
        let dir = std::env::temp_dir().join(format!("poly-abs-{}", std::process::id()));
        let file = dir.join("sample.bin");
        std::fs::create_dir_all(&dir).expect("tmp");
        std::fs::write(&file, b"ok\0/home/build/secret\0tail".as_slice()).expect("write");
        let hits = absolute_paths(&file).expect("scan");
        assert!(hits.iter().any(|hit| hit.contains("/home/")), "{hits:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn absolute_path_scan_ignores_relative_path_segments() {
        let dir = std::env::temp_dir().join(format!("poly-rel-{}", std::process::id()));
        let file = dir.join("sample.bin");
        std::fs::create_dir_all(&dir).expect("tmp");
        std::fs::write(
            &file,
            b"registry/src/index.crates.io/serde/src/private/doc.rs\0var/log\0".as_slice(),
        )
        .expect("write");
        let hits = absolute_paths(&file).expect("scan");
        assert!(hits.is_empty(), "{hits:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
