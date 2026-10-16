//! napi（C ABI）出口（T-M3-01）：一份 UDL → RNOH / ArkTS 宿主可装载的 NAPI 模块。
//!
//! ## 为什么是「生成」而不是「手写绑定」
//!
//! 接口只有一份（核心 crate 的 UDL，见 [`super::interface_source`]）。uniffi 引擎产出
//! Kotlin / Swift，本模块是它的 napi 出口：**由 UDL 直出 C 胶水**（[`generate`]），
//! 仓库里同样不手写第二份绑定。胶水做的全部事情是把 napi_value 换成 uniffi 的 C ABI
//! 参数（RustBuffer / 标量），调用 `uniffi_<核心库>_fn_func_<函数>`，再把结果换回
//! napi_value——符号名与签名对着 uniffi 0.32 的 `paxFFI.h`（Swift 绑定侧的真源头）钉死，
//! 真跑断言见 `tests/napi_real.rs`（编出的 `.so` 符号表 ↔ UDL ↔ 胶水三处互证）。
//!
//! ## 编译链（HarmonyOS triple）
//!
//! ```text
//! cargo build --release --lib --target aarch64-unknown-linux-ohos    # linker = OHOS clang
//!   → target/<triple>/release/lib<核心>.a（staticlib：含整个 core）
//! <SDK>/native/llvm/bin/aarch64-unknown-linux-ohos-clang -shared 胶水.c lib<核心>.a
//!   → bridge/harmonyos/<abi>/lib<napi 模块>.so（自包含单文件：无 .so 间依赖）
//! ```
//!
//! 单文件自包含（胶水 + staticlib 一起链）是刻意的：RNOH/ArkTS 只按名字装载一个 `.so`，
//! 省掉「核心 .so 也得在装载路径上」的第二个失败模式。前置：核心 crate 的
//! `[lib] crate-type` 需含 `"staticlib"`（缺失时报修复提示，绝不产半份产物）。
//!
//! ## 注册入口（两个都产出，指向同一个 Init）
//!
//! - `napi_register_module_v1`：node / RNOH 的 context-aware NAPI 模块查找入口（真跑实证）；
//! - `napi_module_register(&mod)`（constructor，仅 `__OHOS__`）：ArkTS `import … from
//!   "lib<模块>.so"` 的装载路径，`nm_modname` = UDL namespace。
//!
//! ## 工具缺失语义（skip-if-missing 同 [`super::aar`]）
//!
//! SDK 缺失 / 编译失败 → `poly::E_BRIDGE_0003` + 修复提示；类型超出 v0 覆盖面（如
//! `sequence<…>` / optional / record）→ `poly::E_BRIDGE_0002` + 明确未覆盖说明，
//! 不产「看起来能编」的假胶水。
//!
//! shortcut: v0 覆盖标量 + string/boolean；`u64`/`i64` 经 JS number 传（>2^53 精度损失，
//! 见生成物注释），`sequence<…>` / optional / record / enum 走 E_BRIDGE_0002 拒绝——扩类型面
//! 时改 [`map_type`] 一处即可。
//! shortcut: 模块 `.so` 未接库级缓存（每次 cargo 增量 + clang 直编）；T-M3-03 的 HAP 二跑
//! 缓存需求落地时，按 [`super::aar::cache_key`] 的形状补键。

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{BridgeLang, BridgeOutput, InterfaceSource};
use crate::diagnostics::{Diagnostic, codes};
use crate::events::{BuildEvent, EventSink};
use crate::plan::{Project, StageError};

/// 一个 HarmonyOS ABI：HAP 库目录名 ↔ Rust triple ↔ OHOS clang 包装器前缀。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OhosAbi {
    /// HAP 内 `libs/<abi>/` 的目录名（与 `poly-bundle` 的口径同域）。
    pub dir: &'static str,
    /// Rust 目标三元组（rustup target）。
    pub triple: &'static str,
    /// OHOS SDK 的 clang 包装器前缀（`<前缀>-clang`）。
    pub clang: &'static str,
}

/// 三个 HarmonyOS ABI（固定顺序：真机 64 位优先）。
pub const ABIS: [OhosAbi; 3] = [
    OhosAbi {
        dir: "arm64-v8a",
        triple: "aarch64-unknown-linux-ohos",
        clang: "aarch64-unknown-linux-ohos",
    },
    OhosAbi {
        dir: "armeabi-v7a",
        triple: "armv7-unknown-linux-ohos",
        clang: "armv7-unknown-linux-ohos",
    },
    OhosAbi {
        dir: "x86_64",
        triple: "x86_64-unknown-linux-ohos",
        clang: "x86_64-unknown-linux-ohos",
    },
];

impl OhosAbi {
    /// 按目录名 / triple 解析（`--abi` 参数与测试用）。
    pub fn parse(text: &str) -> Option<Self> {
        ABIS.into_iter()
            .find(|abi| abi.dir == text || abi.triple == text)
    }

    /// 链接器环境变量名：`CARGO_TARGET_<TRIPLE 大写去连字符>_LINKER`。
    fn linker_var(self) -> String {
        format!(
            "CARGO_TARGET_{}_LINKER",
            self.triple.replace('-', "_").to_uppercase()
        )
    }
}

/// HarmonyOS SDK（= 含 `native/` 的 openharmony 目录）定位结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OhosSdk {
    /// SDK 根（`…/sdk/default/openharmony`）。
    pub root: PathBuf,
}

impl OhosSdk {
    /// 按环境变量定位：`$POLYPACK_OHOS_SDK` → `$DEVECO_SDK_HOME` → `$HARMONYOS_TOOLS` →
    /// `$OHOS_SDK_HOME`，每个根试 5 种层级落点（与 `poly-toolchain` 的 harmonyos-sdk
    /// 探测器同口径——桥编译链不依赖 poly-toolchain，只共用约定）。
    pub fn discover() -> Option<Self> {
        let mut candidates = Vec::new();
        for key in [
            "POLYPACK_OHOS_SDK",
            "DEVECO_SDK_HOME",
            "HARMONYOS_TOOLS",
            "OHOS_SDK_HOME",
        ] {
            let Some(value) = std::env::var_os(key).filter(|value| !value.is_empty()) else {
                continue;
            };
            let root = PathBuf::from(value);
            for nested in [
                "",
                "sdk",
                "sdk/default/openharmony",
                "default/openharmony",
                "openharmony",
            ] {
                candidates.push(root.join(nested));
            }
        }
        candidates
            .into_iter()
            .find(|root| {
                root.join("native/llvm/bin").is_dir() && root.join("native/sysroot").is_dir()
            })
            .map(|root| Self { root })
    }

    /// 该 ABI 的 clang 包装器：`native/llvm/bin/<前缀>-clang`（`-target` 与 `--sysroot` 已内置）。
    pub fn clang(&self, abi: OhosAbi) -> Option<PathBuf> {
        let path = self
            .root
            .join("native/llvm/bin")
            .join(format!("{}-clang", abi.clang));
        path.is_file().then_some(path)
    }

    /// sysroot 的 include 根（napi 头所在）。
    pub fn include_dir(&self) -> PathBuf {
        self.root.join("native/sysroot/usr/include")
    }

    /// 引擎身份串（诊断与事件流用）。
    pub fn describe(&self) -> String {
        format!("ohos-sdk@{}", self.root.display())
    }
}

/// 由 UDL 直出 napi C 胶水，落点 `<root>/bridge/napi/<namespace>.c`。
pub fn generate(project: &Project, source: &InterfaceSource) -> Result<BridgeOutput, StageError> {
    let namespace = super::namespace(source)?;
    let lib = super::core_lib_name(source)?;
    let interface = super::udl::parse_source(source.primary())?;
    if interface.functions.is_empty() {
        return Err(Diagnostic::error(
            codes::E_BRIDGE_0002,
            format!("UDL 的 namespace {namespace} 里没有函数，napi 模块会是空壳"),
        )
        .with_hint("在 UDL 的 namespace 块内声明函数（如 `string greet(string name);`）")
        .into());
    }
    let text = emit_c(&namespace, &lib, &interface, source)?;
    let dir = project.root.join("bridge").join(BridgeLang::Napi.dir());
    std::fs::create_dir_all(&dir).map_err(|err| {
        Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!("创建 napi 绑定目录失败（{}）: {err}", dir.display()),
        )
        .with_hint("确认工程目录可写；`poly bridge` 需要 <工程根>/bridge/ 的写权限")
    })?;
    let file = format!("{namespace}.c");
    let path = dir.join(&file);
    std::fs::write(&path, text).map_err(|err| {
        Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!("写入 napi 绑定失败（{}）: {err}", path.display()),
        )
        .with_hint("确认磁盘空间与写权限")
    })?;
    Ok(BridgeOutput {
        lang: BridgeLang::Napi,
        dir,
        files: vec![PathBuf::from(file)],
    })
}

/// 交叉编译核心 crate（`cargo build --release --lib --target <triple>`），返回 staticlib 路径。
///
/// 与 [`super::aar::build_lib`] 同一形状：linker 指 OHOS clang 包装器（`-target` +
/// `--sysroot` 内置），`CC`/`AR` 也一并钉死（核心依赖带 C 代码时 `cc` crate 消费）。
pub fn build_lib(
    core_dir: &Path,
    abi: OhosAbi,
    sdk: &OhosSdk,
    events: &mut dyn EventSink,
) -> Result<PathBuf, StageError> {
    let clang = sdk.clang(abi).ok_or_else(|| missing_clang(abi, sdk))?;
    let bin = clang.parent().unwrap_or(&sdk.root);
    let output = Command::new("cargo")
        .args(["build", "--release", "--lib", "--target", abi.triple])
        .current_dir(core_dir)
        .env("CARGO_TARGET_DIR", core_dir.join("target"))
        .env(abi.linker_var(), &clang)
        .env(format!("CC_{}", abi.triple.replace('-', "_")), &clang)
        .env(
            format!("AR_{}", abi.triple.replace('-', "_")),
            bin.join("llvm-ar"),
        )
        .env(
            format!("RANLIB_{}", abi.triple.replace('-', "_")),
            bin.join("llvm-ranlib"),
        )
        .output()
        .map_err(|err| {
            Diagnostic::error(
                codes::E_BRIDGE_0003,
                format!("无法执行 cargo 交叉编译（{}）: {err}", abi.triple),
            )
            .with_hint("确认 cargo 在 PATH 中，且核心 crate 目录存在")
        })?;
    if !output.status.success() {
        return Err(Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!(
                "核心 crate 交叉编译失败（{}）: {}",
                abi.triple,
                stderr_tail(&output.stderr)
            ),
        )
        .with_hint(format!(
            "①`rustup target add {}`；②确认 OHOS clang 可用（{}）",
            abi.triple,
            clang.display()
        ))
        .into());
    }
    let lib_name = crate::compile::read_crate_name(core_dir)?.replace('-', "_");
    let staticlib = core_dir
        .join("target")
        .join(abi.triple)
        .join("release")
        .join(format!("lib{lib_name}.a"));
    if !staticlib.is_file() {
        return Err(Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!("交叉编译成功但未找到 {}", staticlib.display()),
        )
        .with_hint(
            "napi 模块单文件自包含需要核心 crate 的 [lib] crate-type 含 \"staticlib\"\
             （与 \"cdylib\" 并列即可）",
        )
        .into());
    }
    events.emit(&BuildEvent::Compiled {
        crate_name: staticlib
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        triple: abi.triple.to_string(),
    });
    Ok(staticlib)
}

/// 胶水 + staticlib → 自包含 napi 模块 `.so`（落点 `<out_dir>/lib<模块名>.so`）。
pub fn build_module(
    glue: &Path,
    staticlib: &Path,
    module: &str,
    abi: OhosAbi,
    sdk: &OhosSdk,
    out_dir: &Path,
    events: &mut dyn EventSink,
) -> Result<PathBuf, StageError> {
    let clang = sdk.clang(abi).ok_or_else(|| missing_clang(abi, sdk))?;
    std::fs::create_dir_all(out_dir).map_err(|err| {
        Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!("创建模块产物目录失败（{}）: {err}", out_dir.display()),
        )
        .with_hint("确认输出目录可写、磁盘空间充足")
    })?;
    let so = out_dir.join(format!("lib{module}.so"));
    let include = sdk.include_dir();
    let output = Command::new(&clang)
        .arg("-shared")
        .arg("-fPIC")
        .arg("-O2")
        .arg(format!("-I{}", include.display()))
        .arg(glue)
        .arg(staticlib)
        .arg("-o")
        .arg(&so)
        .output()
        .map_err(|err| {
            Diagnostic::error(
                codes::E_BRIDGE_0003,
                format!("无法执行 OHOS clang（{}）: {err}", clang.display()),
            )
            .with_hint("确认 SDK 完整（native/llvm/bin 下有该 ABI 的 clang 包装器）")
        })?;
    if !output.status.success() {
        return Err(Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!(
                "napi 模块链接失败（{}）: {}",
                abi.triple,
                stderr_tail(&output.stderr)
            ),
        )
        .with_hint("核对胶水与核心库的符号一致性（重跑 poly bridge generate --lang napi）")
        .into());
    }
    events.emit(&BuildEvent::Warning {
        stage: "bridge".to_string(),
        message: format!(
            "napi 模块已链接：{}（{} 字节）",
            so.display(),
            std::fs::metadata(&so).map(|meta| meta.len()).unwrap_or(0)
        ),
    });
    Ok(so)
}

/// 缺该 ABI 的 clang → `poly::E_BRIDGE_0003` + 修复提示。
fn missing_clang(abi: OhosAbi, sdk: &OhosSdk) -> StageError {
    Diagnostic::error(
        codes::E_BRIDGE_0003,
        format!(
            "HarmonyOS SDK 缺 {} 的 clang 包装器（{}）",
            abi.triple,
            sdk.root.display()
        ),
    )
    .with_hint(
        "设 $HARMONYOS_TOOLS（command-line-tools 根）/ $DEVECO_SDK_HOME（DevEco 的 sdk 目录）\
         / $POLYPACK_OHOS_SDK（直接指向 …/sdk/default/openharmony）；`poly doctor` 可核对",
    )
    .into()
}

// ---------------------------------------------------------------- 胶水生成

/// v0 支持的类型（↔ uniffi FFI C 类型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CType {
    /// `string` ↔ `RustBuffer`（UTF-8 字节，经 `rustbuffer_from_bytes` 拷入 Rust 侧）。
    Str,
    /// `boolean` ↔ `int8_t`。
    Bool,
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    /// 仅返回位：`void`。
    Void,
}

/// UDL 类型文本 → C 类型；未覆盖返回 `None`（调用方报 E_BRIDGE_0002）。
fn map_type(udl: &str) -> Option<CType> {
    Some(match udl {
        "string" => CType::Str,
        "boolean" => CType::Bool,
        "u8" => CType::U8,
        "u16" => CType::U16,
        "u32" => CType::U32,
        "u64" => CType::U64,
        "i8" => CType::I8,
        "i16" => CType::I16,
        "i32" => CType::I32,
        "i64" => CType::I64,
        "f32" => CType::F32,
        "f64" => CType::F64,
        "void" => CType::Void,
        _ => return None,
    })
}

impl CType {
    /// uniffi FFI 侧的 C 类型名。
    fn ffi(self) -> &'static str {
        match self {
            CType::Str => "RustBuffer",
            CType::Bool => "int8_t",
            CType::U8 => "uint8_t",
            CType::U16 => "uint16_t",
            CType::U32 => "uint32_t",
            CType::U64 => "uint64_t",
            CType::I8 => "int8_t",
            CType::I16 => "int16_t",
            CType::I32 => "int32_t",
            CType::I64 => "int64_t",
            CType::F32 => "float",
            CType::F64 => "double",
            CType::Void => "void",
        }
    }
}

/// 生成整份 C 胶水（纯函数：输出只由接口定义决定，同输入字节一致）。
fn emit_c(
    namespace: &str,
    lib: &str,
    interface: &super::udl::Interface,
    source: &InterfaceSource,
) -> Result<String, StageError> {
    // 先全量类型检查：任何未覆盖类型都在生成前一次性报清（不产半份胶水）
    for function in &interface.functions {
        for (name, ty) in function.args.iter().zip(&function.arg_types) {
            if map_type(ty).is_none() {
                return Err(unsupported_type(
                    ty,
                    &format!("参数 {name}（{}）", function.name),
                ));
            }
        }
        if map_type(&function.ret).is_none() {
            return Err(unsupported_type(&function.ret, &function.name));
        }
        if map_type(&function.ret) == Some(CType::Void) {
            return Err(Diagnostic::error(
                codes::E_BRIDGE_0002,
                format!("napi 出口暂不支持 void 返回（{}）", function.name),
            )
            .with_hint("UDL 的 void 函数当前无 napi 消费点；改返回具体类型，或走 Kotlin/Swift 出口")
            .into());
        }
    }

    let udl_rel = source
        .primary()
        .strip_prefix(&source.crate_dir)
        .unwrap_or(source.primary())
        .to_string_lossy()
        .replace('\\', "/");
    let mut out = String::new();
    out.push_str(&format!(
        "/* 由 poly bridge 生成（T-M2-01 生成器 · napi 出口，{}）——勿手改。\n\
         * 接口来源：{udl_rel}（namespace {namespace}）；改接口后重跑 `poly bridge generate --lang napi`。\n\
         * 生成物只做 napi_value ↔ uniffi C ABI 的搬运；符号与布局对齐 uniffi 0.32 的 FFI.h。 */\n",
        super::BRIDGE_SCHEMA
    ));
    out.push_str("#include <stdlib.h>\n#include <string.h>\n\n");
    out.push_str(
        "/* OHOS：napi 运行时头（sysroot/usr/include/napi/native_api.h）；\n\
         * node / RNOH：同源 NAPI ABI（node_api.h 由宿主运行时提供符号）。 */\n\
         #if defined(__OHOS__)\n\
         #include \"napi/native_api.h\"\n\
         #else\n\
         #include \"node_api.h\"\n\
         #endif\n\n",
    );
    out.push_str(&format!(
        "/* uniffi 的 C ABI（布局为 FFI.h 的 UNIFFI_SHARED_HEADER_V4 契约）。 */\n\
         typedef struct RustBuffer {{ uint64_t capacity; uint64_t len; uint8_t *data; }} RustBuffer;\n\
         typedef struct ForeignBytes {{ int32_t len; const uint8_t *data; }} ForeignBytes;\n\
         typedef struct RustCallStatus {{ int8_t code; RustBuffer error_buf; }} RustCallStatus;\n\n\
         extern RustBuffer ffi_{lib}_rustbuffer_from_bytes(ForeignBytes bytes, RustCallStatus *out_status);\n\
         extern void ffi_{lib}_rustbuffer_free(RustBuffer buf, RustCallStatus *out_status);\n"
    ));
    for function in &interface.functions {
        out.push_str(&ffi_decl(lib, function));
    }
    out.push_str(
        "\n#define POLY_CHECK(expr, msg)                     \\\n\
         \x20 do {                                          \\\n\
         \x20   if ((expr) != napi_ok) {                    \\\n\
         \x20     napi_throw_error(env, NULL, (msg));       \\\n\
         \x20     return NULL;                              \\\n\
         \x20   }                                           \\\n\
         \x20 } while (0)\n\n",
    );
    out.push_str(&format!(
        "/* 调用失败：code 1/2 的 error_buf 归外方释放（uniffi 约定）；3=Cancelled 不释放。 */\n\
         static napi_value PolyFailed(napi_env env, const char *func, RustCallStatus *status) {{\n\
         \x20 char text[256];\n\
         \x20 size_t len = 0;\n\
         \x20 if (status->error_buf.data != NULL && status->error_buf.len > 0) {{\n\
         \x20   len = status->error_buf.len > 200 ? 200 : (size_t)status->error_buf.len;\n\
         \x20 }}\n\
         \x20 snprintf(text, sizeof text, \"poly: %s 调用失败（uniffi status=%d）%.*s\", func,\n\
         \x20          (int)status->code, (int)len,\n\
         \x20          len > 0 ? (const char *)status->error_buf.data : \"\");\n\
         \x20 if (status->code == 1 || status->code == 2) {{\n\
         \x20   RustCallStatus free_status = {{0}};\n\
         \x20   ffi_{lib}_rustbuffer_free(status->error_buf, &free_status);\n\
         \x20 }}\n\
         \x20 napi_throw_error(env, NULL, text);\n\
         \x20 return NULL;\n\
         }}\n"
    ));
    for function in &interface.functions {
        out.push_str(&emit_callback(namespace, lib, function));
    }
    out.push_str(&emit_init(namespace, &interface.functions));
    Ok(out)
}

/// 一条 `extern` 声明：`uniffi_<lib>_fn_func_<名>(<参数...>, RustCallStatus *)`。
fn ffi_decl(lib: &str, function: &super::udl::Function) -> String {
    let params: Vec<String> = function
        .args
        .iter()
        .zip(&function.arg_types)
        .map(|(name, ty)| format!("{} {name}", map_type(ty).expect("已全量检查").ffi()))
        .collect();
    let ret = map_type(&function.ret).expect("已全量检查").ffi();
    let mut all = params;
    all.push("RustCallStatus *out_status".to_string());
    format!(
        "extern {ret} uniffi_{lib}_fn_func_{}({});\n",
        function.name,
        all.join(", ")
    )
}

/// 一个 napi 回调：取参 → 转换 → 调 FFI → 转换返回。
fn emit_callback(namespace: &str, lib: &str, function: &super::udl::Function) -> String {
    let name = &function.name;
    let argc = function.args.len();
    let mut out = format!(
        "\n/* {namespace}.{name}（{argc} 参） */\nstatic napi_value PolyCall_{name}(napi_env env, napi_callback_info info) {{\n"
    );
    if argc == 0 {
        out.push_str("  size_t argc = 0;\n");
        out.push_str(&format!(
            "  POLY_CHECK(napi_get_cb_info(env, info, &argc, NULL, NULL, NULL), \"poly: 读取 {name} 参数失败\");\n"
        ));
    } else {
        out.push_str(&format!(
            "  size_t argc = {argc};\n  napi_value argv[{argc}];\n"
        ));
        out.push_str(&format!(
            "  POLY_CHECK(napi_get_cb_info(env, info, &argc, argv, NULL, NULL), \"poly: 读取 {name} 参数失败\");\n\
             \x20 if (argc < {argc}) {{ napi_throw_error(env, NULL, \"poly: {name} 需要 {argc} 个参数\"); return NULL; }}\n"
        ));
    }
    // status 先声明：string 参数的 rustbuffer_from_bytes 也经它回报（uniffi 外方持有的约定）
    out.push_str("  RustCallStatus status = {0};\n");
    for (index, (arg, ty)) in function.args.iter().zip(&function.arg_types).enumerate() {
        let ctype = map_type(ty).expect("已全量检查");
        out.push_str(&emit_arg(name, lib, arg, ctype, index));
    }
    let ret = map_type(&function.ret).expect("已全量检查");
    let mut call_args: Vec<String> = function
        .args
        .iter()
        .enumerate()
        .map(|(index, arg)| format!("{arg}_{index}"))
        .collect();
    call_args.push("&status".to_string());
    out.push_str(&format!(
        "  {} ret = uniffi_{lib}_fn_func_{name}({});\n",
        ret.ffi(),
        call_args.join(", ")
    ));
    out.push_str(&format!(
        "  if (status.code != 0) {{ return PolyFailed(env, \"{name}\", &status); }}\n"
    ));
    out.push_str(&emit_return(name, lib, ret));
    out.push_str("}\n");
    out
}

/// 一个参数的 JS → C 转换（含类型校验）。
fn emit_arg(func: &str, lib: &str, arg: &str, ctype: CType, index: usize) -> String {
    let msg = format!(
        "poly: {func} 的参数 {arg} 需要 {}",
        match ctype {
            CType::Str => "string",
            CType::Bool => "boolean",
            CType::F32 | CType::F64 => "number",
            CType::U8 | CType::U16 | CType::U32 | CType::U64 => "非负整数（number）",
            _ => "整数（number）",
        }
    );
    match ctype {
        CType::Str => format!(
            "  size_t len_{index} = 0;\n  \
             POLY_CHECK(napi_get_value_string_utf8(env, argv[{index}], NULL, 0, &len_{index}), \"{msg}\");\n  \
             char *buf_{index} = (char *)malloc(len_{index} + 1);\n  \
             if (buf_{index} == NULL) {{ napi_throw_error(env, NULL, \"poly: {func} 的参数 {arg} 缓冲分配失败\"); return NULL; }}\n  \
             POLY_CHECK(napi_get_value_string_utf8(env, argv[{index}], buf_{index}, len_{index} + 1, &len_{index}), \"{msg}\");\n  \
             ForeignBytes bytes_{index};\n  \
             bytes_{index}.len = (int32_t)len_{index};\n  \
             bytes_{index}.data = (const uint8_t *)buf_{index};\n  \
             RustBuffer {arg}_{index} = ffi_{lib}_rustbuffer_from_bytes(bytes_{index}, &status);\n  \
             free(buf_{index});\n  \
             if (status.code != 0) {{ return PolyFailed(env, \"{func}\", &status); }}\n"
        ),
        CType::Bool => format!(
            "  bool bool_{index} = false;\n  \
             POLY_CHECK(napi_get_value_bool(env, argv[{index}], &bool_{index}), \"{msg}\");\n  \
             int8_t {arg}_{index} = bool_{index} ? 1 : 0;\n"
        ),
        CType::U8 | CType::U16 | CType::U32 => format!(
            "  uint32_t tmp_{index} = 0;\n  \
             POLY_CHECK(napi_get_value_uint32(env, argv[{index}], &tmp_{index}), \"{msg}\");\n  \
             {} {arg}_{index} = ({})tmp_{index};\n",
            ctype.ffi(),
            ctype.ffi()
        ),
        CType::U64 => format!(
            "  int64_t tmp_{index} = 0;\n  \
             POLY_CHECK(napi_get_value_int64(env, argv[{index}], &tmp_{index}), \"{msg}\");\n  \
             uint64_t {arg}_{index} = (uint64_t)tmp_{index};\n"
        ),
        CType::I8 | CType::I16 | CType::I32 => format!(
            "  int32_t tmp_{index} = 0;\n  \
             POLY_CHECK(napi_get_value_int32(env, argv[{index}], &tmp_{index}), \"{msg}\");\n  \
             {} {arg}_{index} = ({})tmp_{index};\n",
            ctype.ffi(),
            ctype.ffi()
        ),
        CType::I64 => format!(
            "  int64_t {arg}_{index} = 0;\n  \
             POLY_CHECK(napi_get_value_int64(env, argv[{index}], &{arg}_{index}), \"{msg}\");\n"
        ),
        CType::F32 | CType::F64 => format!(
            "  double tmp_{index} = 0;\n  \
             POLY_CHECK(napi_get_value_double(env, argv[{index}], &tmp_{index}), \"{msg}\");\n  \
             {} {arg}_{index} = ({})tmp_{index};\n",
            ctype.ffi(),
            ctype.ffi()
        ),
        CType::Void => unreachable!("void 参数不存在"),
    }
}

/// 返回值的 C → JS 转换。
fn emit_return(func: &str, lib: &str, ctype: CType) -> String {
    match ctype {
        CType::Str => format!(
            "  napi_value result;\n  \
             POLY_CHECK(napi_create_string_utf8(env, ret.data == NULL ? \"\" : (const char *)ret.data, ret.len, &result), \"poly: {func} 的返回值转换失败\");\n  \
             RustCallStatus free_status = {{0}};\n  \
             ffi_{lib}_rustbuffer_free(ret, &free_status);\n  \
             return result;\n"
        ),
        CType::Bool => format!(
            "  napi_value result;\n  \
             POLY_CHECK(napi_get_boolean(env, ret != 0, &result), \"poly: {func} 的返回值转换失败\");\n  \
             return result;\n"
        ),
        // JS number 承载整数：u64/i64 超过 2^53 有精度损失（v0 明示边界，不静默）
        CType::U8 | CType::U16 | CType::U32 | CType::U64 => format!(
            "  napi_value result;\n  \
             POLY_CHECK(napi_create_int64(env, (int64_t)ret, &result), \"poly: {func} 的返回值转换失败\");\n  \
             return result;\n"
        ),
        CType::I8 | CType::I16 | CType::I32 | CType::I64 | CType::F32 | CType::F64 => format!(
            "  napi_value result;\n  \
             POLY_CHECK(napi_create_double(env, (double)ret, &result), \"poly: {func} 的返回值转换失败\");\n  \
             return result;\n"
        ),
        CType::Void => unreachable!("void 返回已在全量检查时拒绝"),
    }
}

/// 模块注册：属性表 + `napi_register_module_v1`（node/RNOH）+ `napi_module_register`
/// 构造函数（ArkTS 装载路径，仅 `__OHOS__`）。
fn emit_init(namespace: &str, functions: &[super::udl::Function]) -> String {
    let mut out = format!(
        "\n/* 模块注册：两个入口共用同一张属性表。 */\nstatic napi_value PolyInit(napi_env env, napi_value exports) {{\n  napi_property_descriptor props[] = {{\n"
    );
    for function in functions {
        out.push_str(&format!(
            "      {{\"{}\", NULL, PolyCall_{}, NULL, NULL, NULL, napi_default, NULL}},\n",
            function.name, function.name
        ));
    }
    out.push_str(
        "  };\n\
         \x20 POLY_CHECK(napi_define_properties(env, exports, sizeof props / sizeof props[0], props),\n\
         \x20            \"poly: 注册模块函数失败\");\n\
         \x20 return exports;\n\
         }\n",
    );
    out.push_str(&format!(
        "\n/* 入口一：OHOS ArkTS `import … from \"lib{namespace}.so\"`（nm_modname 必须一致）。 */\n\
         #if defined(__OHOS__)\n\
         static napi_module poly_module = {{\n\
         \x20   .nm_version = 1,\n\
         \x20   .nm_flags = 0,\n\
         \x20   .nm_filename = NULL,\n\
         \x20   .nm_register_func = PolyInit,\n\
         \x20   .nm_modname = \"{namespace}\",\n\
         \x20   .nm_priv = NULL,\n\
         \x20   .reserved = {{0}},\n\
         }};\n\n\
         __attribute__((constructor)) static void PolyRegisterModule(void) {{\n\
         \x20   napi_module_register(&poly_module);\n\
         }}\n\
         #endif\n\n\
         /* 入口二：node / RNOH 的 context-aware NAPI 查找入口（真跑实证的装载路径）。 */\n\
         napi_value napi_register_module_v1(napi_env env, napi_value exports) {{\n\
         \x20 return PolyInit(env, exports);\n\
         }}\n"
    ));
    out
}

/// 未覆盖类型 → `poly::E_BRIDGE_0002` + 明确范围说明。
fn unsupported_type(ty: &str, where_: &str) -> StageError {
    Diagnostic::error(
        codes::E_BRIDGE_0002,
        format!("napi 出口暂不支持类型 `{ty}`（{where_}）"),
    )
    .with_hint(
        "v0 覆盖：string / boolean / u8…u64 / i8…i64 / f32 / f64（+ void 暂无消费点）；\
         复合类型（sequence / record / enum / optional）待扩面，可先走 Kotlin/Swift 出口",
    )
    .into()
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
