//! T-M2-14 真跑（`POLY_CONTRACT_REAL=1` + macOS 宿主 + Xcode + rustup Apple triple）：
//! 夹具核心静态库 → cargo 交叉编译三 triple → lipo 合并切片 → `xcodebuild -create-xcframework`
//! → 工程骨架 → `xcodebuild archive` → 归档 `Payload/` 直压 → 未签名 IPA（含结构断言）。
//!
//! 默认 mock 模式 / 非 macOS 宿主 / 缺 Xcode 或 Apple triple 一律 SKIP 后返回（绝不红）——
//! 与 M1 契约测试同一 SKIP 纪律（`crates/poly-cli/tests/contract_m1.rs`）。这一份是
//! 「iOS 链路在真机上确实跑通」的证据；mock 侧的命令组装断言见 `packagers_ios.rs`。
//!
//! 未签名 IPA 走 `ditto -c -k` 直压（不经 `exportArchive`）：无 Team 的未签名归档在
//! `-exportArchive` 下必失败（macos-14 真跑实证「No Team Found in Archive」）。
//! `exportArchive` 留给 T-M2-10 的签名导出（`POLY_IOS_EXPORT_METHOD` / ExportOptions.plist）。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use poly_bundle::packagers::ios::{Ios, swift};
use poly_bundle::{BundleInputs, IosInputs, Packager, ProductInfo};
use poly_targets::{Host, TargetSpec};

/// 夹具核心库基础名（= fixtures/ios_core 的 crate 名）。
const LIB: &str = "pax_core";
/// 产品名（`lib<Pax Notes>.a` 之外的产物命名基准）。
const PRODUCT: &str = "Pax Notes";

/// 真跑开关：`POLY_CONTRACT_REAL=1`（或 `true`）；默认 mock。
fn real_mode() -> bool {
    matches!(
        std::env::var("POLY_CONTRACT_REAL").as_deref(),
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

/// 真跑前置：模式 + 宿主 + 工具链，任一不满足即打印 SKIP 原因并返回 false。
fn prerequisites() -> bool {
    if !real_mode() {
        eprintln!("SKIP：mock 模式（默认）；真跑设 POLY_CONTRACT_REAL=1");
        return false;
    }
    if !cfg!(target_os = "macos") {
        eprintln!("SKIP：iOS 链路需 macOS 宿主（Xcode / lipo / xcodebuild）");
        return false;
    }
    if common::skip_without_any(&["xcodebuild", "xcrun", "lipo"]) {
        return false;
    }
    for triple in [
        swift::DEVICE_TRIPLE,
        swift::SIM_ARM64_TRIPLE,
        swift::SIM_X86_64_TRIPLE,
    ] {
        if !rustup_target_installed(triple) {
            eprintln!("SKIP：rustup 未装 {triple}（rustup target add {triple}）");
            return false;
        }
    }
    true
}

/// 工作目录（每测试独占，结束即删）。
fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("poly-ios-real-{tag}-{}", std::process::id()));
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

/// 目标声明（`ios`：`hosts = [Macos]`，与内建 spec 同形）。
fn spec() -> TargetSpec {
    let mut spec = TargetSpec::new_stub("ios", 1, &[Host::Macos]);
    spec.package.format = "ipa".to_string();
    spec.package.builder = "ios-xcodebuild".to_string();
    spec
}

/// 真跑全链路：T-M2-03 切片链 + T-M2-08 archive / Payload 直压 → 未签名 IPA。
#[test]
fn ios_end_to_end_builds_unsigned_ipa() {
    if !prerequisites() {
        return;
    }
    let dir = work_dir("e2e");
    let inputs = inputs(&dir, fixture_crate(&dir));
    let spec = spec();

    // T-M2-03：真 cargo 交叉编译 ×3 → lipo →（绑定已在位，跳过 bindgen）→ create-xcframework → lipo -info。
    let xcframework = swift::package(&inputs, &spec).expect("XCFramework 真跑");
    assert_eq!(xcframework.format, "xcframework");
    let layout = swift::validate_xcframework_layout(&inputs.out_dir).expect("结构校验");
    assert_eq!(layout.slices.len(), 2, "{layout:?}");

    // 切片架构真枚举：device = arm64；simulator = arm64 + x86_64（lipo 合并结果）。
    let device = swift::lipo_archs(&swift::slice_lib_path(
        &inputs.out_dir,
        swift::DEVICE_SLICE,
        LIB,
    ))
    .expect("lipo -archs device");
    assert_eq!(device, vec!["arm64".to_string()], "device 切片架构");
    let sim = swift::lipo_archs(&swift::slice_lib_path(
        &inputs.out_dir,
        swift::SIM_SLICE,
        LIB,
    ))
    .expect("lipo -archs simulator");
    let mut sim_sorted = sim.clone();
    sim_sorted.sort();
    assert_eq!(
        sim_sorted,
        vec!["arm64".to_string(), "x86_64".to_string()],
        "simulator 切片须双架构合一：{sim:?}"
    );

    // T-M2-08：骨架工程 + 真 xcodebuild archive + 归档 Payload 直压。
    let ipa = Ios.package(&inputs, &spec).expect("IPA 真跑");
    assert_eq!(ipa.format, "ipa");
    assert!(
        ipa.path.extension().is_some_and(|ext| ext == "ipa"),
        "{ipa:?}"
    );

    // 产物结构：IPA 是 zip，必含 Payload/<产品>.app 与其 Info.plist / 可执行文件。
    let names = poly_bundle::zipwrite::entry_names(&ipa.path).expect("IPA 条目清单");
    let app = format!("Payload/{PRODUCT}.app");
    for entry in [format!("{app}/Info.plist"), format!("{app}/{PRODUCT}")] {
        assert!(
            names.iter().any(|name| name == &entry),
            "IPA 缺 {entry}：{names:?}"
        );
    }
    // 真跑产物留档：不删工作目录（CI 由 `留产物` 步上传；本地留一份供人工核验）。
    eprintln!(
        "OK iOS 真跑：{}（{} 字节，{} 条目）",
        ipa.path.display(),
        ipa.bytes,
        names.len()
    );
}
