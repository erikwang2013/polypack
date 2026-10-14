//! PolyPack 打包器矩阵（M1+ 实现）。
//!
//! 首批打包器：MSI / NSIS（T-M1-05）、DMG（T-M1-06）、AppImage / deb（T-M1-07）、
//! Web 静态站点（T-M1-08）。契约见 ROADMAP §4.1 `Packager`。
//!
//! 入口：
//! - [`packager::Packager`]：`id` / `format` / `plan`（生成脚本与命令，纯函数）/ `package`（执行）
//! - [`packager::find`]：按（目标 id，产物格式）取内建打包器
//! - [`tauri::bare_product`]：T-M1-03 裸产物布局与 WebView 资源注入

pub mod error;
pub mod exec;
pub mod fsutil;
pub mod icons;
pub mod inputs;
pub mod packager;
pub mod packagers;
pub mod sri;
pub mod tauri;

pub use error::{BundleError, codes};
pub use icons::{IconSet, derive as derive_icons};
pub use inputs::{BareProduct, BundleInputs, Icon, IconKind, ProductInfo};
pub use packager::{
    Artifact, ArtifactKind, BundlePlan, Command, Packager, ScriptFile, Skipped, builtin, find,
    for_spec,
};
