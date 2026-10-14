//! Xcode / Command Line Tools 探测（`xcodebuild -version`，仅 macOS 宿主有）。

use std::path::PathBuf;

use crate::probe::{Probe, ProbeResult};

use super::{PROBE_TIMEOUT, found_result, missing_result, run_capture};

/// Xcode 探测器。
pub struct Xcode;

impl Probe for Xcode {
    fn id(&self) -> &'static str {
        "xcode"
    }

    fn required_for(&self) -> &'static [&'static str] {
        &["ios", "macos"]
    }

    fn locate(&self) -> ProbeResult {
        let version = run_capture("xcodebuild", &["-version"], PROBE_TIMEOUT)
            .and_then(|out| out.lines().next().map(|line| line.trim().to_string()));
        let Some(version) = version else {
            return missing_result(
                self.id(),
                self.required_for(),
                if cfg!(target_os = "macos") {
                    "未找到 xcodebuild（安装 Xcode 或 xcode-select --install）"
                } else {
                    "仅 macOS 宿主（本机无 xcodebuild）"
                },
            );
        };
        let path = run_capture("xcode-select", &["-p"], PROBE_TIMEOUT)
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(|| PathBuf::from("xcodebuild"));
        found_result(self.id(), self.required_for(), path, Some(version))
    }
}
