//! Rust 工具链探测：rustc / cargo 版本与 sysroot —— L1 全部目标的基础。

use std::path::PathBuf;

use crate::probe::{Probe, ProbeResult};

use super::{L1_TARGETS, PROBE_TIMEOUT, found_result, missing_result, run_capture};

/// rustc + cargo 探测器。
pub struct Rust;

impl Probe for Rust {
    fn id(&self) -> &'static str {
        "rust"
    }

    fn required_for(&self) -> &'static [&'static str] {
        &L1_TARGETS
    }

    fn locate(&self) -> ProbeResult {
        let Some(rustc) = run_capture("rustc", &["--version"], PROBE_TIMEOUT) else {
            return missing_result(
                self.id(),
                self.required_for(),
                "未找到 rustc（安装：https://rustup.rs）",
            );
        };
        // cargo 版本尽力采集：缺失不改变 found 判定（构建阶段会再报 E_PROBE_0001）
        let version = match run_capture("cargo", &["--version"], PROBE_TIMEOUT) {
            Some(cargo) => format!("{rustc}；{cargo}"),
            None => rustc,
        };
        let path = run_capture("rustc", &["--print", "sysroot"], PROBE_TIMEOUT)
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(|| PathBuf::from("rustc"));
        found_result(self.id(), self.required_for(), path, Some(version))
    }
}
