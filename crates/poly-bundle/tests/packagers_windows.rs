//! T-M1-05 契约测试（mock 模式）：WiX MSI 与 NSIS 的计划断言 + 宿主工具缺失契约。
//!
//! 本宿主（Linux）无 `candle` / `light` / `makensis`：`package()` 必须返回
//! [`BundleError::ToolMissing`]（`poly::E_BND_0001`）——「真机执行阻塞（宿主）」的证据；
//! 装了工具的真实宿主（windows-2022）如实跳过该契约而非误报。
//!
//! 输入用 `std::env::temp_dir()` 现造（每测试独立目录，`cargo test` 并行安全）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use poly_bundle::packagers::windows::{Nsis, WixMsi};
use poly_bundle::{
    BareProduct, BundleError, BundleInputs, BundlePlan, Icon, IconKind, Packager, ProductInfo,
    codes, exec,
};
use poly_targets::{Host, TargetSpec};

/// 每个测试独占的临时目录。
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("poly-bundle-win-{tag}-{}", std::process::id()));
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

/// Windows 裸产物（T-M1-03 布局：`<root>/<bin>.exe` + WebView 资源同目录）。
fn inputs(tag: &str, ico: bool) -> BundleInputs {
    let dir = temp_dir(tag);
    let root = dir.join("bare");
    write(&root.join("pax-notes.exe"), b"MZ\x90\x00");
    write(&root.join("resources/index.html"), b"<html></html>");
    write(
        &root.join("RUNTIME-DEPENDENCIES.txt"),
        "运行时依赖：\n- WebView2 Runtime（Win10 1803+ 常已预装）\n".as_bytes(),
    );

    let mut inputs = BundleInputs::new(product(), &dir);
    inputs.bare = Some(BareProduct {
        root: root.clone(),
        executable: root.join("pax-notes.exe"),
        host: Host::Win,
    });
    if ico {
        let path = dir.join("pax.ico");
        write(&path, b"\x00\x00\x01\x00");
        inputs.icons.push(Icon {
            path,
            kind: IconKind::Ico,
            size: None,
        });
    }
    inputs
}

/// 目标声明（`package.format` 由调用方指定，用于格式守卫与正常路径）。
fn spec(format: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub("windows", 1, &[Host::Win]);
    spec.package.format = format.to_string();
    spec.package.builder = "wix".to_string();
    spec
}

/// 计划中的脚本文本（按扩展名取）。
fn script(plan: &BundlePlan, suffix: &str) -> String {
    plan.scripts
        .iter()
        .find(|s| s.path.to_string_lossy().ends_with(suffix))
        .unwrap_or_else(|| panic!("计划缺少 {suffix} 脚本：{:?}", plan.scripts))
        .contents
        .clone()
}

/// 抽取所有 `<Component Id="...">`（测试内最小解析，不引 XML 依赖）。
fn component_ids(wxs: &str) -> Vec<String> {
    wxs.split("<Component Id=\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap_or_default().to_string())
        .collect()
}

/// 抽取 `UpgradeCode`。
fn upgrade_code_of(wxs: &str) -> String {
    wxs.split("UpgradeCode=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn msi_plan_is_wix_v3_with_tree_shortcut_and_ui() {
    let inputs = inputs("msi-plan", true);
    let plan = WixMsi.plan(&inputs, &spec("msi")).expect("plan");
    let wxs = script(&plan, ".wxs");

    for needle in [
        "<Wix xmlns=\"http://schemas.microsoft.com/wix/2006/wi\">",
        "<Product Id=\"*\" Name=\"Pax Notes\" Version=\"1.2.3\" Manufacturer=\"PolyPack\" UpgradeCode=\"",
        "Language=\"1033\" Codepage=\"65001\">",
        "InstallerVersion=\"500\"",
        "<MajorUpgrade ",
        "<MediaTemplate EmbedCab=\"yes\"/>",
        "<Directory Id=\"TARGETDIR\" Name=\"SourceDir\">",
        "<Directory Id=\"ProgramFiles64Folder\">",
        "<Directory Id=\"INSTALLDIR\" Name=\"Pax Notes\">",
        "Name=\"resources\"",
        "<File ",
        "<Icon Id=\"ProductIcon\"",
        "<Property Id=\"ARPPRODUCTICON\" Value=\"ProductIcon\"/>",
        "<Shortcut ",
        "On=\"uninstall\"",
        "RegistryValue Root=\"HKCU\" Key=\"Software\\Pax Notes\" Name=\"installed\" Type=\"integer\" Value=\"1\" KeyPath=\"yes\"",
        "<Feature Id=\"MainFeature\" Level=\"1\"",
        "<UIRef Id=\"WixUI_InstallDir\"/>",
        "<Property Id=\"WIXUI_INSTALLDIR\" Value=\"INSTALLDIR\"/>",
    ] {
        assert!(wxs.contains(needle), "wxs 缺少 `{needle}`：\n{wxs}");
    }

    // 裸产物内每个文件都有对应 `<File Source=...>`（绝对路径写进 Source）。
    let root = inputs.bare.as_ref().unwrap().root.clone();
    for rel in [
        "pax-notes.exe",
        "resources/index.html",
        "RUNTIME-DEPENDENCIES.txt",
    ] {
        let source = format!("Source=\"{}\"", root.join(rel).display());
        assert!(wxs.contains(&source), "wxs 缺少文件条目 {source}：\n{wxs}");
    }

    // Component Id 唯一，且全部被 Feature 引用。
    let ids = component_ids(&wxs);
    assert!(ids.len() >= 4, "组件数不足（3 文件 + 快捷方式）：{ids:?}");
    let unique: BTreeSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "Component Id 重复：{ids:?}");
    for id in &ids {
        assert!(
            wxs.contains(&format!("<ComponentRef Id=\"{id}\"/>")),
            "Feature 未引用组件 {id}"
        );
    }

    // 命令链：candle → light（x64 + en-US，与 tauri-bundler 命名一致）。
    assert_eq!(plan.commands.len(), 2);
    assert_eq!(plan.commands[0].program, "candle");
    assert!(
        plan.commands[0].line().contains("-arch x64"),
        "{}",
        plan.commands[0].line()
    );
    assert!(
        plan.commands[0].line().ends_with(".wxs"),
        "{}",
        plan.commands[0].line()
    );
    assert_eq!(plan.commands[1].program, "light");
    assert!(plan.commands[1].line().contains("-cultures:en-us"));
    assert!(
        plan.commands[1]
            .line()
            .contains("Pax Notes_1.2.3_x64_en-US.msi"),
        "{}",
        plan.commands[1].line()
    );
}

#[test]
fn upgrade_code_is_stable_per_identifier() {
    let inputs = inputs("msi-upgrade-code", true);
    let first = WixMsi.plan(&inputs, &spec("msi")).expect("plan");
    let second = WixMsi.plan(&inputs, &spec("msi")).expect("plan");
    assert_eq!(first, second, "同一输入两次 plan 必须逐字节一致");

    let code = upgrade_code_of(&script(&first, ".wxs"));
    assert_eq!(code.len(), 36, "UpgradeCode 应为 GUID：{code}");

    let mut other = inputs.clone();
    other.product.identifier = "com.polypack.other".to_string();
    let other_plan = WixMsi.plan(&other, &spec("msi")).expect("plan");
    let other_code = upgrade_code_of(&script(&other_plan, ".wxs"));
    assert_ne!(code, other_code, "换标识符必须换 UpgradeCode");
}

#[test]
fn nsis_plan_covers_install_and_uninstall_sections() {
    let inputs = inputs("nsis-plan", true);
    let plan = Nsis.plan(&inputs, &spec("nsis")).expect("plan");
    let nsi = script(&plan, ".nsi");

    for needle in [
        "Unicode true",
        "!include \"MUI2.nsh\"",
        "Name \"Pax Notes\"",
        "OutFile \"Pax Notes_1.2.3_x64-setup.exe\"",
        "InstallDir \"$PROGRAMFILES64\\Pax Notes\"",
        "VIProductVersion \"1.2.3.0\"",
        "VIAddVersionKey \"ProductName\" \"Pax Notes\"",
        "VIAddVersionKey \"FileVersion\" \"1.2.3.0\"",
        "VIAddVersionKey \"CompanyName\" \"PolyPack\"",
        "VIAddVersionKey \"ProductVersion\" \"1.2.3\"",
        "Icon \"",
        "UninstallIcon \"",
        "!insertmacro MUI_PAGE_DIRECTORY",
        "!insertmacro MUI_PAGE_INSTFILES",
        "!insertmacro MUI_UNPAGE_CONFIRM",
        "!insertmacro MUI_UNPAGE_INSTFILES",
        "!insertmacro MUI_LANGUAGE \"English\"",
        "Section \"Install\"",
        "SetOutPath \"$INSTDIR\"",
        "WriteUninstaller \"$INSTDIR\\uninstall.exe\"",
        "$SMPROGRAMS",
        "$DESKTOP",
        "Section \"Uninstall\"",
        "Delete \"$INSTDIR\\uninstall.exe\"",
        "RMDir /r \"$INSTDIR\"",
        "DeleteRegKey HKCU \"Software\\Pax Notes\"",
    ] {
        assert!(nsi.contains(needle), "nsi 缺少 `{needle}`：\n{nsi}");
    }

    // 整树安装：裸产物根 + Windows 分隔符。
    let root = inputs.bare.as_ref().unwrap().root.clone();
    let tree = format!(
        "File /r \"{}\\*.*\"",
        root.display().to_string().replace('/', "\\")
    );
    assert!(nsi.contains(&tree), "nsi 缺少整树安装：{tree}\n{nsi}");

    assert_eq!(plan.commands.len(), 1);
    assert_eq!(plan.commands[0].program, "makensis");
    assert!(
        plan.commands[0].line().ends_with(".nsi"),
        "{}",
        plan.commands[0].line()
    );
    assert!(
        !plan.commands[0].line().contains("setup.exe"),
        "makensis 自动补 .exe，不得手工拼接：{}",
        plan.commands[0].line()
    );
}

#[test]
fn ico_is_optional_and_reported_as_skipped() {
    let inputs = inputs("msi-no-ico", false);
    let plan = WixMsi
        .plan(&inputs, &spec("msi"))
        .expect("ICO 缺失不得让 plan 失败");
    let wxs = script(&plan, ".wxs");
    assert!(
        !wxs.contains("ProductIcon"),
        "无 ICO 时不得写图标行：\n{wxs}"
    );
    assert!(
        plan.skipped.iter().any(|s| s.tool == "icons(ICO)"),
        "跳过项须登记：{:?}",
        plan.skipped
    );

    let nsi = script(&Nsis.plan(&inputs, &spec("nsis")).unwrap(), ".nsi");
    assert!(!nsi.contains("Icon \""), "无 ICO 时不得写 Icon 行：\n{nsi}");
}

#[test]
fn format_guard_rejects_mismatched_spec() {
    let inputs = inputs("windows-format-guard", false);
    for packager in [&WixMsi as &dyn Packager, &Nsis] {
        let err = packager
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
}

#[test]
fn package_fails_with_tool_missing_on_host_without_wix() {
    let inputs = inputs("msi-package", true);
    if exec::tool_present("candle") {
        eprintln!("SKIP: 宿主已装 WiX（candle），ToolMissing 契约仅适用于无工具宿主");
        return;
    }
    let err = WixMsi
        .package(&inputs, &spec("msi"))
        .expect_err("宿主无 candle/light：必须如实失败");
    assert_eq!(err.code(), "poly::E_BND_0001");
    match err {
        BundleError::ToolMissing { tool, .. } => assert_eq!(tool, "candle"),
        other => panic!("期望 ToolMissing，实得 {other:?}"),
    }
    // 脚本先落盘（失败可手动重跑）：证据即 out_dir 内的 .wxs。
    assert!(
        inputs.out_dir.join("pax-notes.wxs").is_file(),
        "脚本应先落盘：{}",
        inputs.out_dir.display()
    );
}

#[test]
fn package_fails_with_tool_missing_on_host_without_nsis() {
    let inputs = inputs("nsis-package", true);
    if exec::tool_present("makensis") {
        eprintln!("SKIP: 宿主已装 NSIS（makensis），ToolMissing 契约仅适用于无工具宿主");
        return;
    }
    let err = Nsis
        .package(&inputs, &spec("nsis"))
        .expect_err("宿主无 makensis：必须如实失败");
    assert_eq!(err.code(), "poly::E_BND_0001");
    match err {
        BundleError::ToolMissing { tool, .. } => assert_eq!(tool, "makensis"),
        other => panic!("期望 ToolMissing，实得 {other:?}"),
    }
}
