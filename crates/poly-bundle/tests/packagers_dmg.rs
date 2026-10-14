//! T-M1-06 契约测试（mock 模式）：DMG 暂存布局、hdiutil 命令与宿主工具缺失契约。
//!
//! 本宿主（Linux）无 `hdiutil`：`package()` 必须返回 [`BundleError::ToolMissing`]
//! （`poly::E_BND_0001`）——「真机执行阻塞（宿主）」的证据；macOS 真机（macos-14）上
//! `hdiutil verify` 与挂载/卸载冒烟由每夜模式承担。
//!
//! 输入用 `std::env::temp_dir()` 现造（每测试独立目录，`cargo test` 并行安全）。

mod common;

use std::path::{Path, PathBuf};

use poly_bundle::packagers::dmg::Dmg;
use poly_bundle::{BareProduct, BundleError, BundleInputs, Packager, ProductInfo, codes, exec};
use poly_targets::{Host, TargetSpec};

/// 每个测试独占的临时目录。
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("poly-bundle-dmg-{tag}-{}", std::process::id()));
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

/// macOS 裸产物（T-M1-03 布局：`<root>/<产品名>.app`）。
fn inputs(tag: &str) -> BundleInputs {
    let dir = temp_dir(tag);
    let root = dir.join("bare");
    let app = root.join("Pax Notes.app");
    write(&app.join("Contents/MacOS/pax-notes"), b"\xcf\xfa\xed\xfe");
    write(&app.join("Contents/Resources/index.html"), b"<html></html>");
    write(
        &app.join("Contents/Resources/RUNTIME-DEPENDENCIES.txt"),
        "运行时依赖：\n- 无外部运行时（WebKit 随系统提供）\n".as_bytes(),
    );

    let mut inputs = BundleInputs::new(product(), &dir);
    inputs.bare = Some(BareProduct {
        root,
        executable: app.join("Contents/MacOS/pax-notes"),
        host: Host::Macos,
    });
    inputs
}

/// 目标声明（`package.format` 由调用方指定，用于格式守卫与正常路径）。
fn spec(format: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub("macos", 1, &[Host::Macos]);
    spec.package.format = format.to_string();
    spec.package.builder = "hdiutil".to_string();
    spec
}

#[test]
fn stage_builds_drag_and_drop_layout() {
    // DMG 暂存要建 `Applications -> /Applications` 符号链接（非 Unix 宿主跳过）。
    if common::skip_unless_unix("DMG 暂存布局的符号链接") {
        return;
    }
    let inputs = inputs("stage");
    let stage = Dmg.stage(&inputs, &spec("dmg")).expect("stage");

    assert_eq!(stage, inputs.out_dir.join("dmg-stage"));
    assert!(
        stage
            .join("Pax Notes.app/Contents/MacOS/pax-notes")
            .is_file(),
        ".app 未拷入暂存目录：{}",
        stage.display()
    );
    assert!(
        stage
            .join("Pax Notes.app/Contents/Resources/index.html")
            .is_file()
    );

    let link = stage.join("Applications");
    let meta = std::fs::symlink_metadata(&link).expect("Applications 链接");
    assert!(meta.is_symlink(), "Applications 必须是符号链接：{meta:?}");
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        Path::new("/Applications")
    );
}

#[test]
fn stage_requires_an_app_bundle() {
    let mut inputs = inputs("stage-empty");
    // 裸产物根里没有 `*.app`：空 DMG 会「打包成功但装不了东西」，必须快速失败。
    let root = inputs.out_dir.join("bare-empty");
    write(&root.join("pax-notes"), b"MZ");
    inputs.bare.as_mut().unwrap().root = root;
    let err = Dmg
        .stage(&inputs, &spec("dmg"))
        .expect_err("无 .app 必须失败");
    assert_eq!(err.code(), codes::E_BND_0002);
}

#[test]
fn plan_emits_hdiutil_create_and_verify() {
    let inputs = inputs("plan");
    let plan = Dmg.plan(&inputs, &spec("dmg")).expect("plan");

    let readme = plan
        .scripts
        .iter()
        // Path::ends_with 按组件比较，Windows 的 `\` 分隔符同样成立。
        .find(|s| s.path.ends_with("dmg-stage/README.txt"))
        .expect("暂存目录内 README");
    assert!(
        readme.contents.contains("拖入 Applications"),
        "README 须说明拖拽安装布局：{}",
        readme.contents
    );

    assert_eq!(plan.commands.len(), 2);
    let create = plan.commands[0].line();
    assert_eq!(plan.commands[0].program, "hdiutil");
    assert!(
        create.starts_with("hdiutil create -volname \"Pax Notes 1.2.3\" -srcfolder "),
        "{create}"
    );
    assert!(create.contains("-ov -format UDZO "), "{create}");
    assert!(create.ends_with("Pax Notes_1.2.3_x64.dmg\""), "{create}");

    assert_eq!(plan.commands[1].program, "hdiutil");
    assert!(
        plan.commands[1].line().starts_with("hdiutil verify ")
            && plan.commands[1].line().ends_with(".dmg\""),
        "{}",
        plan.commands[1].line()
    );
}

#[test]
fn format_guard_rejects_mismatched_spec() {
    let inputs = inputs("format-guard");
    let err = Dmg
        .plan(&inputs, &spec("appimage"))
        .expect_err("格式不符必须拒绝");
    assert_eq!(err.code(), codes::E_BND_0003);
    assert_eq!(err.code(), "poly::E_BND_0003");
    assert!(
        matches!(
            err,
            BundleError::InvalidInput {
                field: "spec.package.format",
                ..
            }
        ),
        "实得 {err:?}"
    );
}

#[test]
fn package_fails_with_tool_missing_on_host_without_hdiutil() {
    // 暂存要先建 `Applications -> /Applications` 符号链接：非 Unix 宿主会先报
    // ToolMissing("symlink")（而非 hdiutil），故本断言只在 Unix 宿主成立。
    if common::skip_unless_unix("DMG 暂存布局的符号链接") {
        return;
    }
    let inputs = inputs("package");
    if exec::tool_present("hdiutil") {
        eprintln!("SKIP: 宿主已有 hdiutil（macOS），ToolMissing 契约仅适用于无工具宿主");
        return;
    }
    let err = Dmg
        .package(&inputs, &spec("dmg"))
        .expect_err("宿主无 hdiutil：必须如实失败");
    assert_eq!(err.code(), "poly::E_BND_0001");
    match err {
        BundleError::ToolMissing { tool, .. } => assert_eq!(tool, "hdiutil"),
        other => panic!("期望 ToolMissing，实得 {other:?}"),
    }
    // 脚本先落盘（失败可手动重跑）：证据即暂存目录内的 README。
    assert!(
        inputs.out_dir.join("dmg-stage/README.txt").is_file(),
        "脚本应先落盘：{}",
        inputs.out_dir.display()
    );
}
