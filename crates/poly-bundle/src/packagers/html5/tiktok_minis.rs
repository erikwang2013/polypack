//! TikTok Minis 打包器与结构自检。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use poly_targets::TargetSpec;

use crate::error::BundleError;
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, BundlePlan, Packager};
use crate::webapp::{self, ENTRY};
use crate::zipwrite;

use super::{
    FORMAT, MINIS_CONFIG, MINIS_MANIFEST, TIKTOK_MINIS_ID, check_format, checked_entries,
    index_html, json_entry, json_text, package_path, require_entry, stage_dist,
};

const TIKTOK_WORK_DIR: &str = "tiktok-minis";

/// TikTok Minis 打包器（`zip`）。
pub struct TiktokMinis;

impl Packager for TiktokMinis {
    fn id(&self) -> &'static str {
        TIKTOK_MINIS_ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, FORMAT, TIKTOK_MINIS_ID)?;
        require_entry(inputs.require_ui_dist(TIKTOK_MINIS_ID)?, TIKTOK_MINIS_ID)?;
        Ok(BundlePlan::new())
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        self.plan(inputs, spec)?;
        let dist = inputs.require_ui_dist(TIKTOK_MINIS_ID)?;
        let stage = inputs.out_dir.join(TIKTOK_WORK_DIR);
        stage_dist(dist, &stage, TIKTOK_MINIS_ID)?;

        // 资源清单只覆盖 dist 文件（配置与清单自身不是应用资源）。
        let resources = resource_tree(&webapp::collect_files(&stage)?);
        let config = json!({ "build": { "htmlEntry": ENTRY, "outputDir": "." } });
        fsutil::write_text(
            &stage.join(MINIS_CONFIG),
            &json_text(&config, &stage.join(MINIS_CONFIG))?,
        )?;
        let manifest = json!({ "name": MINIS_MANIFEST, "resource_list": resources });
        fsutil::write_text(
            &stage.join(MINIS_MANIFEST),
            &json_text(&manifest, &stage.join(MINIS_MANIFEST))?,
        )?;

        let package = package_path(inputs);
        zipwrite::write_zip(&webapp::collect_files(&stage)?, &package)?;
        let structure = check_tiktok_minis_structure(&package)?;

        let mut notes = vec![
            format!(
                "产物：{}（TikTok Minis HTML5 zip，{} 条目；{MINIS_CONFIG} + {MINIS_MANIFEST} \
                 + UI dist 整树）",
                package.display(),
                structure.entries.len()
            ),
            format!(
                "{MINIS_CONFIG}：build.htmlEntry={} outputDir={}（dev 块按 CLI build 输出删除）",
                structure.html_entry, structure.output_dir
            ),
            format!(
                "{MINIS_MANIFEST}：resource_list {} 项（由包内文件生成；.map 与清单自身不计）",
                structure.resources
            ),
            format!(
                "确定性 zip：条目排序 + 固定时间戳（{TIKTOK_MINIS_ID} 与 verify 腿 \
                 tiktok-minis-structure-check 共用同一份结构检查）"
            ),
            "平台侧：index.html 须由 UI 构建带入 TTMinis SDK 引用（打包器不注入 clientKey）；\
             上传 / 沙箱审核归 TikTok 开发者平台，本机不可得——SKIP-with-print，experimental，\
             不声明商店受理"
                .to_string(),
        ];
        if let Some(boundary) = &spec.notes {
            notes.push(format!("目标声明边界：{boundary}"));
        }
        fsutil::file_artifact(TIKTOK_MINIS_ID, FORMAT, &package, notes)
    }
}

/// TikTok Minis zip 结构自检（打包器自检与 verify 腿同一实现）。
///
/// 规则：zip 条目非空且路径相对；包根 `index.html` 与 `minis.config.json`
/// （无 `dev` 块，`build.htmlEntry` / `build.outputDir` 非空）；
/// `minis.manifest.json` 的 `resource_list` 与包内文件一一对应（排除 config / manifest
/// 自身、`.map` 与无扩展名文件）。缺陷一律 [`BundleError::Archive`] 硬失败。
pub fn check_tiktok_minis_structure(package: &Path) -> Result<TiktokMinisStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: package.to_path_buf(),
        reason,
    };
    let entries = checked_entries(package)?;
    index_html(package, &entries)?;

    let config = json_entry(package, MINIS_CONFIG)?;
    if !config.is_object() {
        return Err(bad(format!("{MINIS_CONFIG} 必须是 JSON 对象")));
    }
    if config.get("dev").is_some() {
        return Err(bad(format!(
            "{MINIS_CONFIG} 不得含 dev 块（CLI build 输出会删除 dev）"
        )));
    }
    let build = config
        .get("build")
        .and_then(Value::as_object)
        .ok_or_else(|| bad(format!("{MINIS_CONFIG} 缺 build 对象")))?;
    let field = |key: &str| -> Result<String, BundleError> {
        build
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| bad(format!("{MINIS_CONFIG}.build 缺必填非空字符串：{key}")))
    };
    let html_entry = field("htmlEntry")?;
    let output_dir = field("outputDir")?;

    let manifest = json_entry(package, MINIS_MANIFEST)?;
    if manifest.get("name").and_then(Value::as_str) != Some(MINIS_MANIFEST) {
        return Err(bad(format!(
            "{MINIS_MANIFEST}.name 必须是 {MINIS_MANIFEST}"
        )));
    }
    let list = manifest
        .get("resource_list")
        .and_then(Value::as_array)
        .ok_or_else(|| bad(format!("{MINIS_MANIFEST} 缺 resource_list 数组")))?;
    let mut resources = Vec::new();
    flatten_resources(list, "", &mut resources).map_err(&bad)?;
    resources.sort();
    let mut expected: Vec<String> = entries
        .iter()
        .filter(|name| {
            name.as_str() != MINIS_CONFIG
                && name.as_str() != MINIS_MANIFEST
                && listed_resource(name)
        })
        .cloned()
        .collect();
    expected.sort();
    if resources != expected {
        let missing = expected
            .iter()
            .find(|name| !resources.contains(name))
            .map(String::as_str)
            .unwrap_or("—");
        let extra = resources
            .iter()
            .find(|name| !expected.contains(name))
            .map(String::as_str)
            .unwrap_or("—");
        return Err(bad(format!(
            "{MINIS_MANIFEST} 资源树与包内文件不一致（清单 {} 项 / 实际 {} 项；缺 {missing}，多 {extra}）",
            resources.len(),
            expected.len()
        )));
    }
    let count = resources.len();
    Ok(TiktokMinisStructure {
        entries,
        html_entry,
        output_dir,
        resources: count,
    })
}

/// TikTok Minis 结构自检结果（verify 腿的判决面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiktokMinisStructure {
    /// zip 条目名（排序）。
    pub entries: Vec<String>,
    /// `minis.config.json.build.htmlEntry`。
    pub html_entry: String,
    /// `minis.config.json.build.outputDir`。
    pub output_dir: String,
    /// `minis.manifest.json.resource_list` 展开后的资源数。
    pub resources: usize,
}

/// 资源清单节点（文件夹嵌套）。
#[derive(Default)]
struct ResourceNode {
    files: Vec<String>,
    dirs: BTreeMap<String, ResourceNode>,
}

/// 包内文件 → `minis.manifest.json.resource_list` 树（文件在前、文件夹按名排序）。
fn resource_tree(files: &[(String, PathBuf)]) -> Vec<Value> {
    let mut root = ResourceNode::default();
    for (name, _) in files {
        if !listed_resource(name) {
            continue;
        }
        let mut node = &mut root;
        let mut parts = name.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                node = node.dirs.entry(part.to_string()).or_default();
            } else {
                node.files.push(part.to_string());
            }
        }
    }
    render_resources(&root)
}

fn render_resources(node: &ResourceNode) -> Vec<Value> {
    let mut out: Vec<Value> = node
        .files
        .iter()
        .map(|name| json!({ "type": "file", "name": name }))
        .collect();
    for (name, child) in &node.dirs {
        out.push(json!({
            "type": "folder",
            "name": name,
            "children": render_resources(child),
        }));
    }
    out
}

/// 展开 `resource_list` 为 `a/b.js` 形态的相对路径（校验条目形态）。
fn flatten_resources(items: &[Value], prefix: &str, out: &mut Vec<String>) -> Result<(), String> {
    for item in items {
        let kind = item
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "resource 缺 type".to_string())?;
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .ok_or_else(|| "resource 缺非空 name".to_string())?;
        if name == "." || name == ".." || name.contains(['/', '\\']) {
            return Err(format!("resource 名称非法：{name}"));
        }
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        match kind {
            "file" => out.push(path),
            "folder" => {
                let children = item
                    .get("children")
                    .and_then(Value::as_array)
                    .ok_or_else(|| format!("folder {path} 缺 children 数组"))?;
                flatten_resources(children, &path, out)?;
            }
            other => return Err(format!("resource type 非法：{other}")),
        }
    }
    Ok(())
}

/// CLI 资源清单只收有非空扩展名的文件，且排除 `.map`。
fn listed_resource(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name);
    match base.rsplit_once('.') {
        Some((stem, ext)) => !stem.is_empty() && !ext.is_empty() && ext != "map",
        None => false,
    }
}
