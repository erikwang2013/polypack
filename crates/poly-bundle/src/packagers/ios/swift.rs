//! Swift 绑定产物（T-M2-03）：核心静态库编译为 device + simulator 切片的 XCFramework。
//!
//! 链路（`plan()` 的纯命令组装，`package()` 按序执行）：
//!
//! ```text
//! ensure_staging_dirs()                                    # 先铺切片目录骨架（lipo 不建父目录）
//! cargo build --target aarch64-apple-ios       --release   # device 切片
//! cargo build --target aarch64-apple-ios-sim   --release   # simulator（Apple Silicon）
//! cargo build --target x86_64-apple-ios        --release   # simulator（Intel）
//! lipo -create <sim arm64 .a> <sim x86_64 .a> -output <切片暂存>/…-simulator/lib<lib>.a
//! uniffi-bindgen generate --library <device .a> --language swift --out-dir <out>/bridge/swift
//! —— stage_slices()：device .a 拷入切片目录；两切片各备 Headers/（modulemap 更名）——
//! xcodebuild -create-xcframework -library … -headers …（device + simulator 两切片）
//! lipo -info <两切片 .a>                                    # 切片架构可枚举校验
//! ```
//!
//! 产物布局（T-M2-03 约定，`validate_xcframework_layout` 逐项校验）：
//!
//! ```text
//! <out>/bridge/swift/<lib>.swift · <lib>FFI.h · <lib>FFI.modulemap   # T-M2-01 生成器产物
//! <out>/xcframework-slices/ios-arm64/{lib<lib>.a, Headers/{module.modulemap, <lib>FFI.h}}
//! <out>/xcframework-slices/ios-arm64_x86_64-simulator/{…}
//! <out>/<产品名>.xcframework/Info.plist + 两切片目录
//! ```
//!
//! **绑定来源**：`bridge/swift/` 三件套已在位（T-M2-01 生成器产物 / 宿主工程随附）即直接复用，
//! 不依赖 `uniffi-bindgen` CLI；缺失才要求 CLI 现场生成（与 T-M2-08 复用宿主工程同一取舍）。
//!
//! **宿主门控**：切片编译需 macOS（cargo Apple triple 标准库 + lipo / xcodebuild）——
//! 非 macOS 宿主 `package()` 在动手前返回 [`BundleError::ToolMissing`]（绝不静默降级）；
//! `plan()` / `validate_xcframework_layout()` / [`lipo_archs`] 为纯组装与只读校验，
//! 任意宿主可跑（mock 契约测试的基础）。macos-14 CI 上真跑由 T-M2-14 nightly 覆盖。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::{BundleError, io_err};
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, IosInputs};
use crate::packager::{Artifact, BundlePlan, Command};

use super::{IOS, ios_inputs, require_macos};

/// XCFramework 设备切片目录名（`<platform>-<arch>` 惯例）。
pub const DEVICE_SLICE: &str = "ios-arm64";
/// XCFramework 模拟器切片目录名（arm64 + x86_64 合一，经 `lipo -create`）。
pub const SIM_SLICE: &str = "ios-arm64_x86_64-simulator";
/// 设备静态库 triple。
pub const DEVICE_TRIPLE: &str = "aarch64-apple-ios";
/// 模拟器（Apple Silicon）triple。
pub const SIM_ARM64_TRIPLE: &str = "aarch64-apple-ios-sim";
/// 模拟器（Intel）triple。
pub const SIM_X86_64_TRIPLE: &str = "x86_64-apple-ios";

/// Swift 绑定落点（`uniffi-bindgen --out-dir`）。
///
/// T-M2-01（m2-core）接口约定：宿主工程与 T-M2-03 切片都以本目录为绑定的唯一来源。
pub fn bridge_dir(out_dir: &Path) -> PathBuf {
    out_dir.join("bridge/swift")
}

/// 本次打包实际采用的绑定目录：`IosInputs.bindings_dir`（T-M2-01 生成器落点
/// `<工程根>/bridge/swift`）优先，缺省回落 [`bridge_dir`]（`out_dir` 下的老路径）。
pub fn bindings_dir(inputs: &BundleInputs) -> PathBuf {
    inputs
        .ios
        .as_ref()
        .and_then(|ios| ios.bindings_dir.clone())
        .unwrap_or_else(|| bridge_dir(&inputs.out_dir))
}

/// 切片暂存目录（`xcodebuild -create-xcframework` 的输入）。
pub fn slices_dir(out_dir: &Path) -> PathBuf {
    out_dir.join("xcframework-slices")
}

/// XCFramework 产物路径（`<out>/<产品名>.xcframework`）。
pub fn xcframework_path(inputs: &BundleInputs) -> PathBuf {
    inputs
        .out_dir
        .join(format!("{}.xcframework", inputs.product.name))
}

/// T-M2-01 生成器产物三件套（Swift 绑定 + FFI 头 + modulemap）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwiftBindings {
    /// `<lib>.swift`（宿主工程的 Swift 绑定）。
    pub swift: PathBuf,
    /// `<lib>FFI.h`（C ABI 头）。
    pub header: PathBuf,
    /// `<lib>FFI.modulemap`（Xcode 切片内更名 `module.modulemap`）。
    pub modulemap: PathBuf,
}

/// XCFramework 结构校验结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XcFrameworkLayout {
    /// `.xcframework` 根目录。
    pub root: PathBuf,
    /// 实际切片目录名（应与 [`DEVICE_SLICE`] / [`SIM_SLICE`] 集合一致）。
    pub slices: Vec<String>,
    /// Info.plist `AvailableLibraries` 中的 `LibraryIdentifier` 集合。
    pub libraries: Vec<String>,
}

// ---------------------------------------------------------------- 计划与执行

/// 生成 Swift 绑定产物的打包计划（纯组装：只读输入元数据，不写盘、不执行工具）。
pub fn plan(inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
    let (pre, post) = command_split(inputs, spec)?;
    let mut plan = BundlePlan::new();
    for command in pre.into_iter().chain(post) {
        plan = plan.with_command(command);
    }
    Ok(plan)
}

/// 执行全链路并汇总 XCFramework 产物（非 macOS 宿主在动手前响亮拒绝）。
pub fn package(inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
    // 宿主门控最前置：Apple triple 无法在非 macOS 宿主构建（无 Apple SDK），先于一切校验。
    require_macos("xcodebuild")?;
    let (pre, post) = command_split(inputs, spec)?;

    // 目录骨架必须先于 pre：`lipo -create` 不建输出父目录（macos-14 真跑实证：
    // `… .a.lipo (No such file or directory)`）。`stage_slices` 只纯拷贝文件且必须排在
    // cargo 之后（拷的是 cargo 产物），故不能靠它铺目录。
    ensure_staging_dirs(inputs)?;
    exec::run_all(&pre)?;
    stage_slices(inputs, spec)?;
    exec::run_all(&post)?;

    let layout = validate_xcframework_layout(&inputs.out_dir)?;
    fsutil::dir_artifact(
        IOS,
        "xcframework",
        &layout.root,
        vec![
            format!(
                "切片：{}（device）+ {}（simulator，arm64/x86_64 合一）；Swift 绑定见 {}",
                DEVICE_SLICE,
                SIM_SLICE,
                bindings_dir(inputs).display()
            ),
            "真机执行阻塞（宿主）：切片编译需 macOS（cargo Apple triple + lipo/xcodebuild）；\
             macos-14 CI nightly 真跑（T-M2-14）"
                .to_string(),
            "最小宿主工程 xcodebuild 链接冒烟：ios packager 骨架（T-M2-08）在 macOS 宿主消费本产物"
                .to_string(),
        ],
    )
}

/// 切片目录骨架（两切片各自 `Headers/`，`out_dir` 随之建立；幂等）。
///
/// `lipo -create` 与 `xcodebuild -create-xcframework` 都不创建输出父目录，故写盘命令
/// 之前必须先铺骨架；缺目录时命令报错落点晦涩（`.a.lipo (No such file or directory)`）。
pub fn ensure_staging_dirs(inputs: &BundleInputs) -> Result<PathBuf, BundleError> {
    for slice in [DEVICE_SLICE, SIM_SLICE] {
        let headers = slice_headers(&inputs.out_dir, slice);
        std::fs::create_dir_all(&headers).map_err(io_err(&headers))?;
    }
    Ok(slices_dir(&inputs.out_dir))
}

/// 切片暂存：device 静态库拷入切片目录；两切片各备 `Headers/`。
///
/// `module.modulemap` 是 Xcode 唯一认的模块图名：uniffi 生成的 `<lib>FFI.modulemap`
/// 在此更名落位，`-headers` 指向的目录才能被 `import <lib>FFI` 解析。
pub fn stage_slices(inputs: &BundleInputs, spec: &TargetSpec) -> Result<PathBuf, BundleError> {
    let ios = ios_inputs(inputs, spec)?;
    let core_crate = core_crate(ios)?;
    let lib = lib_name(ios)?;
    let out_dir = &inputs.out_dir;

    fsutil::copy_file(
        &built_lib(core_crate, DEVICE_TRIPLE, lib),
        &slice_lib_path(out_dir, DEVICE_SLICE, lib),
        false,
    )?;
    let bindings = swift_binding_files(&bindings_dir(inputs), lib)?;
    for slice in [DEVICE_SLICE, SIM_SLICE] {
        let headers = slice_headers(out_dir, slice);
        fsutil::copy_file(
            &bindings.header,
            &headers.join(format!("{lib}FFI.h")),
            false,
        )?;
        fsutil::copy_file(
            &bindings.modulemap,
            &headers.join("module.modulemap"),
            false,
        )?;
    }
    Ok(slices_dir(out_dir))
}

/// 命令组装：`pre`（cargo ×3 → lipo 合并 → uniffi-bindgen）× `stage_slices()` × `post`（组装 + 校验）。
fn command_split(
    inputs: &BundleInputs,
    spec: &TargetSpec,
) -> Result<(Vec<Command>, Vec<Command>), BundleError> {
    let ios = ios_inputs(inputs, spec)?;
    let core_crate = core_crate(ios)?;
    let lib = lib_name(ios)?;
    let out_dir = &inputs.out_dir;

    let mut pre = vec![
        Command::new("cargo", ["build", "--target", DEVICE_TRIPLE, "--release"]).in_dir(core_crate),
        Command::new(
            "cargo",
            ["build", "--target", SIM_ARM64_TRIPLE, "--release"],
        )
        .in_dir(core_crate),
        Command::new(
            "cargo",
            ["build", "--target", SIM_X86_64_TRIPLE, "--release"],
        )
        .in_dir(core_crate),
        Command::new(
            "lipo",
            [
                "-create".into(),
                path_arg(&built_lib(core_crate, SIM_ARM64_TRIPLE, lib)),
                path_arg(&built_lib(core_crate, SIM_X86_64_TRIPLE, lib)),
                "-output".into(),
                path_arg(&slice_lib_path(out_dir, SIM_SLICE, lib)),
            ],
        ),
    ];
    // 三件套已在位即复用（T-M2-01 生成器 / 宿主工程随附）：不因缺 CLI 而拒绝已有产物。
    let bindings = bindings_dir(inputs);
    if swift_binding_files(&bindings, lib).is_err() {
        // 现场补生成与 T-M2-01 生成器同参：uniffi ≥0.29 的 `--config` 只认全局形态
        // `[defaults.bindings.swift]`（缺配置则 header/modulemap 退回 namespace 命名，
        // 与 `lib<lib>.a` 不一致）；`--no-format` 保证产物字节确定（在场格式化工具不改变输出）。
        let mut args = vec![
            "generate".to_string(),
            "--library".to_string(),
            path_arg(&built_lib(core_crate, DEVICE_TRIPLE, lib)),
            "--language".to_string(),
            "swift".to_string(),
            "--out-dir".to_string(),
            path_arg(&bindings),
            "--no-format".to_string(),
        ];
        // 配置在绑定目录的上一级（`<根>/bridge/uniffi.toml`，T-M2-01 约定）；缺则按 uniffi 默认。
        if let Some(config) = bindings
            .parent()
            .map(|dir| dir.join("uniffi.toml"))
            .filter(|path| path.is_file())
        {
            args.push("--config".to_string());
            args.push(path_arg(&config));
        }
        pre.push(Command::new("uniffi-bindgen", args));
    }

    let post = vec![
        Command::new(
            "xcodebuild",
            [
                "-create-xcframework".into(),
                "-library".into(),
                path_arg(&slice_lib_path(out_dir, DEVICE_SLICE, lib)),
                "-headers".into(),
                path_arg(&slice_headers(out_dir, DEVICE_SLICE)),
                "-library".into(),
                path_arg(&slice_lib_path(out_dir, SIM_SLICE, lib)),
                "-headers".into(),
                path_arg(&slice_headers(out_dir, SIM_SLICE)),
                "-output".into(),
                path_arg(&xcframework_path(inputs)),
            ],
        ),
        // 切片架构可枚举校验（T-M2-03 验收：lipo -info）
        Command::new(
            "lipo",
            [
                "-info".into(),
                path_arg(&slice_lib_path(out_dir, DEVICE_SLICE, lib)),
            ],
        ),
        Command::new(
            "lipo",
            [
                "-info".into(),
                path_arg(&slice_lib_path(out_dir, SIM_SLICE, lib)),
            ],
        ),
    ];
    Ok((pre, post))
}

// ---------------------------------------------------------------- 只读校验

/// 校验 XCFramework 结构（纯只读，任意宿主可跑）：
/// `Info.plist` 的 `LibraryIdentifier` 集合须等于两切片；每切片恰一个 `*.a`，
/// 且 `Headers/module.modulemap` 与至少一个 `*.h` 在位。
pub fn validate_xcframework_layout(root: &Path) -> Result<XcFrameworkLayout, BundleError> {
    let bundle = find_xcframework(root)?;
    let info = bundle.join("Info.plist");
    let text = std::fs::read_to_string(&info).map_err(|_| BundleError::MissingInput {
        packager: IOS,
        field: "*.xcframework/Info.plist",
    })?;
    assert_identifiers(&library_identifiers(&text))?;

    let mut slices = Vec::new();
    for slice in [DEVICE_SLICE, SIM_SLICE] {
        let dir = bundle.join(slice);
        match archive_count(&dir)? {
            1 => {}
            0 => {
                return Err(BundleError::MissingInput {
                    packager: IOS,
                    field: "*.xcframework/<slice>/*.a",
                });
            }
            n => {
                return Err(BundleError::InvalidInput {
                    packager: IOS,
                    field: "*.xcframework/<slice>/*.a",
                    value: n.to_string(),
                    reason: format!("切片 {slice} 内静态库不唯一（每个切片恰一个）"),
                });
            }
        }
        let modulemap = dir.join("Headers/module.modulemap");
        if !modulemap.is_file() {
            return Err(BundleError::MissingInput {
                packager: IOS,
                field: "*.xcframework/<slice>/Headers/module.modulemap",
            });
        }
        if !has_extension(&dir.join("Headers"), "h") {
            return Err(BundleError::MissingInput {
                packager: IOS,
                field: "*.xcframework/<slice>/Headers/*.h",
            });
        }
        slices.push(slice.to_string());
    }
    Ok(XcFrameworkLayout {
        root: bundle,
        slices,
        libraries: library_identifiers(&text),
    })
}

/// 校验 T-M2-01 生成器的 Swift 绑定三件套（`<lib>.swift` / `<lib>FFI.h` / `<lib>FFI.modulemap`）。
pub fn swift_binding_files(dir: &Path, lib_name: &str) -> Result<SwiftBindings, BundleError> {
    let candidates = SwiftBindings {
        swift: dir.join(format!("{lib_name}.swift")),
        header: dir.join(format!("{lib_name}FFI.h")),
        modulemap: dir.join(format!("{lib_name}FFI.modulemap")),
    };
    for path in [&candidates.swift, &candidates.header, &candidates.modulemap] {
        if !path.is_file() {
            return Err(BundleError::MissingInput {
                packager: IOS,
                field: "bridge/swift/*（uniffi-bindgen 三件套）",
            });
        }
    }
    Ok(candidates)
}

/// 枚举静态库切片架构（`lipo -archs`；T-M2-03 验收的切片枚举校验）。
///
/// 非 macOS 宿主无 `lipo` → [`BundleError::ToolMissing`]（真机执行阻塞）。
pub fn lipo_archs(lib: &Path) -> Result<Vec<String>, BundleError> {
    let output = crate::exec::run(&Command::new("lipo", ["-archs".to_string(), path_arg(lib)]))?;
    let text = String::from_utf8_lossy(&output.stdout);
    // 旧版 lipo 形态为 `Architectures in the fat file: <path> are: arm64 x86_64`；
    // `-archs` 新形态直接输出架构列表。两者都取 `are:` 之后的空白分隔词。
    let listed = text.rsplit("are:").next().unwrap_or(&text);
    let archs: Vec<String> = listed.split_whitespace().map(str::to_string).collect();
    if archs.is_empty() {
        return Err(BundleError::InvalidInput {
            packager: IOS,
            field: "lipo -archs",
            value: text.trim().to_string(),
            reason: "lipo 未输出架构列表（静态库形态异常？）".to_string(),
        });
    }
    Ok(archs)
}

// ---------------------------------------------------------------- 路径与输入

/// 核心 crate 根（`ios.core_crate`；由 [`ios_inputs`] 保证在位）。
fn core_crate(ios: &IosInputs) -> Result<&Path, BundleError> {
    ios.core_crate.as_deref().ok_or(BundleError::MissingInput {
        packager: IOS,
        field: "ios.core_crate",
    })
}

/// 核心库基础名（`ios.lib_name`；由 [`ios_inputs`] 保证非空）。
fn lib_name(ios: &IosInputs) -> Result<&str, BundleError> {
    ios.lib_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .ok_or(BundleError::MissingInput {
            packager: IOS,
            field: "ios.lib_name",
        })
}

/// cargo 产物静态库（`<core_crate>/target/<triple>/release/lib<lib>.a`）。
///
/// `pub` 供契约测试复用同一构造：逐段 `join`（Windows 分隔符与产物一致）。
pub fn built_lib(core_crate: &Path, triple: &str, lib_name: &str) -> PathBuf {
    core_crate
        .join("target")
        .join(triple)
        .join("release")
        .join(format!("lib{lib_name}.a"))
}

/// 切片内静态库路径（`xcodebuild -create-xcframework` 输入 / `lipo -info` 目标）。
pub fn slice_lib_path(out_dir: &Path, slice: &str, lib_name: &str) -> PathBuf {
    slices_dir(out_dir)
        .join(slice)
        .join(format!("lib{lib_name}.a"))
}

/// 切片 Headers 目录（`-headers` 参数落点）。
fn slice_headers(out_dir: &Path, slice: &str) -> PathBuf {
    slices_dir(out_dir).join(slice).join("Headers")
}

/// 参数形态（路径一律显式传参，避免拼接成 shell 串）。
fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

// ---------------------------------------------------------------- 极小解析器

/// 定位 `<root>` 下唯一的 `*.xcframework`。
fn find_xcframework(root: &Path) -> Result<PathBuf, BundleError> {
    let entries = std::fs::read_dir(root).map_err(io_err(root))?;
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.extension().is_some_and(|ext| ext == "xcframework"))
        .collect();
    found.sort();
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(BundleError::MissingInput {
            packager: IOS,
            field: "*.xcframework",
        }),
        n => Err(BundleError::InvalidInput {
            packager: IOS,
            field: "*.xcframework",
            value: n.to_string(),
            reason: "输出目录内 XCFramework 不唯一（每次打包只产一枚）".to_string(),
        }),
    }
}

/// 切片目录根的 `*.a` 数量（不递归：Cargo 产物与 xcodebuild 输入都在切片根）。
fn archive_count(slice_dir: &Path) -> Result<usize, BundleError> {
    let entries = std::fs::read_dir(slice_dir).map_err(io_err(slice_dir))?;
    Ok(entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "a"))
        .count())
}

/// 目录内是否存在某扩展名的文件。
fn has_extension(dir: &Path, extension: &str) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.path().extension().is_some_and(|ext| ext == extension))
    })
}

/// 扫描 plist 文本中的 `LibraryIdentifier` 值（`<key>…</key>` 后接首个 `<string>…</string>`）。
fn library_identifiers(plist: &str) -> Vec<String> {
    const KEY: &str = "<key>LibraryIdentifier</key>";
    let mut out = Vec::new();
    let mut rest = plist;
    while let Some(pos) = rest.find(KEY) {
        rest = &rest[pos + KEY.len()..];
        let Some(open) = rest.find("<string>") else {
            break;
        };
        let after = &rest[open + "<string>".len()..];
        let Some(end) = after.find("</string>") else {
            break;
        };
        out.push(after[..end].trim().to_string());
        rest = &after[end..];
    }
    out
}

/// 断言 `LibraryIdentifier` 集合恰为两切片（缺 / 多 / 名称漂移都响亮拒绝）。
fn assert_identifiers(actual: &[String]) -> Result<(), BundleError> {
    let expected = [DEVICE_SLICE, SIM_SLICE];
    let mut sorted = actual.to_vec();
    sorted.sort();
    if sorted == expected {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: IOS,
        field: "*.xcframework/Info.plist:LibraryIdentifier",
        value: actual.join(","),
        reason: format!(
            "切片标识须恰为 {}（device + simulator）",
            expected.join(" + ")
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 解析器非空转：xcodebuild 生成形态（紧凑 `key`+`string`）与多切片。
    #[test]
    fn library_identifier_scanner_reads_compact_plist() {
        let plist = r#"<key>AvailableLibraries</key>
<array>
    <dict>
        <key>LibraryIdentifier</key><string>ios-arm64</string>
        <key>LibraryPath</key><string>libpax_core.a</string>
        <key>SupportedArchitectures</key><array><string>arm64</string></array>
    </dict>
    <dict>
        <key>LibraryIdentifier</key><string>ios-arm64_x86_64-simulator</string>
    </dict>
</array>"#;
        assert_eq!(
            library_identifiers(plist),
            vec!["ios-arm64", "ios-arm64_x86_64-simulator"]
        );
        assert!(library_identifiers("").is_empty());
    }

    /// 切片标识漂移必失败（篡改证明断言非空转）。
    #[test]
    fn identifier_mismatch_is_rejected() {
        assert!(assert_identifiers(&[DEVICE_SLICE.to_string(), SIM_SLICE.to_string()]).is_ok());
        assert!(
            assert_identifiers(&[DEVICE_SLICE.to_string()]).is_err(),
            "缺 simulator 切片应失败"
        );
        assert!(
            assert_identifiers(&[DEVICE_SLICE.to_string(), "ios-x86_64-simulator".to_string()])
                .is_err(),
            "切片名漂移应失败"
        );
    }
}
