//! PolyPack WASM 插件宿主（M8 实现）。
//!
//! 插件经 WIT 接口描述、wasmtime 沙箱运行，能力显式授权（ROADMAP §4.5）：
//! - 默认无文件系统、无网络；`polypack.toml` 的 `[[plugins]]` 逐项授权后才可访问；
//! - 越权访问返回稳定错误码（`poly::E_PLUGIN_0001` / `_0002`）；
//! - fuel / epoch / 内存三道上限，配置只能收紧不能放开（[`limits`]）。
//!
//! 接口版本：`target-spec/v1`（[`TARGET_SPEC_VERSION`]），非 v1 插件拒绝并给迁移提示。

#![warn(missing_docs)]

pub mod capability;
pub mod error;
pub mod limits;
pub mod provider;
pub mod sandbox;

pub use capability::{AuthorizedUrl, CapabilityPolicy};
pub use error::PluginError;
pub use limits::ResourceLimits;
pub use provider::PluginProvider;
pub use sandbox::{
    PackageInput, PackagedArtifact, Plugin, Sandbox, TARGET_SPEC_VERSION, TargetSpecInfo,
    content_digest, validate_spec_version,
};
