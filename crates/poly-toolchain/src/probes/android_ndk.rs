//! Android NDK 探测：`$ANDROID_NDK_HOME` 或 `<sdk>/ndk/<最新版本>`。

use std::path::{Path, PathBuf};

use crate::probe::{Probe, ProbeResult};

use super::android_sdk::sdk_dir;
use super::{found_result, missing_result};

/// Android NDK 探测器。
pub struct AndroidNdk;

impl Probe for AndroidNdk {
    fn id(&self) -> &'static str {
        "android-ndk"
    }

    fn required_for(&self) -> &'static [&'static str] {
        &["android"]
    }

    fn locate(&self) -> ProbeResult {
        let dir = std::env::var_os("ANDROID_NDK_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir())
            .or_else(|| sdk_dir().and_then(|sdk| latest_ndk(&sdk)));
        match dir {
            Some(dir) => {
                let version = ndk_version(&dir);
                found_result(self.id(), self.required_for(), dir, version)
            }
            None => missing_result(
                self.id(),
                self.required_for(),
                "未找到 Android NDK（设置 $ANDROID_NDK_HOME，或 sdkmanager \"ndk;<版本>\"）",
            ),
        }
    }
}

/// `<sdk>/ndk/` 下数字版本号最大的子目录（`source.properties` 等非版本目录跳过）。
fn latest_ndk(sdk: &Path) -> Option<PathBuf> {
    let mut best: Option<(Vec<u32>, PathBuf)> = None;
    for entry in std::fs::read_dir(sdk.join("ndk")).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(key) = version_key(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        if best.as_ref().is_none_or(|(top, _)| key > *top) {
            best = Some((key, path));
        }
    }
    best.map(|(_, path)| path)
}

/// 目录名 → 数字版本键（`27.0.12077973` → `[27, 0, 12077973]`）；非纯数字名返回 `None`。
fn version_key(name: &str) -> Option<Vec<u32>> {
    let key: Vec<u32> = name
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    if key.is_empty() { None } else { Some(key) }
}

/// 版本：优先 `source.properties` 的 `Pkg.Revision`，回退目录名。
fn ndk_version(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("source.properties"))
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let value = line
                    .trim()
                    .strip_prefix("Pkg.Revision")?
                    .split('=')
                    .nth(1)?;
                Some(value.trim().to_string())
            })
        })
        .or_else(|| Some(dir.file_name()?.to_string_lossy().into_owned()))
}
