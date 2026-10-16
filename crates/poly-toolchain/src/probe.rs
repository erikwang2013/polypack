//! 工具链探测契约（ROADMAP §4.1）：`locate` 只读探测，绝不自动安装。

use std::path::PathBuf;

use serde::Serialize;

/// 一个被定位到的工具链。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Toolchain {
    /// 工具链标识（如 `android-sdk`）。
    pub id: String,
    /// 安装路径。
    pub path: PathBuf,
    /// 版本（探测得到时输出）。
    pub version: Option<String>,
}

/// 一次探测的结果（未找到时 `found` 为 `None`，`note` 说明原因）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeResult {
    /// 探测项标识。
    pub id: &'static str,
    /// 找到的工具链（若有）。
    pub found: Option<Toolchain>,
    /// 该工具链服务于哪些目标（TargetSpec.id）。
    pub required_for: Vec<&'static str>,
    /// 未找到时的原因 / 建议。
    pub note: Option<String>,
}

/// 工具链探测器。
pub trait Probe: Send + Sync {
    /// 探测项标识。
    fn id(&self) -> &'static str;
    /// 该工具链服务于哪些目标。
    fn required_for(&self) -> &'static [&'static str];
    /// 只读探测（不安装、不修改系统）。
    fn locate(&self) -> ProbeResult;
}

/// 默认探测集合：rust / android-sdk / android-ndk / harmonyos-sdk / xcode / msvc / emsdk。
///
/// 具体实现见 [`crate::probes`]（每探测器一文件）。
pub fn default_probes() -> Vec<Box<dyn Probe>> {
    vec![
        Box::new(crate::probes::rust::Rust),
        Box::new(crate::probes::android_sdk::AndroidSdk),
        Box::new(crate::probes::android_ndk::AndroidNdk),
        Box::new(crate::probes::harmonyos_sdk::HarmonyOsSdk),
        Box::new(crate::probes::xcode::Xcode),
        Box::new(crate::probes::msvc::Msvc),
        Box::new(crate::probes::emsdk::Emsdk),
    ]
}
