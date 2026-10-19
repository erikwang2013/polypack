//! uniffi 桥生成器（T-M2-01）：**单处接口定义** → Kotlin / Swift 绑定，接口按 `bridge/v1` 版本化。
//!
//! ## 单处接口定义（single source of truth）
//!
//! 接口只写一份：核心 crate 的 UDL（`<core>/src/*.udl`，uniffi 的接口定义语言）。四端绑定由
//! 生成器统一产出，**不手写任何一份**：Kotlin / Swift 走 uniffi 引擎（[`BridgeEngine::Uniffi`]），
//! napi 走自带的 UDL→C 胶水出口（[`BridgeEngine::PolyNapi`]，T-M3-01：RNOH / ArkTS 宿主可装载的
//! 自包含 `.so`，见 [`napi`]）；wasm-bindgen（小程序沙箱，T-M3-07）是编译产物形态、走 [`wasm`]
//! （[`BridgeEngine::WasmBindgen`]）；cffi 已登记在 [`BridgeLang`] 上但标为 M3 消费点
//! （[`BridgeEngine::Reserved`]），M3 接入时只换引擎、接口定义不动。
//!
//! ## 引擎（谁真正生成绑定代码）
//!
//! 生成动作交给 uniffi 官方工具 [`engine::UniffiEngine`]，本模块只负责：定位接口定义、
//! 组织调用参数（`--language` / `--out-dir` / `--no-format`）、校验产物形态、算缓存键。
//! 引擎缺失时按既有 skip-if-missing 语义给出 `poly::E_BRIDGE_0001` + 修复提示，绝不静默产假产物。
//!
//! ## 缓存键（T-M2-01 验收：接口变更即触发库级缓存失效）
//!
//! [`bridge_cache_key`] 复用 §5.2 的五输入公式，并把**接口摘要**（[`interface_digest`]）
//! 并入 `toolchain` 位——对桥产物而言「uniffi 引擎版本 + `bridge/v1` 接口摘要」正是决定产物
//! 字节的工具链部分；接口改一个字，键即变。
//!
//! ## 落点
//!
//! ```text
//! <root>/bridge/kotlin/uniffi/<ns>/<ns>.kt      # Kotlin 绑定（uniffi 的包布局）
//! <root>/bridge/swift/<ns>.swift · <ns>FFI.h · <ns>FFI.modulemap   # Swift 绑定三件套
//! <root>/bridge/napi/<ns>.c                     # napi C 胶水（T-M3-01；模块 .so 见 [`napi`]）
//! <root>/bridge/android/<lib>.aar               # T-M2-02 产物（见 [`aar`]）
//! ```
//!
//! Swift 三件套的落点与 T-M2-03（XCFramework）的 `bridge_dir(out_dir) = out_dir/bridge/swift`
//! 约定一致：`out_dir` 取工程根即得本布局。

pub mod aar;
mod engine;
pub mod kotlinc;
pub mod napi;
pub mod udl;
pub mod wasm;
pub mod wasm_opt;

pub use engine::UniffiEngine;

use std::fmt;
use std::path::{Path, PathBuf};

use poly_cache::{Cache, CacheInputs, Digest, cache_key};

use crate::compile;
use crate::diagnostics::{Diagnostic, codes};
use crate::events::BuildEvent;
use crate::plan::{Project, StageError};

/// 接口与产物的版本串：接口定义的**版本化单位**。
///
/// 出现在每条生成的缓存键里，也随 `--json` 事件上报；接口语义有破坏性变更时递增
/// （`bridge/v1` → `bridge/v2`），旧产物自然失效、无需人工清缓存。
pub const BRIDGE_SCHEMA: &str = "bridge/v1";

/// 桥的目标语言（= 四端出口）。
///
/// Kotlin / Swift 由 uniffi 引擎真实产出；wasm-bindgen 由 T-M3-07 接入（[`wasm`]，编译产物
/// 形态）；napi / cffi 仍标为 M3 消费点，已在枚举与目录布局上登记，接入时只换引擎。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BridgeLang {
    /// Kotlin（Android / JVM）。
    Kotlin,
    /// Swift（iOS / macOS）。
    Swift,
    /// napi（C ABI：RNOH / ArkTS / Node），T-M3-01 接入。
    Napi,
    /// wasm-bindgen（小程序沙箱，T-M3-07 接入）。
    WasmBindgen,
    /// cffi（QNX / RTOS），M3 接入。
    Cffi,
}

impl BridgeLang {
    /// 全部出口（固定顺序，供 `--lang all` 与测试遍历）。
    pub const ALL: [Self; 5] = [
        Self::Kotlin,
        Self::Swift,
        Self::Napi,
        Self::WasmBindgen,
        Self::Cffi,
    ];

    /// 语言标识（命令行取值 / 事件流字段）。
    pub fn id(self) -> &'static str {
        match self {
            Self::Kotlin => "kotlin",
            Self::Swift => "swift",
            Self::Napi => "napi",
            Self::WasmBindgen => "wasm-bindgen",
            Self::Cffi => "cffi",
        }
    }

    /// 绑定落点子目录名（`<root>/bridge/<dir>`）。
    pub fn dir(self) -> &'static str {
        match self {
            Self::Kotlin => "kotlin",
            Self::Swift => "swift",
            Self::Napi => "napi",
            Self::WasmBindgen => "wasm-bindgen",
            Self::Cffi => "cffi",
        }
    }

    /// 该语言的生成引擎。
    pub fn engine(self) -> BridgeEngine {
        match self {
            Self::Kotlin | Self::Swift => BridgeEngine::Uniffi,
            Self::Napi => BridgeEngine::PolyNapi,
            Self::WasmBindgen => BridgeEngine::WasmBindgen,
            Self::Cffi => BridgeEngine::Reserved { since: "M3" },
        }
    }

    /// 从标识串解析（`--lang` 用）。
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|lang| lang.id() == text)
    }
}

impl fmt::Display for BridgeLang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// 生成引擎：uniffi（绑定形态）、poly 自带的 napi C 胶水（T-M3-01）与 wasm-bindgen
/// （编译产物形态，T-M3-07），其余出口是 M3 的登记位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeEngine {
    /// uniffi（Kotlin / Swift）：由 UDL 经引擎生成绑定源码。
    Uniffi,
    /// poly 自带的 UDL→C 胶水出口（napi，T-M3-01；见 [`napi`]）。
    PolyNapi,
    /// wasm-bindgen（小程序沙箱，T-M3-07）：编译产物形态（shim + cargo + 胶水 + 适配层），
    /// 不落 [`generate`] 的绑定目录，走 [`wasm::generate`]（CLI：`poly bridge wasm`）。
    WasmBindgen,
    /// 尚未接入的出口（M3 消费；调用即 `poly::E_BRIDGE_0002`）。
    Reserved {
        /// 计划接入的里程碑。
        since: &'static str,
    },
}

impl BridgeEngine {
    /// 引擎身份串（进桥缓存键的 toolchain 位；换引擎即失效）。
    pub fn id(self) -> &'static str {
        match self {
            Self::Uniffi => "uniffi",
            Self::PolyNapi => "poly-napi",
            Self::WasmBindgen => "wasm-bindgen",
            Self::Reserved { .. } => "reserved",
        }
    }
}

/// 单处接口定义：核心 crate 的 UDL 源文件（按相对路径升序，保证摘要确定）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceSource {
    /// 核心 crate 目录（UDL 所在处的上溯基准）。
    pub crate_dir: PathBuf,
    /// UDL 文件绝对路径（升序）。
    pub files: Vec<PathBuf>,
}

impl InterfaceSource {
    /// 主导 UDL（首个）——uniffi 的单文件入口；多文件时取排序首个，其余只参与摘要。
    pub fn primary(&self) -> &Path {
        &self.files[0]
    }
}

/// 一次生成的产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeOutput {
    /// 目标语言。
    pub lang: BridgeLang,
    /// 落点目录。
    pub dir: PathBuf,
    /// 生成的文件（升序；相对落点目录的路径）。
    pub files: Vec<PathBuf>,
}

/// 定位单处接口定义：核心 crate 的 `src/**/*.udl`。
///
/// 找不到任何 UDL → `poly::E_BRIDGE_0002` + 修复提示（接口定义是桥的前置，缺失不是「跳过」）。
pub fn interface_source(project: &Project) -> Result<InterfaceSource, StageError> {
    let crate_dir = project.root.join(&project.config.core.crate_path);
    let mut files = Vec::new();
    collect_udl(&crate_dir.join("src"), &mut files)?;
    files.sort();
    if files.is_empty() {
        return Err(Diagnostic::error(
            codes::E_BRIDGE_0002,
            format!(
                "核心 crate 未定义桥接口（{} 下没有 .udl）",
                crate_dir.join("src").display()
            ),
        )
        .with_hint(
            "在核心 crate 的 src/ 下写一份 UDL（uniffi 的接口定义语言），\
             例：namespace demo { string hello(string name); };",
        )
        .into());
    }
    Ok(InterfaceSource { crate_dir, files })
}

/// 接口摘要：`bridge/v1` 域前缀 ‖ 各 UDL 的相对路径与内容的长度前缀混入。
///
/// 接口改一个字（含改文件名）摘要即变，进而使 [`bridge_cache_key`] 变——这是
/// 「接口变更即触发库级缓存失效」的判据本身。
pub fn interface_digest(source: &InterfaceSource) -> Result<Digest, StageError> {
    let mut payload = String::from(BRIDGE_SCHEMA);
    for file in &source.files {
        let rel = file
            .strip_prefix(&source.crate_dir)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(file).map_err(|err| {
            Diagnostic::error(
                codes::E_BRIDGE_0002,
                format!("无法读取接口定义 {}: {err}", file.display()),
            )
            .with_hint("确认 UDL 文件存在且当前用户可读")
        })?;
        // 长度前缀：消除「路径尾 ‖ 内容首」的拼接歧义
        payload.push_str(&format!("\n{}:{rel}\n{}:{text}", rel.len(), text.len()));
    }
    Ok(Digest::of_bytes(payload.as_bytes()))
}

/// 桥产物的库级缓存键（§5.2 五输入公式 + 接口摘要），**逐语言**。
///
/// `toolchain` 位承载「生成引擎（[`BridgeEngine::id`]）‖ `bridge/v1` 接口摘要」：对桥产物
/// 而言它们就是决定产物字节的工具链，接口或引擎变更即失效。`sources` / `lockfile` 取核心
/// crate（含 UDL）。`target_spec` 位承载语言：各语言的绑定是独立产物（Kotlin 未变时 Swift
/// 可单独重生成）。
pub fn bridge_cache_key(
    project: &Project,
    interface: &Digest,
    profile: &str,
    lang: BridgeLang,
) -> Result<Digest, StageError> {
    let crate_dir = project.root.join(&project.config.core.crate_path);
    let sources = Digest::of_tree(&crate_dir, aar::SOURCE_EXCLUDE).map_err(cache_error)?;
    let lockfile = core_lockfile_digest(&crate_dir)?;
    Ok(cache_key(&CacheInputs {
        sources,
        lockfile,
        target_spec: format!("{BRIDGE_SCHEMA}/{}", lang.id()),
        toolchain: format!("{BRIDGE_SCHEMA}+{}+{interface}", lang.engine().id()),
        profile: profile.to_string(),
        app_id: None,
    }))
}

/// 逐语言的缓存键清单（[`restore_or_generate`] 的输入）。
pub fn bridge_cache_keys(
    project: &Project,
    interface: &Digest,
    profile: &str,
    langs: &[BridgeLang],
) -> Result<Vec<(BridgeLang, Digest)>, StageError> {
    langs
        .iter()
        .map(|lang| Ok((*lang, bridge_cache_key(project, interface, profile, *lang)?)))
        .collect()
}

/// 核心 crate 锁文件摘要；缺失按空串参与（与 M1 的 compile 支线同口径）。
fn core_lockfile_digest(crate_dir: &Path) -> Result<Digest, StageError> {
    match std::fs::read(crate_dir.join("Cargo.lock")) {
        Ok(bytes) => Ok(Digest::of_bytes(&bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Digest::of_bytes(b"")),
        Err(err) => Err(source_error(err)),
    }
}

/// 生成两端绑定：按 `langs` 顺序逐语言调用引擎，落点 `<root>/bridge/<dir>`。
///
/// 每个语言一条 [`BuildEvent::Cached`] 之外的进度事件（见 [`emit_generated`]）；引擎缺失或
/// 生成失败即 `poly::E_BRIDGE_0001`（含修复提示），不产半份产物。
pub fn generate(
    project: &Project,
    langs: &[BridgeLang],
    events: &mut dyn crate::events::EventSink,
) -> Result<Vec<BridgeOutput>, StageError> {
    let source = interface_source(project)?;
    // 先验语言再找引擎：出口未接入（或形态不同）是用户请求的问题，不该被「引擎不在场」掩盖
    for lang in langs {
        match lang.engine() {
            BridgeEngine::Reserved { since } => {
                return Err(Diagnostic::error(
                    codes::E_BRIDGE_0002,
                    format!("出口 {} 的生成器尚未接入（计划于 {since}）", lang.id()),
                )
                .with_hint(
                    "已接入：--lang kotlin / swift（uniffi 引擎）/ napi（UDL→C 胶水，M3 的 T-M3-01）；\
                     wasm 走 `poly bridge wasm`；cffi 由 M3 后续接入同一生成器",
                )
                .into());
            }
            // wasm 出口是编译产物形态（shim + cargo + 胶水 + 适配层），不落绑定目录：
            // 走 `poly bridge wasm`，别在绑定流水线里产出半份形态的产物
            BridgeEngine::WasmBindgen => {
                return Err(Diagnostic::error(
                    codes::E_BRIDGE_0002,
                    format!("出口 {} 是编译产物形态，不走绑定生成流水线", lang.id()),
                )
                .with_hint(
                    "小程序沙箱的 wasm 出口走 `poly bridge wasm`（产出胶水 + 适配层 + 白名单清单）",
                )
                .into());
            }
            BridgeEngine::Uniffi | BridgeEngine::PolyNapi => {}
        }
    }
    let root = project.root.join("bridge");
    // uniffi 引擎按需定位：只请求 napi 时不该要求 uniffi-bindgen 在场（反之亦然）；
    // 且「先探引擎再落盘」——缺引擎时连 uniffi.toml 都不写（缺引擎不产假产物的既有断言）
    let uniffi = if langs
        .iter()
        .any(|lang| lang.engine() == BridgeEngine::Uniffi)
    {
        let engine =
            engine::UniffiEngine::discover(&source.crate_dir).ok_or_else(missing_engine)?;
        // 库名与绑定名必须一致（不一致只在设备运行时炸），生成前先钉死
        let config = root.join("uniffi.toml");
        engine::write_config(&config, &namespace(&source)?, &core_lib_name(&source)?)?;
        Some((engine, config))
    } else {
        None
    };
    let mut outputs = Vec::new();
    for lang in langs {
        let output = match lang.engine() {
            BridgeEngine::Uniffi => {
                let (engine, config) = uniffi.as_ref().expect("uniffi 出口必先定位引擎");
                let dir = root.join(lang.dir());
                let files = engine.generate(&source, *lang, &dir, config)?;
                BridgeOutput {
                    lang: *lang,
                    dir,
                    files,
                }
            }
            BridgeEngine::PolyNapi => napi::generate(project, &source)?,
            // 保留出口与 wasm 的产物形态出口都在前置检查里被拒（wasm 走 `poly bridge wasm`）
            BridgeEngine::WasmBindgen | BridgeEngine::Reserved { .. } => {
                unreachable!("保留/产物形态出口已在前置检查拒绝")
            }
        };
        emit_generated(events, output.lang, &output.dir, &output.files);
        outputs.push(output);
    }
    Ok(outputs)
}

/// 引擎缺失 → `poly::E_BRIDGE_0001` + 修复提示（skip-if-missing：不产假绑定）。
fn missing_engine() -> StageError {
    Diagnostic::error(
        codes::E_BRIDGE_0001,
        "未找到 uniffi 生成引擎（$POLYPACK_UNIFFI_BINDGEN / PATH / 核心 crate 的 src/bin 三处都没有）",
    )
    .with_hint(
        "核心 crate 加 uniffi 依赖与 src/bin/uniffi-bindgen.rs（fn main() { uniffi::uniffi_bindgen_main() }），\
         或 cargo install uniffi --features cli --bin uniffi-bindgen",
    )
    .into()
}

/// 生成进度事件（人读 / JSON 两路共用同一字段）。
fn emit_generated(
    events: &mut dyn crate::events::EventSink,
    lang: BridgeLang,
    dir: &Path,
    files: &[PathBuf],
) {
    events.emit(&BuildEvent::Warning {
        stage: "bridge".to_string(),
        message: format!(
            "已生成 {lang} 绑定 {} 个文件 → {}（{BRIDGE_SCHEMA}）",
            files.len(),
            dir.display()
        ),
    });
}

/// 生成产物入库（库级缓存）：逐语言 `store_tree(<root>/bridge/<语言目录>)`，键见
/// [`bridge_cache_key`]。
///
/// 只缓存**绑定**目录：`bridge/android/` 的 AAR 有它自己的键（[`aar::cache_key`]），
/// 混进绑定树会让「改了 ABI 集合的第二次 AAR 构建」被旧字节顶掉。
/// 全部语言命中才返回 `true`（部分是异常态：说明缓存里只有半份，此时整体重生成）。
pub fn restore_or_generate(
    cache: &Cache,
    keys: &[(BridgeLang, Digest)],
    project: &Project,
    events: &mut dyn crate::events::EventSink,
) -> Result<bool, StageError> {
    let langs: Vec<BridgeLang> = keys.iter().map(|(lang, _)| *lang).collect();
    let root = project.root.join("bridge");
    let mut restored = 0usize;
    for (lang, key) in keys {
        if cache
            .restore_tree(key, &root.join(lang.dir()))
            .map_err(cache_error)?
        {
            restored += 1;
        }
    }
    if restored == keys.len() && !keys.is_empty() {
        events.emit(&BuildEvent::Cached {
            key: keys[0].1.as_str().to_string(),
            hit: true,
        });
        return Ok(true);
    }
    if restored > 0 {
        events.emit(&BuildEvent::Warning {
            stage: "bridge".to_string(),
            message: format!("缓存只命中 {restored}/{} 个语言，整体重生成", keys.len()),
        });
    }
    generate(project, &langs, events)?;
    for (lang, key) in keys {
        if let Err(err) = cache.store_tree(key, &root.join(lang.dir())) {
            events.emit(&BuildEvent::Warning {
                stage: "bridge".to_string(),
                message: format!("{lang} 绑定缓存写入失败（不阻断生成）: {err}"),
            });
        }
    }
    Ok(false)
}

/// 递归收集 `dir` 下的 `.udl`（不存在即空集）。
fn collect_udl(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), StageError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(source_error(err)),
    };
    for entry in entries {
        let entry = entry.map_err(source_error)?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(source_error)?;
        if file_type.is_dir() {
            collect_udl(&path, out)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "udl") {
            out.push(path);
        }
    }
    Ok(())
}

/// IO 失败 → `poly::E_BRIDGE_0002`（接口定义不可读）。
fn source_error(err: std::io::Error) -> StageError {
    Diagnostic::error(codes::E_BRIDGE_0002, format!("读取接口定义失败: {err}"))
        .with_hint("确认核心 crate 的 UDL 存在、路径可读（polypack.toml 的 [core].crate）")
        .into()
}

/// 缓存读写失败 → `poly::E_BRIDGE_0003`。
pub(crate) fn cache_error(err: poly_cache::CacheError) -> StageError {
    Diagnostic::error(codes::E_BRIDGE_0003, format!("桥产物缓存读写失败: {err}"))
        .with_hint("缓存目录不可写或已损坏；poly clean --all 后重试")
        .into()
}

/// 核心 crate 名（AAR 的 `lib<name>.so` 与清单用）。
pub fn core_lib_name(source: &InterfaceSource) -> Result<String, StageError> {
    Ok(compile::read_crate_name(&source.crate_dir)?.replace('-', "_"))
}

/// uniffi 的包名（= UDL 的 namespace，绑定文件与包目录都用它）。
pub fn namespace(source: &InterfaceSource) -> Result<String, StageError> {
    Ok(udl::parse_source(source.primary())?.namespace)
}
