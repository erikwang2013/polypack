//! T-M2-01 / T-M2-02 / T-M3-01 验收（无工具面）：UDL 解析、接口摘要、缓存键、保留出口、
//! napi 胶水生成（不依赖引擎）、AAR 布局与二跑一致性、缓存命中不调引擎。
//!
//! 这一份**不需要** uniffi 引擎 / NDK / kotlinc：三宿主 CI 上必须全绿。真跑（真生成绑定、
//! 真交叉编译、真 kotlinc）在 `bridge_real.rs`，由 `POLYPACK_CONTRACT_REAL=1` 门控。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use polypack_cache::{Cache, Digest};
use polypack_core::bridge::{self, BridgeLang, aar, udl};
use polypack_core::config::PolypackConfig;
use polypack_core::events::HumanSink;
use polypack_core::plan::Project;
use polypack_core::{StageError, codes};

// ---------------------------------------------------------------- 测试支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-bridge-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
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

/// 示例接口：单处定义（核心 crate 的唯一 UDL）。
const UDL: &str = "\
// 演示接口：注释与声明都要被解析器正确绕过
namespace pax {
  string greet(string name);
  u32 add(u32 a, u32 b);
};
";

/// 建一个最小工程：`polypack.toml` 语义 + 核心 crate（UDL + Cargo.toml）。
fn fixture(root: &Path, udl: &str) -> Project {
    let core = root.join("core");
    std::fs::create_dir_all(core.join("src")).expect("建核心 crate");
    std::fs::write(core.join("src/pax.udl"), udl).expect("写 UDL");
    std::fs::write(
        core.join("Cargo.toml"),
        "[workspace]\n\n[package]\nname = \"pax-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("写核心 Cargo.toml");
    std::fs::write(core.join("Cargo.lock"), "# 夹具：锁文件只参与摘要\n").expect("写锁文件");
    project(root)
}

fn project(root: &Path) -> Project {
    let config = PolypackConfig::parse(
        "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("配置合法");
    Project::new(root.to_path_buf(), config)
}

/// 断言诊断码与提示（提示里必须给出可执行的下一步）。
fn assert_code(error: StageError, code: polypack_core::Code, hint_needle: &str) {
    assert_eq!(
        error.diagnostic.code, code,
        "诊断码：{}",
        error.diagnostic.message
    );
    let hint = error.diagnostic.hint.unwrap_or_default();
    assert!(
        hint.contains(hint_needle),
        "修复提示应含 {hint_needle:?}，实际：{hint}"
    );
}

/// 列出 zip 归档的条目名（升序）。
fn entry_names(aar: &Path) -> Vec<String> {
    let file = std::fs::File::open(aar).expect("打开 AAR");
    let mut zip = zip::ZipArchive::new(file).expect("AAR 是合法 zip");
    (0..zip.len())
        .map(|index| {
            zip.by_index(index)
                .expect("读条目")
                .name()
                .expect("条目名")
                .into_owned()
        })
        .collect()
}

/// 读归档中某个条目的字节。
fn entry_bytes(aar: &Path, name: &str) -> Vec<u8> {
    use std::io::Read;
    let file = std::fs::File::open(aar).expect("打开 AAR");
    let mut zip = zip::ZipArchive::new(file).expect("AAR 是合法 zip");
    let mut entry = zip.by_name(name).expect("条目存在");
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).expect("读条目");
    bytes
}

// ---------------------------------------------------------------- UDL 解析

#[test]
fn udl_parses_namespace_and_signatures() {
    let iface = udl::parse(UDL).expect("UDL 合法");
    assert_eq!(iface.namespace, "pax");
    assert_eq!(iface.functions.len(), 2);
    assert_eq!(iface.functions[0].name, "greet");
    assert_eq!(iface.functions[0].args, vec!["name".to_string()]);
    assert_eq!(iface.functions[0].ret, "string");
    assert_eq!(iface.functions[1].name, "add");
    assert_eq!(
        iface.functions[1].args,
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(iface.functions[1].ret, "u32");
}

#[test]
fn udl_skips_non_function_declarations() {
    let iface = udl::parse(
        "namespace demo {\n\
         dictionary Point { u32 x; u32 y; };\n\
         enum Color { \"red\", \"green\" };\n\
         callback OnDone = void (string reason);\n\
         string ping();\n\
         };\n",
    )
    .expect("UDL 合法");
    // 只有顶层函数进清单：dictionary / enum / callback 不该被误判为函数
    assert_eq!(
        iface
            .functions
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        vec!["ping"]
    );
}

#[test]
fn udl_without_namespace_is_rejected() {
    let err = udl::parse("string lonely();").expect_err("缺 namespace 必须报错");
    assert!(err.contains("namespace"), "报错应点明缺 namespace：{err}");
}

// ---------------------------------------------------------------- 接口摘要与缓存键

#[test]
fn interface_digest_is_deterministic_and_change_sensitive() {
    let temp = TempDir::new("digest");
    let project = fixture(temp.path(), UDL);
    let first = bridge::interface_source(&project).expect("找到 UDL");
    let digest = bridge::interface_digest(&first).expect("摘要");

    // 同输入同摘要（两跑之间不抖动）
    let again = bridge::interface_digest(&bridge::interface_source(&project).expect("找到 UDL"))
        .expect("摘要");
    assert_eq!(digest, again);

    // 接口改一个字 → 摘要变（缓存失效的判据）
    std::fs::write(
        project.root.join("core/src/pax.udl"),
        UDL.replace("greet", "greetz"),
    )
    .expect("改 UDL");
    let changed = bridge::interface_digest(&bridge::interface_source(&project).expect("找到 UDL"))
        .expect("摘要");
    assert_ne!(digest, changed, "接口变更必须改摘要");

    // 只改文件名（同内容）→ 摘要也变（路径参与摘要）
    std::fs::rename(
        project.root.join("core/src/pax.udl"),
        project.root.join("core/src/pax2.udl"),
    )
    .expect("改名");
    let renamed = bridge::interface_digest(&bridge::interface_source(&project).expect("找到 UDL"))
        .expect("摘要");
    assert_ne!(digest, renamed, "改名必须改摘要");
}

#[test]
fn bridge_cache_key_tracks_interface_and_engine() {
    let temp = TempDir::new("key");
    let project = fixture(temp.path(), UDL);
    let source = bridge::interface_source(&project).expect("找到 UDL");
    let interface = bridge::interface_digest(&source).expect("摘要");
    let kotlin = BridgeLang::Kotlin;
    let key = bridge::bridge_cache_key(&project, &interface, "local", kotlin).expect("键");

    // 同输入同键
    let same = bridge::bridge_cache_key(&project, &interface, "local", kotlin).expect("键");
    assert_eq!(key, same);

    // 接口摘要变 → 键变（T-M2-01 验收：接口变更进库级缓存键）
    let other = Digest::of_bytes(b"other-interface");
    let by_interface = bridge::bridge_cache_key(&project, &other, "local", kotlin).expect("键");
    assert_ne!(key, by_interface);

    // 换引擎（napi = polypack 自带的 C 胶水出口）→ 键变；引擎由语言推出，产物也各自独立
    let by_engine =
        bridge::bridge_cache_key(&project, &interface, "local", BridgeLang::Napi).expect("键");
    assert_ne!(key, by_engine);

    // 换 profile → 键变
    let by_profile = bridge::bridge_cache_key(&project, &interface, "ci", kotlin).expect("键");
    assert_ne!(key, by_profile);

    // 换语言 → 键变（各语言的绑定是独立产物）
    let by_lang =
        bridge::bridge_cache_key(&project, &interface, "local", BridgeLang::Swift).expect("键");
    assert_ne!(key, by_lang);

    // 逐语言清单与语言顺序一致
    let keys = bridge::bridge_cache_keys(
        &project,
        &interface,
        "local",
        &[BridgeLang::Kotlin, BridgeLang::Swift],
    )
    .expect("键清单");
    assert_eq!(
        keys.iter().map(|(lang, _)| *lang).collect::<Vec<_>>(),
        vec![kotlin, BridgeLang::Swift]
    );
    assert_eq!(keys[0].1, key);
}

#[test]
fn missing_interface_definition_is_reported() {
    let temp = TempDir::new("no-udl");
    let project = project(temp.path()); // 没有核心 crate / UDL
    assert_code(
        bridge::interface_source(&project).expect_err("缺 UDL 必须报错"),
        codes::E_BRIDGE_0002,
        "UDL",
    );
}

// ---------------------------------------------------------------- 保留出口（M3）

#[test]
fn reserved_exits_fail_before_engine_lookup() {
    let temp = TempDir::new("reserved");
    let project = fixture(temp.path(), UDL);
    let mut sink = HumanSink { out: Vec::new() };
    // 仅剩 Cffi 一个保留出口（Napi 于 T-M3-01 转正、Wasm 于 T-M3-07 走编译产物形态）
    let lang = BridgeLang::Cffi;
    let err = bridge::generate(&project, &[lang], &mut sink).expect_err("保留出口必须报错");
    assert_code(err, codes::E_BRIDGE_0002, "M3");
    assert!(
        !project.root.join("bridge").join(lang.dir()).exists(),
        "{} 不该产出目录",
        lang.dir()
    );
    // wasm 出口（T-M3-07）已接引擎但是**编译产物形态**：绑定流水线拒绝并指向 `polypack bridge wasm`
    //（避免在 `bridge/wasm-bindgen` 落半份绑定形态的产物）
    let err = bridge::generate(&project, &[BridgeLang::WasmBindgen], &mut sink)
        .expect_err("wasm 出口不走绑定流水线");
    assert_code(err, codes::E_BRIDGE_0002, "polypack bridge wasm");
    assert!(
        !project
            .root
            .join("bridge")
            .join(BridgeLang::WasmBindgen.dir())
            .exists(),
        "wasm 产物只由 `polypack bridge wasm` 落盘"
    );
    // 五端出口都登记在枚举上（M3 接引擎时不改接口定义）
    assert_eq!(BridgeLang::ALL.len(), 5);
    assert_eq!(
        BridgeLang::parse("wasm-bindgen"),
        Some(BridgeLang::WasmBindgen)
    );
    assert_eq!(BridgeLang::parse("cffi"), Some(BridgeLang::Cffi));
}

#[test]
fn missing_engine_is_reported_not_faked() {
    let temp = TempDir::new("no-engine");
    let project = fixture(temp.path(), UDL);
    // 夹具核心 crate 无 src/bin/uniffi-bindgen.rs（且测试进程的 PATH 里没有该工具）：
    // 这一条只在「引擎确实不在场」时断言，避免在装过 uniffi-bindgen 的机器上误红。
    let installed = std::env::var_os("POLYPACK_UNIFFI_BINDGEN").is_some()
        || uniffi_on_path().is_some()
        || project
            .root
            .join("core/src/bin/uniffi-bindgen.rs")
            .is_file();
    if installed {
        eprintln!("SKIP：本机已装 uniffi-bindgen（引擎在场，缺引擎分支无从触发）");
        return;
    }
    let mut sink = HumanSink { out: Vec::new() };
    let err = bridge::generate(&project, &[BridgeLang::Kotlin], &mut sink).expect_err("缺引擎报错");
    assert_code(err, codes::E_BRIDGE_0001, "uniffi");
    assert!(
        !project.root.join("bridge/kotlin").exists(),
        "缺引擎时不该留下空产物目录（更不该产假绑定）"
    );
}

/// PATH 上是否有 uniffi-bindgen（测试自身的 skip 判据，与生产逻辑同口径）。
fn uniffi_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .flat_map(|dir| ["uniffi-bindgen", "uniffi-bindgen.exe"].map(|name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

// ---------------------------------------------------------------- napi 出口（T-M3-01）

/// napi 出口：UDL 直出 C 胶水，**不需要 uniffi 引擎在场**；符号与装载入口对着
/// uniffi 0.32 的 FFI ABI 与 OHOS/node 的 NAPI 装载约定钉死。
#[test]
fn napi_exit_generates_glue_without_uniffi_engine() {
    let temp = TempDir::new("napi");
    let project = fixture(temp.path(), UDL);
    let mut sink = HumanSink { out: Vec::new() };
    let outputs = bridge::generate(&project, &[BridgeLang::Napi], &mut sink).expect("napi 生成");
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].lang, BridgeLang::Napi);
    assert_eq!(outputs[0].files, vec![PathBuf::from("pax.c")]);
    let glue = outputs[0].dir.join("pax.c");
    assert!(glue.is_file(), "胶水落点 {}", glue.display());

    let text = std::fs::read_to_string(&glue).expect("读胶水");
    for needle in [
        // OHOS 侧头（__OHOS__ 分支）与 node / RNOH 侧头
        "napi/native_api.h",
        "node_api.h",
        // uniffi 0.32 的 FFI 符号：前缀是归一化 crate 名（pax-core → pax_core），不是 namespace
        "uniffi_pax_core_fn_func_greet(",
        "uniffi_pax_core_fn_func_add(",
        "ffi_pax_core_rustbuffer_from_bytes(",
        // 两个装载入口：node/RNOH 查找入口 + ArkTS 的模块注册构造函数
        "napi_register_module_v1",
        "napi_module_register(&polypack_module)",
        // ArkTS 装载名 = UDL namespace
        ".nm_modname = \"pax\"",
    ] {
        assert!(text.contains(needle), "胶水缺 {needle}：\n{text}");
    }

    // 二跑字节一致（纯函数：输出只由接口定义决定，缓存键成立的前提）
    let first = std::fs::read(&glue).expect("读胶水");
    bridge::generate(&project, &[BridgeLang::Napi], &mut sink).expect("二跑");
    assert_eq!(
        first,
        std::fs::read(&glue).expect("再读"),
        "二跑必须字节一致"
    );

    // 出口登记：napi 已接入（不再是 Reserved），引擎是 polypack 自带的 C 胶水
    assert_eq!(BridgeLang::Napi.engine(), bridge::BridgeEngine::PolyNapi);
    assert_eq!(BridgeLang::Napi.engine().id(), "polypack-napi");
}

/// napi 的 v0 类型面之外（复合类型）→ `polypack::E_BRIDGE_0002`，且不落半份胶水。
#[test]
fn napi_exit_refuses_unsupported_types_without_partial_output() {
    let temp = TempDir::new("napi-unsupported");
    let project = fixture(
        temp.path(),
        "namespace pax {\n  sequence<u32> values();\n};\n",
    );
    let mut sink = HumanSink { out: Vec::new() };
    let err =
        bridge::generate(&project, &[BridgeLang::Napi], &mut sink).expect_err("复合类型必须拒绝");
    assert_code(err, codes::E_BRIDGE_0002, "sequence");
    assert!(
        !project.root.join("bridge/napi").exists(),
        "类型拒绝不产半份胶水（更不产假绑定）"
    );
}

// ---------------------------------------------------------------- AAR 组装（T-M2-02）

/// 假的 `.so` 载荷：内容随意，只验条目与一致性（真交叉编译见 `bridge_real.rs`）。
fn fake_libs(root: &Path) -> Vec<(aar::Abi, PathBuf)> {
    let libs = root.join("target");
    std::fs::create_dir_all(&libs).expect("建 libs 目录");
    aar::ABIS
        .into_iter()
        .map(|abi| {
            let path = libs.join(format!("libpax_core-{}.so", abi.dir));
            std::fs::write(&path, format!("ELF-ish payload for {}", abi.dir)).expect("写 .so");
            (abi, path)
        })
        .collect()
}

#[test]
fn aar_layout_is_the_consumer_contract() {
    let temp = TempDir::new("aar");
    let root = temp.path();
    let sources = root.join("bridge/kotlin");
    std::fs::create_dir_all(sources.join("uniffi/pax")).expect("建绑定源码目录");
    std::fs::write(sources.join("uniffi/pax/pax.kt"), "package uniffi.pax\n").expect("写 .kt");

    let out = root.join("bridge/android/pax_core.aar");
    let manifest = aar::manifest_xml("pax", aar::DEFAULT_MIN_SDK, "1.0");
    let libs = fake_libs(root);
    aar::assemble(&out, &manifest, &libs, None, &sources).expect("组装 AAR");

    // 消费方契约：条目名与 polypack-bundle 的 NativeLib.abi / dex_inputs 同域
    assert_eq!(
        entry_names(&out),
        vec![
            "AndroidManifest.xml",
            "bridge-sources/uniffi/pax/pax.kt",
            "jni/arm64-v8a/libpax_core-arm64-v8a.so",
            "jni/armeabi-v7a/libpax_core-armeabi-v7a.so",
            "jni/x86/libpax_core-x86.so",
            "jni/x86_64/libpax_core-x86_64.so",
        ]
    );
    // 分隔符不变式单列：Windows 上 `\` 条目名能被「照抄 CI 输出」修成假绿，这条盯死
    assert!(
        entry_names(&out).iter().all(|name| !name.contains('\\')),
        "zip 条目名恒用 `/` 分隔：{:?}",
        entry_names(&out)
    );
    let xml = String::from_utf8(entry_bytes(&out, "AndroidManifest.xml")).expect("清单是 UTF-8");
    assert!(xml.contains("package=\"pax\""), "{xml}");
    assert!(xml.contains("minSdkVersion=\"24\""), "{xml}");
}

#[test]
fn aar_bytes_are_reproducible_without_classes_jar() {
    let temp = TempDir::new("aar-repro");
    let root = temp.path();
    let libs = fake_libs(root);
    let manifest = aar::manifest_xml("pax", aar::DEFAULT_MIN_SDK, "1.0");
    let empty = root.join("no-sources");

    let first = root.join("a.aar");
    let second = root.join("b.aar");
    aar::assemble(&first, &manifest, &libs, None, &empty).expect("组装 A");
    aar::assemble(&second, &manifest, &libs, None, &empty).expect("组装 B");
    assert_eq!(
        std::fs::read(&first).expect("读 A"),
        std::fs::read(&second).expect("读 B"),
        "同输入二跑必须字节一致（缓存键的成立前提）"
    );
    // 无 classes.jar 时不伪造：条目里就是不出现它
    assert!(!entry_names(&first).iter().any(|name| name == "classes.jar"));
}

#[test]
fn aar_cache_key_tracks_abi_set() {
    let temp = TempDir::new("aar-key");
    let project = fixture(temp.path(), UDL);
    let source = bridge::interface_source(&project).expect("找到 UDL");
    let interface = bridge::interface_digest(&source).expect("摘要");

    let two = aar::cache_key(
        &project,
        &[aar::ABIS[0], aar::ABIS[2]],
        &interface,
        "ndk",
        "local",
    )
    .expect("键");
    let four = aar::cache_key(&project, &aar::ABIS, &interface, "ndk", "local").expect("键");
    assert_ne!(two, four, "ABI 集合是键的一部分（换 ABI 即重编）");

    let same = aar::cache_key(
        &project,
        &[aar::ABIS[0], aar::ABIS[2]],
        &interface,
        "ndk",
        "local",
    )
    .expect("键");
    assert_eq!(two, same, "同输入同键");
}

#[test]
fn abi_table_covers_four_android_abis() {
    assert_eq!(
        aar::ABIS.map(|abi| abi.dir),
        ["arm64-v8a", "armeabi-v7a", "x86_64", "x86"],
        "ABI 目录名与 polypack-bundle 的 NativeLib.abi 同域"
    );
    // 目录名与 triple 两种写法都能解析（--abi 参数）
    assert_eq!(aar::Abi::parse("arm64-v8a"), Some(aar::ABIS[0]));
    assert_eq!(aar::Abi::parse("aarch64-linux-android"), Some(aar::ABIS[0]));
    assert_eq!(aar::Abi::parse("armeabi-v7a"), Some(aar::ABIS[1]));
    assert_eq!(aar::Abi::parse("i686-linux-android"), Some(aar::ABIS[3]));
    assert_eq!(aar::Abi::parse("riscv64"), None);
    // armv7 的 clang 前缀与 rust triple 不同（NDK 侧写作 armv7a-…-androideabi）
    assert_eq!(aar::ABIS[1].clang, "armv7a-linux-androideabi");
}

// ---------------------------------------------------------------- 缓存

#[test]
fn cache_hit_restores_bindings_without_engine() {
    let temp = TempDir::new("cache");
    let root = temp.path();
    let project = fixture(root, UDL);
    let source = bridge::interface_source(&project).expect("找到 UDL");
    let interface = bridge::interface_digest(&source).expect("摘要");
    let keys = bridge::bridge_cache_keys(
        &project,
        &interface,
        "local",
        &[BridgeLang::Kotlin, BridgeLang::Swift],
    )
    .expect("键清单");

    // 预置：把「上一跑的产物树」放进缓存，然后删掉现场产物
    for (lang, _) in &keys {
        let generated = root.join("bridge").join(lang.dir()).join("fixture");
        std::fs::create_dir_all(&generated).expect("建产物目录");
        std::fs::write(generated.join("binding.txt"), lang.id()).expect("写产物");
    }
    let cache = Cache::from_root(root.join(".polypack/cache"));
    for (lang, key) in &keys {
        cache
            .store_tree(key, &root.join("bridge").join(lang.dir()))
            .expect("入库");
    }
    std::fs::remove_dir_all(root.join("bridge")).expect("清现场");

    // 命中：不调用引擎（测试环境根本没有引擎，能还原就说明没走生成）
    let mut sink = HumanSink { out: Vec::new() };
    let hit = bridge::restore_or_generate(&cache, &keys, &project, &mut sink).expect("还原");
    assert!(hit, "全部语言命中才算命中");
    for (lang, _) in &keys {
        assert!(
            root.join("bridge")
                .join(lang.dir())
                .join("fixture/binding.txt")
                .is_file(),
            "{} 的产物已还原",
            lang.id()
        );
    }
    let log = String::from_utf8_lossy(&sink.out).to_string();
    assert!(log.contains("命中"), "事件流应报告命中：{log}");
}

/// 半份缓存不是命中：缺语言的缓存条目必须触发重生成（缺引擎时如实报错）。
#[test]
fn partial_cache_is_not_a_hit() {
    let temp = TempDir::new("cache-partial");
    let root = temp.path();
    let project = fixture(root, UDL);
    let source = bridge::interface_source(&project).expect("找到 UDL");
    let interface = bridge::interface_digest(&source).expect("摘要");
    let keys = bridge::bridge_cache_keys(&project, &interface, "local", &[BridgeLang::Kotlin])
        .expect("键清单");

    let generated = root.join("bridge/kotlin");
    std::fs::create_dir_all(&generated).expect("建产物目录");
    std::fs::write(generated.join("binding.txt"), "kotlin").expect("写产物");
    let cache = Cache::from_root(root.join(".polypack/cache"));
    cache.store_tree(&keys[0].1, &generated).expect("入库");
    std::fs::remove_dir_all(root.join("bridge")).expect("清现场");

    // 缓存里只有 Kotlin，却要 Kotlin + Swift：必须走生成（本环境无引擎 → 如实报错）
    let full = bridge::bridge_cache_keys(
        &project,
        &interface,
        "local",
        &[BridgeLang::Kotlin, BridgeLang::Swift],
    )
    .expect("键清单");
    let mut sink = HumanSink { out: Vec::new() };
    let err = bridge::restore_or_generate(&cache, &full, &project, &mut sink)
        .expect_err("半份缓存不得冒充命中");
    assert!(
        err.diagnostic.message.contains("uniffi"),
        "应报缺引擎：{}",
        err.diagnostic.message
    );
    let log = String::from_utf8_lossy(&sink.out).to_string();
    assert!(log.contains("整体重生成"), "事件流应说明只命中一半：{log}");
}
