//! Android SDK 探测：`$ANDROID_HOME` / `$ANDROID_SDK_ROOT` / `~/Android/Sdk`。

use std::path::{Path, PathBuf};

use crate::probe::{Probe, ProbeResult};

use super::{found_result, home_dir, missing_result};

/// Android SDK 探测器。
pub struct AndroidSdk;

impl Probe for AndroidSdk {
    fn id(&self) -> &'static str {
        "android-sdk"
    }

    fn required_for(&self) -> &'static [&'static str] {
        &["android"]
    }

    fn locate(&self) -> ProbeResult {
        match sdk_dir() {
            Some(dir) => {
                let version = platforms_version(&dir);
                found_result(self.id(), self.required_for(), dir, version)
            }
            None => missing_result(
                self.id(),
                self.required_for(),
                "未找到 Android SDK（检查 $ANDROID_HOME / $ANDROID_SDK_ROOT / ~/Android/Sdk）",
            ),
        }
    }
}

/// 按优先级返回存在的 SDK 目录（android-ndk 探测器复用此解析）。
pub(crate) fn sdk_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for key in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(value) = std::env::var_os(key)
            && !value.is_empty()
        {
            candidates.push(PathBuf::from(value));
        }
    }
    if let Some(home) = home_dir() {
        candidates.push(home.join("Android/Sdk"));
    }
    candidates.into_iter().find(|dir| dir.is_dir())
}

/// 从 `platforms/` 目录推断 SDK 版本：最大的 `android-<API>`（预览版名如
/// `android-Baklava` 无数值 API，跳过）。
fn platforms_version(sdk: &Path) -> Option<String> {
    let mut best: Option<(u32, String)> = None;
    for entry in std::fs::read_dir(sdk.join("platforms")).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(api) = name
            .strip_prefix("android-")
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if best.as_ref().is_none_or(|(top, _)| api > *top) {
            best = Some((api, name));
        }
    }
    best.map(|(_, name)| name)
}
