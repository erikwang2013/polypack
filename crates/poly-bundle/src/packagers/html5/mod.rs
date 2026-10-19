//! HTML5 L2 平台打包器：TikTok Minis / Facebook Instant Games。
//!
//! 两目标共用「UI dist 整树 + 包根平台配置」形态：react-dom 静态产物拷入暂存树，
//! 生成平台配置（TikTok 另生成资源清单），走 [`crate::zipwrite`] 的确定性归档；
//! [`check_tiktok_minis_structure`] / [`check_facebook_instant_games_structure`] 是
//! 打包器自检与 verify 腿的**同一实现**。
//!
//! 字段来源（不臆造）：
//! - TikTok Minis：CLI `@tiktok-minis/cli` 1.2.0（<https://www.npmjs.com/package/@tiktok-minis/cli>）
//!   build 输出——`minis.config.json` 删除 `dev`、保留 `build.htmlEntry` / `build.outputDir`；
//!   `minis.manifest.json` 为 `{name, resource_list:[{type:file|folder,name,children}]}`，
//!   CLI 只列有非空扩展名的文件并排除 `.map`。
//! - Facebook Instant Games：官方 Bundle Config 文档
//!   <https://developers.facebook.com/docs/games/instant-games/bundle-config>——
//!   包根 `index.html` + `fbapp-config.json`，`instant_games` 下
//!   `platform_version` / `orientation` / `navigation_menu_version`。
//!
//! 边界（如实注记）：本机无平台沙箱 / 商店凭据，结构自检是唯一真跑面；两个目标
//! `experimental = true`，绝不声明商店受理。TikTok 的 SDK 引用与 clientKey 注入属 UI
//! 构建面，打包器不伪造；Facebook 的 SDK 引用同理。

mod facebook;
mod tiktok_minis;

pub use facebook::{
    FacebookInstantGames, FacebookInstantGamesStructure, check_facebook_instant_games_structure,
};
pub use tiktok_minis::{TiktokMinis, TiktokMinisStructure, check_tiktok_minis_structure};

use std::path::{Path, PathBuf};

use serde_json::Value;

use poly_targets::TargetSpec;

use crate::error::BundleError;
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::webapp::ENTRY;
use crate::zipwrite;

use super::web::assert_html_structure;

/// TikTok Minis 目标 id。
pub const TIKTOK_MINIS_ID: &str = "tiktok-minis";
/// Facebook Instant Games 目标 id。
pub const FACEBOOK_INSTANT_GAMES_ID: &str = "facebook-instant-games";

const FORMAT: &str = "zip";
/// TikTok Minis 项目配置（CLI build 输出保留 build、删除 dev）。
pub const MINIS_CONFIG: &str = "minis.config.json";
/// TikTok Minis 包根资源清单。
pub const MINIS_MANIFEST: &str = "minis.manifest.json";
/// Facebook Instant Games 包根配置。
pub const FBAPP_CONFIG: &str = "fbapp-config.json";

/// 条目非空 + 路径相对（`/` 开头 / `..` / 反斜杠一律拒绝）。
fn checked_entries(package: &Path) -> Result<Vec<String>, BundleError> {
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
    Ok(entries)
}

/// 包根 `index.html` 必须存在且为 UTF-8 文本。
fn index_html(package: &Path, entries: &[String]) -> Result<String, BundleError> {
    if !entries.iter().any(|entry| entry == ENTRY) {
        return Err(BundleError::Archive {
            path: package.to_path_buf(),
            reason: format!("缺包根 {ENTRY}（平台入口必须落在 zip 根）"),
        });
    }
    zipwrite::entry_text(package, ENTRY)?.ok_or_else(|| BundleError::Archive {
        path: package.to_path_buf(),
        reason: format!("缺包根 {ENTRY}（平台入口必须落在 zip 根）"),
    })
}

/// 读包根 JSON 配置（缺失 / 非法 JSON 都是 E_BND_0006）。
fn json_entry(package: &Path, name: &str) -> Result<Value, BundleError> {
    let text = zipwrite::entry_text(package, name)?.ok_or_else(|| BundleError::Archive {
        path: package.to_path_buf(),
        reason: format!("缺 {name}（必须落在包根）"),
    })?;
    serde_json::from_str(&text).map_err(|err| BundleError::Archive {
        path: package.to_path_buf(),
        reason: format!("{name} 非法 JSON：{err}"),
    })
}

/// 暂存树 = UI dist 整树（与 static-site 同一份 HTML 守底断言）。
fn stage_dist(dist: &Path, stage: &Path, packager: &'static str) -> Result<(), BundleError> {
    let _ = std::fs::remove_dir_all(stage);
    fsutil::copy_tree(dist, stage)?;
    assert_html_structure(stage, packager)
}

/// 入口必须在 dist 根（平台配置 / 清单都指向包根 `index.html`）。
fn require_entry(dist: &Path, packager: &'static str) -> Result<(), BundleError> {
    if dist.join(ENTRY).is_file() {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager,
        field: "ui_dist(index.html)",
        value: dist.display().to_string(),
        reason: format!("入口 {ENTRY} 不在 dist 根——平台包要求入口落在 zip 根"),
    })
}

fn package_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(format!(
        "{}-{}.zip",
        inputs.product.kebab_name(),
        inputs.product.version
    ))
}

fn json_text(value: &Value, path: &Path) -> Result<String, BundleError> {
    serde_json::to_string_pretty(value)
        .map(|text| format!("{text}\n"))
        .map_err(|error| BundleError::Io {
            path: path.to_path_buf(),
            source: std::io::Error::other(error),
        })
}

/// 打包格式与打包器声明必须一致（防「按 zip 目标产出别的格式」）。
fn check_format(spec: &TargetSpec, want: &str, packager: &'static str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager,
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}
