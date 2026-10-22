//! emsdk（Emscripten）探测：`$EMSDK` 或 `~/emsdk`。

use std::path::{Path, PathBuf};

use crate::probe::{Probe, ProbeResult};

use super::{found_result, home_dir, missing_result};

/// Emscripten SDK 探测器。
pub struct Emsdk;

impl Probe for Emsdk {
    fn id(&self) -> &'static str {
        "emsdk"
    }

    fn required_for(&self) -> &'static [&'static str] {
        &["web"]
    }

    fn locate(&self) -> ProbeResult {
        let dir = std::env::var_os("EMSDK")
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir())
            .or_else(|| {
                home_dir()
                    .map(|home| home.join("emsdk"))
                    .filter(|d| d.is_dir())
            });
        match dir {
            Some(dir) => {
                let version = emsdk_version(&dir);
                found_result(self.id(), self.required_for(), dir, version)
            }
            None => missing_result(
                self.id(),
                self.required_for(),
                "未找到 emsdk（git clone https://github.com/emscripten-core/emsdk 后 ./emsdk install latest）",
            ),
        }
    }
}

/// 版本：读 `<root>/.emscripten` 中 `EMSDK_VERSION` 的带引号值；读不到返回 `None`。
fn emsdk_version(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join(".emscripten")).ok()?;
    text.lines().find_map(|line| {
        let value = line
            .trim()
            .strip_prefix("EMSDK_VERSION")?
            .split('=')
            .nth(1)?;
        Some(value.trim().trim_matches(['\'', '"']).to_string())
    })
}
