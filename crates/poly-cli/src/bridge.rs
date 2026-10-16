//! `poly bridge`：uniffi 桥生成器、Android AAR 组装与 HarmonyOS NAPI 模块（T-M2-01 / T-M2-02 / T-M3-01）。
//!
//! - `poly bridge generate`：由单处接口定义（核心 crate 的 UDL）生成 Kotlin / Swift 绑定与
//!   napi C 胶水，落点 `<root>/bridge/{kotlin,swift,napi}`；`--cache` 走库级缓存（键含接口
//!   摘要，见 `poly_core::bridge::bridge_cache_key`）。
//! - `poly bridge aar`：核心 → 各 ABI 的 JNI `.so`（NDK 交叉编译）+ 绑定 → `.aar`
//!   （`jni/<abi>/` + `classes.jar` + 最小清单），走库级缓存（二跑命中）。
//! - `poly bridge ohos`：胶水 → 各 ABI 的自包含 NAPI 模块 `.so`（HarmonyOS triple 交叉编译），
//!   落点 `<root>/bridge/harmonyos/<abi>/lib<ns>.so`（RNOH / ArkTS 宿主直接装载）。
//! - `poly bridge wasm`（T-M3-07）：小程序沙箱的 wasm 出口——UDL 白名单 → shim（wasm-bindgen
//!   包装）→ wasm32 编译 → 胶水 + 加载适配层 + 白名单清单，走库级缓存（命中还原不需要 CLI）。
//!
//! 引擎 / NDK / OHOS SDK / kotlinc / wasm-bindgen 任一缺失都按 skip-if-missing 语义如实报告，绝不产假产物。

use std::path::PathBuf;

use poly_cache::Cache;
use poly_core::bridge::{self, BridgeLang, aar};
use poly_core::{Diagnostic, Project, codes};

use crate::project;

/// `poly bridge` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 子命令。
    #[command(subcommand)]
    pub command: BridgeCommand,
}

/// bridge 子命令。
#[derive(Debug, clap::Subcommand)]
pub enum BridgeCommand {
    /// 生成 Kotlin / Swift 绑定与 napi C 胶水（单处接口定义 → 各端绑定）。
    Generate(GenerateArgs),
    /// 组装 Android AAR（各 ABI 的 JNI `.so` + 绑定 + 最小清单）。
    Aar(AarArgs),
    /// 构建 HarmonyOS NAPI 模块（胶水 + 各 ABI 自包含 `.so`，RNOH / ArkTS 可装载）。
    Ohos(OhosArgs),
    /// 编译小程序沙箱的 wasm 出口（UDL 白名单 → shim → wasm32 → 胶水 + 适配层 + 清单）。
    Wasm(WasmArgs),
}

/// `poly bridge wasm` 的参数。
#[derive(Debug, clap::Args)]
pub struct WasmArgs {
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// 产物落点（缺省 `<root>/bridge/wasm-bindgen/`；随代码包分发的那四个文件）。
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// 构建 profile（缓存键的「构建参数」输入）。
    #[arg(long, value_enum, default_value_t = crate::build::ProfileArg::Local)]
    pub profile: crate::build::ProfileArg,
    /// 不走缓存（强制重编译 shim 与重生成胶水）。
    #[arg(long = "no-cache")]
    pub no_cache: bool,
}

/// `poly bridge generate` 的参数。
#[derive(Debug, clap::Args)]
pub struct GenerateArgs {
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// 目标语言（可重复；缺省 `all` = 全部已接入出口：kotlin + swift + napi）。
    #[arg(long = "lang", value_name = "LANG")]
    pub langs: Vec<String>,
    /// 走库级缓存（键含接口摘要）。
    #[arg(long)]
    pub cache: bool,
    /// 只打印会做什么，不落盘。
    #[arg(long = "dry-run")]
    pub dry_run: bool,
}

/// `poly bridge aar` 的参数。
#[derive(Debug, clap::Args)]
pub struct AarArgs {
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// ABI 目录名或 triple（可重复；缺省 arm64-v8a + x86_64）。
    #[arg(long = "abi", value_name = "ABI")]
    pub abis: Vec<String>,
    /// `minSdkVersion`（也是 NDK clang 的 API 等级）。
    #[arg(long = "min-sdk", default_value_t = aar::DEFAULT_MIN_SDK)]
    pub min_sdk: u32,
    /// 产物落点（缺省 `<root>/bridge/android/<核心库名>.aar`）。
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// 额外编译 Kotlin 绑定为 `classes.jar`（需 kotlinc 与 JNA，缺则如实告警并跳过）。
    #[arg(long = "kotlin")]
    pub kotlin: bool,
    /// 构建 profile（缓存键的「构建参数」输入）。
    #[arg(long, value_enum, default_value_t = crate::build::ProfileArg::Local)]
    pub profile: crate::build::ProfileArg,
    /// 不走缓存（强制重新交叉编译与组装）。
    #[arg(long = "no-cache")]
    pub no_cache: bool,
}

/// `poly bridge ohos` 的参数。
#[derive(Debug, clap::Args)]
pub struct OhosArgs {
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// ABI 目录名或 triple（可重复；缺省 arm64-v8a = 真机）。
    #[arg(long = "abi", value_name = "ABI")]
    pub abis: Vec<String>,
}

/// 执行 `poly bridge`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    match &args.command {
        BridgeCommand::Generate(args) => generate(args, json),
        BridgeCommand::Aar(args) => assemble_aar(args, json),
        BridgeCommand::Ohos(args) => assemble_ohos(args, json),
        BridgeCommand::Wasm(args) => build_wasm(args, json),
    }
}

/// `poly bridge generate`。
fn generate(args: &GenerateArgs, json: bool) -> Result<i32, Diagnostic> {
    let project = project::load(&args.config)?;
    let langs = parse_langs(&args.langs)?;
    if args.dry_run {
        let source = bridge::interface_source(&project).map_err(err)?;
        let digest = bridge::interface_digest(&source).map_err(err)?;
        report(
            json,
            serde_json::json!({
                "event": "bridge-plan",
                "schema": bridge::BRIDGE_SCHEMA,
                "udl": source.files.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>(),
                "interface": digest.as_str(),
                "langs": langs.iter().map(|lang| lang.id()).collect::<Vec<_>>(),
            }),
        );
        return Ok(0);
    }
    let mut events = project::sink(json);
    let outputs = if args.cache {
        let cache = Cache::from_root(project::cache_root(&project.root));
        let source = bridge::interface_source(&project).map_err(err)?;
        let interface = bridge::interface_digest(&source).map_err(err)?;
        let keys = bridge::bridge_cache_keys(&project, &interface, "local", &langs).map_err(err)?;
        bridge::restore_or_generate(&cache, &keys, &project, events.as_mut()).map_err(err)?;
        logged_outputs(&project, &langs)?
    } else {
        bridge::generate(&project, &langs, events.as_mut()).map_err(err)?
    };
    report(
        json,
        serde_json::json!({
            "event": "bridge-generated",
            "schema": bridge::BRIDGE_SCHEMA,
            "outputs": outputs.iter().map(|output| serde_json::json!({
                "lang": output.lang.id(),
                "dir": output.dir.to_string_lossy(),
                "files": output.files.iter().map(|file| file.to_string_lossy()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }),
    );
    Ok(0)
}

/// 读回实际产物清单（缓存命中与现场生成同一口径：以磁盘为准）。
fn logged_outputs(
    project: &Project,
    langs: &[BridgeLang],
) -> Result<Vec<bridge::BridgeOutput>, Diagnostic> {
    let root = project.root.join("bridge");
    let mut outputs = Vec::new();
    for lang in langs {
        let dir = root.join(lang.dir());
        let files = list_files(&dir).map_err(|err| {
            Diagnostic::error(
                codes::E_BRIDGE_0001,
                format!("缓存还原后读取产物失败（{}）: {err}", dir.display()),
            )
            .with_hint("poly clean --all 后重跑 poly bridge generate --cache")
        })?;
        if files.is_empty() {
            return Err(Diagnostic::error(
                codes::E_BRIDGE_0001,
                format!("{} 下没有绑定产物", dir.display()),
            )
            .with_hint("去掉 --cache 重新生成，以暴露真实引擎错误"));
        }
        outputs.push(bridge::BridgeOutput {
            lang: *lang,
            dir,
            files,
        });
    }
    Ok(outputs)
}

/// `poly bridge aar`。
fn assemble_aar(args: &AarArgs, json: bool) -> Result<i32, Diagnostic> {
    let project = project::load(&args.config)?;
    let source = bridge::interface_source(&project).map_err(err)?;
    let lib_name = bridge::core_lib_name(&source).map_err(err)?;
    let namespace = bridge::namespace(&source).map_err(err)?;
    let interface = bridge::interface_digest(&source).map_err(err)?;
    let abis = parse_abis(&args.abis)?;
    let out = args.out.clone().unwrap_or_else(|| {
        project
            .root
            .join("bridge")
            .join("android")
            .join(format!("{lib_name}.aar"))
    });

    let ndk = aar::ndk_root().ok_or_else(|| {
        Diagnostic::error(
            codes::E_BRIDGE_0003,
            "未找到 Android NDK（交叉编译 JNI .so 的前提）",
        )
        .with_hint(
            "设 $ANDROID_NDK_HOME / $POLYPACK_NDK_HOME 指向 NDK 根，\
             或用 sdkmanager 装：sdkmanager \"ndk;27.0.12077973\"",
        )
    })?;

    let mut events = project::sink(json);
    let cache = Cache::from_root(project::cache_root(&project.root));
    let engine = "ndk-cargo";
    let key = if args.no_cache {
        None
    } else {
        Some(
            aar::cache_key(
                &project,
                &abis,
                &interface,
                engine,
                profile_name(args.profile),
            )
            .map_err(err)?,
        )
    };

    // 命中：还原 AAR 字节，跳过全部交叉编译
    if let Some(key) = &key
        && let Some(bytes) = cache.lookup(key).map_err(cache_err)?
    {
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| project::io_error("创建 AAR 目录", parent, err))?;
        }
        std::fs::write(&out, &bytes).map_err(|err| project::io_error("写入 AAR", &out, err))?;
        report(
            json,
            serde_json::json!({
                "event": "bridge-aar",
                "schema": bridge::BRIDGE_SCHEMA,
                "cached": true,
                "key": key.as_str(),
                "aar": out.to_string_lossy(),
                "abi": abis.iter().map(|abi| abi.dir).collect::<Vec<_>>(),
            }),
        );
        return Ok(0);
    }

    // 未命中：生成绑定 → 逐 ABI 交叉编译 → 组装
    let langs = [BridgeLang::Kotlin];
    bridge::generate(&project, &langs, events.as_mut()).map_err(err)?;
    let core_dir = project.root.join(&project.config.core.crate_path);
    let mut libs = Vec::new();
    for abi in &abis {
        events.emit(&poly_core::BuildEvent::Warning {
            stage: "bridge".to_string(),
            message: format!("交叉编译 {}（{}）…", abi.dir, abi.triple),
        });
        let so =
            aar::build_lib(&core_dir, *abi, &ndk, args.min_sdk, events.as_mut()).map_err(err)?;
        libs.push((*abi, so));
    }

    let classes_jar = if args.kotlin {
        match compile_kotlin(&project, &out, events.as_mut()) {
            Ok(path) => Some(path),
            Err(err) => {
                events.emit(&poly_core::BuildEvent::Warning {
                    stage: "bridge".to_string(),
                    message: format!(
                        "Kotlin 绑定未能编译为 classes.jar（AAR 仍组装，dex 由消费方补）：{}",
                        err.diagnostic.message
                    ),
                });
                None
            }
        }
    } else {
        events.emit(&poly_core::BuildEvent::Warning {
            stage: "bridge".to_string(),
            message: "未请求 --kotlin：AAR 不含 classes.jar（仅有 .so、绑定源码与清单）"
                .to_string(),
        });
        None
    };

    let manifest = aar::manifest_xml(&namespace, args.min_sdk, "1.0");
    let sources = project.root.join("bridge").join("kotlin");
    aar::assemble(&out, &manifest, &libs, classes_jar.as_deref(), &sources).map_err(err)?;
    let bytes = std::fs::metadata(&out)
        .map_err(|err| project::io_error("读取 AAR", &out, err))?
        .len();

    // 入库失败一律只告警不阻断——但绝不塞一条空 blob 进缓存（会被当成命中还原出空 AAR）
    if let Some(key) = &key {
        match std::fs::read(&out) {
            Ok(blob) => {
                if let Err(err) = cache.store(key, &blob) {
                    events.emit(&poly_core::BuildEvent::Warning {
                        stage: "bridge".to_string(),
                        message: format!("AAR 缓存写入失败（不阻断）: {err}"),
                    });
                }
            }
            Err(err) => events.emit(&poly_core::BuildEvent::Warning {
                stage: "bridge".to_string(),
                message: format!("AAR 已产出但读取失败，未入缓存（不阻断）: {err}"),
            }),
        }
    }
    report(
        json,
        serde_json::json!({
            "event": "bridge-aar",
            "schema": bridge::BRIDGE_SCHEMA,
            "cached": false,
            "key": key.as_ref().map(|key| key.as_str().to_string()),
            "aar": out.to_string_lossy(),
            "bytes": bytes,
            "classes_jar": classes_jar.map(|path| path.to_string_lossy().to_string()),
            "abi": abis.iter().map(|abi| abi.dir).collect::<Vec<_>>(),
        }),
    );
    Ok(0)
}

/// `poly bridge wasm`（T-M3-07）：小程序沙箱的 wasm 出口。
///
/// 缓存键里的引擎串取**钉住的**版本常量（生成期才核对 CLI 版本）——命中还原不需要
/// wasm-bindgen CLI 在场，CI 里可只靠缓存出产物。
fn build_wasm(args: &WasmArgs, json: bool) -> Result<i32, Diagnostic> {
    let project = project::load(&args.config)?;
    let out = args.out.clone().unwrap_or_else(|| {
        project
            .root
            .join("bridge")
            .join(BridgeLang::WasmBindgen.dir())
    });
    let source = bridge::interface_source(&project).map_err(err)?;
    let interface = bridge::interface_digest(&source).map_err(err)?;
    let engine = format!("wasm-bindgen@{}", bridge::wasm::WASM_BINDGEN_VERSION);
    let key = if args.no_cache {
        None
    } else {
        Some(
            bridge::wasm::cache_key(&project, &interface, &engine, profile_name(args.profile))
                .map_err(err)?,
        )
    };
    let cache = Cache::from_root(project::cache_root(&project.root));

    // 命中：还原分发树，跳过 shim 编译与胶水生成
    if let Some(key) = &key
        && cache.restore_tree(key, &out).map_err(cache_err)?
    {
        let files = list_files(&out).map_err(|err| {
            Diagnostic::error(
                codes::E_BRIDGE_0003,
                format!("缓存还原后读取产物失败（{}）: {err}", out.display()),
            )
            .with_hint("poly clean --all 后重跑 poly bridge wasm")
        })?;
        report_wasm(json, true, Some(key), &out, &files, None);
        return Ok(0);
    }

    let mut events = project::sink(json);
    let product = bridge::wasm::generate(&project, &out, events.as_mut()).map_err(err)?;
    if let Some(key) = &key
        && let Err(err) = cache.store_tree(key, &product.dir)
    {
        events.emit(&poly_core::BuildEvent::Warning {
            stage: "bridge".to_string(),
            message: format!("wasm 出口缓存写入失败（不阻断）: {err}"),
        });
    }
    report_wasm(
        json,
        false,
        key.as_ref(),
        &product.dir,
        &product.files,
        Some(product.manifest.functions.len()),
    );
    Ok(0)
}

/// wasm 出口的输出报告（人读 / JSON 同一份字段）。
fn report_wasm(
    json: bool,
    cached: bool,
    key: Option<&poly_cache::Digest>,
    dir: &std::path::Path,
    files: &[PathBuf],
    functions: Option<usize>,
) {
    let mut value = serde_json::json!({
        "event": "bridge-wasm",
        "schema": bridge::BRIDGE_SCHEMA,
        "sandbox": bridge::wasm::SANDBOX_SCHEMA,
        "cached": cached,
        "key": key.map(|key| key.as_str().to_string()),
        "dir": dir.to_string_lossy(),
        "files": files.iter().map(|file| file.to_string_lossy()).collect::<Vec<_>>(),
    });
    if let Some(functions) = functions {
        value["functions"] = serde_json::json!(functions);
    }
    report(json, value);
}

/// 编译 Kotlin 绑定为 `classes.jar`（kotlinc 缺失 → 错误，由调用方降级为告警）。
fn compile_kotlin(
    project: &Project,
    aar_path: &std::path::Path,
    events: &mut dyn poly_core::EventSink,
) -> Result<PathBuf, poly_core::StageError> {
    let compiler = bridge::kotlinc::Kotlinc::discover().ok_or_else(|| {
        Diagnostic::error(
            codes::E_BRIDGE_0003,
            "未找到 Kotlin 编译器（kotlinc）与 JNA",
        )
        .with_hint(
            "装 kotlinc（或把官方 kotlin-compiler-*.zip 解到 ~/.cache/polypack/tools/kotlin/）\
             并放一个 jna-*.jar 在同一目录（或设 $POLYPACK_JNA_JAR）",
        )
    })?;
    let kotlin_dir = project.root.join("bridge").join("kotlin");
    let sources = kotlin_sources(&kotlin_dir).map_err(|err| {
        Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!(
                "读取 Kotlin 绑定源码失败（{}）: {err}",
                kotlin_dir.display()
            ),
        )
        .with_hint("先跑 poly bridge generate --lang kotlin")
    })?;
    if sources.is_empty() {
        return Err(Diagnostic::error(
            codes::E_BRIDGE_0003,
            format!("{} 下没有 .kt 绑定源码", kotlin_dir.display()),
        )
        .with_hint("先跑 poly bridge generate --lang kotlin")
        .into());
    }
    let jar = aar_path.with_extension("classes.jar");
    compiler.compile(&sources, &jar)?;
    events.emit(&poly_core::BuildEvent::Warning {
        stage: "bridge".to_string(),
        message: format!(
            "Kotlin 绑定已编译：{} 个源文件 → {}",
            sources.len(),
            jar.display()
        ),
    });
    Ok(jar)
}

/// 递归收集 `.kt` 源文件（升序）。
fn kotlin_sources(dir: &std::path::Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if !current.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&current)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "kt") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// profile 名（缓存键的「构建参数」输入）。
fn profile_name(profile: crate::build::ProfileArg) -> &'static str {
    match profile {
        crate::build::ProfileArg::Local => "local",
        crate::build::ProfileArg::Ci => "ci",
    }
}

/// 解析 `--lang`（缺省 = 已接入出口 kotlin + swift + napi）。
fn parse_langs(values: &[String]) -> Result<Vec<BridgeLang>, Diagnostic> {
    if values.is_empty() || values.iter().any(|value| value == "all") {
        return Ok(vec![
            BridgeLang::Kotlin,
            BridgeLang::Swift,
            BridgeLang::Napi,
        ]);
    }
    let mut langs = Vec::new();
    for value in values {
        let lang = BridgeLang::parse(value).ok_or_else(|| unknown_lang(value))?;
        if !langs.contains(&lang) {
            langs.push(lang);
        }
    }
    Ok(langs)
}

/// 未知语言 → `poly::E_BRIDGE_0002` + 可选值清单。
fn unknown_lang(value: &str) -> Diagnostic {
    Diagnostic::error(codes::E_BRIDGE_0002, format!("未知的绑定语言: {value}")).with_hint(
        "可选：kotlin / swift（uniffi 引擎）、napi（C 胶水，M3 已接入）、\
         wasm-bindgen（编译产物形态，走 `poly bridge wasm`）、cffi（M3 待接入）、all",
    )
}

/// `poly bridge ohos`：UDL → C 胶水 → 各 ABI 自包含 NAPI 模块 `.so`。
///
/// 胶水由 T-M3-01 的 napi 出口产出（同一份 UDL，绝不手写第二份绑定）；模块 = 胶水 +
/// 核心 staticlib 一起链，单文件自包含（RNOH / ArkTS 只装载一个 `.so`）。
fn assemble_ohos(args: &OhosArgs, json: bool) -> Result<i32, Diagnostic> {
    let project = project::load(&args.config)?;
    let source = bridge::interface_source(&project).map_err(err)?;
    let namespace = bridge::namespace(&source).map_err(err)?;
    let abis = parse_ohos_abis(&args.abis)?;
    let sdk = bridge::napi::OhosSdk::discover().ok_or_else(missing_ohos_sdk)?;

    let mut events = project::sink(json);
    let output = bridge::napi::generate(&project, &source).map_err(err)?;
    let glue = match output.files.first() {
        Some(file) => output.dir.join(file),
        None => {
            return Err(Diagnostic::error(
                codes::E_BRIDGE_0003,
                "napi 生成未产出胶水文件（生成器异常）",
            )
            .with_hint("报告 bug：poly bridge generate --lang napi 应对任意合法 UDL 产出 .c"));
        }
    };
    let core_dir = project.root.join(&project.config.core.crate_path);
    let mut modules = Vec::new();
    for abi in &abis {
        events.emit(&poly_core::BuildEvent::Warning {
            stage: "bridge".to_string(),
            message: format!("交叉编译 {}（{}）…", abi.dir, abi.triple),
        });
        let lib = bridge::napi::build_lib(&core_dir, *abi, &sdk, events.as_mut()).map_err(err)?;
        let out_dir = project.root.join("bridge").join("harmonyos").join(abi.dir);
        let so = bridge::napi::build_module(
            &glue,
            &lib,
            &namespace,
            *abi,
            &sdk,
            &out_dir,
            events.as_mut(),
        )
        .map_err(err)?;
        modules.push(so);
    }
    report(
        json,
        serde_json::json!({
            "event": "bridge-ohos",
            "schema": bridge::BRIDGE_SCHEMA,
            "sdk": sdk.describe(),
            "glue": glue.to_string_lossy(),
            "modules": modules.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>(),
            "abi": abis.iter().map(|abi| abi.dir).collect::<Vec<_>>(),
        }),
    );
    Ok(0)
}

/// 解析 OHOS `--abi`（缺省 arm64-v8a：真机）。
fn parse_ohos_abis(values: &[String]) -> Result<Vec<bridge::napi::OhosAbi>, Diagnostic> {
    if values.is_empty() {
        return Ok(vec![bridge::napi::ABIS[0]]);
    }
    let mut abis = Vec::new();
    for value in values {
        let abi = bridge::napi::OhosAbi::parse(value).ok_or_else(|| {
            let choices: Vec<&str> = bridge::napi::ABIS.iter().map(|abi| abi.dir).collect();
            Diagnostic::error(codes::E_BRIDGE_0003, format!("未知的 ABI: {value}"))
                .with_hint(format!("可选：{}", choices.join(" / ")))
        })?;
        if !abis.contains(&abi) {
            abis.push(abi);
        }
    }
    Ok(abis)
}

/// OHOS SDK 缺失 → `poly::E_BRIDGE_0003` + 修复提示（skip-if-missing：不产半份模块）。
fn missing_ohos_sdk() -> Diagnostic {
    Diagnostic::error(
        codes::E_BRIDGE_0003,
        "未找到 HarmonyOS SDK（交叉编译 NAPI 模块 `.so` 的前提）",
    )
    .with_hint(
        "设 $HARMONYOS_TOOLS（command-line-tools 根）/ $DEVECO_SDK_HOME（DevEco 的 sdk 目录）\
         / $POLYPACK_OHOS_SDK（直接指向 …/sdk/default/openharmony）；poly doctor 可核对",
    )
}

/// 解析 `--abi`（缺省 arm64-v8a + x86_64：真机与模拟器/仿真器各一）。
fn parse_abis(values: &[String]) -> Result<Vec<aar::Abi>, Diagnostic> {
    if values.is_empty() {
        return Ok(vec![aar::ABIS[0], aar::ABIS[2]]);
    }
    let mut abis = Vec::new();
    for value in values {
        let abi = aar::Abi::parse(value).ok_or_else(|| {
            let choices: Vec<&str> = aar::ABIS.iter().map(|abi| abi.dir).collect();
            Diagnostic::error(codes::E_BRIDGE_0003, format!("未知的 ABI: {value}"))
                .with_hint(format!("可选：{}", choices.join(" / ")))
        })?;
        if !abis.contains(&abi) {
            abis.push(abi);
        }
    }
    Ok(abis)
}

/// 递归列出目录下的普通文件（相对路径，升序）。
fn list_files(dir: &std::path::Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        if !current.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&current)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
            } else {
                out.push(path.strip_prefix(dir).unwrap_or(&path).to_path_buf());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 输出报告：`--json` 单行 JSON（stdout 保持纯 JSON Lines），否则人读。
fn report(json: bool, value: serde_json::Value) {
    if json {
        println!("{}", serde_json::to_string(&value).expect("报告可序列化"));
        return;
    }
    let event = value["event"].as_str().unwrap_or("bridge");
    println!("poly bridge · {event}");
    for (key, item) in value.as_object().into_iter().flatten() {
        if key == "event" {
            continue;
        }
        println!("  {key}: {item}");
    }
}

/// `StageError` → 诊断。
fn err(error: poly_core::StageError) -> Diagnostic {
    error.diagnostic
}

/// 缓存错误 → 诊断。
fn cache_err(error: poly_cache::CacheError) -> Diagnostic {
    Diagnostic::error(codes::E_CACHE_0001, format!("缓存读写失败: {error}"))
        .with_hint("poly clean --all 后重试；或加 --no-cache")
}
