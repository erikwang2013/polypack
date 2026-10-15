//! Windows 打包器（T-M1-05）：WiX v3 MSI（`candle` + `light`）与 NSIS（`makensis`）。
//!
//! 两者消费 T-M1-03 的 Windows 裸产物（`<root>/<bin>.exe` + WebView 资源同目录，见
//! [`crate::tauri`]）：MSI 由 `<out>/<kebab>.wxs` 声明目录树、开始菜单快捷方式与 ARP 图标，
//! NSIS 由 `<out>/<kebab>.nsi` 整树 `File /r` 安装并写卸载器。宿主缺 WiX / NSIS 时按
//! [`crate::exec`] 契约返回 [`BundleError::ToolMissing`]（绝不静默降级）：mock 模式契约测试
//! 以此断言「真机执行阻塞（宿主）」，装上工具的真实宿主（windows-2022）由测试自行跳过。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::{BundleError, io_err};
use crate::exec;
use crate::fsutil;
use crate::inputs::{BareProduct, BundleInputs, IconKind, nsis_version, wix_version};
use crate::packager::{Artifact, BundlePlan, Command, Packager};
use crate::tauri;

/// WiX MSI 打包器（脚本 `<out>/<kebab>.wxs`，产物 `<产品名>_<版本>_x64_en-US.msi`）。
pub struct WixMsi;

/// NSIS 打包器（脚本 `<out>/<kebab>.nsi`，产物 `<产品名>_<版本>_x64-setup.exe`）。
pub struct Nsis;

const MSI: &str = "msi";
const NSIS: &str = "nsis";

impl Packager for WixMsi {
    fn id(&self) -> &'static str {
        "windows"
    }

    fn format(&self) -> &'static str {
        MSI
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, MSI)?;
        let bare = inputs.require_bare(MSI)?;
        let wxs = script_path(inputs, "wxs");
        let wixobj = script_path(inputs, "wixobj");
        let msi = msi_path(inputs);
        let mut plan = BundlePlan::new()
            .with_script(&wxs, wix_script(inputs, bare)?)
            .with_command(Command::new(
                "candle",
                [
                    "-nologo".into(),
                    "-arch".into(),
                    "x64".into(),
                    "-out".into(),
                    path_arg(&wixobj),
                    path_arg(&wxs),
                ],
            ))
            // 不带 `-ext WixUIExtension` / `-cultures:en-us`：其本地化 wxl 会把 DB 码页钉死为
            // 1252（`-loc` 同 Culture 覆盖不生效，jpackage JDK-8290471 同款），非拉丁描述（如
            // 中文）在 light 链接期报 LGHT0311（windows-2022 真跑实证）。UI-less 形态下码页由
            // Product/@Codepage=65001 决定，任意 Unicode 字符串可入库（官方不支持 UTF-8+对话框，
            // 故对话框留待有 65001 本地化方案时再接）。
            .with_command(Command::new(
                "light",
                [
                    "-nologo".into(),
                    "-out".into(),
                    path_arg(&msi),
                    path_arg(&wixobj),
                ],
            ));
        if inputs.icon(IconKind::Ico).is_none() {
            plan = plan.skipping("icons(ICO)", "未提供 ICO 图标：安装器无图标");
        }
        Ok(plan)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let bare = inputs.require_bare(MSI)?;
        fsutil::write_scripts(&plan)?;
        exec::run_all(&plan.commands)?;

        fsutil::file_artifact(
            self.id(),
            MSI,
            &msi_path(inputs),
            vec![
                runtime_note(bare),
                "安装冒烟：msiexec /i <msi>（静默加 /qn）；卸载：msiexec /x <msi>".to_string(),
            ],
        )
    }
}

impl Packager for Nsis {
    fn id(&self) -> &'static str {
        "windows"
    }

    fn format(&self) -> &'static str {
        NSIS
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, NSIS)?;
        let bare = inputs.require_bare(NSIS)?;
        let nsi = script_path(inputs, "nsi");
        // Windows 上 makensis 自动补 `.exe`：产物名里不手工拼扩展名。
        let mut plan = BundlePlan::new()
            .with_script(&nsi, nsi_script(inputs, bare))
            .with_command(Command::new("makensis", ["/V2".into(), path_arg(&nsi)]));
        if inputs.icon(IconKind::Ico).is_none() {
            plan = plan.skipping("icons(ICO)", "未提供 ICO 图标：安装器无图标");
        }
        Ok(plan)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let bare = inputs.require_bare(NSIS)?;
        fsutil::write_scripts(&plan)?;
        exec::run_all(&plan.commands)?;

        fsutil::file_artifact(
            self.id(),
            NSIS,
            &nsis_path(inputs),
            vec![
                runtime_note(bare),
                "安装冒烟：静默安装（/S）→ 启动 → 卸载（uninstall.exe /S）".to_string(),
            ],
        )
    }
}

/// 打包格式与打包器声明必须一致（防「按 appimage 目标产出 msi」）。
fn check_format(spec: &TargetSpec, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: "windows",
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}

/// 脚本路径（`<out>/<kebab>.<ext>`）。
fn script_path(inputs: &BundleInputs, ext: &str) -> PathBuf {
    inputs
        .out_dir
        .join(format!("{}.{ext}", inputs.product.kebab_name()))
}

/// MSI 产物路径（命名与 tauri-bundler 一致：`<产品名>_<版本>_x64_en-US.msi`）。
fn msi_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(format!(
        "{}_{}_x64_en-US.msi",
        inputs.product.name, inputs.product.version
    ))
}

/// NSIS 产物路径（`OutFile` 用同一文件名，由 makensis 补 `.exe`）。
fn nsis_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(nsis_file_name(inputs))
}

/// NSIS 产物文件名。
fn nsis_file_name(inputs: &BundleInputs) -> String {
    format!(
        "{}_{}_x64-setup.exe",
        inputs.product.name, inputs.product.version
    )
}

/// 在安装目录内的可执行相对路径（快捷方式 Target 用）。
fn exe_rel(inputs: &BundleInputs, bare: &BareProduct) -> PathBuf {
    bare.executable.strip_prefix(&bare.root).map_or_else(
        |_| PathBuf::from(format!("{}.exe", inputs.product.binary_name())),
        Path::to_path_buf,
    )
}

/// 生成 WiX v3 脚本：目录树（裸产物逐文件 Component）、开始菜单快捷方式、Feature 与安装 UI。
fn wix_script(inputs: &BundleInputs, bare: &BareProduct) -> Result<String, BundleError> {
    let product = &inputs.product;
    let mut root = Node::default();
    for (rel, abs) in tree_files(&bare.root)? {
        root.insert(&rel, abs);
    }

    let mut ids = Ids::default();
    let menu_dir = ids.directory("ProgramMenu");
    let shortcut = ids.component_named("ApplicationShortcut");
    let remove_folder = ids.reserve("RemoveStartMenuShortcut");
    let mut tree = String::new();
    // INSTALLDIR 自身在缩进 4：其子节点从 5 起。
    emit_tree(&root, "", &mut ids, 5, &mut tree);

    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<Wix xmlns=\"http://schemas.microsoft.com/wix/2006/wi\">\n");
    // Codepage=65001（UTF-8）：产品名/描述等入 DB 字符串可为任意 Unicode（中文描述实证
    // LGHT0311 修复点）；改小码页前先想清楚非拉丁用户的代价。
    line(
        &mut out,
        1,
        &format!(
            "<Product Id=\"*\" Name=\"{}\" Version=\"{}\" Manufacturer=\"{}\" UpgradeCode=\"{}\" Language=\"1033\" Codepage=\"65001\">",
            xml_escape(&product.name),
            wix_version(&product.version),
            xml_escape(&product.manufacturer),
            upgrade_code(&product.identifier),
        ),
    );
    line(
        &mut out,
        2,
        "<Package InstallerVersion=\"500\" Compressed=\"yes\" InstallScope=\"perMachine\"/>",
    );
    // 同版本允许覆盖安装（每夜冒烟会重装同一版本产物）。
    line(
        &mut out,
        2,
        "<MajorUpgrade AllowSameVersionUpgrades=\"yes\" DowngradeErrorMessage=\"A newer version of [ProductName] is already installed.\"/>",
    );
    line(&mut out, 2, "<MediaTemplate EmbedCab=\"yes\"/>");
    if let Some(icon) = inputs.icon(IconKind::Ico) {
        line(
            &mut out,
            2,
            &format!(
                "<Icon Id=\"ProductIcon\" SourceFile=\"{}\"/>",
                xml_escape(&icon.path.display().to_string())
            ),
        );
        line(
            &mut out,
            2,
            "<Property Id=\"ARPPRODUCTICON\" Value=\"ProductIcon\"/>",
        );
    }
    line(
        &mut out,
        2,
        "<Directory Id=\"TARGETDIR\" Name=\"SourceDir\">",
    );
    line(&mut out, 3, "<Directory Id=\"ProgramFiles64Folder\">");
    line(
        &mut out,
        4,
        &format!(
            "<Directory Id=\"INSTALLDIR\" Name=\"{}\">",
            xml_escape(&product.name)
        ),
    );
    out.push_str(&tree);
    line(&mut out, 4, "</Directory>");
    line(&mut out, 3, "</Directory>");
    line(&mut out, 3, "<Directory Id=\"ProgramMenuFolder\">");
    line(
        &mut out,
        4,
        &format!(
            "<Directory Id=\"{menu_dir}\" Name=\"{}\">",
            xml_escape(&product.name)
        ),
    );
    line(
        &mut out,
        5,
        &format!("<Component Id=\"{shortcut}\" Guid=\"*\">"),
    );
    line(
        &mut out,
        6,
        &format!(
            "<Shortcut Id=\"ApplicationStartMenuShortcut\" Name=\"{}\" Description=\"{}\" Target=\"[INSTALLDIR]\\{}\" WorkingDirectory=\"INSTALLDIR\"/>",
            xml_escape(&product.name),
            xml_escape(&product.description),
            windows_path(&exe_rel(inputs, bare)),
        ),
    );
    line(
        &mut out,
        6,
        &format!(
            "<RemoveFolder Id=\"{remove_folder}\" Directory=\"{menu_dir}\" On=\"uninstall\"/>"
        ),
    );
    // ARP 的 KeyPath（HKCU），与 NSIS 卸载段清掉的注册表项同址。
    line(
        &mut out,
        6,
        &format!(
            "<RegistryValue Root=\"HKCU\" Key=\"Software\\{}\" Name=\"installed\" Type=\"integer\" Value=\"1\" KeyPath=\"yes\"/>",
            xml_escape(&product.name),
        ),
    );
    line(&mut out, 5, "</Component>");
    line(&mut out, 4, "</Directory>");
    line(&mut out, 3, "</Directory>");
    line(&mut out, 2, "</Directory>");
    line(
        &mut out,
        2,
        &format!(
            "<Feature Id=\"MainFeature\" Level=\"1\" Title=\"{}\" Description=\"{}\">",
            xml_escape(&product.name),
            xml_escape(&product.description),
        ),
    );
    for component in &ids.components {
        line(&mut out, 3, &format!("<ComponentRef Id=\"{component}\"/>"));
    }
    line(&mut out, 2, "</Feature>");
    line(&mut out, 1, "</Product>");
    out.push_str("</Wix>\n");
    Ok(out)
}

/// 生成 NSIS 脚本：MUI2 页面、整树安装、卸载器、快捷方式与注册表清理。
fn nsi_script(inputs: &BundleInputs, bare: &BareProduct) -> String {
    let product = &inputs.product;
    let name = &product.name;
    let root = nsis_escape(&windows_path(&bare.root));
    let exe = nsis_escape(&windows_path(&exe_rel(inputs, bare)));

    let mut out = String::new();
    out.push_str("Unicode true\n");
    out.push_str("!include \"MUI2.nsh\"\n\n");
    out.push_str(&format!("Name \"{name}\"\n"));
    out.push_str(&format!(
        "OutFile \"{}\"\n",
        nsis_escape(&nsis_file_name(inputs))
    ));
    out.push_str(&format!("InstallDir \"$PROGRAMFILES64\\{name}\"\n"));
    out.push_str(&format!(
        "VIProductVersion \"{}\"\n",
        nsis_version(&product.version)
    ));
    out.push_str(&format!("VIAddVersionKey \"ProductName\" \"{name}\"\n"));
    out.push_str(&format!(
        "VIAddVersionKey \"FileVersion\" \"{}\"\n",
        nsis_version(&product.version)
    ));
    out.push_str(&format!(
        "VIAddVersionKey \"CompanyName\" \"{}\"\n",
        product.manufacturer
    ));
    out.push_str(&format!(
        "VIAddVersionKey \"ProductVersion\" \"{}\"\n",
        product.version
    ));
    if let Some(icon) = inputs.icon(IconKind::Ico) {
        let ico = nsis_escape(&windows_path(&icon.path));
        out.push_str(&format!("Icon \"{ico}\"\n"));
        out.push_str(&format!("UninstallIcon \"{ico}\"\n"));
    }
    out.push('\n');
    for page in [
        "MUI_PAGE_DIRECTORY",
        "MUI_PAGE_INSTFILES",
        "MUI_UNPAGE_CONFIRM",
        "MUI_UNPAGE_INSTFILES",
        "MUI_LANGUAGE \"English\"",
    ] {
        out.push_str(&format!("!insertmacro {page}\n"));
    }
    out.push_str(&format!(
        "\nSection \"Install\"\n  SetOutPath \"$INSTDIR\"\n  File /r \"{root}\\*.*\"\n  WriteUninstaller \"$INSTDIR\\uninstall.exe\"\n  CreateDirectory \"$SMPROGRAMS\\{name}\"\n  CreateShortcut \"$SMPROGRAMS\\{name}\\{name}.lnk\" \"$INSTDIR\\{exe}\"\n  CreateShortcut \"$DESKTOP\\{name}.lnk\" \"$INSTDIR\\{exe}\"\nSectionEnd\n",
    ));
    out.push_str(&format!(
        "\nSection \"Uninstall\"\n  Delete \"$INSTDIR\\uninstall.exe\"\n  Delete \"$SMPROGRAMS\\{name}\\{name}.lnk\"\n  RMDir \"$SMPROGRAMS\\{name}\"\n  Delete \"$DESKTOP\\{name}.lnk\"\n  RMDir /r \"$INSTDIR\"\n  DeleteRegKey HKCU \"Software\\{name}\"\nSectionEnd\n",
    ));
    out
}

/// 产物说明：裸产物内附的运行时依赖（T-M1-03 写入 [`tauri::DEPS_FILE`]）。
fn runtime_note(bare: &BareProduct) -> String {
    let deps = bare.root.join(tauri::DEPS_FILE);
    let first = std::fs::read_to_string(&deps).ok().and_then(|text| {
        text.lines()
            .find(|l| l.trim_start().starts_with("- "))
            .map(str::trim)
            .map(str::to_string)
    });
    match first {
        Some(dep) => format!("WebView2 运行时依赖见产物内 {}（{dep}）", tauri::DEPS_FILE),
        None => format!(
            "WebView2 运行时依赖见产物内 {}（未读到依赖条目，需人工核对）",
            tauri::DEPS_FILE
        ),
    }
}

/// 收集裸产物内的文件（相对路径 → 绝对路径），按相对路径排序。
fn tree_files(root: &Path) -> Result<Vec<(PathBuf, PathBuf)>, BundleError> {
    let mut files = Vec::new();
    walk(root, Path::new(""), &mut files)?;
    files.sort();
    Ok(files)
}

/// 递归收集文件（符号链接按文件登记，不跟随）。
fn walk(dir: &Path, rel: &Path, files: &mut Vec<(PathBuf, PathBuf)>) -> Result<(), BundleError> {
    for entry in std::fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        let path = entry.path();
        let child = rel.join(entry.file_name());
        if entry.file_type().map_err(io_err(&path))?.is_dir() {
            walk(&path, &child, files)?;
        } else {
            files.push((child, path));
        }
    }
    Ok(())
}

/// 裸产物目录树（`BTreeMap` 按名排序，保证脚本逐字节稳定）。
#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: BTreeMap<String, PathBuf>,
}

impl Node {
    /// 按相对路径插入一个文件。
    fn insert(&mut self, rel: &Path, abs: PathBuf) {
        let mut parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let Some(file) = parts.pop() else {
            return;
        };
        let mut node = self;
        for part in parts {
            node = node.dirs.entry(part).or_default();
        }
        node.files.insert(file, abs);
    }
}

/// 递归发射 `<Directory>` / `<Component>` / `<File>`（目录层级按相对路径还原）。
fn emit_tree(node: &Node, rel: &str, ids: &mut Ids, indent: usize, out: &mut String) {
    for (name, child) in &node.dirs {
        let child_rel = join_rel(rel, name);
        let id = ids.directory(&child_rel);
        line(
            out,
            indent,
            &format!("<Directory Id=\"{id}\" Name=\"{}\">", xml_escape(name)),
        );
        emit_tree(child, &child_rel, ids, indent + 1, out);
        line(out, indent, "</Directory>");
    }
    for (name, abs) in &node.files {
        let file_rel = join_rel(rel, name);
        let component = ids.component(&file_rel);
        let file = ids.file(&file_rel);
        let source = xml_escape(&abs.display().to_string());
        line(
            out,
            indent,
            &format!("<Component Id=\"{component}\" Guid=\"*\">"),
        );
        line(
            out,
            indent + 1,
            &format!("<File Id=\"{file}\" Source=\"{source}\" KeyPath=\"yes\"/>"),
        );
        line(out, indent, "</Component>");
    }
}

/// 拼接相对路径（根层不带前导 `/`，Id 才不会多出无谓的 `_` 前缀）。
fn join_rel(rel: &str, name: &str) -> String {
    if rel.is_empty() {
        name.to_string()
    } else {
        format!("{rel}/{name}")
    }
}

/// XML `Id` 分配器：相对路径消毒 → 稳定 Id；撞名按序号后缀规避（不同路径消毒后可能同名）。
#[derive(Default)]
struct Ids {
    seen: BTreeSet<String>,
    components: Vec<String>,
}

impl Ids {
    /// 目录 Id。
    fn directory(&mut self, rel: &str) -> String {
        self.reserve(&format!("dir_{}", sanitize(rel)))
    }

    /// 文件 Id。
    fn file(&mut self, rel: &str) -> String {
        self.reserve(&format!("fil_{}", sanitize(rel)))
    }

    /// 组件 Id（同时登记进 `<Feature>` 引用表）。
    fn component(&mut self, rel: &str) -> String {
        let id = self.reserve(&format!("cmp_{}", sanitize(rel)));
        self.components.push(id.clone());
        id
    }

    /// 具名组件 Id（快捷方式等非路径派生组件）。
    fn component_named(&mut self, name: &str) -> String {
        let id = self.reserve(name);
        self.components.push(id.clone());
        id
    }

    /// 分配一个未占用的 Id。
    fn reserve(&mut self, base: &str) -> String {
        if self.seen.insert(base.to_string()) {
            return base.to_string();
        }
        let mut n = 2;
        loop {
            let candidate = format!("{base}_{n}");
            if self.seen.insert(candidate.clone()) {
                return candidate;
            }
            n += 1;
        }
    }
}

/// 相对路径 → 合法 XML `Name`（字母开头，只含 `[A-Za-z0-9_]`）。
fn sanitize(rel: &str) -> String {
    let mut out: String = rel
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    if !out.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        out.insert(0, 'x');
    }
    out
}

/// XML 属性 / 文本转义（产品名等已由 `inputs` 白名单限制，裸产物路径可能含任意字符）。
///
/// 公开给契约测试按**同一口径**构造 `Source="…"` needle：宿主路径合法可含 `&`/`>`
/// （如 `C:\Users\R&D\…`），测试自建转义表会与实际产物漂移。
pub fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// NSIS 脚本转义：`$` 在脚本里引入变量（`$INSTDIR` / `$0`…），字面量须写 `$$`
/// （同 [`xml_escape`]：产品名由 `inputs` 白名单限制，裸产物路径可能含任意字符）。
///
/// 公开给契约测试按**同一口径**构造 path needle：宿主路径合法可含 `$`
/// （如 `C:\$Recycle.Bin\…`），测试自建转义表会与实际产物漂移。
pub fn nsis_escape(text: &str) -> String {
    text.replace('$', "$$")
}

/// `UpgradeCode`：由产品标识符派生的稳定 GUID（同标识符恒等，换标识符即变）。
///
/// 无 uuid 依赖：FNV-1a 双种子取 16 字节，按 UUID v5 形态置版本 / 变体位。
fn upgrade_code(identifier: &str) -> String {
    let bytes = identifier.as_bytes();
    let mut raw = [0u8; 16];
    raw[..8].copy_from_slice(&fnv1a64(bytes, 0xcbf2_9ce4_8422_2325).to_be_bytes());
    raw[8..].copy_from_slice(&fnv1a64(bytes, 0x9e37_79b9_7f4a_7c15).to_be_bytes());
    raw[6] = (raw[6] & 0x0f) | 0x50;
    raw[8] = (raw[8] & 0x3f) | 0x80;
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        raw[0],
        raw[1],
        raw[2],
        raw[3],
        raw[4],
        raw[5],
        raw[6],
        raw[7],
        raw[8],
        raw[9],
        raw[10],
        raw[11],
        raw[12],
        raw[13],
        raw[14],
        raw[15],
    )
}

/// FNV-1a 64 位（固定种子 ⇒ 跨运行稳定）。
fn fnv1a64(bytes: &[u8], seed: u64) -> u64 {
    let mut hash = seed;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Windows 形态路径（NSIS 与 WiX 快捷方式 Target 用 `\` 分隔）。
fn windows_path(path: &Path) -> String {
    path.display().to_string().replace('/', "\\")
}

/// 参数形态（路径一律显式传参，避免拼接成 shell 串）。
fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

/// 追加一行（带两空格缩进）。
fn line(out: &mut String, indent: usize, text: &str) {
    for _ in 0..indent {
        out.push_str("  ");
    }
    out.push_str(text);
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colliding_paths_get_distinct_ids() {
        // `a/b` 与 `a_b` 消毒后同名：必须靠序号后缀区分。
        let mut ids = Ids::default();
        let first = ids.component("a/b");
        let second = ids.component("a_b");
        assert_ne!(first, second);
        assert_eq!(first, "cmp_a_b");
        assert_eq!(second, "cmp_a_b_2");
    }

    #[test]
    fn ids_are_legal_xml_names() {
        let mut ids = Ids::default();
        for rel in ["2d/assets", "a b", "中文/索引.html"] {
            let id = ids.directory(rel);
            assert!(id.chars().next().is_some_and(|c| c.is_ascii_alphabetic()));
            assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        }
    }

    #[test]
    fn xml_escape_covers_attribute_chars() {
        assert_eq!(xml_escape("a&b<c>\"d\""), "a&amp;b&lt;c&gt;&quot;d&quot;");
    }

    #[test]
    fn nsis_escape_doubles_dollar_signs() {
        // 变量形态（`$INSTDIR`）与数字寄存器形态（`$0`）都按字面量转义。
        assert_eq!(nsis_escape("$INSTDIR\\x"), "$$INSTDIR\\x");
        assert_eq!(
            nsis_escape("C:\\$Recycle.Bin\\$0"),
            "C:\\$$Recycle.Bin\\$$0"
        );
        assert_eq!(nsis_escape("Pax Notes"), "Pax Notes");
    }

    #[test]
    fn upgrade_code_is_a_stable_guid() {
        let code = upgrade_code("com.polypack.paxnotes");
        assert_eq!(code.len(), 36);
        assert_eq!(code.matches('-').count(), 4);
        assert_eq!(code, upgrade_code("com.polypack.paxnotes"));
        assert_ne!(code, upgrade_code("com.polypack.other"));
    }
}
