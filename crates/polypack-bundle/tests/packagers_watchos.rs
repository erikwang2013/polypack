//! T-M7-06 契约测试（mock 模式）：watchOS 伴生包 `xcodebuild archive` → 可签 `.app`，
//! 复用 M2 Apple 宿主门控（ToolMissing `polypack::E_BND_0001`，R3 远程农场 macOS worker）。
//!
//! 本宿主（Linux）无 xcodebuild：`package()` 必须在动手前返回 ToolMissing 且不落盘；
//! `plan()` 为纯组装，任意宿主真跑断言。macOS 宿主复跑：`xcodebuild -project
//! PolyPackWatch.xcodeproj -scheme PolyPackWatch -destination 'generic/platform=watchOS' build`。

mod common;

use std::path::PathBuf;

use polypack_bundle::packagers::watchos::{DESTINATION, PROJECT, SCHEME, Watchos};
use polypack_bundle::{BundleError, BundleInputs, IosInputs, Packager, ProductInfo, codes};
use polypack_targets::{Host, Registry, TargetSpec};

/// 每个测试独占的临时目录。
fn temp_dir(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("polypack-bundle-watchos-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 产品元数据（`polypack init --target watchos` 的模板值）。
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

/// 目标声明（内建 watchos spec 的最小同形）。
fn spec() -> TargetSpec {
    let mut spec = TargetSpec::new_stub("watchos", 6, &[Host::Macos]);
    spec.package.format = "watch".to_string();
    spec.package.builder = "watchos-xcodebuild".to_string();
    spec
}

/// 打包输入：模板工程（`PolyPackWatch.xcodeproj/`）随开关决定是否落盘。
fn inputs(tag: &str, with_project: bool) -> BundleInputs {
    let dir = temp_dir(tag);
    if with_project {
        std::fs::create_dir_all(dir.join(PROJECT)).unwrap();
    }
    let mut inputs = BundleInputs::new(product(), &dir);
    inputs.ios = Some(IosInputs {
        host_project: Some(dir.clone()),
        ..IosInputs::default()
    });
    inputs
}

/// 产物落点（签名 / 校验共用）。
fn app(inputs: &BundleInputs) -> PathBuf {
    inputs
        .out_dir
        .join("export")
        .join("Payload")
        .join("Pax Notes.app")
}

/// 打包器注册：`watchos` + `watch` 由本打包器服务，且不挤占 ios / ipa 链。
#[test]
fn watchos_packager_is_registered_for_watch_format() {
    let packager = polypack_bundle::find("watchos", "watch").expect("watchos/watch 应命中内建打包器");
    assert_eq!(packager.id(), "watchos");
    assert_eq!(packager.format(), "watch");

    let builtin = Registry::builtin()
        .get("watchos")
        .expect("内建 watchos 目标")
        .clone();
    assert_eq!(builtin.package.format, "watch");
    assert!(
        polypack_bundle::for_spec(&builtin).is_some(),
        "目标声明的 format 应解析到打包器"
    );

    // 服务范围闭合：Apple 链仍只服务 ios / tvos（watch 包不冒充 ipa）。
    assert!(polypack_bundle::find("ios", "watch").is_none());
    assert!(polypack_bundle::find("watchos", "ipa").is_none());
}

/// 命令序：xcodebuild archive（未签名）→ ditto 归档 app 到 Payload → plutil 校验清单。
#[test]
fn plan_assembles_archive_to_payload_chain() {
    let inputs = inputs("plan", true);
    let plan = Watchos.plan(&inputs, &spec()).expect("plan");
    assert!(plan.scripts.is_empty(), "本链不落盘脚本");
    assert!(plan.skipped.is_empty(), "本链无跳过项");

    let programs: Vec<&str> = plan.commands.iter().map(|c| c.program.as_str()).collect();
    assert_eq!(programs, ["xcodebuild", "ditto", "plutil"]);

    let archive = &plan.commands[0];
    assert_eq!(archive.args[0], "archive");
    let line = archive.line();
    for needle in [
        format!("-project {}", inputs.out_dir.join(PROJECT).display()),
        format!("-scheme {SCHEME}"),
        format!("-destination {DESTINATION}"),
        "-configuration Release".to_string(),
        "CODE_SIGNING_ALLOWED=NO".to_string(),
    ] {
        assert!(line.contains(&needle), "archive 命令缺 {needle}：{line}");
    }
    let archive_path = inputs
        .out_dir
        .join("Pax Notes.xcarchive")
        .display()
        .to_string();
    let flag = archive
        .args
        .iter()
        .position(|arg| arg == "-archivePath")
        .expect("archive 命令带 -archivePath");
    assert_eq!(
        archive.args.get(flag + 1),
        Some(&archive_path),
        "归档落点 = <out>/<产品>.xcarchive"
    );

    let ditto = &plan.commands[1];
    // Windows 的 Path display 用反斜杠；断言按逻辑路径（/）归一后比较。
    assert!(
        ditto.args[0]
            .replace('\\', "/")
            .ends_with("Products/Applications/PolyPackWatch.app"),
        "来源应为归档 app：{}",
        ditto.args[0]
    );
    assert_eq!(ditto.args[1], app(&inputs).display().to_string());

    let lint = &plan.commands[2];
    assert_eq!(lint.args[0], "-lint");
    assert_eq!(
        lint.args[1],
        app(&inputs).join("Info.plist").display().to_string(),
        "plutil 校验 Payload 内 Info.plist"
    );
}

/// 缺输入如实报 MissingInput（`E_BND_0002`）：缺 ios 段 / 缺模板工程各一条。
#[test]
fn missing_project_is_reported_as_missing_input() {
    let no_ios = BundleInputs::new(product(), temp_dir("missing-ios"));
    let err = Watchos
        .plan(&no_ios, &spec())
        .expect_err("缺 ios 段必须报错");
    assert_eq!(err.code(), codes::E_BND_0002);
    match err {
        BundleError::MissingInput { packager, field } => {
            assert_eq!(packager, "watchos");
            assert_eq!(field, "ios.host_project");
        }
        other => panic!("期望 MissingInput，实得 {other:?}"),
    }

    let inputs = inputs("missing-project", false);
    let err = Watchos
        .plan(&inputs, &spec())
        .expect_err("缺模板工程必须报错");
    assert_eq!(err.code(), codes::E_BND_0002);
    match err {
        BundleError::MissingInput { packager, field } => {
            assert_eq!(packager, "watchos");
            assert_eq!(field, "watchos.project");
        }
        other => panic!("期望 MissingInput，实得 {other:?}"),
    }
}

/// 格式声明不符响亮拒绝（`E_BND_0003`），防「按 watch 目标产出别的格式」。
#[test]
fn format_mismatch_is_rejected() {
    let inputs = inputs("format", true);
    let mut wrong = spec();
    wrong.package.format = "ipa".to_string();
    let err = Watchos.plan(&inputs, &wrong).expect_err("格式不符必须拒绝");
    assert_eq!(err.code(), codes::E_BND_0003);
}

/// 宿主门控（R3）：非 macOS 宿主 `package()` 在动手前返回 ToolMissing（无脚本 / 产物落盘）。
#[test]
fn package_is_gated_to_macos_hosts() {
    if common::skip_if_present("xcodebuild") {
        return;
    }
    let inputs = inputs("gate", true);
    let err = Watchos
        .package(&inputs, &spec())
        .expect_err("非 macOS 宿主必须如实失败");
    assert_eq!(err.code(), codes::E_BND_0001);
    match err {
        BundleError::ToolMissing {
            packager,
            tool,
            hint,
        } => {
            assert_eq!(packager, "watchos");
            assert_eq!(tool, "xcodebuild");
            assert!(
                hint.contains("远程农场") && hint.contains("R3"),
                "修复提示须点名远程农场 macOS worker：{hint}"
            );
        }
        other => panic!("期望 ToolMissing，实得 {other:?}"),
    }
    assert!(
        !inputs.out_dir.join("export").exists(),
        "宿主门控须先于落盘：{}",
        inputs.out_dir.display()
    );
}
