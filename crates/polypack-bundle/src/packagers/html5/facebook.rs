//! Facebook Instant Games 打包器与结构自检。

use std::path::Path;

use serde_json::{Value, json};

use polypack_targets::TargetSpec;

use crate::error::BundleError;
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, BundlePlan, Packager};
use crate::webapp::{self, ENTRY};
use crate::zipwrite;

use super::{
    FACEBOOK_INSTANT_GAMES_ID, FBAPP_CONFIG, FORMAT, check_format, checked_entries, index_html,
    json_entry, json_text, package_path, require_entry, stage_dist,
};

const FACEBOOK_WORK_DIR: &str = "facebook-instant-games";

/// Facebook Instant Games 打包器（`zip`）。
pub struct FacebookInstantGames;

impl Packager for FacebookInstantGames {
    fn id(&self) -> &'static str {
        FACEBOOK_INSTANT_GAMES_ID
    }

    fn format(&self) -> &'static str {
        FORMAT
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, FORMAT, FACEBOOK_INSTANT_GAMES_ID)?;
        require_entry(
            inputs.require_ui_dist(FACEBOOK_INSTANT_GAMES_ID)?,
            FACEBOOK_INSTANT_GAMES_ID,
        )?;
        Ok(BundlePlan::new())
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        self.plan(inputs, spec)?;
        let dist = inputs.require_ui_dist(FACEBOOK_INSTANT_GAMES_ID)?;
        let stage = inputs.out_dir.join(FACEBOOK_WORK_DIR);
        stage_dist(dist, &stage, FACEBOOK_INSTANT_GAMES_ID)?;

        let config = json!({
            "instant_games": {
                "platform_version": "RICH_GAMEPLAY",
                "orientation": "LANDSCAPE",
                "navigation_menu_version": "NAV_FLOATING",
            }
        });
        fsutil::write_text(
            &stage.join(FBAPP_CONFIG),
            &json_text(&config, &stage.join(FBAPP_CONFIG))?,
        )?;

        let package = package_path(inputs);
        zipwrite::write_zip(&webapp::collect_files(&stage)?, &package)?;
        let structure = check_facebook_instant_games_structure(&package)?;

        let mut notes = vec![
            format!(
                "产物：{}（Facebook Instant Games zip，{} 条目；包根 {ENTRY} + {FBAPP_CONFIG} \
                 + UI dist 整树）",
                package.display(),
                structure.entries.len()
            ),
            format!(
                "{FBAPP_CONFIG}：platform_version={} orientation={} navigation_menu_version={}",
                structure.platform_version,
                structure.orientation,
                structure.navigation_menu_version
            ),
            format!(
                "确定性 zip：条目排序 + 固定时间戳（{FACEBOOK_INSTANT_GAMES_ID} 与 verify 腿 \
                 facebook-instant-games-structure-check 共用同一份结构检查）"
            ),
            "平台侧：index.html 须引用 connect.facebook.net 的 fbinstant SDK；Meta 应用后台 / \
             沙箱审核本机不可得——SKIP-with-print，experimental，不声明商店受理"
                .to_string(),
        ];
        if let Some(boundary) = &spec.notes {
            notes.push(format!("目标声明边界：{boundary}"));
        }
        fsutil::file_artifact(FACEBOOK_INSTANT_GAMES_ID, FORMAT, &package, notes)
    }
}

/// Facebook Instant Games zip 结构自检（打包器自检与 verify 腿同一实现）。
///
/// 规则：zip 条目非空且路径相对；包根 `index.html` 与 `fbapp-config.json`；
/// `instant_games` 下
/// `platform_version` / `orientation` / `navigation_menu_version` 非空且取值在官方枚举内。
pub fn check_facebook_instant_games_structure(
    package: &Path,
) -> Result<FacebookInstantGamesStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: package.to_path_buf(),
        reason,
    };
    let entries = checked_entries(package)?;
    index_html(package, &entries)?;

    let config = json_entry(package, FBAPP_CONFIG)?;
    let instant = config
        .get("instant_games")
        .and_then(Value::as_object)
        .ok_or_else(|| bad(format!("{FBAPP_CONFIG} 缺 instant_games 对象")))?;
    let field = |key: &str| -> Result<String, BundleError> {
        instant
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_string)
            .ok_or_else(|| {
                bad(format!(
                    "{FBAPP_CONFIG}.instant_games 缺必填非空字符串：{key}"
                ))
            })
    };
    let platform_version = field("platform_version")?;
    let orientation = field("orientation")?;
    let navigation_menu_version = field("navigation_menu_version")?;
    if !matches!(platform_version.as_str(), "RICH_GAMEPLAY" | "CLASSIC") {
        return Err(bad(format!(
            "{FBAPP_CONFIG}.instant_games.platform_version 非法：{platform_version}"
        )));
    }
    if !matches!(orientation.as_str(), "PORTRAIT" | "LANDSCAPE") {
        return Err(bad(format!(
            "{FBAPP_CONFIG}.instant_games.orientation 非法：{orientation}"
        )));
    }
    if !matches!(navigation_menu_version.as_str(), "NAV_FLOATING" | "NAV_BAR") {
        return Err(bad(format!(
            "{FBAPP_CONFIG}.instant_games.navigation_menu_version 非法：{navigation_menu_version}"
        )));
    }
    Ok(FacebookInstantGamesStructure {
        entries,
        platform_version,
        orientation,
        navigation_menu_version,
    })
}

/// Facebook Instant Games 结构自检结果（verify 腿的判决面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacebookInstantGamesStructure {
    /// zip 条目名（排序）。
    pub entries: Vec<String>,
    /// `fbapp-config.json.instant_games.platform_version`。
    pub platform_version: String,
    /// `fbapp-config.json.instant_games.orientation`。
    pub orientation: String,
    /// `fbapp-config.json.instant_games.navigation_menu_version`。
    pub navigation_menu_version: String,
}
