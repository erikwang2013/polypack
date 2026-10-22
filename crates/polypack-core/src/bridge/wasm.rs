//! 小程序沙箱的 wasm 出口（T-M3-07）：UDL → 白名单 → shim → wasm32 编译 → wasm-bindgen
//! → 适配层与清单（随代码包分发）。
//!
//! ## 链路
//!
//! ```text
//! <core>/src/*.udl（单处接口定义，与其它出口同源）
//!   → 白名单检查：参数 / 返回类型必须在沙箱可用子集内（越出即 polypack::E_BRIDGE_0004，零产物）
//!   → 生成 shim crate（<root>/.polypack/wasm-shim/<lib>_wasm/：`#[wasm_bindgen]` 包住 UDL 函数）
//!   → cargo build --release --target wasm32-unknown-unknown（CARGO_TARGET_DIR 隔离在 shim 内）
//!   → wasm-bindgen CLI --target no-modules（官方发行版，定位见 [`WasmBindgen`]）
//!   → wasm-opt -Oz 裁剪（binaryen，定位见 [`WasmOpt`]；T-M3-08 的体积裁剪时机在
//!     **出口目录生成后、随包拷贝前**——随包条目即优化后字节）
//!   → <root>/bridge/wasm-bindgen/：
//!       <lib>_wasm.js（胶水；尾附 CJS 出口）· <lib>_wasm_bg.wasm
//!       polypack-loader.js（加载/初始化适配层）· manifest.json（白名单清单 + 体积元数据）
//! ```
//!
//! ## API 面白名单（两道闸，验收对应两处负例）
//!
//! 1. **生成期**：UDL 每个函数的参数与返回类型必须在 [`SANDBOX_TYPES`]（`string` 与数值标量；
//!    `u64`/`i64` 要 BigInt、`sequence`/对象要宿主对象搬运——小程序沙箱不保证）内。违规即
//!    `polypack::E_BRIDGE_0004` 并在报文里点名函数与类型——**绝不静默裁剪**（裁剪会把错误推迟到
//!    运行期才暴露）。
//! 2. **加载期**：适配层对照清单核对胶水暴露的函数面——多出的函数拒绝（白名单外接口）、
//!    缺失的白名单函数也拒绝；同时核对 wasm 的导入模块只来自胶水自身（产物无原生模块依赖）。
//!
//! ## 工具（skip-if-missing）
//!
//! wasm-bindgen CLI 缺失 → `polypack::E_BRIDGE_0001` + 修复提示（含官方发行包落缓存目录的指引），
//! 绝不产假产物。CLI 版本必须与 shim 钉住的 crate 版本完全一致（CLI 与 crate 错配的原始报错
//! 很难定位，故生成期显式核对）。
//!
//! ## 体积（T-M3-08，R10 对策）
//!
//! wasm-opt（binaryen）缺失或裁剪失败**不阻断**：告警放行、`size.wasm_opt.applied=false`
//! 如实记进清单（绝不静默——静默会让「没裁剪」伪装成「裁剪无收益」）。前后字节对比写进
//! 清单的 `size` 段（产物元数据），并随 `polypack bridge wasm` 报告与构建事件流输出。
//!
//! shortcut: shim 只覆盖顶层函数（与 [`udl`] 的「浅解析」同层）；`dictionary` / `enum` /
//! `interface` / `callback` 类型在 M3 白名单里就没有放行，需要时先扩解析器与映射表。

use std::path::{Path, PathBuf};
use std::process::Command;

use polypack_cache::Digest;
use serde::Serialize;

use super::udl;
pub use super::wasm_opt::{SizeInfo, WasmOpt, WasmOptInfo};
use crate::diagnostics::{Diagnostic, codes};
use crate::events::{BuildEvent, EventSink};
use crate::plan::{Project, StageError};

/// wasm 编译目标三元组（rustup target）。
pub const WASM_TRIPLE: &str = "wasm32-unknown-unknown";

/// shim 钉住的 wasm-bindgen crate 版本：CLI 必须完全一致（官方要求），故一并钉死。
pub const WASM_BINDGEN_VERSION: &str = "0.2.129";

/// 沙箱清单（`manifest.json`）的版本串；字段演进时递增。
pub const SANDBOX_SCHEMA: &str = "wasm-sandbox/v1";

/// 加载适配层的随包文件名。
pub const LOADER_FILE: &str = "polypack-loader.js";

/// 白名单清单的随包文件名。
pub const MANIFEST_FILE: &str = "manifest.json";

/// 沙箱可用的**标量**类型白名单（数字与布尔；`string` / `void` 由 [`sandbox_type`] 另判）。
///
/// 布尔收两种拼写：uniffi UDL 的 `boolean`（真 UDL 拼写）与手写 UDL 可能出现的 `bool`。
/// 刻意不放行：`u64` / `i64`（wasm-bindgen 映射 BigInt，小程序基础库不保证支持）、
/// `sequence` / `record` / `enum` / `interface`（需宿主对象搬运或反射，沙箱不保证）。
pub const SANDBOX_TYPES: [&str; 10] = [
    "bool", "boolean", "u8", "u16", "u32", "i8", "i16", "i32", "f32", "f64",
];

/// 加载适配层模板（占位符 [`MANIFEST_PLACEHOLDER`] 被清单 JSON 替换）。
const LOADER_TEMPLATE: &str = include_str!("wasm_loader.js");

/// 适配层模板里清单 JSON 的占位符。
const MANIFEST_PLACEHOLDER: &str = "/*__POLYPACK_MANIFEST__*/";

/// 胶水尾附的 CJS 出口：wasm-bindgen 的 no-modules 形态把对象赋给模块级 `let`，
/// 小程序模块系统（require）拿不到它——附加一行导出，产物字节随之确定。
const GLUE_TAIL: &str = "\n// polypack: 小程序模块系统出口（polypack bridge wasm 追加，勿手改）\n\
    if (typeof module !== \"undefined\" && module.exports) { module.exports = wasm_bindgen; }\n";

// ---------------------------------------------------------------- 白名单（生成期闸）

/// 该类型是否在沙箱可用子集内。
fn sandbox_type(ty: &str) -> bool {
    ty == "string" || ty == "void" || SANDBOX_TYPES.contains(&ty)
}

/// 生成期白名单检查：任一函数的参数 / 返回类型越出沙箱子集即 `polypack::E_BRIDGE_0004`。
pub fn check_whitelist(interface: &udl::Interface) -> Result<(), StageError> {
    for function in &interface.functions {
        for (ty, arg) in function.arg_types.iter().zip(&function.args) {
            if !sandbox_type(ty) {
                return Err(rejected(&function.name, &format!("参数 {arg}"), ty));
            }
        }
        if !sandbox_type(&function.ret) {
            return Err(rejected(&function.name, "返回值", &function.ret));
        }
    }
    Ok(())
}

/// 白名单拒绝诊断：点名函数、位置与类型 + 沙箱子集清单（可执行的下一步）。
fn rejected(function: &str, position: &str, ty: &str) -> StageError {
    Diagnostic::error(
        codes::E_BRIDGE_0004,
        format!("wasm 沙箱白名单拒绝：{function} 的{position}类型 {ty} 不在沙箱可用子集内"),
    )
    .with_hint(format!(
        "沙箱可用：{} / string / void；需要其它类型时把接口收窄成上述类型（如把 u64 拆成两个 u32），\
         或该接口改走 native 出口（uniffi 桥）",
        SANDBOX_TYPES.join(" / ")
    ))
    .into()
}

// ---------------------------------------------------------------- 生成物（shim / 清单 / 适配层）

/// shim 的 UDL → Rust 类型映射（白名单已先行把关，这里的其它取值不会出现）。
fn rust_type(ty: &str) -> &str {
    match ty {
        "string" => "String",
        "boolean" => "bool",
        other => other,
    }
}

/// shim crate 的 `src/lib.rs`：每个白名单函数一个 `#[wasm_bindgen]` 包装，转调核心 crate。
///
/// 生成物不含任何运行期逻辑——白名单检查在生成前完成，产物只是转调。
pub fn shim_lib_rs(interface: &udl::Interface, core_crate: &str) -> String {
    let mut out = String::from(
        "//! 由 polypack bridge wasm 生成（T-M3-07）：UDL 白名单函数 → wasm-bindgen 包装。勿手改。\n\n\
         use wasm_bindgen::prelude::*;\n\n",
    );
    for function in &interface.functions {
        let ret = if function.ret == "void" {
            String::new()
        } else {
            format!(" -> {}", rust_type(&function.ret))
        };
        let args: Vec<String> = function
            .args
            .iter()
            .zip(&function.arg_types)
            .map(|(name, ty)| format!("{name}: {}", rust_type(ty)))
            .collect();
        out.push_str(&format!(
            "#[wasm_bindgen]\npub fn {}({}){ret} {{\n    {core_crate}::{}({})\n}}\n\n",
            function.name,
            args.join(", "),
            function.name,
            function.args.join(", ")
        ));
    }
    out
}

/// shim crate 的 `Cargo.toml`：路径依赖核心 crate（绝对路径，`/` 分隔——Windows 反斜杠在
/// TOML 字符串里是转义），wasm-bindgen 版本钉死。`[workspace]` 空表让它自成 workspace。
pub fn shim_manifest_toml(core_dir: &Path, package: &str, core_package: &str) -> String {
    let core = core_dir.to_string_lossy().replace('\\', "/");
    format!(
        "# 由 polypack bridge wasm 生成（T-M3-07）：wasm-bindgen 包装 crate。勿手改。\n\
         [package]\n\
         name = \"{package}\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         publish = false\n\
         \n\
         [lib]\n\
         crate-type = [\"cdylib\"]\n\
         \n\
         [dependencies]\n\
         {core_package} = {{ path = \"{core}\" }}\n\
         wasm-bindgen = \"={WASM_BINDGEN_VERSION}\"\n\
         \n\
         [workspace]\n"
    )
}

/// 沙箱清单（`manifest.json`）：白名单函数面 + 加载期核对所需的模块名 + 体积元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Manifest {
    /// 清单自身的版本（[`SANDBOX_SCHEMA`]）。
    pub schema: String,
    /// 接口版本（[`super::BRIDGE_SCHEMA`]——与其它出口同一份接口定义）。
    pub bridge: String,
    /// UDL namespace。
    pub namespace: String,
    /// wasm-bindgen 的 out-name（`<核心库名>_wasm`）。
    pub out_name: String,
    /// 胶水文件（require 路径）。
    pub glue: String,
    /// wasm 文件（落点相对路径）。
    pub wasm: String,
    /// 胶水自身模块名——wasm 导入只允许来自它（无原生模块依赖的判据）。
    pub import_module: String,
    /// 生成引擎（`wasm-bindgen@<版本>`；进缓存键与报告）。
    pub engine: String,
    /// 白名单函数面（UDL 声明序）。
    pub functions: Vec<ManifestFunction>,
    /// 体积元数据（T-M3-08；缓存命中路径经 [`read_size`] 回读）。
    pub size: SizeInfo,
}

/// 清单里的一个函数（名 / 参数 / 返回）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestFunction {
    /// 函数名（UDL 与核心 crate 同名）。
    pub name: String,
    /// 参数（有序）。
    pub args: Vec<ManifestArg>,
    /// 返回类型（`void` 表示无返回）。
    pub ret: String,
}

/// 清单里的一个参数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestArg {
    /// 参数名。
    pub name: String,
    /// 参数类型（UDL 原文）。
    #[serde(rename = "type")]
    pub ty: String,
}

/// 组装清单（确定性字段序；函数按 UDL 声明序）。
pub fn manifest_for(
    interface: &udl::Interface,
    out_name: &str,
    engine: &str,
    size: SizeInfo,
) -> Manifest {
    Manifest {
        schema: SANDBOX_SCHEMA.to_string(),
        bridge: super::BRIDGE_SCHEMA.to_string(),
        namespace: interface.namespace.clone(),
        out_name: out_name.to_string(),
        glue: format!("./{out_name}.js"),
        wasm: format!("./{out_name}_bg.wasm"),
        import_module: format!("./{out_name}_bg.js"),
        engine: engine.to_string(),
        size,
        functions: interface
            .functions
            .iter()
            .map(|function| ManifestFunction {
                name: function.name.clone(),
                args: function
                    .args
                    .iter()
                    .zip(&function.arg_types)
                    .map(|(name, ty)| ManifestArg {
                        name: name.clone(),
                        ty: ty.clone(),
                    })
                    .collect(),
                ret: function.ret.clone(),
            })
            .collect(),
    }
}

/// 清单 JSON（pretty：人读产物；字段序确定，二跑字节一致）。
pub fn manifest_json(manifest: &Manifest) -> String {
    serde_json::to_string_pretty(manifest).expect("清单可序列化") + "\n"
}

/// 生成加载适配层：模板 + 清单 JSON（紧凑——嵌进 JS 常量）。
pub fn loader_js(manifest: &Manifest) -> String {
    let json = serde_json::to_string(manifest).expect("清单可序列化");
    LOADER_TEMPLATE.replace(MANIFEST_PLACEHOLDER, &json)
}

// ---------------------------------------------------------------- wasm-bindgen CLI（定位与调用）

/// wasm-bindgen CLI 的定位结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmBindgen {
    /// 可执行文件。
    pub bin: PathBuf,
}

impl WasmBindgen {
    /// 定位顺序：`$POLYPACK_WASM_BINDGEN` → `PATH` → 工具缓存
    /// `~/.cache/polypack/tools/wasm-bindgen/`（官方发行包解包形态，版本目录 / 解包目录两种落法都认）。
    pub fn discover() -> Option<Self> {
        if let Some(path) = std::env::var_os("POLYPACK_WASM_BINDGEN")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
        {
            return Some(Self { bin: path });
        }
        if let Some(path) = bindgen_on_path() {
            return Some(Self { bin: path });
        }
        cached_bindgen().map(|bin| Self { bin })
    }

    /// 实际版本号（跑 `--version` 取 `wasm-bindgen x.y.z` 的第三段）。
    pub fn version(&self) -> Result<String, StageError> {
        let output = Command::new(&self.bin)
            .arg("--version")
            .output()
            .map_err(|err| {
                Diagnostic::error(
                    codes::E_BRIDGE_0001,
                    format!("无法执行 wasm-bindgen CLI（{}）: {err}", self.bin.display()),
                )
                .with_hint(wasm_bindgen_hint())
            })?;
        if !output.status.success() {
            return Err(Diagnostic::error(
                codes::E_BRIDGE_0001,
                format!(
                    "wasm-bindgen --version 退出非零（{}）: {}",
                    self.bin.display(),
                    stderr_tail(&output.stderr)
                ),
            )
            .with_hint(wasm_bindgen_hint())
            .into());
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let version = text
            .split_whitespace()
            .last()
            .unwrap_or_default()
            .to_string();
        if version.is_empty() {
            return Err(Diagnostic::error(
                codes::E_BRIDGE_0001,
                format!("wasm-bindgen --version 输出不可解析: {text}"),
            )
            .with_hint(wasm_bindgen_hint())
            .into());
        }
        Ok(version)
    }

    /// 版本核对：CLI 与 shim 钉住的 crate 版本必须完全一致（不一致的原始报错很难定位）。
    pub fn check_version(&self) -> Result<String, StageError> {
        let version = self.version()?;
        if version != WASM_BINDGEN_VERSION {
            return Err(Diagnostic::error(
                codes::E_BRIDGE_0001,
                format!(
                    "wasm-bindgen CLI 版本 {version} ≠ 工具链钉住的 {WASM_BINDGEN_VERSION}（{}）",
                    self.bin.display()
                ),
            )
            .with_hint(format!(
                "换用与 crate 同版本的 CLI（官方发行包 wasm-bindgen-{WASM_BINDGEN_VERSION}-<宿主>.tar.gz，\
                 解到 ~/.cache/polypack/tools/wasm-bindgen/），或设 $POLYPACK_WASM_BINDGEN 指向它"
            ))
            .into());
        }
        Ok(version)
    }

    /// 由编译好的 wasm 生成胶水：`--target no-modules`（单文件、全局对象形态——小程序模块系统
    /// 可直接 require），`--no-typescript`（小程序侧不用 .d.ts，产出面最小）。
    pub fn generate(&self, wasm: &Path, out_dir: &Path, out_name: &str) -> Result<(), StageError> {
        let output = Command::new(&self.bin)
            .arg("--target")
            .arg("no-modules")
            .arg("--no-typescript")
            .arg("--out-dir")
            .arg(out_dir)
            .arg("--out-name")
            .arg(out_name)
            .arg(wasm)
            .output()
            .map_err(|err| {
                Diagnostic::error(
                    codes::E_BRIDGE_0001,
                    format!("无法启动 wasm-bindgen CLI（{}）: {err}", self.bin.display()),
                )
                .with_hint(wasm_bindgen_hint())
            })?;
        if !output.status.success() {
            return Err(Diagnostic::error(
                codes::E_BRIDGE_0001,
                format!(
                    "wasm-bindgen 生成胶水失败（{}）: {}",
                    wasm.display(),
                    stderr_tail(&output.stderr)
                ),
            )
            .with_hint("核对 CLI 版本与 crate 是否一致；shim 需先真编译出 .wasm（见本阶段日志）")
            .into());
        }
        Ok(())
    }
}

/// `$POLYPACK_WASM_BINDGEN` 之外的 PATH 查找（Windows 认 `.exe`）。
fn bindgen_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let candidates = ["wasm-bindgen", "wasm-bindgen.exe"];
    for dir in std::env::split_paths(&path) {
        for candidate in candidates {
            let full = dir.join(candidate);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

/// 工具缓存根：`~/.cache/polypack/tools/<tool>`（与 kotlinc / bundletool 的约定同源）。
pub(super) fn tool_cache_dir(tool: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    Some(home.join(".cache/polypack/tools").join(tool))
}

/// 缓存里找 CLI：官方发行包解包后形态不一（`<版本>/<发行目录>/wasm-bindgen` 或
/// `<发行目录>/wasm-bindgen` 或直接 `<wasm-bindgen>`），下探 ≤2 层收集候选，
/// 优先路径含钉住版本的，其次取字典序最大（与 kotlinc 的 jna 取法同风格）。
fn cached_bindgen() -> Option<PathBuf> {
    let root = tool_cache_dir("wasm-bindgen")?;
    let mut candidates = Vec::new();
    let mut stack = vec![(root.clone(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 2 {
            continue;
        }
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push((path, depth + 1));
            } else if path
                .file_name()
                .is_some_and(|name| name == "wasm-bindgen" || name == "wasm-bindgen.exe")
            {
                candidates.push(path);
            }
        }
    }
    candidates.sort();
    candidates
        .iter()
        .rev()
        .find(|path| path.to_string_lossy().contains(WASM_BINDGEN_VERSION))
        .or_else(|| candidates.last())
        .cloned()
}

/// CLI 缺失 / 失败的修复提示（官方发行包落缓存目录，无 npm / cargo install 的供应链面）。
fn wasm_bindgen_hint() -> String {
    format!(
        "取官方发行包：wasm-bindgen-{WASM_BINDGEN_VERSION}-<宿主三元组>.tar.gz\
         （github.com/wasm-bindgen/wasm-bindgen/releases），解到 ~/.cache/polypack/tools/wasm-bindgen/；\
         或设 $POLYPACK_WASM_BINDGEN 指向 CLI 可执行文件"
    )
}

// ---------------------------------------------------------------- 全流程

/// 一次 wasm 出口的产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmProduct {
    /// 随代码包分发的产物目录（`<root>/bridge/wasm-bindgen`）。
    pub dir: PathBuf,
    /// 目录内文件（相对路径，升序）。
    pub files: Vec<PathBuf>,
    /// 白名单清单（与写盘的 `manifest.json` 同值）。
    pub manifest: Manifest,
}

/// 跑完整条链路（产物落 `out_dir`，缺省取 `bridge/wasm-bindgen` 由调用方传入）；
/// 任一环节失败都以稳定诊断中止，不留半份产物。
pub fn generate(
    project: &Project,
    out_dir: &Path,
    events: &mut dyn EventSink,
) -> Result<WasmProduct, StageError> {
    let source = super::interface_source(project)?;
    let interface = udl::parse_source(source.primary())?;
    // 白名单先行：越出的接口在这里就被拒绝（编译期闸），不让它进 shim / 工具链
    check_whitelist(&interface)?;
    let core_dir = &source.crate_dir;
    let package = crate::compile::read_crate_name(core_dir)?;
    let core_crate = package.replace('-', "_");
    check_core_crate_type(core_dir, &package)?;
    let bindgen = WasmBindgen::discover().ok_or_else(missing_tool)?;
    let engine = format!("wasm-bindgen@{}", bindgen.check_version()?);

    // shim 落 .polypack（构建中间物不混进随代码包分发的产物树；target/ 也在其内）
    let out_name = format!("{core_crate}_wasm");
    let shim_dir = project
        .root
        .join(".polypack")
        .join("wasm-shim")
        .join(&out_name);
    write_shim(&shim_dir, core_dir, &package, &out_name, &interface)?;
    events.emit(&BuildEvent::Warning {
        stage: "bridge".to_string(),
        message: format!(
            "wasm shim 已生成（{}），编译到 {WASM_TRIPLE} …",
            shim_dir.display()
        ),
    });
    let wasm = build_shim(&shim_dir, &out_name, events)?;

    clear_dir(out_dir)?;
    bindgen.generate(&wasm, out_dir, &out_name)?;
    // 裁剪时机（T-M3-08 接口契约）：出口目录生成后、随包拷贝前——随包条目即优化后字节
    let size =
        super::wasm_opt::optimize_wasm(&out_dir.join(format!("{out_name}_bg.wasm")), events)?;
    append_glue_tail(&out_dir.join(format!("{out_name}.js")))?;

    let manifest = manifest_for(&interface, &out_name, &engine, size);
    std::fs::write(out_dir.join(MANIFEST_FILE), manifest_json(&manifest))
        .map_err(|err| io_error(&out_dir.join(MANIFEST_FILE), err))?;
    std::fs::write(out_dir.join(LOADER_FILE), loader_js(&manifest))
        .map_err(|err| io_error(&out_dir.join(LOADER_FILE), err))?;

    let files = list_files(out_dir)?;
    events.emit(&BuildEvent::Warning {
        stage: "bridge".to_string(),
        message: format!(
            "wasm 出口产物 {} 个文件 → {}（{SANDBOX_SCHEMA}，白名单 {} 个函数）",
            files.len(),
            out_dir.display(),
            manifest.functions.len()
        ),
    });
    Ok(WasmProduct {
        dir: out_dir.to_path_buf(),
        files,
        manifest,
    })
}

/// 写 shim 的两个源文件（幂等：内容确定，同输入同字节）。
fn write_shim(
    shim_dir: &Path,
    core_dir: &Path,
    core_package: &str,
    out_name: &str,
    interface: &udl::Interface,
) -> Result<(), StageError> {
    let src = shim_dir.join("src");
    std::fs::create_dir_all(&src).map_err(|err| io_error(&src, err))?;
    let toml = shim_manifest_toml(core_dir, out_name, core_package);
    // 包名里的 `-` 在 Rust 里以 `_` 引用（shim 无连字符，这里显式断言生成前提）
    let lib_rs = shim_lib_rs(interface, &core_package.replace('-', "_"));
    std::fs::write(shim_dir.join("Cargo.toml"), toml)
        .map_err(|err| io_error(&shim_dir.join("Cargo.toml"), err))?;
    std::fs::write(src.join("lib.rs"), lib_rs).map_err(|err| io_error(&src.join("lib.rs"), err))
}

/// shim 的 wasm 编译（`cargo build --release --target wasm32-unknown-unknown --lib`）。
///
/// `CARGO_TARGET_DIR` 钉在 shim 目录内：不污染核心 crate 的 `target/`，多 agent / 多工程
/// 并行时也不互相抢锁。
fn build_shim(
    shim_dir: &Path,
    out_name: &str,
    events: &mut dyn EventSink,
) -> Result<PathBuf, StageError> {
    let output = Command::new("cargo")
        .args(["build", "--release", "--lib", "--target", WASM_TRIPLE])
        .current_dir(shim_dir)
        .env("CARGO_TARGET_DIR", shim_dir.join("target"))
        .output()
        .map_err(|err| {
            Diagnostic::error(
                codes::E_BRIDGE_0003,
                format!("无法执行 cargo（wasm shim 编译）: {err}"),
            )
            .with_hint("确认 cargo 在 PATH 中；shim 目录可写")
        })?;
    if !output.status.success() {
        return Err(Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!(
                "wasm shim 编译失败（{WASM_TRIPLE}）: {}",
                stderr_tail(&output.stderr)
            ),
        )
        .with_hint(format!(
            "①`rustup target add {WASM_TRIPLE}`；②核心 crate 的 [lib] crate-type 需含 \"lib\"/\"rlib\"；\
             ③UDL 函数名需与核心 crate 的 pub fn 同名同参（shim 直接转调）"
        ))
        .into());
    }
    let wasm = shim_dir
        .join("target")
        .join(WASM_TRIPLE)
        .join("release")
        .join(format!("{out_name}.wasm"));
    if !wasm.is_file() {
        return Err(Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!("shim 编译成功但未找到 {}", wasm.display()),
        )
        .with_hint("核对 shim 的 [lib] crate-type 含 \"cdylib\"（wasm 载体）")
        .into());
    }
    events.emit(&BuildEvent::Compiled {
        crate_name: format!("{out_name}.wasm"),
        triple: WASM_TRIPLE.to_string(),
    });
    Ok(wasm)
}

/// 核心 crate 必须能被 shim 链接：`[lib] crate-type` 缺 `lib`/`rlib` 时 shim 编译会以
/// 「unlinked crate」报错（原始报错不提 crate-type，故这里前置说清）。
fn check_core_crate_type(core_dir: &Path, package: &str) -> Result<(), StageError> {
    let manifest = core_dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|err| io_error(&manifest, err))?;
    let value: toml::Value = toml::from_str(&text).map_err(|err| {
        Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!(
                "核心 crate 的 Cargo.toml 解析失败（{}）: {err}",
                manifest.display()
            ),
        )
        .with_hint("核对 [lib] 段语法；wasm shim 需要它的 crate-type 与路径依赖")
    })?;
    let crate_types: Vec<&str> = value
        .get("lib")
        .and_then(|lib| lib.get("crate-type"))
        .and_then(|types| types.as_array())
        .map(|types| types.iter().filter_map(toml::Value::as_str).collect())
        .unwrap_or_else(|| vec!["lib"]);
    if crate_types
        .iter()
        .any(|kind| matches!(*kind, "lib" | "rlib" | "dylib"))
    {
        return Ok(());
    }
    Err(Diagnostic::error(
        codes::E_BRIDGE_0003,
        format!(
            "核心 crate {package} 的 [lib] crate-type 缺 \"lib\"/\"rlib\"（当前 {crate_types:?}），\
             wasm shim 无法链接它"
        ),
    )
    .with_hint(
        "在 <core>/Cargo.toml 的 [lib] crate-type 里加上 \"lib\"（如 [\"cdylib\", \"staticlib\", \"lib\"]）——\
         cdylib 只服务 uniffi 桥，wasm 出口需要 rlib 形态的核心库",
    )
    .into())
}

/// 工具缺失 → `polypack::E_BRIDGE_0001` + 修复提示（skip-if-missing：不产假产物）。
fn missing_tool() -> StageError {
    Diagnostic::error(
        codes::E_BRIDGE_0001,
        "未找到 wasm-bindgen CLI（$POLYPACK_WASM_BINDGEN / PATH / ~/.cache/polypack/tools/wasm-bindgen 三处都没有）",
    )
    .with_hint(wasm_bindgen_hint())
    .into()
}

/// 给胶水尾附 CJS 出口（见 [`GLUE_TAIL`]）。
fn append_glue_tail(glue: &Path) -> Result<(), StageError> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(glue)
        .map_err(|err| io_error(glue, err))?;
    file.write_all(GLUE_TAIL.as_bytes())
        .map_err(|err| io_error(glue, err))
}

/// wasm 出口产物的库级缓存键：核心 crate 源树 + 锁文件 + 出口与 triple + **接口摘要** +
/// 引擎（CLI 版本）+ **裁剪工具指纹** + profile。
///
/// 裁剪工具进键（T-M3-08）：wasm-opt 版本（或缺失）变则产物字节变——缺它的机器不该拿到
/// 别机裁过的树，换版本也不该复用旧字节。
pub fn cache_key(
    project: &Project,
    interface: &Digest,
    engine: &str,
    profile: &str,
) -> Result<Digest, StageError> {
    let source = super::interface_source(project)?;
    let core_dir = &source.crate_dir;
    let core_crate = super::core_lib_name(&source)?;
    let sources = Digest::of_tree(core_dir, super::aar::SOURCE_EXCLUDE).map_err(|err| {
        Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!("核心 crate 源树摘要失败: {err}"),
        )
        .with_hint("确认 [core].crate 指向的目录存在且可读")
    })?;
    let lockfile = match std::fs::read(core_dir.join("Cargo.lock")) {
        Ok(bytes) => Digest::of_bytes(&bytes),
        Err(_) => Digest::of_bytes(b""),
    };
    Ok(polypack_cache::cache_key(&polypack_cache::CacheInputs {
        sources,
        lockfile,
        target_spec: format!("wasm-bindgen:{WASM_TRIPLE}:{core_crate}_wasm"),
        // 裁剪工具缺失时记 `wasm-opt:none`：键区分「裁过的树」与「没裁的树」
        toolchain: format!(
            "{}+{SANDBOX_SCHEMA}+{engine}+{interface}+{}",
            super::BRIDGE_SCHEMA,
            super::wasm_opt::WasmOpt::discover()
                .map(|tool| tool.fingerprint())
                .unwrap_or_else(|| "wasm-opt:none".to_string())
        ),
        profile: profile.to_string(),
        app_id: None,
    }))
}

/// 清空目录（不存在即建空目录）——上一跑的产物混进分发树会以「多一个文件」的方式坑消费方。
fn clear_dir(dir: &Path) -> Result<(), StageError> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(io_error(dir, err)),
    }
    std::fs::create_dir_all(dir).map_err(|err| io_error(dir, err))
}

/// 递归列出目录下普通文件的相对路径（升序）。
fn list_files(dir: &Path) -> Result<Vec<PathBuf>, StageError> {
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
                out.push(path.strip_prefix(dir).unwrap_or(&path).to_path_buf());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// stderr 尾部关键行（≤3 行）。
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
        format!("wasm 出口 IO 失败（{}）: {err}", path.display()),
    )
    .with_hint(
        "确认工程目录可写；shim 落 <root>/.polypack/wasm-shim/，产物落 <root>/bridge/wasm-bindgen/",
    )
    .into()
}
