//! T-M2-01 验收的 Swift 侧真跑（`POLY_CONTRACT_REAL=1` + macOS 宿主 + Xcode）：
//! **真生成器输出**（uniffi，经 `poly_core::bridge::generate` 的生产参数）× **Apple 工具链**
//! （`swiftc -typecheck` 过 iOS simulator SDK）。
//!
//! 为什么单列一条腿：`bridge.yml` 在 ubuntu 真生成并对三件套做结构断言，但 ubuntu 没有
//! `swiftc`（该工作流自述「Swift 侧编译留待 macOS 作业」）；iOS 打包腿（`ios_real_run.rs` /
//! `contract_m2.rs`）编译的是**夹具手写三件套**（`ios_core/Cargo.toml` 自述「刻意不依赖
//! uniffi」）。于是「真生成器输出过一遍 Apple 编译」此前没有任何腿覆盖——本条补上，
//! 同时把生成器产出与打包器消费契约（`swift::swift_binding_files`）钉在一起。
//!
//! 夹具复用 `poly-core` 的 `bridge_core`（真 uniffi 依赖 + 自托管 `src/bin/uniffi-bindgen.rs`，
//! 与 `bridge_real.rs` 同源；跨组共享测试资产，只读用不改）。mock 模式 / 非 macOS 宿主 /
//! 缺 swiftc 一律 SKIP+NOTE（与契约腿同纪律，绝不红）。

use std::path::{Path, PathBuf};
use std::process::Command;

use poly_core::bridge::{self, BridgeLang};
use poly_core::config::PolypackConfig;
use poly_core::events::HumanSink;
use poly_core::plan::Project;

/// 夹具核心 crate 名（= `bridge_core/Cargo.toml` 的 `[lib] name`，也是绑定三件套的前缀）。
const LIB: &str = "pax_core";

/// 临时工程配置（与 `bridge_real.rs` 的同形：core 目录即 `<root>/core`）。
const CONFIG: &str = "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n\
                      [ui]\nframework = \"react\"\nentry = \"index.html\"\n\
                      [core]\ncrate = \"core\"\n";

/// 调用面：用**真签名**调生成绑定的公开面（`public func greet(name:)` / `add(a:b:)`；
/// 签名由生成器决定，本地真生成实证后写定）。`import` 之外不求任何 FFI 声明：
/// 模块图没接上时下面这些调用会因 `uniffi_pax_core_*` 未定义而编译失败——响亮，不静默。
const CALLER: &str = "import pax_coreFFI\n\n\
                      print(\"PAX_GREET=\\(greet(name: \"Pax\"))\")\n\
                      print(\"PAX_ADD=\\(add(a: 2, b: 3))\")\n";

/// 真跑开关：`POLY_CONTRACT_REAL=1`（或 `true`）；默认 mock。
fn real_mode() -> bool {
    matches!(
        std::env::var("POLY_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// 真生成器（真 uniffi 输出）× Apple 工具链（`swiftc -typecheck`）。
#[test]
fn generated_swift_bindings_typecheck_with_apple_toolchain() {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真跑设 POLY_CONTRACT_REAL=1（真生成器 × Apple 编译）");
        return;
    }
    if !cfg!(target_os = "macos") {
        eprintln!(
            "SKIP：Apple 工具链（swiftc / SDK）仅随 macOS 提供；生成本身在 `bridge.yml` 的 ubuntu 腿覆盖"
        );
        return;
    }
    if ["xcrun", "swiftc"].iter().any(|tool| !tool_present(tool)) {
        eprintln!("SKIP：缺 xcrun / swiftc（Xcode 命令行工具未装）");
        return;
    }

    // 夹具复制成临时工程：`cargo run --bin uniffi-bindgen` 会写 `<core>/target/`，不落仓库树。
    let root = std::env::temp_dir().join(format!("poly-bindings-ios-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy_tree(&fixture_dir(), &root.join("core"));
    let project = Project {
        root: root.clone(),
        config: PolypackConfig::parse(CONFIG).expect("临时工程配置合法"),
    };
    // 引擎身份进证据行：三处定位里命中的是夹具自托管 bin（版本与夹具的 uniffi 依赖锁一处）。
    let engine = poly_core::bridge::UniffiEngine::discover(&root.join("core"))
        .expect("夹具自带 src/bin/uniffi-bindgen.rs");
    let mut sink = HumanSink { out: Vec::new() };
    let outputs = bridge::generate(&project, &[BridgeLang::Swift], &mut sink)
        .expect("真生成 Swift 绑定（需要网络拉夹具的 uniffi 依赖）");
    assert_eq!(outputs.len(), 1, "只请求 Swift：{outputs:?}");
    assert_eq!(outputs[0].lang, BridgeLang::Swift);

    let bindings = root.join("bridge/swift");
    assert_eq!(outputs[0].dir, bindings, "落点须与打包器约定一致");
    // 生成器产出 ↔ 打包器消费契约的交点：三件套必须过打包器自己的校验
    let files = poly_bundle::packagers::ios::swift::swift_binding_files(&bindings, LIB)
        .expect("生成器产物应满足打包器的绑定三件套校验");
    let swift_text = std::fs::read_to_string(&files.swift).expect("绑定源码可读");
    for func in ["greet", "add"] {
        assert!(
            swift_text.contains(&format!("func {func}(")),
            "Swift 绑定缺 UDL 声明的 {func}"
        );
    }

    // Apple 工具链：真产物 + 真调用面一起 typecheck。部署目标取 SDK 版本（写死版本号会随
    // runner 的 Xcode 升降而红）。
    let caller = root.join("caller.swift");
    std::fs::write(&caller, CALLER).expect("调用面可写");
    let sdk = xcrun(&["--sdk", "iphonesimulator", "--show-sdk-path"]);
    let sdk_version = xcrun(&["--sdk", "iphonesimulator", "--show-sdk-version"]);
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    let target = format!("{arch}-apple-ios{sdk_version}-simulator");
    let modulemap = format!("-fmodule-map-file={}", files.modulemap.display());
    let bindings_arg = bindings.to_string_lossy().to_string();
    let swift_arg = files.swift.to_string_lossy().to_string();
    let caller_arg = caller.to_string_lossy().to_string();
    let check = Command::new("swiftc")
        .args([
            "-typecheck",
            "-sdk",
            &sdk,
            "-target",
            &target,
            "-Xcc",
            &modulemap,
            "-I",
            &bindings_arg,
            &swift_arg,
            &caller_arg,
        ])
        .output()
        .expect("swiftc 可启动");
    assert!(
        check.status.success(),
        "真生成器输出未过 Apple 编译（{target}）：{}",
        String::from_utf8_lossy(&check.stderr)
    );

    eprintln!(
        "OK iOS 绑定真编译：{} 的三件套 + 真调用面过 swiftc -typecheck（{target}，SDK iOS {sdk_version}）",
        engine.describe()
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 仓库里的 uniffi 夹具目录（与 `bridge_real.rs` 同源；只读用）。
fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../poly-core/tests/fixtures/bridge_core")
}

/// 递归复制目录（夹具只有几个小文件）。
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// `PATH` 上是否有该可执行文件。
fn tool_present(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(name).is_file()))
}

/// `xcrun <args>` 的 stdout（去尾换行）；失败即 panic 带 stderr。
fn xcrun(args: &[&str]) -> String {
    let out = Command::new("xcrun")
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("xcrun {args:?} 启动失败：{err}"));
    assert!(
        out.status.success(),
        "xcrun {args:?} 退出 {}：{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}
