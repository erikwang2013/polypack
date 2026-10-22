//! 具体探测器实现（每探测器一文件）与共享的子进程探测工具。
//!
//! 全部探测只读：命令不存在 / 超时 / 非零退出一律如实返回缺失，绝不 panic，
//! 单个子进程最多等待 [`PROBE_TIMEOUT`]。

pub mod android_ndk;
pub mod android_sdk;
pub mod emsdk;
pub mod harmonyos_sdk;
pub mod msvc;
pub mod qnx_sdp;
pub mod rust;
pub mod xcode;

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::probe::{ProbeResult, Toolchain};

/// 单个子进程探测的超时上限。
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// L1 七个目标 id（与 `polypack-targets` 内建目标一致）。
pub(crate) const L1_TARGETS: [&str; 7] = [
    "android",
    "harmonyos",
    "ios",
    "linux",
    "macos",
    "web",
    "windows",
];

/// 运行命令并返回修剪后的 stdout；命令不存在、非零退出或超时返回 `None`。
///
/// stdout 在进程退出后一次性读取，stderr 直接丢弃（版本命令的 stderr 无信息量）。
/// 这是全部子进程探测的唯一入口，超时语义在此统一。
pub fn run_capture(program: impl AsRef<OsStr>, args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = Command::new(program.as_ref())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            // 进程已结束：下面的 wait_with_output 只会读尽管道，不再阻塞
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            // 超时或等待出错：杀进程后按缺失处理
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// 用户主目录（`$HOME`，Windows 回退 `%USERPROFILE%`）。
pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// 构造「已找到」的探测结果。
pub(crate) fn found_result(
    id: &'static str,
    required_for: &'static [&'static str],
    path: PathBuf,
    version: Option<String>,
) -> ProbeResult {
    ProbeResult {
        id,
        found: Some(Toolchain {
            id: id.to_string(),
            path,
            version,
        }),
        required_for: required_for.to_vec(),
        note: None,
    }
}

/// 构造「未找到」的探测结果（`note` 说明原因与建议）。
pub(crate) fn missing_result(
    id: &'static str,
    required_for: &'static [&'static str],
    note: impl Into<String>,
) -> ProbeResult {
    ProbeResult {
        id,
        found: None,
        required_for: required_for.to_vec(),
        note: Some(note.into()),
    }
}
