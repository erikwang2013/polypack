//! KaiOS 打包器：react-dom web 栈产物 → `manifest.webapp` + UI dist 的 zip。
//!
//! 产物为单文件 `zip`（builder `kaios-webapp`）：暂存树 = UI dist 整树 + 包根
//! `manifest.webapp`（KaiOS / Firefox OS Open Web App 打包应用形态，`name` / `version` /
//! `launch_path` 必填）。归档走 [`crate::zipwrite`]（条目排序 + 固定时间戳），同输入二跑
//! 字节一致；[`check_kaios_structure`] 是打包器自检与 verify 腿 `kaios-structure-check`
//! 的**同一实现**。
//!
//! 边界（如实注记）：普通应用无需本地签名（`signing = None`），提审与平台侧签名归
//! KaiStore；本机无 KaiOS 设备 / 模拟器与 KaiStore 凭据——结构自检是本机真跑面。

use std::path::Path;

use polypack_targets::TargetSpec;

use crate::error::BundleError;
use crate::fsutil;
use crate::inputs::{BundleInputs, IconKind};
use crate::packager::{Artifact, BundlePlan, Packager};
use crate::webapp::{self, ENTRY};
use crate::zipwrite;

use super::web::assert_html_structure;

/// 目标 id / 产物格式（与 `TargetSpec.package.{format,builder}` 一致）。
pub const ID: &str = "kaios";
const FORMAT: &str = "zip";

/// 包根清单文件名（KaiOS 打包应用清单）。
pub const MANIFEST: &str = "manifest.webapp";

/// 包内图标名（有已知尺寸的 PNG 图标输入时拷入）。
const ICON: &str = "icon.png";

/// 打包暂存目录（`<out_dir>/kaios/`：产物是它旁边的单文件）。
const WORK_DIR: &str = "kaios";

/// KaiOS 打包器（`zip`）。
pub struct Kaios;

impl Packager for Kaios {
    fn id(&self) -> &'static str {
        ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, FORMAT)?;
        require_entry(inputs.require_ui_dist(ID)?)?;
        Ok(BundlePlan::new())
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        self.plan(inputs, spec)?;
        let dist = inputs.require_ui_dist(ID)?;
        let stage = inputs.out_dir.join(WORK_DIR);
        let _ = std::fs::remove_dir_all(&stage);
        fsutil::copy_tree(dist, &stage)?;
        // 与 static-site 同一份 HTML 结构守底断言（html-validate 不在 KaiOS 链上）。
        assert_html_structure(&stage, ID)?;

        let text = manifest_text(inputs, &stage)?;
        fsutil::write_text(&stage.join(MANIFEST), &text)?;

        let stem = format!("{}-{}", inputs.product.kebab_name(), inputs.product.version);
        let package = inputs.out_dir.join(format!("{stem}.zip"));
        zipwrite::write_zip(&webapp::collect_files(&stage)?, &package)?;
        // 结构自检：产物必过自己的检查器（与 verify 腿同一实现）。
        let structure = check_kaios_structure(&package)?;

        let mut notes = vec![
            format!(
                "产物：{}（KaiOS 打包应用 zip，{} 条目；{MANIFEST} + UI dist 整树）",
                package.display(),
                structure.entries.len()
            ),
            format!(
                "{MANIFEST}：name={} version={} launch_path={}（生成自 polypack.toml）",
                structure.name, structure.version, structure.launch_path
            ),
            format!(
                "确定性 zip：条目排序 + 固定时间戳（{ID} 与 verify 腿 kaios-structure-check \
                 共用同一份结构检查）"
            ),
            "签名 / 发布：普通应用本地不签名（signing = None）；提审与平台侧签名归 KaiStore；\
             polypack-publish 无 kaistore 客户端（publish 留空）"
                .to_string(),
            "设备 / 商店腿 SKIP-with-print：本机无 KaiOS 设备 / 模拟器与 KaiStore 凭据；\
             结构自检是本机真跑面"
                .to_string(),
        ];
        if let Some(boundary) = &spec.notes {
            notes.push(format!("目标声明边界：{boundary}"));
        }
        fsutil::file_artifact(ID, FORMAT, &package, notes)
    }
}

/// `manifest.webapp` 文本（JSON；图标仅在有已知尺寸的 PNG 输入时随包）。
fn manifest_text(inputs: &BundleInputs, stage: &Path) -> Result<String, BundleError> {
    let product = &inputs.product;
    let mut manifest = serde_json::json!({
        "name": product.name,
        "version": product.version,
        "description": product.description,
        "launch_path": format!("/{ENTRY}"),
        "developer": { "name": product.manufacturer },
        "type": "web",
    });
    if let Some((icon, size)) = inputs
        .icon(IconKind::Png)
        .and_then(|icon| icon.size.map(|size| (icon, size)))
    {
        fsutil::copy_file(&icon.path, &stage.join(ICON), false)?;
        manifest["icons"] = serde_json::json!({ (size.to_string()): format!("/{ICON}") });
    }
    let text = serde_json::to_string_pretty(&manifest).map_err(|error| BundleError::Io {
        path: stage.join(MANIFEST),
        source: std::io::Error::other(error),
    })?;
    Ok(format!("{text}\n"))
}

/// 入口必须在 dist 根（`launch_path` 指向它，缺了就是不可启动的包）。
fn require_entry(dist: &Path) -> Result<(), BundleError> {
    if dist.join(ENTRY).is_file() {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: ID,
        field: "ui_dist(index.html)",
        value: dist.display().to_string(),
        reason: format!("入口 {ENTRY} 不在 dist 根——KaiOS launch_path 指向它"),
    })
}

/// KaiOS 打包应用 zip 的结构自检（打包器自检与 verify 腿同一实现）。
///
/// 规则：zip 条目非空且路径相对；包根 `manifest.webapp` 存在且为 JSON 对象；
/// `name` / `version` / `launch_path` 为非空字符串；`launch_path` 指向包内真实文件。
/// 缺陷一律 [`BundleError::Archive`] 硬失败（E_BND_0006），绝不降级成跳过。
pub fn check_kaios_structure(package: &Path) -> Result<KaiosStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: package.to_path_buf(),
        reason,
    };
    let mut entries = zipwrite::entry_names(package)?;
    entries.sort();
    if entries.is_empty() {
        return Err(bad("包内没有任何条目".to_string()));
    }
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.starts_with('/') || entry.contains("..") || entry.contains('\\'))
    {
        return Err(bad(format!("条目路径形态非法：{entry}")));
    }

    let text = zipwrite::entry_text(package, MANIFEST)?
        .ok_or_else(|| bad(format!("缺 {MANIFEST}（KaiOS 清单必须落在包根）")))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|err| bad(format!("{MANIFEST} 非法 JSON：{err}")))?;
    let field = |key: &str| -> Result<String, BundleError> {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| bad(format!("{MANIFEST} 缺必填非空字符串字段：{key}")))
    };
    let name = field("name")?;
    let version = field("version")?;
    let launch_path = field("launch_path")?;
    let entry = launch_path.trim_start_matches('/');
    if entry.is_empty() || !entries.iter().any(|name| name == entry) {
        return Err(bad(format!(
            "launch_path 指向的入口不在包内：{launch_path}"
        )));
    }
    Ok(KaiosStructure {
        entries,
        name,
        version,
        launch_path,
    })
}

/// 结构自检结果（verify 腿的判决面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KaiosStructure {
    /// zip 条目名（排序）。
    pub entries: Vec<String>,
    /// `manifest.webapp.name`。
    pub name: String,
    /// `manifest.webapp.version`。
    pub version: String,
    /// `manifest.webapp.launch_path`。
    pub launch_path: String,
}

/// 打包格式与打包器声明必须一致（防「按 zip 目标产出别的格式」）。
fn check_format(spec: &TargetSpec, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: ID,
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}
