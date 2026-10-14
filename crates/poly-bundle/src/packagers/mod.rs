//! 各平台打包器实现（每个打包器一个 `Packager` impl）。
//!
//! - [`windows`]：WiX MSI 与 NSIS（T-M1-05，生成 .wxs / .nsi 与调用命令）
//! - [`dmg`]：macOS DMG（T-M1-06，hdiutil 组装 + 拖拽安装布局）
//! - [`linux`]：deb 与 AppImage（T-M1-07）
//! - [`web`]：Web 静态站点（T-M1-08，含 SRI 清单）

pub mod dmg;
pub mod linux;
pub mod web;
pub mod windows;
