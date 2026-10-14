//! MSVC（Visual Studio）探测：Windows 上经 `vswhere` 定位，其他宿主如实跳过。

use std::path::PathBuf;

use crate::probe::{Probe, ProbeResult};

use super::{PROBE_TIMEOUT, found_result, missing_result, run_capture};

/// MSVC 探测器。
pub struct Msvc;

impl Probe for Msvc {
    fn id(&self) -> &'static str {
        "msvc"
    }

    fn required_for(&self) -> &'static [&'static str] {
        &["windows"]
    }

    fn locate(&self) -> ProbeResult {
        if !cfg!(target_os = "windows") {
            return missing_result(
                self.id(),
                self.required_for(),
                "仅 Windows 宿主（本机非 Windows）",
            );
        }
        let vswhere = vswhere_path();
        if !vswhere.is_file() {
            return missing_result(
                self.id(),
                self.required_for(),
                "未找到 vswhere（安装 Visual Studio 2017+ 或独立 Build Tools）",
            );
        }
        let version = run_capture(
            &vswhere,
            &[
                "-latest",
                "-products",
                "*",
                "-property",
                "installationVersion",
            ],
            PROBE_TIMEOUT,
        );
        let Some(version) = version else {
            return missing_result(
                self.id(),
                self.required_for(),
                "vswhere 未报告安装（未安装 VS Build Tools？）",
            );
        };
        let path = run_capture(
            &vswhere,
            &["-latest", "-products", "*", "-property", "installationPath"],
            PROBE_TIMEOUT,
        )
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir())
        .unwrap_or(vswhere);
        found_result(self.id(), self.required_for(), path, Some(version))
    }
}

/// `vswhere.exe` 的标准安装位置（Visual Studio Installer 固定装在此处）。
fn vswhere_path() -> PathBuf {
    let pf86 = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    pf86.join("Microsoft Visual Studio/Installer/vswhere.exe")
}
