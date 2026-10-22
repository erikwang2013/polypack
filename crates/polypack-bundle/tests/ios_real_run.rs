//! T-M2-14 真跑（`POLYPACK_CONTRACT_REAL=1` + macOS 宿主 + Xcode + rustup Apple triple）：
//! 夹具核心静态库 → cargo 交叉编译 → [lipo 合并切片] → `xcodebuild -create-xcframework`
//! → 工程骨架 → `xcodebuild archive` → 归档 `Payload/` 直压 → 未签名 IPA（含结构断言）
//! → iOS simulator 里真调 sim 切片（`simctl spawn` 运行时对拍）。
//!
//! T-M4-07：同文件承载 tvOS 腿（复用同一链，triple / 切片 / SDK 经 `ApplePlatform` 描述符
//! 切换；tvOS 模拟器单架构、无 lipo 合并）。tvOS 前置另需 `appletvos` SDK 与 rustup
//! tvOS 两条 target——CI `mobile-contract.yml` 未装（工作流不在本任务线的改动范围），
//! 缺则 SKIP-with-print 点名要装的 target，绝不假装已跑。
//!
//! 默认 mock 模式 / 非 macOS 宿主 / 缺 Xcode 或 Apple triple 一律 SKIP 后返回（绝不红）——
//! 与 M1 契约测试同一 SKIP 纪律（`crates/polypack-cli/tests/contract_m1.rs`）。这一份是
//! 「Apple 链路在真机上确实跑通」的证据；mock 侧的命令组装断言见 `packagers_ios.rs`。
//!
//! 未签名 IPA 走 `ditto -c -k` 直压（不经 `exportArchive`）：无 Team 的未签名归档在
//! `-exportArchive` 下必失败（macos-14 真跑实证「No Team Found in Archive」）。
//! `exportArchive` 留给 T-M2-10 的签名导出（`POLYPACK_IOS_EXPORT_METHOD` / ExportOptions.plist）。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use polypack_bundle::packagers::ios::{Ios, apple_platform, swift};
use polypack_bundle::{BundleInputs, IosInputs, Packager, ProductInfo};
use polypack_targets::{Host, TargetSpec};

/// 夹具核心库基础名（= fixtures/ios_core 的 crate 名）。
const LIB: &str = "pax_core";
/// 产品名（`lib<Pax Notes>.a` 之外的产物命名基准）。
const PRODUCT: &str = "Pax Notes";

/// 真跑开关：`POLYPACK_CONTRACT_REAL=1`（或 `true`）；默认 mock。
fn real_mode() -> bool {
    matches!(
        std::env::var("POLYPACK_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// rustup 是否已装某 target（缺则 SKIP：`rustup target add <triple>` 由 CI 准备）。
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

/// 真跑前置：模式 + 宿主 + 工具链 + 平台交叉目标，任一不满足即打印 SKIP 原因并返回 false。
///
/// triple 清单从平台描述符派生（T-M4-07：ios 三条 / tvos 两条），与 `swift` 链同源。
fn prerequisites(target_id: &str) -> bool {
    let Some(platform) = apple_platform(target_id) else {
        eprintln!("SKIP：{target_id} 无 Apple 平台描述符（本腿只服务 ios / tvos）");
        return false;
    };
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真跑设 POLYPACK_CONTRACT_REAL=1");
        return false;
    }
    if !cfg!(target_os = "macos") {
        eprintln!("SKIP：{target_id} 链路需 macOS 宿主（Xcode / lipo / xcodebuild）");
        return false;
    }
    if common::skip_without_any(&["xcodebuild", "xcrun", "lipo"]) {
        return false;
    }
    // tvOS 另需 platform SDK 在位（Xcode 需装 tvOS platform 组件）
    if target_id == "tvos" {
        let sdk = Command::new("xcodebuild")
            .args(["-showsdks"])
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("appletvos"));
        if !sdk {
            eprintln!("SKIP：Xcode 未装 tvOS platform（xcodebuild -showsdks 无 appletvos）");
            return false;
        }
    }
    let triples =
        std::iter::once(platform.device_triple).chain(platform.sim_triples.iter().copied());
    for triple in triples {
        if !rustup_target_installed(triple) {
            eprintln!(
                "SKIP：rustup 未装 {triple}（rustup target add {triple}）——CI mobile-contract.yml \
                 未装 tvOS target，此腿在补齐前恒 SKIP（工作流改动报 lead）"
            );
            return false;
        }
    }
    true
}

/// triple → lipo 架构名（期望值从平台描述符派生，不写死）。
fn lipo_arch(triple: &str) -> String {
    match triple.split('-').next() {
        Some("aarch64") => "arm64".to_string(),
        Some(other) => other.to_string(),
        None => triple.to_string(),
    }
}

/// 工作目录（每测试独占，结束即删）。
fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("polypack-ios-real-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 夹具核心 crate 的工作副本（`cargo build` 会写入 `target/`，不污染仓库）。
fn fixture_crate(dir: &Path) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ios_core");
    let crate_dir = dir.join("core");
    copy_tree(&source, &crate_dir);
    crate_dir
}

/// 递归复制（夹具极小，够用即可）。
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

/// 绑定三件套（夹具形态：真绑定由 T-M2-01 生成器产出，此处只保证 C ABI 面与头一致）。
fn write_bindings(out_dir: &Path) {
    let bridge = swift::bridge_dir(out_dir);
    std::fs::create_dir_all(&bridge).unwrap();
    std::fs::write(
        bridge.join(format!("{LIB}FFI.h")),
        b"#include <stdint.h>\nint32_t pax_add(int32_t a, int32_t b);\n",
    )
    .unwrap();
    std::fs::write(
        bridge.join(format!("{LIB}FFI.modulemap")),
        format!("module {LIB}FFI {{\n  header \"{LIB}FFI.h\"\n  export *\n}}\n"),
    )
    .unwrap();
    std::fs::write(
        bridge.join(format!("{LIB}.swift")),
        format!(
            "import {LIB}FFI\n\npublic func paxAdd(_ a: Int32, _ b: Int32) -> Int32 {{\n    pax_add(a, b)\n}}\n"
        ),
    )
    .unwrap();
}

/// 输入：夹具 crate + 绑定三件套（无宿主工程 → 走骨架生成）。
fn inputs(dir: &Path, core_crate: PathBuf) -> BundleInputs {
    write_bindings(dir);
    let mut inputs = BundleInputs::new(
        ProductInfo {
            name: PRODUCT.to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "T-M2-14 真跑夹具".to_string(),
            homepage: None,
        },
        dir,
    );
    inputs.ios = Some(IosInputs {
        core_crate: Some(core_crate),
        lib_name: Some(LIB.to_string()),
        host_project: None,
        bindings_dir: None,
    });
    inputs
}

/// 目标声明（`hosts = [Macos]`，与内建 spec 同形；T-M4-07：按目标 id 取 builder 口径）。
fn spec_for(target_id: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub(target_id, 1, &[Host::Macos]);
    spec.package.format = "ipa".to_string();
    spec.package.builder = format!("{target_id}-xcodebuild");
    spec
}

/// 真跑全链路：切片链 + archive / Payload 直压 → 未签名 IPA。
///
/// 两个 `#[test]` 各自先过平台前置（缺工具链 / triples 即 SKIP），再走同一实现——
/// 期望值（切片集、架构、产物 target）全部从平台描述符派生，不在断言里写死 ios。
fn end_to_end_builds_unsigned_ipa(target_id: &str, tag: &str) {
    let platform = apple_platform(target_id).expect("Apple 平台描述符");
    let dir = work_dir(tag);
    let inputs = inputs(&dir, fixture_crate(&dir));
    let spec = spec_for(target_id);

    // 切片链：真 cargo 交叉编译（device + 各 sim triple）→ [lipo 合并] →（绑定已在位，
    // 跳过 bindgen）→ create-xcframework → lipo -info。
    let xcframework = swift::package(&inputs, &spec).expect("XCFramework 真跑");
    assert_eq!(xcframework.format, "xcframework");
    assert_eq!(
        xcframework.target, target_id,
        "XCFramework 产物须标注真实目标（T-M4-07）"
    );
    let layout = swift::validate_xcframework_layout(&inputs.out_dir, platform).expect("结构校验");
    assert_eq!(layout.slices.len(), 2, "{layout:?}");
    assert_eq!(
        layout.slices,
        platform.slices().map(str::to_string).to_vec()
    );

    // 切片架构真枚举：device = 设备 triple 的架构；simulator = sim triples 的架构集
    // （ios 双架构经 lipo 合一 / tvos 单架构直用——期望从描述符派生）。
    let device = swift::lipo_archs(&swift::slice_lib_path(
        &inputs.out_dir,
        platform.device_slice,
        LIB,
    ))
    .expect("lipo -archs device");
    assert_eq!(
        device,
        vec![lipo_arch(platform.device_triple)],
        "device 切片架构"
    );
    let sim = swift::lipo_archs(&swift::slice_lib_path(
        &inputs.out_dir,
        platform.sim_slice,
        LIB,
    ))
    .expect("lipo -archs simulator");
    let mut sim_sorted = sim.clone();
    sim_sorted.sort();
    let mut expected_sim: Vec<String> = platform.sim_triples.iter().map(|t| lipo_arch(t)).collect();
    expected_sim.sort();
    assert_eq!(sim_sorted, expected_sim, "simulator 切片架构集：{sim:?}");

    // 骨架工程 + 真 xcodebuild archive + 归档 Payload 直压。
    let ipa = Ios.package(&inputs, &spec).expect("IPA 真跑");
    assert_eq!(ipa.format, "ipa");
    assert_eq!(ipa.target, target_id, "IPA 产物须标注真实目标（T-M4-07）");
    assert!(
        ipa.path.extension().is_some_and(|ext| ext == "ipa"),
        "{ipa:?}"
    );

    // 产物结构：IPA 是 zip，必含 Payload/<产品>.app 与其 Info.plist / 可执行文件。
    let names = polypack_bundle::zipwrite::entry_names(&ipa.path).expect("IPA 条目清单");
    let app = format!("Payload/{PRODUCT}.app");
    for entry in [format!("{app}/Info.plist"), format!("{app}/{PRODUCT}")] {
        assert!(
            names.iter().any(|name| name == &entry),
            "IPA 缺 {entry}：{names:?}"
        );
    }
    // 真跑产物留档：不删工作目录（CI 由 `留产物` 步上传；本地留一份供人工核验）。
    eprintln!(
        "OK {target_id} 真跑：{}（{} 字节，{} 条目）",
        ipa.path.display(),
        ipa.bytes,
        names.len()
    );

    if target_id == "ios" {
        simulator_call_parity(&inputs);
    } else {
        // tvOS 运行时对拍（appletvsimulator + Apple TV 模拟器设备）不在本腿范围：
        // 本腿证明「tvOS 切片 + archive + IPA 打包链在 macOS 宿主真跑通」，键序列 /
        // 渲染对拍归 T-M4-06 的设备腿（host/tvos/README「阻塞与证据边界」）。
        eprintln!(
            "NOTE：{target_id} 运行时对拍（simctl spawn appletvsimulator）不在本腿——\
             打包链真跑已覆盖，键序列走查见 T-M4-06 设备腿"
        );
    }
}

/// 真跑全链路（iOS）。
#[test]
fn ios_end_to_end_builds_unsigned_ipa() {
    if !prerequisites("ios") {
        return;
    }
    end_to_end_builds_unsigned_ipa("ios", "e2e");
}

/// 真跑全链路（tvOS，T-M4-07）：同链、tvOS SDK / 切片；前置缺 tvOS target 即 SKIP-with-print。
#[test]
fn tvos_end_to_end_builds_unsigned_ipa() {
    if !prerequisites("tvos") {
        return;
    }
    end_to_end_builds_unsigned_ipa("tvos", "tvos-e2e");
}

/// simulator 运行时调用对拍：sim 切片 + 绑定三件套 → 可执行文件 → 在 iOS simulator 里真跑。
///
/// 断言两条，返回值都须由切片里的 Rust 代码在 simulator 内算出：
/// - `paxAdd(-5, 3) == -2`：符号面（当无符号处理会给出正数，立刻露）；
/// - `paxAdd(2_000_000_000, 2_000_000_000) == -294_967_296`：i32 回绕（i64 / 无符号穿 C ABI 即露）。
///
/// 为什么必须真运行：archive 只编译链接、`lipo -info` 只枚举架构，切片与调用链此前从未被跑过。
/// 绑定面是夹具手写三件套（真 uniffi 绑定面的 simulator 对拍与「真产物链进 archive」合并记 M2+），
/// 故本腿证明的是「切片 + C ABI 调用链在 simulator 上真跑通」。
///
/// shortcut: 设备 / runtime 缺失按 SKIP+NOTE（首轮按 NOTE 观察 runner 环境，真跑证一次后改硬断言）。
fn simulator_call_parity(inputs: &BundleInputs) {
    let list = match Command::new("xcrun")
        .args(["simctl", "list", "devices", "available"])
        .output()
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).to_string(),
        other => {
            eprintln!("SKIP：simctl 不可用（{other:?}）——simulator 调用对拍未跑");
            return;
        }
    };
    let Some(udid) = first_ios_udid(&list) else {
        eprintln!(
            "SKIP：无可用 iOS simulator 设备（Xcode 未装 iOS runtime？）——真跑证一次后改硬断言"
        );
        return;
    };
    // 已启动的设备 boot 会非零退出（不致命）；`bootstatus -b` 负责等到真就绪。
    let _ = Command::new("xcrun")
        .args(["simctl", "boot", &udid])
        .output();
    let booted = Command::new("xcrun")
        .args(["simctl", "bootstatus", &udid, "-b"])
        .output()
        .expect("simctl bootstatus 可启动");
    assert!(
        booted.status.success(),
        "simulator 未就绪（{udid}）：{}",
        String::from_utf8_lossy(&booted.stderr)
    );

    let bindings = swift::bindings_dir(inputs);
    let slice_lib = swift::slice_lib_path(&inputs.out_dir, swift::SIM_SLICE, LIB);
    // 部署目标取 SDK 版本（Xcode 的默认口径）：写死 17.0 会随 runner 的 Xcode 升降而红。
    let sdk_path = xcrun_stdout(&["--sdk", "iphonesimulator", "--show-sdk-path"]);
    let sdk_version = xcrun_stdout(&["--sdk", "iphonesimulator", "--show-sdk-version"]);
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    let target = format!("{arch}-apple-ios{sdk_version}-simulator");

    // 必须叫 `main.swift`：多文件编译时顶层表达式只允许出现在入口文件（swiftc 规则），
    // 否则 `expressions are not allowed at the top level`（macos-14 首次真跑实证）
    let call = inputs.out_dir.join("main.swift");
    std::fs::write(
        &call,
        "print(\"PAX_ADD_NEG=\\(paxAdd(-5, 3))\")\n\
         print(\"PAX_ADD_WRAP=\\(paxAdd(2_000_000_000, 2_000_000_000))\")\n",
    )
    .unwrap();
    let bin = inputs.out_dir.join("sim-call");
    // 模块图显式给出（不赌 clang 的自动发现）：`-I` 让 `<lib>FFI.h` 随模块图解析到位。
    let modulemap = format!(
        "-fmodule-map-file={}",
        bindings.join(format!("{LIB}FFI.modulemap")).display()
    );
    let bindings_arg = bindings.to_string_lossy().to_string();
    let swift_source = bindings
        .join(format!("{LIB}.swift"))
        .to_string_lossy()
        .to_string();
    let call_arg = call.to_string_lossy().to_string();
    let lib_arg = slice_lib.to_string_lossy().to_string();
    let bin_arg = bin.to_string_lossy().to_string();
    let compile = Command::new("swiftc")
        .args([
            "-sdk",
            &sdk_path,
            "-target",
            &target,
            "-Xcc",
            &modulemap,
            "-I",
            &bindings_arg,
            &swift_source,
            &call_arg,
            &lib_arg,
            "-o",
            &bin_arg,
        ])
        .output()
        .expect("swiftc 可启动");
    assert!(
        compile.status.success(),
        "simulator 调用程序编译失败（{target}）：{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new("xcrun")
        .args(["simctl", "spawn", &udid, &bin_arg])
        .output()
        .expect("simctl spawn 可启动");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success()
            && stdout.contains("PAX_ADD_NEG=-2")
            && stdout.contains("PAX_ADD_WRAP=-294967296"),
        "simulator 调用对拍失败（{udid}）：stdout={stdout} stderr={}",
        String::from_utf8_lossy(&run.stderr)
    );
    eprintln!(
        "OK iOS simulator 调用对拍（{udid}）：paxAdd(-5, 3) = -2；\
         paxAdd(2_000_000_000, 2_000_000_000) = -294967296（i32 回绕）"
    );
}

/// `xcrun <args>` 的 stdout（去尾换行）；失败即 panic 带 stderr。
fn xcrun_stdout(args: &[&str]) -> String {
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

/// `simctl list devices available` 文本里首个 **iOS** 段落内的设备 UDID。
///
/// 只有 iOS 段落的设备能跑 simulator 目标的可执行文件（同一份列表里还有 watchOS / tvOS）。
fn first_ios_udid(list: &str) -> Option<String> {
    let mut in_ios = false;
    for line in list.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("--") {
            in_ios = trimmed.contains("iOS");
            continue;
        }
        if !in_ios {
            continue;
        }
        // 形如 `iPhone 15 (D1B6C0F1-…-…) (Shutdown)`
        if let Some((_, rest)) = trimmed.split_once('(')
            && let Some((udid, _)) = rest.split_once(')')
            && udid.len() == 36
            && udid.contains('-')
        {
            return Some(udid.to_string());
        }
    }
    None
}

/// 设备解析只认 iOS 段落（同一份列表里还有 watchOS / tvOS；非 iOS 设备跑不了 simulator 目标）。
#[test]
fn simctl_device_list_parsing_picks_ios_section() {
    let list = "== Devices ==\n\
                -- watchOS 10.5 --\n    Apple Watch Series 9 (AAAAAAAA-0000-4000-8000-000000000001) (Shutdown)\n\
                -- iOS 17.5 --\n    iPhone 15 (BBBBBBBB-0000-4000-8000-000000000002) (Booted)\n\
                \x20   iPhone 15 Pro (CCCCCCCC-0000-4000-8000-000000000003) (Shutdown)\n\
                -- tvOS 17.5 --\n    Apple TV (DDDDDDDD-0000-4000-8000-000000000004) (Shutdown)\n";
    assert_eq!(
        first_ios_udid(list).as_deref(),
        Some("BBBBBBBB-0000-4000-8000-000000000002")
    );
    assert_eq!(first_ios_udid("== Devices ==\n"), None);
}
