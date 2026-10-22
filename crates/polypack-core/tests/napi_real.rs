//! T-M3-01 真跑（`POLYPACK_CONTRACT_REAL=1`）：UDL → C 胶水 → **OHOS triple 真交叉编译** →
//! 自包含 NAPI 模块 `.so`（架构 + 符号契约）→ **本机 node 真装载真调用**，结果与夹具的
//! Rust 单元测试对拍（同一组字面量，两侧都真执行）。
//!
//! 默认（mock 模式）/ 缺工具一律打印 SKIP 原因后返回，绝不红——与既有契约真跑同一纪律
//! （`bridge_real.rs`）。无工具面的结构断言在 `bridge.rs` / CLI 的 `bridge.rs`（每宿主都跑）。
//!
//! shortcut: 设备腿（RNOH 宿主里装载、hdc 推装、真机键序列）本机无设备/模拟器（hdc 无目标），
//! 按 SKIP 打印；模块 `.so` 的「可装载性」用 node 的 NAPI 装载路径实证（与 ArkTS 同源 ABI），
//! 上真机时补一条 hdc/uitest 用例。
//!
//! shortcut: 对拍腿用宿主机 gcc 重编**同一份胶水** + 宿主编译的核心 staticlib；OHOS 侧产物
//! 无法在 x86 宿主上执行——两侧同源同代码，装载语义等价（NAPI 是同源 ABI）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use polypack_core::bridge::{self, BridgeLang, napi};
use polypack_core::config::PolypackConfig;
use polypack_core::events::HumanSink;
use polypack_core::plan::Project;

/// 夹具核心库基础名（= `tests/fixtures/bridge_core` 的 `[lib] name`）。
const LIB: &str = "pax_core";
/// UDL namespace（= NAPI 模块的装载名 `lib<pax>.so` / `.nm_modname`）。
const NS: &str = "pax";
/// UDL 里声明的两个函数：UDL ↔ 胶水 ↔ `.so` 符号表 ↔ node 调用四处都必须出现。
const FUNCS: [&str; 2] = ["greet", "add"];

// ---------------------------------------------------------------- 支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时工程根；drop 即递归删除（`POLYPACK_KEEP_TEMP=1` 保留，CI 留产物通道）。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-napi-real-{tag}-{}-{}",
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
        if std::env::var("POLYPACK_KEEP_TEMP").as_deref() == Ok("1") {
            return;
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 真跑开关：`POLYPACK_CONTRACT_REAL=1`（或 `true`）；默认 mock。
fn real_mode() -> bool {
    matches!(
        std::env::var("POLYPACK_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// 前置不满足 → 打印原因并返回 false（SKIP 纪律：缺工具不红）。
fn gate(what: &str) -> bool {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真跑设 POLYPACK_CONTRACT_REAL=1（{what}）");
        return false;
    }
    true
}

/// 把夹具复制成临时工程（真跑产物不落仓库树）。
fn staged_project(tag: &str) -> (TempDir, Project) {
    let temp = TempDir::new(tag);
    let core = temp.path().join("core");
    copy_tree(&fixture_dir(), &core);
    let config = PolypackConfig::parse(
        "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("配置合法");
    let project = Project::new(temp.path().to_path_buf(), config);
    (temp, project)
}

/// 仓库里的夹具目录。
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

/// `readelf <flag>` 的输出（工具缺失或失败即 None；LC_ALL=C 钉死表头语言）。
fn readelf(so: &Path, flag: &str) -> Option<String> {
    let out = Command::new("readelf")
        .env("LC_ALL", "C")
        .arg(flag)
        .arg(so)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `nm` 的符号表（`dynamic=true` 即 `-D`；工具缺失或失败即 None）。
fn nm(so: &Path, dynamic: bool) -> Option<String> {
    let mut cmd = Command::new("nm");
    if dynamic {
        cmd.arg("-D");
    }
    let out = cmd.arg(so).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// 命令是否在 PATH 上（SKIP 判据）。
fn has_tool(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}

/// rustup 是否装了某 triple。
fn rustup_target_installed(triple: &str) -> bool {
    Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .is_ok_and(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .any(|line| line.trim() == triple)
        })
}

/// 宿主 node 头目录（`node_api.h` 所在）：从 PATH 上的 node 反推（apt：/usr/bin/node →
/// /usr/include/node；nvm：/…/bin/node → /…/include/node），再补两个常规落点。
fn host_node_include() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir.join("node").is_file()
                && let Some(prefix) = dir.parent()
            {
                candidates.push(prefix.join("include/node"));
            }
        }
    }
    candidates.push(PathBuf::from("/usr/include/node"));
    candidates.push(PathBuf::from("/usr/local/include/node"));
    candidates
        .into_iter()
        .find(|dir| dir.join("node_api.h").is_file())
}

// ---------------------------------------------------------------- 真跑

/// 真跑两段：① OHOS aarch64 交叉编译出自包含模块 `.so`（架构 + 符号 + 自包含性契约）；
/// ② 宿主机重编同一份胶水 → node 真装载真调用 → 与夹具 Rust 单元测试对拍。
///
/// 两段各自缺工具各自 SKIP 打印，不互相掩盖（缺 SDK 不阻断对拍段，反之亦然）。
#[test]
fn real_ohos_module_and_node_call_parity() {
    if !gate("napi 胶水 / OHOS 交叉编译 / node 装载对拍") {
        return;
    }
    let (_temp, project) = staged_project("real");
    let root = project.root.clone();
    let mut sink = HumanSink { out: Vec::new() };

    // —— 段零：一份 UDL → C 胶水（napi 出口不依赖 uniffi 引擎在场） ——
    let outputs = bridge::generate(&project, &[BridgeLang::Napi], &mut sink).expect("真生成胶水");
    let glue = root.join("bridge/napi").join(format!("{NS}.c"));
    assert!(glue.is_file(), "缺胶水 {}", glue.display());
    assert_eq!(outputs[0].lang, BridgeLang::Napi);
    assert_eq!(outputs[0].files, vec![PathBuf::from(format!("{NS}.c"))]);
    let text = std::fs::read_to_string(&glue).expect("读胶水");
    for func in FUNCS {
        assert!(
            text.contains(&format!("uniffi_{LIB}_fn_func_{func}(")),
            "胶水缺 {func} 的 FFI 符号（crate 名归一化前缀）：{text}"
        );
    }

    // —— 段一：OHOS triple 真交叉编译（缺 SDK / 缺 rustup target 各自 SKIP） ——
    let sdk = napi::OhosSdk::discover();
    let abi = napi::ABIS[0]; // arm64-v8a：真机口径
    match &sdk {
        Some(sdk) if rustup_target_installed(abi.triple) => {
            let core_dir = root.join("core");
            let staticlib =
                napi::build_lib(&core_dir, abi, sdk, &mut sink).expect("OHOS 真交叉编译");
            let out_dir = root.join("bridge/harmonyos").join(abi.dir);
            let so = napi::build_module(&glue, &staticlib, NS, abi, sdk, &out_dir, &mut sink)
                .expect("真链接模块 .so");
            let bytes = std::fs::metadata(&so).expect("读 .so").len();
            assert!(bytes > 0, "{} 是空文件", so.display());
            assert_eq!(so.file_name().unwrap(), format!("lib{NS}.so").as_str());

            // 架构断言：跨编产物必须是 aarch64（readelf 不在场时按缺工具跳过）
            match readelf(&so, "-h") {
                Some(head) => {
                    let machine = head
                        .lines()
                        .find(|line| line.contains("Machine:"))
                        .unwrap_or_default();
                    assert!(machine.contains("AArch64"), "{machine}");
                }
                None => eprintln!("SKIP：无 readelf，跳过 {} 的架构断言", so.display()),
            }
            // 符号契约：node/RNOH 的装载入口必须导出；ArkTS 装载入口的宿主符号保持未定义
            // （由 RNOH 运行时提供，不 mock 假实现）；核心 FFI 符号随 staticlib 静态入模块
            if let Some(dynamic) = nm(&so, true) {
                assert!(
                    dynamic.contains(" T napi_register_module_v1"),
                    "缺 node/RNOH 的装载入口（dlsym 找不到就装不上）：\n{dynamic}"
                );
                assert!(
                    dynamic.contains(" U napi_module_register"),
                    "ArkTS 装载入口的宿主符号应保持未定义：\n{dynamic}"
                );
            } else {
                eprintln!("SKIP：无 nm -D，跳过 {} 的装载入口断言", so.display());
            }
            if let Some(all) = nm(&so, false) {
                for func in FUNCS {
                    let symbol = format!("uniffi_{LIB}_fn_func_{func}");
                    assert!(
                        all.contains(&symbol),
                        "{} 缺 {symbol}（staticlib 未链入）",
                        so.display()
                    );
                }
            }
            // 自包含性：动态依赖里不得出现核心库（静态入；模块目录再放一个 .so 会双份）
            if let Some(dynamic) = readelf(&so, "-d") {
                let needed: Vec<&str> = dynamic
                    .lines()
                    .filter(|line| line.contains("NEEDED"))
                    .collect();
                assert!(
                    !needed.iter().any(|line| line.contains("pax_core")),
                    "NEEDED 不应含核心库（{LIB}）：{needed:?}"
                );
            }
            eprintln!(
                "真跑：OHOS 模块 {}（{} 字节，{}）",
                so.display(),
                bytes,
                abi.triple
            );
        }
        Some(_) => eprintln!(
            "SKIP：缺 rustup target {}（rustup target add {}）",
            abi.triple, abi.triple
        ),
        None => eprintln!(
            "SKIP：无 HarmonyOS SDK（$HARMONYOS_TOOLS / $DEVECO_SDK_HOME / $POLYPACK_OHOS_SDK）"
        ),
    }

    // —— 段二：宿主机对拍（同一份胶水 → node 真装载真调用） ——
    node_parity_leg(&root, sdk.as_ref());
}

/// 宿主机对拍腿：夹具 Rust 单元测试 + 同一份胶水经 host gcc 编译 → node 装载真调用。
///
/// 两侧断言用**同一组字面量**（夹具 `src/lib.rs` 的 tests）：Rust 侧真跑过，node 侧真调用
/// 后必须给出一致结果——这就是「示例接口调用结果与 Rust 单元测试一致」的判据本身。
/// napi 头优先取 OHOS SDK（与交叉编译腿同源），无 SDK 时退宿主 node 头。
fn node_parity_leg(project_root: &Path, sdk: Option<&napi::OhosSdk>) {
    let core_dir = project_root.join("core");
    // ① Rust 侧基准：夹具单元测试真跑
    let test_out = Command::new("cargo")
        .args(["test", "--release", "--lib"])
        .current_dir(&core_dir)
        .env("CARGO_TARGET_DIR", core_dir.join("target"))
        .output()
        .expect("cargo 可执行");
    assert!(
        test_out.status.success(),
        "夹具 Rust 单元测试失败：\n{}",
        String::from_utf8_lossy(&test_out.stderr)
    );
    eprintln!("真跑：夹具 Rust 单元测试通过（greet/add 基准值）");

    // ② 宿主编译核心 staticlib（同一份源码，host triple）
    let host = Command::new("cargo")
        .args(["build", "--release", "--lib"])
        .current_dir(&core_dir)
        .env("CARGO_TARGET_DIR", core_dir.join("target"))
        .output()
        .expect("cargo 可执行");
    assert!(
        host.status.success(),
        "宿主构建核心失败：\n{}",
        String::from_utf8_lossy(&host.stderr)
    );
    let staticlib = core_dir.join("target/release").join(format!("lib{LIB}.a"));
    assert!(staticlib.is_file(), "缺 {}", staticlib.display());

    // ③ 同一份胶水 + host staticlib → node 可装载件。napi 头用 `-idirafter`（排在 glibc 之后
    //    取用，不遮蔽宿主头；NAPI 是同源 ABI，node 的装载路径与 ArkTS 等价）
    let include = sdk.map(|sdk| sdk.include_dir()).or_else(host_node_include);
    let Some(include) = include else {
        eprintln!("SKIP：无 napi 头（既无 OHOS SDK，宿主也无 node_api.h），跳过对拍");
        return;
    };
    let cc = if has_tool("gcc") { "gcc" } else { "cc" };
    if !has_tool(cc) {
        eprintln!("SKIP：无 gcc/cc，跳过 node 对拍（宿主链接器缺失）");
        return;
    }
    if !has_tool("node") {
        eprintln!(
            "SKIP：无 node，跳过 NAPI 装载对拍（胶水已真生成：{}）",
            project_root
                .join("bridge/napi")
                .join(format!("{NS}.c"))
                .display()
        );
        return;
    }
    let host_dir = project_root.join("bridge/napi/host");
    std::fs::create_dir_all(&host_dir).expect("建宿主产物目录");
    let addon = host_dir.join(format!("{NS}.node"));
    let link = Command::new(cc)
        .arg("-shared")
        .arg("-fPIC")
        .arg("-idirafter")
        .arg(&include)
        .arg(project_root.join("bridge/napi").join(format!("{NS}.c")))
        .arg(&staticlib)
        .args(["-ldl", "-lpthread", "-lm"])
        .arg("-o")
        .arg(&addon)
        .output()
        .expect("cc 可执行");
    assert!(
        link.status.success(),
        "宿主链接失败（头目录 {}）：\n{}",
        include.display(),
        String::from_utf8_lossy(&link.stderr)
    );

    // ④ node 真装载真调用：断言即基准值（与夹具 Rust 单元测试同字面量）
    let script = project_root.join("bridge/napi/parity.js");
    std::fs::write(
        &script,
        "const assert = require('assert');\n\
         const mod = { exports: {} };\n\
         process.dlopen(mod, process.argv[2]);\n\
         const api = mod.exports;\n\
         // 与夹具 Rust 单元测试同一组字面量（tests/fixtures/bridge_core/src/lib.rs）\n\
         assert.strictEqual(api.greet('world'), 'hello, world');\n\
         assert.strictEqual(api.add(2, 3), 5);\n\
         console.log(`napi parity ok: greet(world)=${api.greet('world')} add(2,3)=${api.add(2, 3)}`);\n",
    )
    .expect("写对拍脚本");
    let node = Command::new("node")
        .arg(&script)
        .arg(&addon)
        .output()
        .expect("node 可执行");
    assert!(
        node.status.success(),
        "node 装载/调用失败：\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    eprintln!("真跑：{}", String::from_utf8_lossy(&node.stdout).trim());
}
