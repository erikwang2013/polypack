//! T-M3-07 验收（无工具面）：wasm 沙箱白名单（生成期拒绝 + 加载期拒绝）、shim / 清单 /
//! 适配层生成物的形态与确定性、缓存键，以及 Node 模拟小程序沙箱里的真执行与负例。
//!
//! 这一份**不需要** rustup wasm target / wasm-bindgen CLI（白名单闸、生成物形态、缓存键都不
//! 碰工具链）；Node 沙箱用例只在 `node` 不在 PATH 时 SKIP（三宿主 CI 都有）。真跑（真 cargo
//! wasm32 编译 + 真 wasm-bindgen CLI + 真沙箱执行产物）在 `wasm_real.rs`，由
//! `POLY_CONTRACT_REAL=1` 门控。
//!
//! 沙箱模拟的判据形态：负例里的 wasm 字节是**手工拼的二进制**（无 wasm 工具也能跑），
//! 胶水是手写的最小复刻（模块求值即构造 TextEncoder/TextDecoder——复刻 wasm-bindgen 的
//! no-modules 胶水行为，无 typeof 守卫）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use poly_core::bridge::{self, udl, wasm};
use poly_core::config::PolypackConfig;
use poly_core::events::HumanSink;
use poly_core::plan::Project;
use poly_core::{StageError, codes};

// ---------------------------------------------------------------- 测试支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-wasm-{tag}-{}-{}",
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

/// 示例接口：沙箱子集内的两个函数（与 bridge_real.rs 的真跑夹具同形）。
const UDL: &str = "\
namespace pax {
  string greet(string name);
  u32 add(u32 a, u32 b);
};
";

/// 建一个最小工程：`polypack.toml` 语义 + 核心 crate（UDL + Cargo.toml）。
fn fixture(root: &Path, text: &str) -> Project {
    let core = root.join("core");
    std::fs::create_dir_all(core.join("src")).expect("建核心 crate");
    std::fs::write(core.join("src/pax.udl"), text).expect("写 UDL");
    std::fs::write(
        core.join("Cargo.toml"),
        "[workspace]\n\n[package]\nname = \"pax-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("写核心 Cargo.toml");
    let config = PolypackConfig::parse(
        "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("配置合法");
    Project {
        root: root.to_path_buf(),
        config,
    }
}

/// 解析一份独立 UDL（沙箱用例不建整个工程）。
fn parse_udl(root: &Path, text: &str) -> udl::Interface {
    let file = root.join("pax.udl");
    std::fs::write(&file, text).expect("写 UDL");
    udl::parse_source(&file).expect("UDL 可解析")
}

/// 未裁剪的体积元数据（形态与 `wasm_opt::skipped` 一致：`applied=false` 时 before == after）。
fn untrimmed_size(bytes: u64) -> wasm::SizeInfo {
    wasm::SizeInfo {
        wasm_bytes: bytes,
        wasm_opt: wasm::WasmOptInfo {
            applied: false,
            tool: None,
            args: "-Oz".to_string(),
            before: bytes,
            after: bytes,
            note: Some("测试：未裁剪".to_string()),
        },
    }
}

/// 断言诊断码与提示（提示里必须给出可执行的下一步）。
fn assert_code(error: StageError, code: poly_core::Code, hint_needle: &str) {
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

// ---------------------------------------------------------------- 白名单（生成期闸）

#[test]
fn whitelist_rejects_types_outside_sandbox_subset() {
    let temp = TempDir::new("whitelist");
    // (UDL, 越界函数名, 越界类型原文)——四类：i64 返回、u64 参数、sequence、对象
    let cases: [(&str, &str, &str); 4] = [
        ("namespace pax { i64 tick(); };", "tick", "i64"),
        ("namespace pax { u64 count(u64 n); };", "count", "u64"),
        (
            "namespace pax { sequence<u8> blob(); };",
            "blob",
            "sequence<u8>",
        ),
        (
            "namespace pax { u64 pick(demo::Config c); };",
            "pick",
            "demo::Config",
        ),
    ];
    for (text, function, ty) in cases {
        let interface = parse_udl(temp.path(), text);
        let err = wasm::check_whitelist(&interface).expect_err("越界类型必须拒绝");
        assert_eq!(err.diagnostic.code, codes::E_BRIDGE_0004);
        assert!(
            err.diagnostic.message.contains(function) && err.diagnostic.message.contains(ty),
            "报文需点名函数与类型：{}",
            err.diagnostic.message
        );
        // 提示给出沙箱子集原文与替代路径（改走 native 出口）
        assert_code(err, codes::E_BRIDGE_0004, "boolean");
    }

    // 全流程口径：白名单闸在工具链之前，拒绝即零产物（不装 wasm-bindgen 的机器上也确定）
    let project = fixture(temp.path(), "namespace pax { u64 tick(); };");
    let out = temp.path().join("bridge/wasm-bindgen");
    let mut sink = HumanSink { out: Vec::new() };
    let err = wasm::generate(&project, &out, &mut sink).expect_err("越界类型必须拒绝");
    assert_code(err, codes::E_BRIDGE_0004, "native");
    assert!(!out.exists(), "拒绝时不留半份产物");
}

#[test]
fn whitelist_accepts_sandbox_subset_including_boolean() {
    let temp = TempDir::new("whitelist-ok");
    let interface = parse_udl(
        temp.path(),
        "namespace pax {\n\
           boolean flag(u32 x);\n\
           string greet(string name);\n\
           f64 ratio(f64 a, f64 b);\n\
           void reset();\n\
         };",
    );
    wasm::check_whitelist(&interface).expect("沙箱子集内的类型必须放行");
    // shim 的类型翻译：boolean → bool（uniffi UDL 的布尔拼写是 boolean）、string → String、void 无返回
    let shim = wasm::shim_lib_rs(&interface, "pax_core");
    assert!(
        shim.contains("pub fn flag(x: u32) -> bool {\n    pax_core::flag(x)\n}"),
        "{shim}"
    );
    assert!(
        shim.contains("pub fn greet(name: String) -> String {\n    pax_core::greet(name)\n}"),
        "{shim}"
    );
    assert!(
        shim.contains("pub fn reset() {\n    pax_core::reset()\n}"),
        "{shim}"
    );
}

#[test]
fn udl_arg_types_are_parsed_parallel_to_args() {
    let temp = TempDir::new("udl-argtypes");
    let interface = parse_udl(
        temp.path(),
        "namespace pax {\n\
           u32 add(u32 a, u32 b);\n\
           string join(string left, string right = \"x\");\n\
         };",
    );
    let add = &interface.functions[0];
    assert_eq!(add.args, ["a", "b"]);
    assert_eq!(add.arg_types, ["u32", "u32"]);
    // 默认值不参与类型 / 名字（白名单按类型判，默认值由调用方在 JS 侧给）
    let join = &interface.functions[1];
    assert_eq!(join.args, ["left", "right"]);
    assert_eq!(join.arg_types, ["string", "string"]);
    assert_eq!(join.ret, "string");
}

// ---------------------------------------------------------------- 生成物形态（shim / 清单 / 适配层）

#[test]
fn shim_manifest_pins_wasm_bindgen_and_path_dep() {
    let toml = wasm::shim_manifest_toml(Path::new("/tmp/core-dir"), "pax_core_wasm", "pax-core");
    assert!(
        toml.contains(&format!(
            "wasm-bindgen = \"={}\"",
            wasm::WASM_BINDGEN_VERSION
        )),
        "crate 版本必须钉死（CLI 与 crate 版本错配的报错很难定位）：{toml}"
    );
    assert!(
        toml.contains("pax-core = { path = \"/tmp/core-dir\" }"),
        "{toml}"
    );
    assert!(toml.contains("[workspace]"), "shim 自成 workspace：{toml}");
    // Windows 反斜杠在 TOML 字符串里是转义 → 生成期换成 `/`
    let win = wasm::shim_manifest_toml(Path::new(r"C:\core\dir"), "pax_core_wasm", "pax-core");
    assert!(win.contains("path = \"C:/core/dir\""), "{win}");
}

#[test]
fn manifest_and_loader_are_deterministic() {
    let temp = TempDir::new("manifest");
    let interface = parse_udl(temp.path(), UDL);
    let engine = format!("wasm-bindgen@{}", wasm::WASM_BINDGEN_VERSION);
    let manifest = wasm::manifest_for(&interface, "pax_core_wasm", &engine, untrimmed_size(4321));
    assert_eq!(
        manifest,
        wasm::manifest_for(&interface, "pax_core_wasm", &engine, untrimmed_size(4321))
    );
    assert_eq!(manifest.schema, wasm::SANDBOX_SCHEMA);
    assert_eq!(manifest.bridge, bridge::BRIDGE_SCHEMA);
    assert_eq!(manifest.glue, "./pax_core_wasm.js");
    assert_eq!(manifest.wasm, "./pax_core_wasm_bg.wasm");
    assert_eq!(manifest.import_module, "./pax_core_wasm_bg.js");
    // 清单是随包产物的对外接口面：JSON 形态稳定（尾换行 + 字段序 = 声明序）
    let json = wasm::manifest_json(&manifest);
    assert!(json.ends_with("\n"), "{json}");
    assert_eq!(json, wasm::manifest_json(&manifest));
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("清单是合法 JSON");
    assert_eq!(parsed["schema"], wasm::SANDBOX_SCHEMA);
    assert_eq!(parsed["engine"], engine);
    assert_eq!(parsed["functions"][0]["name"], "greet");
    assert_eq!(parsed["functions"][0]["args"][0]["name"], "name");
    assert_eq!(parsed["functions"][0]["args"][0]["type"], "string");
    assert_eq!(parsed["functions"][1]["name"], "add");
    // 体积元数据（T-M3-08）：wasm 落盘字节 + wasm-opt 前后对比随清单进产物
    assert_eq!(parsed["size"]["wasm_bytes"], 4321);
    assert_eq!(parsed["size"]["wasm_opt"]["applied"], false);
    assert_eq!(parsed["size"]["wasm_opt"]["before"], 4321);
    assert_eq!(parsed["size"]["wasm_opt"]["after"], 4321);
    assert_eq!(parsed["size"]["wasm_opt"]["args"], "-Oz");
    // 适配层：占位符一定被替换（留占位符 = 产物是死的），且二跑字节一致
    let loader = wasm::loader_js(&manifest);
    assert!(
        !loader.contains("__POLYPACK_MANIFEST__"),
        "清单必须已嵌入适配层"
    );
    assert!(loader.contains(r#""greet""#), "{loader}");
    assert_eq!(loader, wasm::loader_js(&manifest));
}

#[test]
fn manifest_size_metadata_round_trips_through_read_size() {
    let temp = TempDir::new("manifest-size");
    let interface = parse_udl(temp.path(), UDL);
    let engine = format!("wasm-bindgen@{}", wasm::WASM_BINDGEN_VERSION);
    let size = wasm::SizeInfo {
        wasm_bytes: 700,
        wasm_opt: wasm::WasmOptInfo {
            applied: true,
            tool: Some("wasm-opt@133".to_string()),
            args: "-Oz".to_string(),
            before: 1000,
            after: 700,
            note: None,
        },
    };
    let manifest = wasm::manifest_for(&interface, "pax_core_wasm", &engine, size.clone());
    std::fs::write(
        temp.path().join(wasm::MANIFEST_FILE),
        wasm::manifest_json(&manifest),
    )
    .expect("写清单");

    // 缓存命中路径（poly-cli 只拿到还原的目录）：凭 manifest.json 回读体积报告
    assert_eq!(bridge::wasm_opt::read_size(temp.path()), Some(size));
    // 旧清单（无 size 块）→ None：不伪造体积报告，也不报错
    std::fs::write(
        temp.path().join(wasm::MANIFEST_FILE),
        "{\"schema\":\"wasm-sandbox/v1\"}\n",
    )
    .expect("写旧清单");
    assert_eq!(bridge::wasm_opt::read_size(temp.path()), None);
    assert_eq!(
        bridge::wasm_opt::read_size(&temp.path().join("absent")),
        None
    );
}

#[test]
fn wasm_cache_key_tracks_interface_engine_and_profile() {
    let temp = TempDir::new("wasm-key");
    let project = fixture(temp.path(), UDL);
    let digest = |project: &Project| {
        let source = bridge::interface_source(project).expect("接口定义可定位");
        bridge::interface_digest(&source).expect("接口摘要")
    };
    let key = |project: &Project, digest: &poly_cache::Digest, engine: &str, profile: &str| {
        wasm::cache_key(project, digest, engine, profile).expect("缓存键")
    };
    let interface = digest(&project);
    let base = key(&project, &interface, "wasm-bindgen@0.2.129", "release");
    // 引擎（CLI 版本）与 profile 都进键：换版本 / 换 profile 即失效
    assert_ne!(
        base,
        key(&project, &interface, "wasm-bindgen@0.2.130", "release")
    );
    assert_ne!(
        base,
        key(&project, &interface, "wasm-bindgen@0.2.129", "debug")
    );
    // 接口改一个字 → 键变（与 Kotlin / Swift 出口同一判据：接口变更即库级缓存失效）
    std::fs::write(
        project.root.join("core/src/pax.udl"),
        "namespace pax {\n  string greet(string who);\n  u32 add(u32 a, u32 b);\n};\n",
    )
    .expect("改 UDL");
    assert_ne!(
        base,
        key(
            &project,
            &digest(&project),
            "wasm-bindgen@0.2.129",
            "release"
        )
    );
}

// ---------------------------------------------------------------- Node 模拟沙箱（加载期闸）

/// Node 不在 PATH → 相关用例 SKIP（不红）。
fn node_present() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

fn harness_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm_sandbox/harness.js")
}

/// 在 Node 模拟沙箱里加载 `dist` 的适配层 + 胶水 + wasm，按 `calls` 调用并回读报告 JSON。
fn run_sandbox(dist: &Path, wasm: &Path, calls: &serde_json::Value) -> serde_json::Value {
    let output = Command::new("node")
        .arg(harness_path())
        .arg(dist)
        .arg(wasm)
        .arg(calls.to_string())
        .output()
        .expect("node 可启动");
    assert!(
        output.status.success(),
        "harness 非零退出：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_else(|| {
            panic!(
                "harness 无输出（stderr：{}）",
                String::from_utf8_lossy(&output.stderr)
            )
        });
    serde_json::from_str(line)
        .unwrap_or_else(|err| panic!("harness 输出非 JSON（{err}）：{stdout}"))
}

/// 手工拼的最小 wasm 模块：只导出 `add(i32, i32) -> i32`。`import_env` 时额外声明
/// `env.x` 导入（负例：产物带原生模块依赖）。
fn wasm_module(import_env: bool) -> Vec<u8> {
    let mut module = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    // type 段：(i32, i32) -> i32
    module.extend_from_slice(&[0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f]);
    if import_env {
        // import 段："env" "x"（func，type 0）；段序 type < import
        module.extend_from_slice(&[
            0x02, 0x09, 0x01, 0x03, b'e', b'n', b'v', 0x01, b'x', 0x00, 0x00,
        ]);
    }
    // function 段：1 个函数用 type 0
    module.extend_from_slice(&[0x03, 0x02, 0x01, 0x00]);
    // export 段："add"（有导入时函数索引让位给导入项）
    let index = if import_env { 0x01 } else { 0x00 };
    module.extend_from_slice(&[0x07, 0x07, 0x01, 0x03, b'a', b'd', b'd', 0x00, index]);
    // code 段：return a + b
    module.extend_from_slice(&[
        0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
    ]);
    module
}

/// 手写胶水：复刻 wasm-bindgen `--target no-modules` 的形态——模块求值即构造
/// TextEncoder / TextDecoder（无 typeof 守卫，适配层的垫片必须更早安装），
/// `initSync({ module })` 接 WebAssembly.Module。
const FAKE_GLUE: &str = r#""use strict";
const __encoder = new TextEncoder();
const __decoder = new TextDecoder("utf-8", { ignoreBOM: true });
let wasm;
function greet(name) { return "hello, " + __decoder.decode(__encoder.encode(name)); }
function add(a, b) { return wasm.add(a, b); }
function initSync(init) { wasm = new WebAssembly.Instance(init.module, {}).exports; }
if (typeof module !== "undefined" && module.exports) { module.exports = { greet: greet, add: add, initSync: initSync }; }
"#;

const FAKE_GLUE_EXPORT: &str = "module.exports = { greet: greet, add: add, initSync: initSync };";

/// 写一份随包产物目录（适配层 + 胶水 + wasm），返回 wasm 文件路径（harness 需要它）。
fn write_dist(dist: &Path, manifest: &wasm::Manifest, glue: &str, wasm: &[u8]) -> PathBuf {
    std::fs::create_dir_all(dist).expect("建产物目录");
    std::fs::write(dist.join(wasm::LOADER_FILE), wasm::loader_js(manifest)).expect("写适配层");
    std::fs::write(dist.join(&manifest.glue[2..]), glue).expect("写胶水");
    let wasm_file = dist.join(&manifest.wasm[2..]);
    std::fs::write(&wasm_file, wasm).expect("写 wasm");
    wasm_file
}

fn sandbox_manifest(root: &Path) -> wasm::Manifest {
    let interface = parse_udl(root, UDL);
    // 体积元数据取手拼模块的真实字节（沙箱用例不跑 wasm-opt：形态与真跑一致即可）
    wasm::manifest_for(
        &interface,
        "pax_core_wasm",
        &format!("wasm-bindgen@{}", wasm::WASM_BINDGEN_VERSION),
        untrimmed_size(wasm_module(false).len() as u64),
    )
}

#[test]
fn sandbox_runs_whitelisted_functions_without_textcodecs() {
    if !node_present() {
        eprintln!("SKIP：node 不在 PATH（wasm 沙箱执行用例）");
        return;
    }
    let temp = TempDir::new("sandbox-run");
    let manifest = sandbox_manifest(temp.path());
    let dist = temp.path().join("dist");
    let wasm_file = write_dist(&dist, &manifest, FAKE_GLUE, &wasm_module(false));

    let report = run_sandbox(
        &dist,
        &wasm_file,
        &serde_json::json!([
            {"fn": "greet", "args": ["世界🚀"]},
            {"fn": "add", "args": [2, 3]},
        ]),
    );
    assert_eq!(report["ok"], true, "沙箱执行失败：{}", report["error"]);
    assert_eq!(
        report["results"][0], "hello, 世界🚀",
        "多字节 / 代理对经垫片往返回环"
    );
    assert_eq!(report["results"][1], 5, "wasm 真执行了 add(2,3)");
    // 对外只暴露白名单调用面（顺序 = UDL 声明序）
    assert_eq!(report["api_keys"], serde_json::json!(["greet", "add"]));
    assert_eq!(report["functions"], serde_json::json!(["greet", "add"]));
    // 小程序基础库缺 TextEncoder / TextDecoder：适配层垫片必须顶上（且先于胶水求值）
    assert_eq!(report["sandbox"]["text_encoder_before"], "undefined");
    assert_eq!(report["sandbox"]["text_decoder_before"], "undefined");
    assert_eq!(report["sandbox"]["text_encoder_after"], "function");
    // 沙箱里没有 process / fetch：产物不依赖宿主原生能力
    assert_eq!(report["sandbox"]["process"], "undefined");
    assert_eq!(report["sandbox"]["fetch"], "undefined");
}

#[test]
fn sandbox_rejects_glue_export_outside_whitelist() {
    if !node_present() {
        eprintln!("SKIP：node 不在 PATH（白名单负例）");
        return;
    }
    let temp = TempDir::new("sandbox-extra");
    let manifest = sandbox_manifest(temp.path());
    let dist = temp.path().join("dist");
    let glue = FAKE_GLUE.replace(
        FAKE_GLUE_EXPORT,
        "module.exports = { greet: greet, add: add, initSync: initSync, sneaky: function (x) { return x; } };",
    );
    assert!(glue.contains("sneaky"), "负例胶水必须带上白名单外接口");
    let wasm_file = write_dist(&dist, &manifest, &glue, &wasm_module(false));

    let report = run_sandbox(&dist, &wasm_file, &serde_json::json!([]));
    assert_eq!(report["ok"], false, "白名单外接口必须拒绝：{report}");
    let error = report["error"].as_str().unwrap_or_default();
    assert!(error.contains("白名单外接口 sneaky"), "{error}");
}

#[test]
fn sandbox_rejects_native_module_import() {
    if !node_present() {
        eprintln!("SKIP：node 不在 PATH（无原生依赖负例）");
        return;
    }
    let temp = TempDir::new("sandbox-native");
    let manifest = sandbox_manifest(temp.path());
    let dist = temp.path().join("dist");
    // wasm 里声明 `env.x` 导入 = 产物依赖宿主原生模块（小程序沙箱不提供）
    let wasm_file = write_dist(&dist, &manifest, FAKE_GLUE, &wasm_module(true));

    let report = run_sandbox(&dist, &wasm_file, &serde_json::json!([]));
    assert_eq!(report["ok"], false, "原生模块导入必须拒绝：{report}");
    let error = report["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("产物导入了非沙箱模块 env"),
        "报文需点名越界的模块：{error}"
    );
}

#[test]
fn sandbox_rejects_missing_whitelisted_function() {
    if !node_present() {
        eprintln!("SKIP：node 不在 PATH（清单缺函数负例）");
        return;
    }
    let temp = TempDir::new("sandbox-missing");
    let manifest = sandbox_manifest(temp.path());
    let dist = temp.path().join("dist");
    let glue = FAKE_GLUE.replace(
        FAKE_GLUE_EXPORT,
        "module.exports = { add: add, initSync: initSync };",
    );
    assert!(!glue.contains("greet: greet"), "负例胶水必须缺 greet");
    let wasm_file = write_dist(&dist, &manifest, &glue, &wasm_module(false));

    let report = run_sandbox(&dist, &wasm_file, &serde_json::json!([]));
    assert_eq!(report["ok"], false, "白名单函数缺失必须拒绝：{report}");
    let error = report["error"].as_str().unwrap_or_default();
    assert!(error.contains("白名单接口 greet 在产物中缺失"), "{error}");
}
