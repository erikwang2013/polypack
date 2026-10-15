//! T-M2-03 / T-M2-08 契约测试（mock 模式）：Swift 绑定链命令组装、切片暂存、
//! XCFramework 结构校验、工程骨架 / 复用与 archive→Payload 直压流水线。
//!
//! 本宿主（Linux）无 Apple 工具链（cargo Apple triple / lipo / xcodebuild / uniffi-bindgen）：
//! `package()` 必须返回 [`BundleError::ToolMissing`]（`poly::E_BND_0001`）——「真机执行
//! 阻塞（宿主）」的证据；macos-14 真机（T-M2-14 nightly）执行全链路真跑。
//! `plan()` 与结构校验为纯函数 / 只读，本宿主全程真跑断言。
//!
//! 输入用 `std::env::temp_dir()` 现造（每测试独立目录，`cargo test` 并行安全）。

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use poly_bundle::packagers::ios::{Ios, swift};
use poly_bundle::{BundleError, BundleInputs, IosInputs, Packager, ProductInfo, codes};
use poly_targets::{Host, TargetSpec};

/// 每个测试独占的临时目录。
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("poly-bundle-ios-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 写文件（自动建父目录）。
fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// 产品元数据（`poly init` 模板值）。
fn product() -> ProductInfo {
    ProductInfo {
        name: "Pax Notes".to_string(),
        version: "1.2.3".to_string(),
        identifier: "com.polypack.paxnotes".to_string(),
        manufacturer: "PolyPack".to_string(),
        description: "Pax 示例笔记应用".to_string(),
        homepage: None,
    }
}

/// 核心库基础名（`lib<name>.a` 的 `<name>`）。
const LIB: &str = "pax_core";

/// 目标声明（`ios` 目标：`hosts = [Macos]`，与 `poly-targets` 内建 spec 同形）。
fn spec(format: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub("ios", 1, &[Host::Macos]);
    spec.package.format = format.to_string();
    spec.package.builder = "ios-xcodebuild".to_string();
    spec
}

/// 假核心 crate（`target/<triple>/release/lib<lib>.a`）+ T-M2-01 Swift 绑定三件套。
fn core_crate(dir: &Path) -> PathBuf {
    let crate_dir = dir.join("core");
    for triple in [
        swift::DEVICE_TRIPLE,
        swift::SIM_ARM64_TRIPLE,
        swift::SIM_X86_64_TRIPLE,
    ] {
        write(
            &crate_dir
                .join("target")
                .join(triple)
                .join("release")
                .join(format!("lib{LIB}.a")),
            format!("!<arch> {triple}").as_bytes(),
        );
    }
    let bridge = swift::bridge_dir(dir);
    write(
        &bridge.join(format!("{LIB}.swift")),
        b"// uniffi bindings\n",
    );
    write(
        &bridge.join(format!("{LIB}FFI.h")),
        b"// FFI header\ntypedef struct PaxCore;",
    );
    write(
        &bridge.join(format!("{LIB}FFI.modulemap")),
        b"module pax_coreFFI {\n  header \"pax_coreFFI.h\"\n}\n",
    );
    crate_dir
}

/// 合规 XCFramework 结构（device + simulator 两切片，各一枚 `*.a` + `Headers/`）。
fn xcframework(dir: &Path) -> PathBuf {
    let root = dir.join("Pax Notes.xcframework");
    write(
        &root.join("Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>AvailableLibraries</key>
    <array>
        <dict>
            <key>LibraryIdentifier</key><string>{}</string>
            <key>LibraryPath</key><string>lib{LIB}.a</string>
            <key>HeadersPath</key><string>Headers</string>
        </dict>
        <dict>
            <key>LibraryIdentifier</key><string>{}</string>
            <key>LibraryPath</key><string>lib{LIB}.a</string>
            <key>HeadersPath</key><string>Headers</string>
        </dict>
    </array>
    <key>CFBundlePackageType</key><string>XFWK</string>
</dict>
</plist>
"#,
            swift::DEVICE_SLICE,
            swift::SIM_SLICE
        )
        .as_bytes(),
    );
    for slice in [swift::DEVICE_SLICE, swift::SIM_SLICE] {
        write(
            &root.join(slice).join(format!("lib{LIB}.a")),
            b"!<arch>\x00",
        );
        write(
            &root.join(slice).join("Headers/module.modulemap"),
            b"module pax_coreFFI {\n  header \"pax_coreFFI.h\"\n}\n",
        );
        write(
            &root.join(slice).join(format!("Headers/{LIB}FFI.h")),
            b"// FFI header",
        );
    }
    root
}

/// 完整打包输入（核心 crate + 合规 XCFramework，无宿主工程 → 走骨架生成）。
fn inputs(tag: &str) -> BundleInputs {
    let dir = temp_dir(tag);
    xcframework(&dir);
    let mut inputs = BundleInputs::new(product(), &dir);
    inputs.ios = Some(IosInputs {
        core_crate: Some(core_crate(&dir)),
        lib_name: Some(LIB.to_string()),
        host_project: None,
        bindings_dir: None,
    });
    inputs
}

// ------------------------------------------------- T-M2-03：Swift 绑定产物

/// 绑定链命令序（绑定已在位）：cargo ×3（Apple triple）→ lipo 合并 simulator →
/// xcodebuild -create-xcframework → lipo -info ×2；**不**调 uniffi-bindgen（三件套已由
/// T-M2-01 生成器 / 宿主工程随附）。
#[test]
fn swift_plan_assembles_cargo_lipo_chain_and_reuses_bindings() {
    let inputs = inputs("swift-plan");
    let plan = swift::plan(&inputs, &spec("ipa")).expect("plan");
    let lines: Vec<String> = plan.commands.iter().map(|c| c.line()).collect();

    assert_eq!(plan.commands.len(), 7, "{lines:#?}");
    assert!(
        !plan.commands.iter().any(|c| c.program == "uniffi-bindgen"),
        "三件套在位即复用，不依赖 CLI：{lines:#?}"
    );
    assert!(
        plan.scripts.is_empty(),
        "绑定链无脚本落盘：{:#?}",
        plan.scripts
    );

    for (index, triple) in [
        swift::DEVICE_TRIPLE,
        swift::SIM_ARM64_TRIPLE,
        swift::SIM_X86_64_TRIPLE,
    ]
    .iter()
    .enumerate()
    {
        let command = &plan.commands[index];
        assert_eq!(command.program, "cargo");
        assert_eq!(
            command.args,
            vec!["build", "--target", triple, "--release"],
            "{lines:#?}"
        );
        assert_eq!(
            command.cwd.as_deref(),
            Some(inputs.ios.as_ref().unwrap().core_crate.as_deref().unwrap()),
            "切片编译须在核心 crate 内（{triple}）"
        );
    }

    // simulator 双架构经 lipo 合并为切片内单一静态库。
    // 期望路径一律经生产同源函数（`built_lib` / `slice_lib_path`）构造：`join("a/b")`
    // 在 Windows 得 `\`+`/` 混合分隔符（`Path::join` 不归一化），与产物不一致。
    let core = inputs.ios.as_ref().unwrap().core_crate.as_ref().unwrap();
    let arg = |path: &Path| path.display().to_string();
    let command = &plan.commands[3];
    assert_eq!(command.program, "lipo", "合并须经 lipo：{lines:#?}");
    assert_eq!(
        command.args,
        vec![
            "-create".to_string(),
            arg(&swift::built_lib(core, swift::SIM_ARM64_TRIPLE, LIB)),
            arg(&swift::built_lib(core, swift::SIM_X86_64_TRIPLE, LIB)),
            "-output".to_string(),
            arg(&swift::slice_lib_path(
                &inputs.out_dir,
                swift::SIM_SLICE,
                LIB
            )),
        ],
        "合并输入须为两个 simulator 切片：{lines:#?}"
    );

    // 组装 + 切片架构枚举。
    let command = &plan.commands[4];
    assert_eq!(command.program, "xcodebuild");
    assert_eq!(
        command.args,
        vec![
            "-create-xcframework".to_string(),
            "-library".to_string(),
            arg(&swift::slice_lib_path(
                &inputs.out_dir,
                swift::DEVICE_SLICE,
                LIB
            )),
            "-headers".to_string(),
            arg(&swift::slices_dir(&inputs.out_dir)
                .join(swift::DEVICE_SLICE)
                .join("Headers")),
            "-library".to_string(),
            arg(&swift::slice_lib_path(
                &inputs.out_dir,
                swift::SIM_SLICE,
                LIB
            )),
            "-headers".to_string(),
            arg(&swift::slices_dir(&inputs.out_dir)
                .join(swift::SIM_SLICE)
                .join("Headers")),
            "-output".to_string(),
            arg(&swift::xcframework_path(&inputs)),
        ],
        "{lines:#?}"
    );
    for (offset, slice) in [swift::DEVICE_SLICE, swift::SIM_SLICE]
        .into_iter()
        .enumerate()
    {
        let command = &plan.commands[5 + offset];
        assert_eq!(command.program, "lipo");
        assert_eq!(
            command.args,
            vec![
                "-info".to_string(),
                arg(&swift::slice_lib_path(&inputs.out_dir, slice, LIB)),
            ],
            "切片架构须可枚举：{lines:#?}"
        );
    }
}

/// 绑定缺失时的命令序：lipo 合并之后插入 uniffi-bindgen（device 静态库 → Swift 三件套，
/// 与 T-M2-01 生成器同参：`--no-format` 常带、`--config` 见 `<绑定目录>/../uniffi.toml`）。
#[test]
fn swift_plan_generates_bindings_when_absent() {
    let inputs = inputs("swift-bindgen");
    let bindings = swift::bridge_dir(&inputs.out_dir);
    std::fs::remove_dir_all(&bindings).unwrap();
    let plan = swift::plan(&inputs, &spec("ipa")).expect("plan");
    let lines: Vec<String> = plan.commands.iter().map(|c| c.line()).collect();

    assert_eq!(plan.commands.len(), 8, "{lines:#?}");
    let core = inputs.ios.as_ref().unwrap().core_crate.as_ref().unwrap();
    let command = &plan.commands[4];
    assert_eq!(command.program, "uniffi-bindgen", "{lines:#?}");
    assert_eq!(
        command.args,
        vec![
            "generate".to_string(),
            "--library".to_string(),
            swift::built_lib(core, swift::DEVICE_TRIPLE, LIB)
                .display()
                .to_string(),
            "--language".to_string(),
            "swift".to_string(),
            "--out-dir".to_string(),
            bindings.display().to_string(),
            "--no-format".to_string(),
        ],
        "绑定源须为 device 切片静态库，参数与 T-M2-01 生成器同参：{lines:#?}"
    );
    assert_eq!(plan.commands[5].program, "xcodebuild", "{lines:#?}");

    // 配置文件在位（`<绑定目录>/../uniffi.toml`）时带上 `--config`：缺它则 uniffi ≥0.29
    // 退回 namespace 命名，header/modulemap 与 `lib<lib>.a` 不一致。
    let config = bindings.parent().unwrap().join("uniffi.toml");
    write(
        &config,
        b"[defaults.bindings.swift]\nmodule_name = \"pax_core\"\n",
    );
    let plan = swift::plan(&inputs, &spec("ipa")).expect("plan");
    let command = &plan.commands[4];
    assert_eq!(
        command.args.last().unwrap(),
        &config.display().to_string(),
        "{:#?}",
        command.args
    );
    assert!(
        command.args.iter().any(|arg| arg == "--config"),
        "{:#?}",
        command.args
    );
}

/// `IosInputs.bindings_dir` 覆盖（T-M2-01 生成器落点 `<工程根>/bridge/swift`）：
/// 三件套在外部位即复用，`out_dir` 下无需再有一份。
#[test]
fn bindings_dir_override_is_honored() {
    let inputs = inputs("bindings-dir");
    let external = inputs.out_dir.join("工程根/bridge/swift");
    std::fs::create_dir_all(external.parent().unwrap()).unwrap();
    std::fs::rename(swift::bridge_dir(&inputs.out_dir), &external).unwrap();
    let mut inputs = inputs;
    inputs.ios.as_mut().unwrap().bindings_dir = Some(external.clone());
    assert_eq!(swift::bindings_dir(&inputs), external);

    let plan = swift::plan(&inputs, &spec("ipa")).expect("plan");
    let lines: Vec<String> = plan.commands.iter().map(|c| c.line()).collect();
    assert_eq!(plan.commands.len(), 7, "外部位三件套即复用：{lines:#?}");
    assert!(
        !plan.commands.iter().any(|c| c.program == "uniffi-bindgen"),
        "{lines:#?}"
    );

    // 切片暂存从覆盖目录取头与模块图。
    swift::stage_slices(&inputs, &spec("ipa")).expect("stage_slices");
    let headers = swift::slices_dir(&inputs.out_dir)
        .join(swift::DEVICE_SLICE)
        .join("Headers");
    assert!(headers.join(format!("{LIB}FFI.h")).is_file(), "{headers:?}");
    assert!(headers.join("module.modulemap").is_file(), "{headers:?}");
}

/// 切片暂存：device 静态库落位；两切片各备 `Headers/module.modulemap`（uniffi 模块图更名）
/// 与 `Headers/<lib>FFI.h`。
#[test]
fn stage_slices_lands_modulemap_and_headers_for_both_slices() {
    let inputs = inputs("stage");
    let slices = swift::stage_slices(&inputs, &spec("ipa")).expect("stage_slices");
    assert_eq!(slices, swift::slices_dir(&inputs.out_dir));

    let core = inputs.ios.as_ref().unwrap().core_crate.as_ref().unwrap();
    let device_a = slices.join(swift::DEVICE_SLICE).join(format!("lib{LIB}.a"));
    assert_eq!(
        std::fs::read(&device_a).unwrap(),
        std::fs::read(swift::built_lib(core, swift::DEVICE_TRIPLE, LIB)).unwrap(),
        "device 切片须为核心 crate 产物本体"
    );
    for slice in [swift::DEVICE_SLICE, swift::SIM_SLICE] {
        let headers = slices.join(slice).join("Headers");
        assert!(
            headers.join("module.modulemap").is_file(),
            "Xcode 只认 module.modulemap（uniffi 产 <lib>FFI.modulemap）：{}",
            headers.display()
        );
        assert!(headers.join(format!("{LIB}FFI.h")).is_file(), "{slice}");
    }

    // 绑定三件套缺失 → 响亮缺输入（不静默出空 headers 的切片）。
    let mut broken = inputs.clone();
    let dir = temp_dir("stage-no-bindings");
    let mut ios = broken.ios.clone().unwrap();
    ios.core_crate = Some(core_crate(&dir));
    std::fs::remove_file(swift::bridge_dir(&dir).join(format!("{LIB}FFI.h"))).unwrap();
    broken.ios = Some(ios);
    broken.out_dir = dir;
    let err = swift::stage_slices(&broken, &spec("ipa")).expect_err("缺 FFI 头必须失败");
    assert_eq!(err.code(), codes::E_BND_0002);
}

/// 不变式：写盘命令的 `-output` 父目录须先由 `ensure_staging_dirs` 铺好。
///
/// macos-14 真跑实证：`lipo -create` 不建输出父目录（`… .a.lipo (No such file or
/// directory)`）；夹具不得自铺切片目录，否则结构测试会再次掩盖执行路径的缺目录。
/// （`uniffi-bindgen --out-dir` 自己会建目录——真跑实证，故不在本不变式内。）
#[test]
fn staging_dirs_cover_every_command_output_parent() {
    let inputs = inputs("staging-dirs");
    let plan = swift::plan(&inputs, &spec("ipa")).expect("plan");
    let sim_parent = swift::slices_dir(&inputs.out_dir).join(swift::SIM_SLICE);
    assert!(!sim_parent.is_dir(), "夹具不应预铺切片目录：{sim_parent:?}");

    let slices = swift::ensure_staging_dirs(&inputs).expect("铺目录骨架");
    swift::ensure_staging_dirs(&inputs).expect("幂等：复用路径 / 重跑再调不得失败");
    assert_eq!(slices, swift::slices_dir(&inputs.out_dir));

    for command in &plan.commands {
        for pair in command.args.windows(2) {
            if pair[0] == "-output" {
                let out = Path::new(&pair[1]);
                assert!(
                    out.parent().unwrap().is_dir(),
                    "`-output` 父目录未铺：{}（命令 {}）",
                    out.display(),
                    command.line()
                );
            }
        }
    }
}

/// XCFramework 结构校验：合规结构通过；切片标识漂移 / 缺库 / 多库 / 缺模块图逐一拒绝。
#[test]
fn xcframework_layout_validates_and_rejects_tampering() {
    let dir = temp_dir("layout");
    let root = xcframework(&dir);
    let layout = swift::validate_xcframework_layout(&dir).expect("合规结构应通过");
    assert_eq!(layout.root, root);
    assert_eq!(
        layout.slices,
        vec![
            swift::DEVICE_SLICE.to_string(),
            swift::SIM_SLICE.to_string()
        ]
    );
    assert_eq!(
        layout.libraries, layout.slices,
        "Info.plist 与切片目录须一致"
    );

    // 篡改 1：Info.plist 切片标识漂移（模拟 xcodebuild 版本差异 / 手改）。
    let plist = root.join("Info.plist");
    let original = std::fs::read_to_string(&plist).unwrap();
    std::fs::write(
        &plist,
        original.replace(swift::SIM_SLICE, "ios-x86_64-simulator"),
    )
    .unwrap();
    let err = swift::validate_xcframework_layout(&dir).expect_err("标识漂移必须失败");
    assert_eq!(err.code(), codes::E_BND_0003);
    std::fs::write(&plist, &original).unwrap();

    // 篡改 2：缺静态库（device 切片为空目录）。
    let sim_a = root.join(swift::DEVICE_SLICE).join(format!("lib{LIB}.a"));
    let renamed = sim_a.with_extension("a.bak");
    std::fs::rename(&sim_a, &renamed).unwrap();
    let err = swift::validate_xcframework_layout(&dir).expect_err("缺库必须失败");
    assert_eq!(err.code(), codes::E_BND_0002);
    std::fs::rename(&renamed, &sim_a).unwrap();

    // 篡改 3：切片内静态库不唯一（两个 .a → xcodebuild 输出形态异常）。
    write(
        &root.join(swift::DEVICE_SLICE).join("libextra.a"),
        b"!<arch>",
    );
    let err = swift::validate_xcframework_layout(&dir).expect_err("多库必须失败");
    assert_eq!(err.code(), codes::E_BND_0003);
    std::fs::remove_file(root.join(swift::DEVICE_SLICE).join("libextra.a")).unwrap();

    // 篡改 4：缺 module.modulemap（uniffi 原样拷贝 <lib>FFI.modulemap 的经典故障）。
    let modulemap = root.join(swift::SIM_SLICE).join("Headers/module.modulemap");
    let stashed = modulemap.with_extension("stash");
    std::fs::rename(&modulemap, &stashed).unwrap();
    let err = swift::validate_xcframework_layout(&dir).expect_err("缺模块图必须失败");
    assert_eq!(err.code(), codes::E_BND_0002);
    std::fs::rename(&stashed, &modulemap).unwrap();

    // 复原后仍通过（证明上面的失败来自篡改而非环境）。
    swift::validate_xcframework_layout(&dir).expect("复原后应再次通过");
}

/// 切片架构枚举（`lipo -archs`）：本宿主无 lipo → ToolMissing（真机执行阻塞）。
#[test]
fn lipo_archs_needs_macos_tooling() {
    if common::skip_if_present("lipo") {
        return;
    }
    let err = swift::lipo_archs(Path::new("/nonexistent/libpax_core.a")).expect_err("无 lipo");
    match err {
        BundleError::ToolMissing { tool, .. } => assert_eq!(tool, "lipo"),
        other => panic!("期望 ToolMissing，实得 {other:?}"),
    }
}

// ------------------------------------------------- T-M2-08：iOS Packager

/// 骨架路径：工程骨架四件套落盘 + archive→Payload 直压→校验命令序。
#[test]
fn ipa_plan_generates_skeleton_project_and_export_chain() {
    let inputs = inputs("ipa-plan");
    let plan = Ios.plan(&inputs, &spec("ipa")).expect("plan");

    let script = |suffix: &str| -> &str {
        &plan
            .scripts
            .iter()
            // 后缀按 `/` 书写：Windows 的 `\` 先归一化再比对
            .find(|s| {
                s.path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with(suffix)
            })
            .unwrap_or_else(|| panic!("缺脚本 {suffix}：{:#?}", plan.scripts))
            .contents
    };

    // 工程骨架：链接 XCFramework（PBXFrameworksBuildPhase）+ 编译 Swift 绑定（Sources）。
    let pbxproj = script("ios-skeleton/Pax Notes.xcodeproj/project.pbxproj");
    assert!(
        pbxproj.contains("wrapper.xcframework"),
        "须链接 XCFramework"
    );
    assert!(
        pbxproj.contains("Pax Notes.xcframework in Frameworks"),
        "须经 Frameworks 阶段链接"
    );
    assert!(
        pbxproj.contains(&format!("{LIB}.swift in Sources")),
        "绑定须进 Sources（编译期校验 modulemap）"
    );
    assert!(
        pbxproj.contains("IPHONEOS_DEPLOYMENT_TARGET = 16.0"),
        "{pbxproj}"
    );
    assert!(pbxproj.contains("CODE_SIGNING_ALLOWED = NO"), "未签名归档");
    assert!(pbxproj.contains("MARKETING_VERSION = 1.2.3"));
    assert!(pbxproj.contains("PRODUCT_BUNDLE_IDENTIFIER = com.polypack.paxnotes"));

    let info = script("ios-skeleton/Pax Notes/Info.plist");
    assert!(info.contains("<key>CFBundleIdentifier</key><string>com.polypack.paxnotes</string>"));
    assert!(info.contains("<key>CFBundleShortVersionString</key><string>1.2.3</string>"));

    let app_swift = script("ios-skeleton/Pax Notes/App.swift");
    assert!(
        app_swift.contains("@main") && app_swift.contains("struct PaxNotesApp: App"),
        "Swift 标识符须为合法形态：{app_swift}"
    );

    // 共享 scheme：`xcodebuild -scheme Pax Notes` 在裸 CI 上无自动生成，必须随骨架落盘。
    let scheme = script("xcshareddata/xcschemes/Pax Notes.xcscheme");
    assert!(
        scheme.contains("BlueprintIdentifier = \"C0DE0000000000000000A002\"")
            && scheme.contains("container:Pax Notes.xcodeproj"),
        "scheme 须指向骨架的 app target：{scheme}"
    );

    let export = script("ExportOptions.plist");
    assert!(export.contains("<key>method</key><string>development</string>"));

    // 命令序：archive（未签名）→ 归档 Payload 直压 → plutil -lint → xcrun lipo -info。
    let lines: Vec<String> = plan.commands.iter().map(|c| c.line()).collect();
    assert_eq!(plan.commands.len(), 5, "{lines:#?}");
    let arg = |path: &Path| path.display().to_string();
    // 期望路径一律经生产同源函数 / 逐段 join 构造（与 `build_commands` 同构）：
    // `join("a/b")` 在 Windows 得 `\`+`/` 混合分隔符，与产物不一致。
    let name = &inputs.product.name;
    let project = Ios.project_path(&inputs, &spec("ipa")).unwrap();
    let archive = inputs.out_dir.join(format!("{name}.xcarchive"));
    let export_dir = inputs.out_dir.join("export");
    let payload = export_dir.join("Payload");
    let app = payload.join(format!("{name}.app"));

    let command = &plan.commands[0];
    assert_eq!(
        command.args,
        vec![
            "archive".to_string(),
            "-project".to_string(),
            arg(&project),
            "-scheme".to_string(),
            "Pax Notes".to_string(),
            "-configuration".to_string(),
            "Release".to_string(),
            "-destination".to_string(),
            "generic/platform=iOS".to_string(),
            "-archivePath".to_string(),
            arg(&archive),
            // 未签名归档：签名位由 T-M2-10 poly-sign 链路接入
            "CODE_SIGNING_ALLOWED=NO".to_string(),
            "CODE_SIGNING_REQUIRED=NO".to_string(),
            "CODE_SIGN_IDENTITY=".to_string(),
        ],
        "{lines:#?}"
    );

    let command = &plan.commands[1];
    assert_eq!(command.program, "ditto", "归档产物拷入 Payload：{lines:#?}");
    assert_eq!(
        command.args,
        vec![
            arg(&archive
                .join("Products")
                .join("Applications")
                .join(format!("{name}.app"))),
            arg(&app),
        ],
        "{lines:#?}"
    );

    let command = &plan.commands[2];
    assert_eq!(command.program, "ditto");
    assert_eq!(
        command.args,
        vec![
            "-c".to_string(),
            "-k".to_string(),
            "--sequesterRsrc".to_string(),
            "--keepParent".to_string(),
            arg(&payload),
            arg(&export_dir.join(format!("{name}.ipa"))),
        ],
        "未签名 IPA = Payload 直压（选项须先于操作数）：{lines:#?}"
    );

    let command = &plan.commands[3];
    assert_eq!(command.program, "plutil");
    assert_eq!(
        command.args,
        vec!["-lint".to_string(), arg(&app.join("Info.plist")),],
        "Info.plist 语法校验：{lines:#?}"
    );
    let command = &plan.commands[4];
    assert_eq!(command.program, "xcrun");
    assert_eq!(
        command.args,
        vec![
            "lipo".to_string(),
            "-info".to_string(),
            arg(&app.join("Pax Notes")),
        ],
        "二进制架构校验：{lines:#?}"
    );
}

/// CLI 形态回归：骨架必须引用**实际**绑定落点，而非写死 `<out>/bridge/swift`。
///
/// dispatch 38693729015 实证：打包器直调（绑定在 `<out>/bridge/swift`）绿，CLI 腿（绑定在
/// `<工程根>/bridge/swift`，`out_dir` 在 `.polypack/packages/ios` 下）红——`xcodebuild
/// archive` 找不到构建输入退出 65。此处把 CLI 的目录形态钉住。
#[test]
fn skeleton_references_bindings_at_actual_bindings_dir() {
    let root = temp_dir("skeleton-refs");
    let out_dir = root.join(".polypack").join("packages").join("ios");
    std::fs::create_dir_all(&out_dir).unwrap();
    // `plan()` 只查 XCFramework 目录在场（布局校验在 `package()`）。
    std::fs::create_dir_all(out_dir.join(format!("{}.xcframework", product().name))).unwrap();
    let bindings = root.join("bridge").join("swift");
    write(&bindings.join(format!("{LIB}.swift")), b"// binding");

    let mut inputs = BundleInputs::new(product(), &out_dir);
    inputs.ios = Some(IosInputs {
        core_crate: None,
        lib_name: Some(LIB.to_string()),
        host_project: None,
        bindings_dir: Some(bindings.clone()),
    });
    let plan = Ios.plan(&inputs, &spec("ipa")).expect("plan");
    let pbxproj = &plan
        .scripts
        .iter()
        .find(|s| {
            s.path
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with("project.pbxproj")
        })
        .expect("骨架 pbxproj")
        .contents;

    let expected = bindings.join(format!("{LIB}.swift"));
    assert!(
        pbxproj.contains(&format!(
            "path = \"{}\"; sourceTree = \"<absolute>\"",
            expected.display()
        )),
        "绑定须按实际落点绝对引用（{expected:?}）：{pbxproj}"
    );
    assert!(
        !pbxproj.contains("../bridge/swift/"),
        "不得再写死相对 `out_dir` 的旧路径：{pbxproj}"
    );
}

/// 复用路径：宿主工程 `ios/<产品名>.xcodeproj` 在位即复用（不落骨架脚本），
/// ExportOptions.plist 仍须写出（T-M2-10 签名导出的输入）。
#[test]
fn ipa_plan_reuses_host_project_when_present() {
    let mut inputs = inputs("ipa-reuse");
    let host = inputs.out_dir.join("host");
    // 复用位与 `resolve_project` 同构（`host/ios/<产品名>.xcodeproj`），
    // 期望侧不再手写 `join("ios/…")`（Windows 分隔符会与产物不一致）。
    let project = host
        .join("ios")
        .join(format!("{}.xcodeproj", inputs.product.name));
    std::fs::create_dir_all(&project).unwrap();
    inputs.ios.as_mut().unwrap().host_project = Some(host.clone());

    let plan = Ios.plan(&inputs, &spec("ipa")).expect("plan");
    assert!(
        !plan
            .scripts
            .iter()
            .any(|s| s.path.to_string_lossy().ends_with("project.pbxproj")),
        "宿主工程在位不得生成骨架：{:#?}",
        plan.scripts
    );
    assert!(
        plan.scripts
            .iter()
            .any(|s| s.path.ends_with("ExportOptions.plist")),
        "ExportOptions.plist 与工程来源无关，必须写出"
    );
    assert_eq!(plan.commands[0].program, "echo", "复用留痕");
    assert!(
        plan.commands[0].line().contains("复用宿主工程"),
        "{}",
        plan.commands[0].line()
    );
    assert!(
        plan.commands[1]
            .line()
            .contains(&project.display().to_string()),
        "archive 须指向宿主工程：{}",
        plan.commands[1].line()
    );
    assert_eq!(Ios.project_path(&inputs, &spec("ipa")).unwrap(), project);
}

/// 缺桥产物 / 输入缺项：响亮拒绝（E_BND_0002），绝不「打包成功但装不上」。
#[test]
fn ipa_plan_requires_xcframework_and_ios_inputs() {
    let inputs = inputs("ipa-no-bridge");
    // 无 XCFramework（T-M2-03 未跑）→ 缺输入。
    let bare_dir = temp_dir("ipa-no-bridge-empty");
    let mut no_bridge = inputs.clone();
    no_bridge.out_dir = bare_dir.clone();
    let err = Ios
        .plan(&no_bridge, &spec("ipa"))
        .expect_err("缺 XCFramework 必须失败");
    assert_eq!(err.code(), codes::E_BND_0002);
    assert!(
        matches!(
            err,
            BundleError::MissingInput {
                field: "ios.xcframework",
                ..
            }
        ),
        "实得 {err:?}"
    );

    // 完全缺 `ios` 段 → 缺输入。
    let mut no_section = inputs.clone();
    no_section.ios = None;
    let err = Ios
        .plan(&no_section, &spec("ipa"))
        .expect_err("缺 ios 输入必须失败");
    assert_eq!(err.code(), codes::E_BND_0002);

    // 缺 lib_name（切片名无从派生）→ 缺输入。
    let mut no_lib = inputs.clone();
    no_lib.ios.as_mut().unwrap().lib_name = None;
    let err = Ios
        .plan(&no_lib, &spec("ipa"))
        .expect_err("缺 lib_name 必须失败");
    assert!(
        matches!(
            err,
            BundleError::MissingInput {
                field: "ios.lib_name",
                ..
            }
        ),
        "实得 {err:?}"
    );

    // 格式守卫：声明非 ipa 的目标不得走本打包器。
    let err = Ios
        .plan(&inputs, &spec("xcframework"))
        .expect_err("格式不符必须拒绝");
    assert_eq!(err.code(), codes::E_BND_0003);
}

/// 宿主门控：非 macOS 宿主 `package()` 在动手前返回 ToolMissing（无脚本落盘）。
#[test]
fn package_is_gated_to_macos_hosts() {
    if common::skip_if_present("xcodebuild") {
        return;
    }
    let inputs = inputs("package-gate");
    let err = Ios
        .package(&inputs, &spec("ipa"))
        .expect_err("非 macOS 宿主必须如实失败");
    assert_eq!(err.code(), codes::E_BND_0001);
    match err {
        BundleError::ToolMissing { packager, tool, .. } => {
            assert_eq!(packager, "ios");
            assert_eq!(tool, "xcodebuild");
        }
        other => panic!("期望 ToolMissing，实得 {other:?}"),
    }
    assert!(
        !inputs.out_dir.join("ios-skeleton").exists(),
        "宿主门控须先于落盘：{}",
        inputs.out_dir.display()
    );

    // T-M2-03 绑定链同样门控（Apple triple 无 Apple SDK 不可构建）。
    let err = swift::package(&inputs, &spec("ipa")).expect_err("非 macOS 宿主必须如实失败");
    assert_eq!(err.code(), codes::E_BND_0001);
}

/// 生成的 pbxproj 对象 ID 结构：恰 24 位十六进制、引用无悬空（Xcode 静默拒绝的经典故障）。
#[test]
fn skeleton_pbxproj_ids_are_wellformed_and_defined() {
    let inputs = inputs("pbxproj-ids");
    let text = Ios
        .plan(&inputs, &spec("ipa"))
        .unwrap()
        .scripts
        .iter()
        .find(|s| s.path.to_string_lossy().ends_with("project.pbxproj"))
        .unwrap()
        .contents
        .clone();

    let mut ids = BTreeSet::new();
    for (index, _) in text.match_indices("C0DE") {
        let id = &text[index..index + 24];
        assert!(
            id.chars().all(|ch| ch.is_ascii_hexdigit()),
            "对象 ID 须为 24 位十六进制，实得 {id:?}"
        );
        ids.insert(id.to_string());
    }
    assert!(ids.len() >= 10, "骨架对象数异常：{}", ids.len());
    for id in &ids {
        assert!(
            text.lines()
                .any(|line| line.contains(id.as_str()) && line.contains("= {")),
            "对象 {id} 被引用但未定义（悬空引用）"
        );
    }
}
