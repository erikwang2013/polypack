//! T-M3-07 真跑（`POLY_CONTRACT_REAL=1`）：真 cargo 编 wasm32 → 真 wasm-bindgen CLI 产胶水 +
//! 适配层 → 真在 Node 模拟的小程序沙箱里执行核心接口（含「无原生模块依赖」的导入表证据与
//! 库级缓存往返——CLI 的 `cached=true` 路径就是它）。
//!
//! 默认（mock 模式）/ 缺工具一律打印 SKIP 原因后返回，绝不红——与 M1/M2 既有契约测试同一纪律。
//! 无工具面的结构断言（白名单 / shim / 清单 / 适配层 / Node 加载期负例）在 `wasm.rs`，每宿主都跑。
//!
//! 夹具复用 `tests/fixtures/bridge_core/`（真 uniffi 核心库：greet / add），真跑把它复制到临时
//! 目录，产物绝不落进仓库树。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use poly_cache::Cache;
use poly_core::bridge::{self, wasm};
use poly_core::config::PolypackConfig;
use poly_core::events::HumanSink;
use poly_core::plan::Project;

/// 夹具核心库基础名（= `tests/fixtures/bridge_core` 的 `[lib] name`）。
const LIB: &str = "pax_core";
/// UDL 里声明的两个函数（真产物 ↔ UDL ↔ 沙箱执行三处都必须一致）。
const FUNCS: [&str; 2] = ["greet", "add"];

// ---------------------------------------------------------------- 支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时工程根；drop 即递归删除。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-wasm-real-{tag}-{}-{}",
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
        // CI 留产物通道：`POLY_KEEP_TEMP=1` 时保留（wasm-real 的 upload-artifact 消费）。
        if std::env::var("POLY_KEEP_TEMP").as_deref() == Ok("1") {
            eprintln!("POLY_KEEP_TEMP=1：保留 {}", self.0.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 真跑开关：`POLY_CONTRACT_REAL=1`（或 `true`）；默认 mock。
fn real_mode() -> bool {
    matches!(
        std::env::var("POLY_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// 前置不满足 → 打印原因并返回 false（SKIP 纪律：缺工具不红）。
fn gate(what: &str) -> bool {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真跑设 POLY_CONTRACT_REAL=1（{what}）");
        return false;
    }
    true
}

/// 把夹具复制成临时工程（真跑产物不落仓库树）。
fn staged_project(tag: &str) -> (TempDir, Project) {
    let temp = TempDir::new(tag);
    copy_tree(&fixture_dir(), &temp.path().join("core"));
    let config = PolypackConfig::parse(
        "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("配置合法");
    let project = Project::new(temp.path().to_path_buf(), config);
    (temp, project)
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bridge_core")
}

/// 递归复制目录（夹具只有几个小文件）。
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("建目标目录");
    for entry in std::fs::read_dir(from).expect("读夹具目录").flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().expect("文件类型").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("复制夹具文件");
        }
    }
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("读 {} 失败: {err}", path.display()))
}

/// Node 不在 PATH → SKIP（不红）。
fn node_present() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// rustup 装了 wasm32-unknown-unknown → 真跑前置；没装即 SKIP（不红）。
fn wasm_target_installed() -> bool {
    Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .is_ok_and(|out| {
            out.status.success()
                && String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .any(|line| line.trim() == wasm::WASM_TRIPLE)
        })
}

/// 在 Node 模拟的小程序沙箱里加载产物目录并调用指定接口（夹具见 `wasm_sandbox/harness.js`）。
fn run_sandbox(dist: &Path, wasm_file: &Path, calls: &serde_json::Value) -> serde_json::Value {
    let harness =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm_sandbox/harness.js");
    let output = Command::new("node")
        .arg(harness)
        .arg(dist)
        .arg(wasm_file)
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
        .unwrap_or_else(|| panic!("harness 无输出：{stdout}"));
    serde_json::from_str(line)
        .unwrap_or_else(|err| panic!("harness 输出非 JSON（{err}）：{stdout}"))
}

// ---------------------------------------------------------------- 真跑

#[test]
fn wasm_pipeline_runs_in_node_sandbox() {
    if !gate("wasm 沙箱真跑（真编 wasm32 + 真 wasm-bindgen + 真沙箱执行）") {
        return;
    }
    if wasm::WasmBindgen::discover().is_none() {
        eprintln!(
            "SKIP：未找到 wasm-bindgen CLI（$POLYPACK_WASM_BINDGEN / PATH / ~/.cache/polypack/tools/wasm-bindgen）"
        );
        return;
    }
    if !node_present() {
        eprintln!("SKIP：node 不在 PATH（沙箱执行无宿主）");
        return;
    }
    if !wasm_target_installed() {
        eprintln!(
            "SKIP：未装 {}（rustup target add {})",
            wasm::WASM_TRIPLE,
            wasm::WASM_TRIPLE
        );
        return;
    }

    let (temp, project) = staged_project("pipeline");
    let out = temp.path().join("bridge/wasm-bindgen");
    let mut sink = HumanSink { out: Vec::new() };
    let product = wasm::generate(&project, &out, &mut sink).expect("wasm 出口真跑必须成功");

    // —— 产物形态：随代码包分发的四个文件（顺序 = list_files 的升序）
    let names: Vec<String> = product
        .files
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(
        names,
        [
            "manifest.json".to_string(),
            format!("{LIB}_wasm.js"),
            format!("{LIB}_wasm_bg.wasm"),
            "polypack-loader.js".to_string(),
        ]
    );
    // 清单 ↔ UDL（单处接口定义）：真产物的接口面与 UDL 声明一致
    let declared: Vec<&str> = product
        .manifest
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect();
    assert_eq!(declared, FUNCS);
    assert_eq!(product.manifest.functions[0].ret, "string");
    assert_eq!(product.manifest.functions[0].args[0].ty, "string");
    assert_eq!(product.manifest.functions[1].ret, "u32");
    assert_eq!(
        product.manifest.engine,
        format!("wasm-bindgen@{}", wasm::WASM_BINDGEN_VERSION)
    );
    // 胶水尾附 CJS 出口（小程序模块系统 require 的入口）
    let glue = read_text(&out.join(format!("{LIB}_wasm.js")));
    assert!(
        glue.contains("module.exports = wasm_bindgen;"),
        "胶水需带 CJS 出口（poly bridge wasm 追加）"
    );
    // wasm 是魔数开头的真二进制；体积记账（T-M3-08 的预算检查基线）
    let wasm_bytes = std::fs::read(out.join(format!("{LIB}_wasm_bg.wasm"))).expect("读 wasm");
    assert_eq!(&wasm_bytes[..4], b"\0asm");
    let loader = read_text(&out.join(wasm::LOADER_FILE));
    eprintln!(
        "证据：wasm {}B / 胶水 {}B / 适配层 {}B / 清单 {}B → {}",
        wasm_bytes.len(),
        glue.len(),
        loader.len(),
        read_text(&out.join(wasm::MANIFEST_FILE)).len(),
        out.display()
    );

    // —— 体积元数据（T-M3-08）：裁剪时机 = 出口目录生成后、随包拷贝前，随包字节即裁剪后字节
    let size = product.manifest.size.clone();
    match wasm::WasmOpt::discover() {
        Some(tool) => {
            // 真裁剪：applied 且有下降；工具标识带版本（缓存键与报告同源）
            assert!(
                size.wasm_opt.applied,
                "wasm-opt 在场时必须真裁剪（applied=false 的 note：{:?}）",
                size.wasm_opt.note
            );
            let fingerprint = tool.fingerprint();
            assert_eq!(
                size.wasm_opt.tool.as_deref(),
                Some(fingerprint.as_str()),
                "元数据里的工具标识 = 现场指纹"
            );
            assert!(
                size.wasm_opt.before > size.wasm_opt.after,
                "真裁剪必须体积下降：{} → {}",
                size.wasm_opt.before,
                size.wasm_opt.after
            );
            eprintln!(
                "证据：wasm-opt 裁剪 {} → {} 字节（-{}%，{}）",
                size.wasm_opt.before,
                size.wasm_opt.after,
                (size.wasm_opt.before - size.wasm_opt.after) * 100 / size.wasm_opt.before,
                fingerprint
            );
        }
        None => {
            // 工具缺席时元数据如实记「未裁」，绝不静默（真实裁剪腿在装了 wasm-opt 的机器上跑）
            eprintln!("NOTE：本机无 wasm-opt——体积元数据如实记 applied=false");
            assert!(!size.wasm_opt.applied);
            assert_eq!(size.wasm_opt.before, size.wasm_opt.after);
            assert!(size.wasm_opt.note.is_some(), "未裁必须写明原因");
        }
    }
    // 元数据 ↔ 落盘字节：随包（出口目录）里的 wasm 就是裁剪后的那一份
    assert_eq!(
        size.wasm_bytes,
        wasm_bytes.len() as u64,
        "wasm_bytes = 落盘字节"
    );
    assert_eq!(size.wasm_opt.after, size.wasm_bytes, "after = 随包字节");
    assert_eq!(size.wasm_opt.args, "-Oz", "裁剪参数如实入清单");
    // 回读路径（CLI 缓存命中的同一条）：清单里的体积块可反序列化且与内存一致
    assert_eq!(
        bridge::wasm_opt::read_size(&out).as_ref(),
        Some(&size),
        "清单里的体积元数据可回读（缓存命中不重算）"
    );

    // —— 真沙箱执行：Node 模拟小程序沙箱（无 TextEncoder/Decoder、无 process/fs/fetch）
    let report = run_sandbox(
        &out,
        &out.join(format!("{LIB}_wasm_bg.wasm")),
        &serde_json::json!([
            {"fn": "greet", "args": ["world"]},
            {"fn": "add", "args": [2, 3]},
        ]),
    );
    assert_eq!(report["ok"], true, "沙箱执行失败：{}", report["error"]);
    assert_eq!(report["results"][0], "hello, world", "核心接口在沙箱内执行");
    assert_eq!(report["results"][1], 5);
    assert_eq!(report["api_keys"], serde_json::json!(FUNCS));
    // wasm-bindgen 胶水在模块求值即构造 TextEncoder/TextDecoder（无 typeof 守卫）——
    // 沙箱里本来没有它们，能跑到这里即证明适配层的垫片是承重的
    assert_eq!(report["sandbox"]["text_encoder_before"], "undefined");
    assert_eq!(report["sandbox"]["text_decoder_before"], "undefined");
    assert_eq!(report["sandbox"]["text_encoder_after"], "function");
    // —— 「产物无原生模块依赖」的字节级判据：导入表只允许来自胶水自身模块
    let imports = report["sandbox"]["imports"].as_array().expect("导入表");
    eprintln!("证据：wasm 导入表 {imports:?}");
    for item in imports {
        let item = item.as_str().unwrap_or_default();
        assert!(
            item.starts_with(&product.manifest.import_module),
            "越界导入（原生模块依赖）：{item}"
        );
    }

    // —— 库级缓存往返：命中还原不需要任何工具（CLI 的 cached=true 路径）
    let cache = Cache::from_root(temp.path().join(".polypack/cache"));
    let source = bridge::interface_source(&project).expect("接口定义可定位");
    let interface = bridge::interface_digest(&source).expect("接口摘要");
    let key =
        wasm::cache_key(&project, &interface, &product.manifest.engine, "local").expect("缓存键");
    cache.store_tree(&key, &out).expect("产物入库");
    std::fs::remove_dir_all(&out).expect("清产物目录");
    assert!(
        cache.restore_tree(&key, &out).expect("查缓存"),
        "缓存应命中"
    );
    let restored = std::fs::read(out.join(format!("{LIB}_wasm_bg.wasm"))).expect("还原的 wasm");
    assert_eq!(
        restored, wasm_bytes,
        "还原字节与生成字节一致（缓存存的是裁剪后产物）"
    );
    // 体积报告随树还原：清单即元数据载体，缓存命中路径不必重跑 wasm-opt
    assert_eq!(
        bridge::wasm_opt::read_size(&out).as_ref(),
        Some(&size),
        "还原后的体积元数据与生成时一致"
    );
    eprintln!("证据：缓存键 {} 命中还原，字节一致", key.as_str());
}
