//! `.fba` 结构自检（T-M7-07；打包器自检与 `poly verify` 腿同一实现）。
//!
//! 三层口径：外层应用清单（`manifestVersion` 6 / `appId` UUID / `buildId` / `sdkVersion`）
//! → `components.watch` 声明的每个 `device-<family>.zip` 在场且其清单与 `appId` / 入口 /
//! 图标一致 → 可选 `companion.zip`（`manifestVersion` 2 + `companion.main` 在场）。

use std::path::Path;

use crate::error::BundleError;
use crate::zipwrite;

use super::{FAMILIES, ID, uuid_is_valid};

fn bad(package: &Path, reason: String) -> BundleError {
    BundleError::InvalidInput {
        packager: ID,
        field: "fba(结构自检)",
        value: package.display().to_string(),
        reason,
    }
}

/// 结构自检（打包器自检与 verify 腿同一实现）：外层清单 + 组件 zip + 组件清单三层。
pub fn check_fitbit_structure(package: &Path) -> Result<FitbitStructure, BundleError> {
    let bad = |reason: String| bad(package, reason);
    let mut entries = zipwrite::entry_names(package)?;
    entries.sort();
    let text = zipwrite::entry_text(package, "manifest.json")?
        .ok_or_else(|| bad("缺 manifest.json（应用清单必须落在 .fba 根）".to_string()))?;
    let manifest: serde_json::Value = serde_json::from_str(&text)
        .map_err(|err| bad(format!("manifest.json 非法 JSON：{err}")))?;
    if manifest["manifestVersion"].as_u64() != Some(6) {
        return Err(bad("manifestVersion 须为 6（官方 SDK 6.x）".to_string()));
    }
    let app_id = manifest["appId"]
        .as_str()
        .filter(|value| uuid_is_valid(value))
        .ok_or_else(|| bad("appId 须为合法 UUID".to_string()))?
        .to_string();
    let build_id = manifest["buildId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| bad("缺 buildId".to_string()))?
        .to_string();
    if manifest["sdkVersion"]["deviceApi"]
        .as_str()
        .is_none_or(|value| value.is_empty())
    {
        return Err(bad("缺 sdkVersion.deviceApi".to_string()));
    }
    let watch = manifest["components"]["watch"]
        .as_object()
        .filter(|watch| !watch.is_empty())
        .ok_or_else(|| bad("components.watch 须为非空对象".to_string()))?;
    let mut families = Vec::new();
    let mut device_main = String::new();
    for (family, component) in watch {
        if !FAMILIES.contains(&family.as_str()) {
            return Err(bad(format!("未知设备家族 {family}")));
        }
        let filename = component["filename"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| bad(format!("watch.{family}.filename 缺失")))?;
        if !entries.iter().any(|entry| entry == filename) {
            return Err(bad(format!("组件 {filename} 不在 .fba 内")));
        }
        let platforms = component["platform"]
            .as_array()
            .filter(|items| !items.is_empty())
            .ok_or_else(|| bad(format!("watch.{family}.platform 须为非空数组")))?;
        if platforms.iter().any(|item| item.as_str().is_none()) {
            return Err(bad(format!("watch.{family}.platform 须为字符串数组")));
        }
        let bytes = zipwrite::entry_bytes(package, filename)?
            .ok_or_else(|| bad(format!("组件 {filename} 读不出字节")))?;
        let names = zipwrite::bytes_entry_names(&bytes, package)?;
        let device_text = zipwrite::bytes_entry(&bytes, "manifest.json", package)?
            .and_then(|raw| String::from_utf8(raw).ok())
            .ok_or_else(|| bad(format!("{filename} 缺 manifest.json")))?;
        let device: serde_json::Value = serde_json::from_str(&device_text)
            .map_err(|err| bad(format!("{filename} 的 manifest.json 非法：{err}")))?;
        if device["appManifestVersion"].as_u64() != Some(1) {
            return Err(bad(format!("{filename} appManifestVersion 须为 1")));
        }
        if device["uuid"].as_str() != Some(&app_id) {
            return Err(bad(format!("{filename} 的 uuid 与应用清单 appId 不一致")));
        }
        let main = device["main"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| bad(format!("{filename} 缺 main")))?;
        if !names.iter().any(|name| name == main) {
            return Err(bad(format!("{filename} 的入口 {main} 不在组件内")));
        }
        if let Some(icon) = device["iconFile"].as_str()
            && !names.iter().any(|name| name == icon)
        {
            return Err(bad(format!("{filename} 的图标 {icon} 不在组件内")));
        }
        if device["name"].as_str().is_none_or(|value| value.is_empty()) {
            return Err(bad(format!("{filename} 缺 name")));
        }
        device_main = main.to_string();
        families.push(family.clone());
    }
    families.sort();
    let companion = if let Some(component) = manifest["components"]["companion"].as_object() {
        let filename = component["filename"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| bad("components.companion.filename 缺失".to_string()))?;
        if !entries.iter().any(|entry| entry == filename) {
            return Err(bad(format!("组件 {filename} 不在 .fba 内")));
        }
        let bytes = zipwrite::entry_bytes(package, filename)?
            .ok_or_else(|| bad(format!("组件 {filename} 读不出字节")))?;
        let names = zipwrite::bytes_entry_names(&bytes, package)?;
        let text = zipwrite::bytes_entry(&bytes, "manifest.json", package)?
            .and_then(|raw| String::from_utf8(raw).ok())
            .ok_or_else(|| bad(format!("{filename} 缺 manifest.json")))?;
        let manifest: serde_json::Value = serde_json::from_str(&text)
            .map_err(|err| bad(format!("{filename} 的 manifest.json 非法：{err}")))?;
        if manifest["manifestVersion"].as_u64() != Some(2) {
            return Err(bad(format!("{filename} manifestVersion 须为 2")));
        }
        let main = manifest["companion"]["main"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| bad(format!("{filename} 缺 companion.main")))?;
        if !names.iter().any(|name| name == main) {
            return Err(bad(format!("{filename} 的入口 {main} 不在组件内")));
        }
        true
    } else {
        false
    };
    let bytes = std::fs::metadata(package)
        .map_err(|source| BundleError::Io {
            path: package.to_path_buf(),
            source,
        })?
        .len();
    if bytes == 0 {
        return Err(bad("包体积为 0".to_string()));
    }
    Ok(FitbitStructure {
        entries,
        app_id,
        build_id,
        families,
        companion,
        device_main,
        bytes,
    })
}

/// 结构自检结果（verify 腿的判决面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FitbitStructure {
    /// `.fba` 条目名（排序）。
    pub entries: Vec<String>,
    /// 应用 UUID（应用清单 `appId`）。
    pub app_id: String,
    /// 构建 id（`0x0` + 15 hex）。
    pub build_id: String,
    /// 设备家族（排序）。
    pub families: Vec<String>,
    /// 是否随包 `companion.zip`。
    pub companion: bool,
    /// 设备组件入口（首个家族）。
    pub device_main: String,
    /// 包体积（字节）。
    pub bytes: u64,
}
