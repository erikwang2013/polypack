//! Tizen `.wgt` 打包器（T-M4-08）：wasm 核心 + react-dom UI 打成 W3C widget 包。
//!
//! 产物 = `<out_dir>/<kebab>-<version>.wgt`（确定性 zip：`config.xml` + `index.html` 为首的
//! UI 载荷 + 随包 wasm 出口 + 图标）。本机无 tizen CLI / 三星签名工具：**结构真跑**是重点
//! （zip 组装与 [`check_wgt_structure`] 本地可真造真验）；`wgt-lint` 在场即真跑、缺席按
//! 环境槽如实跳过并注记（纪律同 M3 devtools 腿）。作者证书签名按凭据引用（`tizen-author`）
//! 标注，本机不做签名——归属见 `polypack-cli` 的 sign 腿。

use std::path::{Path, PathBuf};

use polypack_targets::TargetSpec;

use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, ProductInfo};
use crate::packager::{Artifact, BundlePlan, Command, Packager};
use crate::packagers::windows::xml_escape;
use crate::webapp;
use crate::zipwrite;

/// Tizen 目标 id。
const ID: &str = "tizen-tv";

/// 产物格式（须与 `[targets.tizen-tv].package.format` 一致）。
const FORMAT: &str = "wgt";

/// W3C widget 清单（须落在包根）。
const CONFIG_XML: &str = "config.xml";

/// 打包工作目录（`<out_dir>/wgt/`：落盘载荷的暂存根，产物是它旁边的单文件）。
const WORK_DIR: &str = "wgt";

/// TV profile 基线（Tizen TV 5.0 起必填的 `required_version` 下限；TV web 应用声明面）。
const REQUIRED_VERSION: &str = "2.3";

/// Tizen `.wgt` 打包器。
pub struct TizenWgt;

/// `.wgt` 结构检查结果（打包器自检与 verify 腿 `wgt-lint` 的本地结构面共用同一份实现）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WgtStructure {
    /// zip 条目名（排序）。
    pub entries: Vec<String>,
    /// `widget@id`。
    pub widget_id: String,
    /// `widget@version`。
    pub version: String,
    /// `tizen:application@id`。
    pub app_id: String,
    /// `tizen:application@package`（包 id，应用 id 的前缀）。
    pub package: String,
    /// `tizen:application@required_version`。
    pub required_version: String,
    /// `tizen:profile@name`（TV profile 标记）。
    pub profile: String,
    /// `content@src`（应用入口）。
    pub content_src: String,
    /// `icon@src`。
    pub icon_src: String,
}

impl Packager for TizenWgt {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, ID, FORMAT)?;
        // 载荷源（dist）与产品元数据在计划期先校验：缺 dist / 非法元数据属环境性缺失，
        // 由 CLI 按同一环境槽跳过（口径同 web / miniapp）。
        inputs.require_ui_dist(ID)?;
        match wgt_lint_tool() {
            // 命令落在**未来产物路径**上（产物名由输入决定，计划可预演）——执行在组装之后。
            Ok(tool) => Ok(BundlePlan::new().with_command(Command::new(
                tool.display().to_string(),
                [wgt_path(inputs).display().to_string()],
            ))),
            Err(reason) => Ok(BundlePlan::new().skipping("wgt-lint", reason)),
        }
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let work = inputs.out_dir.join(WORK_DIR);
        let _ = std::fs::remove_dir_all(&work);
        let payload = webapp::stage_payload(inputs, &work, ID)?;
        fsutil::write_text(&work.join(CONFIG_XML), &config_xml(&inputs.product))?;

        let wgt = wgt_path(inputs);
        zipwrite::write_zip(&webapp::collect_files(&work)?, &wgt)?;
        // 结构自检：产物必过自己的检查器（与 verify 腿同一实现，「结构真跑」的本地证据面）
        let structure = check_wgt_structure(&wgt)?;
        // wgt-lint（在场时）：非零退出即硬失败（产物缺陷不降级成跳过）
        exec::run_all(&plan.commands)?;

        let mut notes = vec![
            format!(
                "产物：{}（zip，{} 条目；config.xml + UI 载荷 + 随包 wasm 出口 + 图标）",
                wgt.display(),
                structure.entries.len()
            ),
            format!(
                "Tizen 清单：widget@id={} version={}，tizen:application@id={} package={} \
                 required_version={} profile={}（config.xml）",
                structure.widget_id,
                structure.version,
                structure.app_id,
                structure.package,
                structure.required_version,
                structure.profile
            ),
            format!(
                "TV 形态：1920×1080 viewmodes=maximized，入口 {}；宿主 = Tizen TV web 运行时",
                structure.content_src
            ),
            "签名：作者证书（Tizen author certificate，凭据引用 `tizen-author`）——本机无三星\
             签名工具，产物未签名，签名与侧载安装归平台侧（Samsung Seller Office 侧完成）"
                .to_string(),
        ];
        notes.extend(payload.notes());
        notes.extend(
            plan.skipped
                .iter()
                .map(|skipped| format!("跳过：{}（{}）", skipped.tool, skipped.reason)),
        );
        fsutil::file_artifact(ID, FORMAT, &wgt, notes)
    }
}

/// `.wgt` 产物路径（`<out_dir>/<kebab>-<version>.wgt`）。
fn wgt_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(format!(
        "{}-{}.wgt",
        inputs.product.kebab_name(),
        inputs.product.version
    ))
}

/// `wgt-lint` 工具定位：`$POLYPACK_WGT_LINT`（显式覆盖）→ PATH。
///
/// `Err(reason)` = 环境缺工具（打包腿记 skipped、verify 腿告警跳过），绝不静默降级。
pub fn wgt_lint_tool() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("POLYPACK_WGT_LINT").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "POLYPACK_WGT_LINT 指向的 {} 不是文件",
            path.display()
        ));
    }
    exec::tool_path("wgt-lint").ok_or_else(|| {
        "PATH 里没有 wgt-lint（Tizen Studio / tizen CLI 的 wgt 校验器；或设 \
         POLYPACK_WGT_LINT=<...>/wgt-lint）——SDK 缺席时内建结构断言守底"
            .to_string()
    })
}

/// `.wgt` 结构检查（与打包器同一实现；缺陷一律 [`BundleError::Archive`] 硬失败，不降级）。
///
/// 规则面（Tizen web 应用的清单必填项 + 引用存在性）：`config.xml` 在包根、`widget` 的
/// id / version 齐备、`tizen:application` 的 id / package / required_version 齐备且 app id
/// 以包 id 为前缀、`tizen:profile` 在场（TV profile 标记）、`content@src` 与 `icon@src`
/// 指向包内真实条目。
pub fn check_wgt_structure(wgt: &Path) -> Result<WgtStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: wgt.to_path_buf(),
        reason,
    };
    let mut entries = zipwrite::entry_names(wgt)?;
    entries.sort();
    if !entries.iter().any(|name| name == CONFIG_XML) {
        return Err(bad(format!(
            "缺 {CONFIG_XML}（wgt 清单必须落在包根；现有条目 {} 条）",
            entries.len()
        )));
    }
    let text = zipwrite::entry_text(wgt, CONFIG_XML)?.unwrap_or_default();
    let attr = |tag: &str, name: &str| {
        attr_of(&text, tag, name).ok_or_else(|| bad(format!("{tag}@{name} 缺失（config.xml）")))
    };
    let widget_id = attr("widget", "id")?;
    let version = attr("widget", "version")?;
    let app_id = attr("tizen:application", "id")?;
    let package = attr("tizen:application", "package")?;
    let required_version = attr("tizen:application", "required_version")?;
    let profile = attr("tizen:profile", "name")?;
    let content_src = attr("content", "src")?;
    let icon_src = attr("icon", "src")?;
    // Tizen 的应用 id 形态规则：`<包 id>.<应用段>`（widget id 与 app id 同源同值）
    if !app_id.starts_with(&format!("{package}.")) {
        return Err(bad(format!(
            "tizen:application@id = {app_id} 不以包 id {package} 加 `.` 为前缀（Tizen 应用 id 形态）"
        )));
    }
    if content_src != webapp::ENTRY {
        return Err(bad(format!(
            "content@src = {content_src}，TV 应用入口须为 {}（载荷根）",
            webapp::ENTRY
        )));
    }
    for (label, src) in [("content@src", &content_src), ("icon@src", &icon_src)] {
        let name = src.trim_start_matches("./");
        if !entries.iter().any(|entry| entry == name) {
            return Err(bad(format!("{label} = {src}，但包内没有该条目")));
        }
    }
    Ok(WgtStructure {
        entries,
        widget_id,
        version,
        app_id,
        package,
        required_version,
        profile,
        content_src,
        icon_src,
    })
}

/// `config.xml`（W3C widget 清单 + Tizen TV profile 扩展）。
///
/// `path_arg` 语义的依赖最小化：只内插已过白名单校验的产品元数据（经 [`xml_escape`] 转义）。
fn config_xml(product: &ProductInfo) -> String {
    let app_id = tizen_app_id(product);
    let name = xml_escape(&product.name);
    let description = xml_escape(&product.description);
    let author = xml_escape(&product.manufacturer);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <widget xmlns=\"http://www.w3.org/ns/widgets\" xmlns:tizen=\"http://tizen.org/ns/widgets\" \
         id=\"{app_id}\" version=\"{version}\" width=\"1920\" height=\"1080\" viewmodes=\"maximized\">\n\
         \x20 <name>{name}</name>\n\
         \x20 <description>{description}</description>\n\
         \x20 <author>{author}</author>\n\
         \x20 <icon src=\"{icon}\"/>\n\
         \x20 <content src=\"{entry}\"/>\n\
         \x20 <tizen:application id=\"{app_id}\" package=\"{package}\" required_version=\"{required}\"/>\n\
         \x20 <tizen:profile name=\"tv-samsung\"/>\n\
         </widget>\n",
        version = product.version,
        icon = webapp::ICON,
        entry = webapp::ENTRY,
        package = product.identifier,
        required = REQUIRED_VERSION,
    )
}

/// Tizen 应用 id：`<包 id>.<应用段>`（段内只允许字母数字——kebab 名去连字符）。
fn tizen_app_id(product: &ProductInfo) -> String {
    format!(
        "{}.{}",
        product.identifier,
        product.kebab_name().replace('-', "")
    )
}

/// 从 `<tag …>` 起始标签里取属性值（只认双引号；属性名前须是空白——否则
/// `required_version="` 会被 `version="` 误命中）。
fn attr_of(text: &str, tag: &str, name: &str) -> Option<String> {
    let tag_start = text.find(&format!("<{tag}"))?;
    let rest = &text[tag_start..];
    let segment = &rest[..rest.find('>')?];
    let needle = format!("{name}=\"");
    let (index, _) = segment
        .match_indices(&needle)
        .find(|(index, _)| segment[..*index].ends_with(char::is_whitespace))?;
    let value_start = index + needle.len();
    let value_end = value_start + segment[value_start..].find('"')?;
    Some(segment[value_start..value_end].to_string())
}

/// 打包格式与打包器声明必须一致（防「按 wgt 目标产出别的格式」）。
fn check_format(spec: &TargetSpec, id: &'static str, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: id,
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use polypack_targets::{Bridge, Host, PackageSpec, Runtime};

    fn spec(id: &str, format: &str) -> TargetSpec {
        let mut spec = TargetSpec::new_stub(id, 3, &[Host::Linux]);
        spec.runtime = Runtime::Wasm;
        spec.bridge = Bridge::WasmBindgen;
        spec.package = PackageSpec {
            format: format.to_string(),
            builder: "tizen-wgt".to_string(),
        };
        spec
    }

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

    fn inputs(dir: &Path) -> BundleInputs {
        let dist = dir.join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.html"), "<!doctype html><html></html>").unwrap();
        let mut inputs = BundleInputs::new(product(), dir.join("out"));
        inputs.ui_dist = Some(dist);
        inputs
    }

    /// 真造真验：包出 `.wgt` → 结构检查回读清单（id / 版本 / 入口 / 图标引用齐备）。
    #[test]
    fn packages_wgt_and_check_reads_manifest_back() {
        let base = std::env::temp_dir().join(format!("polypack-tizen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let inputs = inputs(&base);
        let artifact = TizenWgt
            .package(&inputs, &spec("tizen-tv", "wgt"))
            .expect("打包");
        assert_eq!(artifact.format, "wgt");
        assert_eq!(
            artifact.path.extension().and_then(|ext| ext.to_str()),
            Some("wgt")
        );

        let structure = check_wgt_structure(&artifact.path).expect("结构检查");
        assert_eq!(structure.app_id, "com.polypack.paxnotes.paxnotes");
        assert_eq!(structure.package, "com.polypack.paxnotes");
        assert_eq!(structure.version, "1.2.3");
        assert_eq!(structure.required_version, REQUIRED_VERSION);
        assert_eq!(structure.profile, "tv-samsung");
        assert_eq!(structure.content_src, "index.html");
        assert_eq!(structure.icon_src, webapp::ICON);
        assert!(structure.entries.contains(&"index.html".to_string()));
        assert!(structure.entries.contains(&"config.xml".to_string()));

        // 载荷摘要进 notes（同源摘要面），且与源 dist 直接算的一致
        let digest = webapp::tree_digest(inputs.ui_dist.as_deref().unwrap()).unwrap();
        assert!(
            artifact.notes.iter().any(|note| note.contains(&digest)),
            "{:?}",
            artifact.notes
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 结构缺陷必报错：删掉 config.xml / 让入口引用落空 → Archive 错误（E_BND_0006）。
    #[test]
    fn structure_check_flags_missing_manifest_and_dangling_entry() {
        let base = std::env::temp_dir().join(format!("polypack-tizen-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let content = base.join("index.html");
        std::fs::write(&content, "x").unwrap();

        let plain = base.join("no-config.wgt");
        zipwrite::write_zip(&[("index.html".to_string(), content.clone())], &plain).unwrap();
        let err = check_wgt_structure(&plain).expect_err("缺 manifest 必报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0006);
        assert!(err.to_string().contains("config.xml"), "{err}");

        let dangling = base.join("dangling.wgt");
        let config = base.join("config.xml");
        std::fs::write(
            &config,
            config_xml(&product()).replace("index.html", "missing.html"),
        )
        .unwrap();
        zipwrite::write_zip(
            &[
                ("config.xml".to_string(), config),
                ("index.html".to_string(), content),
            ],
            &dangling,
        )
        .unwrap();
        let err = check_wgt_structure(&dangling).expect_err("引用落空必报错");
        assert!(err.to_string().contains("missing.html"), "{err}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// `attr_of` 不误命中后缀属性（`required_version` vs `version`）。
    #[test]
    fn attr_of_requires_whitespace_before_name() {
        let text = "<widget id=\"a\" version=\"1\" required_version=\"2.3\"/>";
        assert_eq!(attr_of(text, "widget", "version").as_deref(), Some("1"));
        assert_eq!(
            attr_of(text, "widget", "required_version").as_deref(),
            Some("2.3")
        );
        assert_eq!(attr_of(text, "widget", "package"), None);
    }
}
