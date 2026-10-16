//! VIDAA / Vizio SmartCast 应用包打包器（T-M4-10）：两个 web 运行时目标 + 沙箱自检。
//!
//! 产物 = `<out_dir>/<kebab>-<version>.zip`（zip 容器，**平台未规定扩展名**——不发明
//! `.xxx` 假后缀，如实用 `.zip`）：UI 载荷 + 随包 wasm 出口 + 图标 + 平台应用清单。
//! 沙箱自检三条规则（入口文件 / 清单字段 / 体积）由 [`check_web_app_structure`] 承担——
//! 打包器自检与 verify 腿 `sandbox-selfcheck` 同一实现；体积的上限判定走 `[budget].size`
//! （verify 腿与 package 阶段一致的预算门禁）。
//!
//! 边界（如实注记，见 `TargetSpec.notes`）：两目标**无本地发布签名**（提审归平台商店侧）、
//! **无推送**（本机无对电视安装 / 推送的工具链）；Vizio SmartCast 应用为远程托管形态，
//! 本包为离线交付与自检载体。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, ProductInfo};
use crate::packager::{Artifact, BundlePlan, Packager};
use crate::webapp;
use crate::zipwrite;

/// 产物格式（须与 `[targets.vidaa-tv]` / `[targets.smartcast-tv]` 的 `package.format` 一致）。
const FORMAT: &str = "web-app";

/// 打包工作目录（`<out_dir>/webapp/`：载荷暂存根，产物是它旁边的单文件）。
const WORK_DIR: &str = "webapp";

/// VIDAA 目标 id。
pub const VIDAA: &str = "vidaa-tv";

/// Vizio SmartCast 目标 id（`smartcast-tv`——与 adapter-tv 的能力矩阵哨兵 id 一致）。
pub const SMARTCAST: &str = "smartcast-tv";

/// 两个 web 运行时目标的应用清单面（沙箱自检的字段规则按平台分表）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebAppPlatform {
    /// VIDAA（海信）：`manifest.json`（`pkgname` / `appname` / `version` / `page.index`）。
    Vidaa,
    /// Vizio SmartCast（`smartcast-tv`）：`appinfo.json`（`name` / `version` / `main`）。
    Smartcast,
}

impl WebAppPlatform {
    /// 目标 id → 平台（非本打包器目标 → `None`）。
    pub fn from_target(id: &str) -> Option<Self> {
        match id {
            VIDAA => Some(Self::Vidaa),
            SMARTCAST => Some(Self::Smartcast),
            _ => None,
        }
    }

    /// 平台 id（= 目标 id，与打包器 id 同源）。
    pub fn id(self) -> &'static str {
        match self {
            Self::Vidaa => VIDAA,
            Self::Smartcast => SMARTCAST,
        }
    }

    /// 平台名（notes 与错误文案用）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Vidaa => "VIDAA（海信）",
            Self::Smartcast => "Vizio SmartCast",
        }
    }

    /// 应用清单文件名。
    pub fn manifest(self) -> &'static str {
        match self {
            Self::Vidaa => "manifest.json",
            Self::Smartcast => "appinfo.json",
        }
    }

    /// 清单必填字段（JSON 指针，字段名）——沙箱自检的字段规则。
    fn required(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Vidaa => &[
                ("/pkgname", "pkgname"),
                ("/appname", "appname"),
                ("/version", "version"),
            ],
            Self::Smartcast => &[
                ("/name", "name"),
                ("/version", "version"),
                ("/main", "main"),
            ],
        }
    }

    /// 入口指针（清单里指向应用入口的字段）。
    fn entry_pointer(self) -> &'static str {
        match self {
            Self::Vidaa => "/page/index",
            Self::Smartcast => "/main",
        }
    }

    /// 应用标识字段（VIDAA = `pkgname`；Vizio = `name`）。
    fn app_pointer(self) -> &'static str {
        match self {
            Self::Vidaa => "/pkgname",
            Self::Smartcast => "/name",
        }
    }
}

/// VIDAA 应用包打包器（`web-app`）。
pub struct VidaaApp;

/// Vizio SmartCast 应用包打包器（`web-app`；目标 id `smartcast-tv`）。
pub struct SmartcastApp;

impl Packager for VidaaApp {
    fn id(&self) -> &'static str {
        VIDAA
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        plan_for(WebAppPlatform::Vidaa, inputs, spec)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        package_for(WebAppPlatform::Vidaa, inputs, spec)
    }
}

impl Packager for SmartcastApp {
    fn id(&self) -> &'static str {
        SMARTCAST
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        plan_for(WebAppPlatform::Smartcast, inputs, spec)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        package_for(WebAppPlatform::Smartcast, inputs, spec)
    }
}

/// 两平台共用的打包计划：无外部工具腿（平台无公开的本地打包校验 CLI），
/// 沙箱自检由内建检查器承担（与 verify 腿同一实现）。
fn plan_for(
    platform: WebAppPlatform,
    inputs: &BundleInputs,
    spec: &TargetSpec,
) -> Result<BundlePlan, BundleError> {
    check_format(spec, platform.id(), FORMAT)?;
    inputs.require_ui_dist(platform.id())?;
    Ok(BundlePlan::new())
}

/// 两平台共用的打包与自检。
fn package_for(
    platform: WebAppPlatform,
    inputs: &BundleInputs,
    spec: &TargetSpec,
) -> Result<Artifact, BundleError> {
    let plan = plan_for(platform, inputs, spec)?;
    let work = inputs.out_dir.join(WORK_DIR);
    let _ = std::fs::remove_dir_all(&work);
    let payload = webapp::stage_payload(inputs, &work, platform.id())?;
    fsutil::write_text(
        &work.join(platform.manifest()),
        &manifest_json(platform, &inputs.product),
    )?;

    let package = package_path(inputs);
    zipwrite::write_zip(&webapp::collect_files(&work)?, &package)?;
    // 沙箱自检：产物必过自己的检查器（与 verify 腿同一实现）
    let structure = check_web_app_structure(&package, platform)?;
    exec::run_all(&plan.commands)?;

    let mut notes = vec![
        format!(
            "产物：{}（zip 容器，{} 条目；{} + UI 载荷 + 随包 wasm 出口 + 图标；\
             平台未规定扩展名，如实用 .zip）",
            package.display(),
            structure.entries.len(),
            platform.manifest()
        ),
        format!(
            "应用清单：{} = {} / version = {}（{}）",
            if platform == WebAppPlatform::Vidaa {
                "pkgname"
            } else {
                "name"
            },
            structure.app,
            structure.version,
            platform.manifest()
        ),
        format!(
            "沙箱自检：入口 {}（指针 {}）在场 / 清单字段齐备 / 体积 {} 字节\
             （上限判定走 [budget].size，verify 腿与 package 阶段同一门禁）",
            structure.entry,
            platform.entry_pointer(),
            structure.bytes
        ),
        format!(
            "边界：{}——无本地发布签名（提审归平台商店侧）、无推送（本机无对电视安装 / 推送的\
             工具链）；安装与商店提交均在平台侧",
            platform.label()
        ),
    ];
    if platform == WebAppPlatform::Smartcast {
        notes.push(
            "Vizio SmartCast 应用为远程托管形态（电视侧加载 URL），本包是离线交付与沙箱自检的\
             载体（T-M4-10 口径）"
                .to_string(),
        );
    }
    notes.extend(payload.notes());
    notes.extend(
        plan.skipped
            .iter()
            .map(|skipped| format!("跳过：{}（{}）", skipped.tool, skipped.reason)),
    );
    fsutil::file_artifact(platform.id(), FORMAT, &package, notes)
}

/// 应用包路径（`<out_dir>/<kebab>-<version>.zip`）。
fn package_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(format!(
        "{}-{}.zip",
        inputs.product.kebab_name(),
        inputs.product.version
    ))
}

/// 应用清单 JSON（平台分表；字符串值经 serde 转义，产品元数据已过白名单校验）。
fn manifest_json(platform: WebAppPlatform, product: &ProductInfo) -> String {
    let value = match platform {
        WebAppPlatform::Vidaa => serde_json::json!({
            "pkgname": product.identifier,
            "appname": product.name,
            "version": product.version,
            "page": { "index": webapp::ENTRY },
        }),
        WebAppPlatform::Smartcast => serde_json::json!({
            "name": product.name,
            "version": product.version,
            "main": webapp::ENTRY,
            "vendor": product.manufacturer,
        }),
    };
    serde_json::to_string_pretty(&value)
        .map(|text| format!("{text}\n"))
        .unwrap_or_else(|_| value.to_string())
}

/// 沙箱自检（打包器自检与 verify 腿 `sandbox-selfcheck` 同一实现）。
///
/// 三条规则：①入口文件——清单的入口指针指向包内真实条目且根入口 [`webapp::ENTRY`] 在场；
/// ②清单字段——平台必填字段齐备非空（VIDAA：`pkgname` / `appname` / `version` +
/// `page.index`；Vizio：`name` / `version` / `main`）；③体积——包体积如实回报，上限判定走
/// `[budget].size`（调用方判定，见 verify 腿）。缺陷一律 [`BundleError::InvalidInput`]
/// 硬失败，绝不降级成跳过。
pub fn check_web_app_structure(
    package: &Path,
    platform: WebAppPlatform,
) -> Result<WebAppStructure, BundleError> {
    let bad = |reason: String| BundleError::InvalidInput {
        packager: platform.id(),
        field: "web-app(沙箱自检)",
        value: package.display().to_string(),
        reason,
    };
    let mut entries = zipwrite::entry_names(package)?;
    entries.sort();
    if entries.is_empty() {
        return Err(bad("包内没有任何条目".to_string()));
    }
    let manifest_name = platform.manifest();
    let text = zipwrite::entry_text(package, manifest_name)?
        .ok_or_else(|| bad(format!("缺 {manifest_name}（应用清单必须落在包根）")))?;
    let manifest: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| bad(format!("{manifest_name} 非法 JSON：{err}")))?;
    let text_field = |pointer: &str| -> Option<String> {
        manifest
            .pointer(pointer)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
    };
    for (pointer, label) in platform.required() {
        if text_field(pointer).is_none() {
            return Err(bad(format!(
                "{manifest_name} 的 {label} 必填且须为非空字符串"
            )));
        }
    }
    let entry = text_field(platform.entry_pointer()).ok_or_else(|| {
        bad(format!(
            "{manifest_name} 缺入口指针 {}",
            platform.entry_pointer()
        ))
    })?;
    let app = text_field(platform.app_pointer()).ok_or_else(|| {
        bad(format!(
            "{manifest_name} 缺应用标识 {}",
            platform.app_pointer()
        ))
    })?;
    let version = text_field("/version").unwrap_or_default();
    if !entries.iter().any(|name| name == &entry) {
        return Err(bad(format!("入口指针 = {entry}，但包内没有该条目")));
    }
    if !entries.iter().any(|name| name == webapp::ENTRY) {
        return Err(bad(format!(
            "包内缺根入口 {}（沙箱自检的入口规则）",
            webapp::ENTRY
        )));
    }
    // 体积：如实回报包字节（上限判定在调用方按 [budget].size 做）
    let bytes = std::fs::metadata(package)
        .map_err(|source| BundleError::Io {
            path: package.to_path_buf(),
            source,
        })?
        .len();
    if bytes == 0 {
        return Err(bad("包体积为 0（空产物）".to_string()));
    }
    Ok(WebAppStructure {
        entries,
        manifest: manifest_name.to_string(),
        app,
        version,
        entry,
        bytes,
    })
}

/// 沙箱自检结果（verify 腿的判决面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAppStructure {
    /// zip 条目名（排序）。
    pub entries: Vec<String>,
    /// 清单文件名（`manifest.json` / `appinfo.json`）。
    pub manifest: String,
    /// 应用标识（VIDAA 取 `pkgname`；Vizio 取 `name`）。
    pub app: String,
    /// 应用版本（两平台的 `version` 字段）。
    pub version: String,
    /// 入口指针指向的包内路径。
    pub entry: String,
    /// 包体积（字节）。
    pub bytes: u64,
}

/// 打包格式与打包器声明必须一致（防「按 web-app 目标产出别的格式」）。
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
    use poly_targets::{Bridge, Host, PackageSpec, Runtime};

    fn spec(id: &str, format: &str) -> TargetSpec {
        let mut spec = TargetSpec::new_stub(id, 3, &[Host::Linux]);
        spec.runtime = Runtime::Wasm;
        spec.bridge = Bridge::WasmBindgen;
        spec.package = PackageSpec {
            format: format.to_string(),
            builder: format!("{id}-web-app"),
        };
        spec
    }

    fn inputs(dir: &Path) -> BundleInputs {
        let dist = dir.join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.html"), "<!doctype html><html></html>").unwrap();
        let mut inputs = BundleInputs::new(
            ProductInfo {
                name: "Pax Notes".to_string(),
                version: "1.2.3".to_string(),
                identifier: "com.polypack.paxnotes".to_string(),
                manufacturer: "PolyPack".to_string(),
                description: "Pax 示例笔记应用".to_string(),
                homepage: None,
            },
            dir.join("out"),
        );
        inputs.ui_dist = Some(dist);
        inputs
    }

    /// 两平台各真造真验一遍：清单字段 / 入口指针 / 体积三条规则都回读成立。
    #[test]
    fn packages_and_self_checks_both_platforms() {
        for platform in [WebAppPlatform::Vidaa, WebAppPlatform::Smartcast] {
            let base = std::env::temp_dir().join(format!(
                "polypack-webtv-{}-{}",
                platform.id(),
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&base);
            let inputs = inputs(&base);
            let packager = crate::packager::find(platform.id(), FORMAT)
                .unwrap_or_else(|| panic!("缺少 {} 的 {FORMAT} 打包器", platform.id()));
            let artifact = packager
                .package(&inputs, &spec(platform.id(), "web-app"))
                .expect("打包");
            assert_eq!(artifact.format, "web-app");
            assert_eq!(
                artifact.path.extension().and_then(|ext| ext.to_str()),
                Some("zip")
            );

            let structure =
                check_web_app_structure(&artifact.path, platform).expect("沙箱自检应通过");
            assert_eq!(structure.entry, "index.html");
            assert_eq!(structure.version, "1.2.3");
            assert_eq!(structure.bytes, artifact.bytes);
            match platform {
                WebAppPlatform::Vidaa => {
                    assert_eq!(structure.manifest, "manifest.json");
                    assert_eq!(structure.app, "com.polypack.paxnotes");
                }
                WebAppPlatform::Smartcast => {
                    assert_eq!(structure.manifest, "appinfo.json");
                    assert_eq!(structure.app, "Pax Notes");
                }
            }
            // 边界注记如实：无签名 / 无推送
            assert!(
                artifact
                    .notes
                    .iter()
                    .any(|note| note.contains("无本地发布签名") && note.contains("无推送")),
                "{:?}",
                artifact.notes
            );
            let _ = std::fs::remove_dir_all(&base);
        }
    }

    /// 缺陷必报错：清单缺字段（改写成空串）→ InvalidInput 硬失败。
    #[test]
    fn self_check_flags_missing_manifest_field() {
        let base = std::env::temp_dir().join(format!("polypack-webtv-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let entry = base.join("index.html");
        std::fs::write(&entry, "x").unwrap();
        let manifest = base.join("manifest.json");
        std::fs::write(&manifest, r#"{"pkgname":"","appname":"A","version":"1"}"#).unwrap();
        let package = base.join("bad.zip");
        zipwrite::write_zip(
            &[
                ("index.html".to_string(), entry),
                ("manifest.json".to_string(), manifest),
            ],
            &package,
        )
        .unwrap();
        let err = check_web_app_structure(&package, WebAppPlatform::Vidaa)
            .expect_err("缺 pkgname 必报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0003);
        assert!(err.to_string().contains("pkgname"), "{err}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
