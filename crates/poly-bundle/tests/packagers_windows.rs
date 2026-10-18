//! T-M1-05 契约测试：WiX MSI 与 NSIS 的计划断言 + 宿主工具缺失契约 + **真机深检**。
//!
//! 无 `candle` / `light` / `makensis` 的宿主（Linux / macOS 腿）：`package()` 必须返回
//! [`BundleError::ToolMissing`]（`poly::E_BND_0001`）——「真机执行阻塞（宿主）」的证据。
//! windows-2022 腿预装 WiX v3.14（`%WIX%\bin` 在 Machine PATH 上），走**真跑深检**：
//! `candle` + `light` 出真 MSI → `msiexec /a` 管理安装解出载荷 → 字节级核对。
//! NSIS 由 CI 安装后走真编译：`makensis` 出非空 PE 安装器（结构断言）；安装 → 启动 →
//! 卸载冒烟尚未实现（见 T-M1-05 条目）。
//!
//! 输入用 `std::env::temp_dir()` 现造（每测试独立目录，`cargo test` 并行安全）。

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use poly_bundle::packagers::windows::{Nsis, WixMsi, nsis_escape, xml_escape};
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
fn msi_plan_is_wix_v3_with_tree_shortcut_and_arp_icon() {
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
    ] {
        assert!(wxs.contains(needle), "wxs 缺少 `{needle}`：\n{wxs}");
    }
    // UI 腿退出（负向守卫）：WixUIExtension 的 en-us wxl 会把 DB 码页钉死 1252，中文描述
    // 在 light 链接期报 LGHT0311（windows-2022 真跑实证）；恢复 UI 须先带 65001 本地化方案。
    for absent in ["WixUI", "WIXUI_INSTALLDIR"] {
        assert!(
            !wxs.contains(absent),
            "wxs 不应再含 `{absent}`（LGHT0311 根因）：\n{wxs}"
        );
    }

    // 裸产物内每个文件都有对应 `<File Source=...>`（绝对路径写进 Source）。
    let root = inputs.bare.as_ref().unwrap().root.clone();
    for rel in [
        "pax-notes.exe",
        "resources/index.html",
        "RUNTIME-DEPENDENCIES.txt",
    ] {
        // 与产物同构：Source 由目录遍历逐段 `dir.join(file_name)` 拼出，测试也逐段 join；
        // 整串 `root.join("resources/index.html")` 在 Windows 上会产出混合分隔符而不匹配。
        // 转义走打包器同一函数：宿主路径合法可含 `&`/`>`（如 `C:\Users\R&D\…`）。
        let abs = rel.split('/').fold(root.clone(), |p, seg| p.join(seg));
        let source = format!("Source=\"{}\"", xml_escape(&abs.display().to_string()));
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

    // 命令链：candle → light（x64）；light 不带 culture/扩展（见上 LGHT0311 负向守卫）。
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
    for absent in ["-cultures", "WixUIExtension"] {
        assert!(
            !plan.commands[1].line().contains(absent),
            "light 不应再带 `{absent}`（LGHT0311 根因）：{}",
            plan.commands[1].line()
        );
    }
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

    // 整树安装：裸产物根 + Windows 分隔符 + NSIS 转义（与生成器同源，不自建转义表）。
    let root = inputs.bare.as_ref().unwrap().root.clone();
    let tree = format!(
        "File /r \"{}\\*.*\"",
        nsis_escape(&root.display().to_string().replace('/', "\\"))
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

/// 宿主路径含 `$`（Windows 合法形态，如 `C:\$Recycle.Bin\…`）：脚本里的字面量须写 `$$`，
/// 否则 makensis 把它当变量（`$Recycle` 未定义报错 / `$0`… 静默按寄存器替换）。
///
/// 本机可证层=脚本文本；makensis 真编译面由 `nsis_real_build_produces_pe_installer`
/// 在装有 NSIS 的宿主（CI windows-2022）覆盖。
#[test]
fn nsi_script_escapes_dollar_in_host_paths() {
    let inputs = inputs("nsis-esc$ape", true);
    let nsi = script(&Nsis.plan(&inputs, &spec("nsis")).unwrap(), ".nsi");

    assert!(
        nsi.contains("nsis-esc$$ape"),
        "路径里的 `$` 须转义为 `$$`：\n{nsi}"
    );
    assert!(
        !nsi.contains("nsis-esc$ape"),
        "不得残留未转义的 `$`（makensis 会当变量）：\n{nsi}"
    );
    // Icon / UninstallIcon 两处同一路径同源转义。
    assert_eq!(
        nsi.matches("nsis-esc$$ape").count(),
        3,
        "裸产物根 + Icon + UninstallIcon 三处都须转义：\n{nsi}"
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

/// 真跑深检（Windows 宿主）：`candle` + `light` 出真 MSI → `msiexec /a` 管理安装解出载荷
/// 做**字节级**核对（文件表真的能解出裸产物，而不是只生成了一个文件）。
///
/// 非 Windows 宿主无 WiX（Linux / macOS 腿）显式 skip；该腿的 ToolMissing 契约由相邻用例覆盖。
#[test]
fn msi_real_build_embeds_the_bare_product_payload() {
    if common::skip_without_any(&["candle", "light", "msiexec"]) {
        return;
    }
    let mut inputs = inputs("msi-real", false);
    inputs.icons.push(Icon {
        // 真 ICO（T-M1-14 派生资产）：安装器图标走 `<Icon>` + ARPPRODUCTICON 真嵌入。
        path: common::fixture("pax-notes.ico"),
        kind: IconKind::Ico,
        size: Some(256),
    });

    let artifact = WixMsi
        .package(&inputs, &spec("msi"))
        .expect("candle + light 真跑");
    assert_eq!(artifact.format, "msi");
    assert!(artifact.bytes > 0, "产物为空");

    // MSI 是 OLE 复合文档（CFB）：魔数证明产物形态真实，而非改名空壳。
    let bytes = std::fs::read(&artifact.path).expect("读 MSI");
    assert_eq!(
        &bytes[..8],
        &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
        "MSI 须为 CFB 复合文档：{}",
        artifact.path.display()
    );

    // 载荷核对：管理安装（`/a`，非系统安装、不改系统状态）把文件表解到 TARGETDIR。
    let target = inputs.out_dir.join("admin-install");
    std::fs::create_dir_all(&target).expect("建解包目录");
    let status = Command::new("msiexec")
        .arg("/a")
        .arg(&artifact.path)
        .args(["/qn"])
        .arg(format!("TARGETDIR={}", target.display()))
        .status()
        .expect("msiexec /a 真跑");
    assert!(status.success(), "管理安装解包失败：{status}");
    let exe = find_named(&target, "pax-notes.exe").expect("文件表须含 pax-notes.exe");
    assert_eq!(
        std::fs::read(&exe).expect("读解出的可执行"),
        b"MZ\x90\x00",
        "载荷字节须与裸产物一致：{}",
        exe.display()
    );
    assert!(
        find_named(&target, "index.html").is_some(),
        "resources 须一并入包：{}",
        target.display()
    );
    // 金样 msi 条目的三条 entries（`*.exe` / `index.html` / `RUNTIME-DEPENDENCIES.txt`）全部点到。
    assert!(
        find_named(&target, "RUNTIME-DEPENDENCIES.txt").is_some(),
        "RUNTIME-DEPENDENCIES.txt 须一并入包：{}",
        target.display()
    );
}

/// 递归找同名文件（管理安装的层级随 Directory 表形态而变，不写死路径）。
#[test]
fn find_named_walks_nested_dirs() {
    let dir = temp_dir("find-named");
    write(&dir.join("a/b/c/pax-notes.exe"), b"MZ");
    write(&dir.join("a/index.html"), b"<html></html>");
    assert!(
        find_named(&dir, "pax-notes.exe").is_some(),
        "深层文件须能找到"
    );
    assert!(find_named(&dir, "index.html").is_some());
    assert!(find_named(&dir, "missing.txt").is_none());
    assert!(
        find_named(&dir.join("nope"), "x").is_none(),
        "不存在的目录不 panic"
    );
}

fn find_named(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()? {
        let path = entry.ok()?.path();
        if path.is_dir() {
            if let Some(found) = find_named(&path, name) {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|n| n == name) {
            return Some(path);
        }
    }
    None
}

/// 真编译（装有 makensis 的宿主，CI windows-2022 安装 NSIS）：生成 `.nsi` → makensis
/// 出 `.exe` → 非空 PE（MZ + `e_lfanew` 处 `PE\0\0`）。
///
/// 与 MSI 真跑不同，这里不做安装/启动/卸载冒烟：该冒烟需在 Windows 上以静默模式
/// 真实改写系统状态（Program Files / 注册表 / 快捷方式），尚未实现，故不声称。
#[test]
fn nsis_real_build_produces_pe_installer() {
    // 打包器按 Windows makensis 约定拼 `/V2`（POSIX makensis 只认 `-V2`），
    // 真编译面由 windows-2022 腿覆盖；非 Windows 宿主显式 skip。
    if !cfg!(windows) {
        eprintln!("SKIP：NSIS 打包器使用 Windows makensis 选项（/V2），非 Windows 宿主不适用");
        return;
    }
    if common::skip_without("makensis") {
        return;
    }
    let mut inputs = inputs("nsis-real", false);
    inputs.icons.push(Icon {
        // 真 ICO（T-M1-14 派生资产）：makensis 会解析并嵌入安装器图标。
        path: common::fixture("pax-notes.ico"),
        kind: IconKind::Ico,
        size: Some(256),
    });

    let artifact = Nsis
        .package(&inputs, &spec("nsis"))
        .expect("makensis 真编译");
    assert_eq!(artifact.format, "nsis");
    assert!(artifact.bytes > 0, "产物为空");
    assert_eq!(
        artifact.path.extension().and_then(|ext| ext.to_str()),
        Some("exe"),
        "产物路径：{}",
        artifact.path.display()
    );
    assert!(
        inputs.out_dir.join("pax-notes.nsi").is_file(),
        "脚本应先落盘：{}",
        inputs.out_dir.display()
    );

    let bytes = std::fs::read(&artifact.path).expect("读 NSIS 安装器");
    assert_eq!(artifact.bytes, bytes.len() as u64, "产物大小与磁盘不一致");
    assert_eq!(
        &bytes[..2],
        b"MZ",
        "NSIS 产物须为 PE：{}",
        artifact.path.display()
    );
    let pe = u32::from_le_bytes([bytes[0x3c], bytes[0x3d], bytes[0x3e], bytes[0x3f]]) as usize;
    assert!(pe + 4 <= bytes.len(), "PE 头偏移越界：{pe}");
    assert_eq!(&bytes[pe..pe + 4], b"PE\0\0", "PE 签名缺失");
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
