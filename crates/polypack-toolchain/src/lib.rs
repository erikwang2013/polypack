//! PolyPack 工具链探测：SDK / NDK / Xcode / emsdk 的定位与版本报告。
//!
//! 本 crate 处于 workspace 依赖图的叶层，不依赖任何其他 polypack-* crate。
//! 探测只读：找不到即如实报告，安装由用户显式触发（M1+ `polypack doctor --install`）。

pub mod probe;
pub mod probes;

pub use probe::{Probe, ProbeResult, Toolchain, default_probes};
pub use probes::qnx_sdp::{SdpEnv, sdp_env, sdp_root};
