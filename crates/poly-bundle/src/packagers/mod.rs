//! 各平台打包器实现（每个打包器一个 `Packager` impl）。
//!
//! - [`windows`]：WiX MSI 与 NSIS（T-M1-05，生成 .wxs / .nsi 与调用命令）
//! - [`dmg`]：macOS DMG（T-M1-06，hdiutil 组装 + 拖拽安装布局）
//! - [`linux`]：deb 与 AppImage（T-M1-07）
//! - [`web`]：Web 静态站点（T-M1-08，含 SRI 清单）
//! - [`android`]：AAB（T-M2-06 / T-M2-07，AAPT2 免 Gradle + bundletool）
//! - [`ios`]：IPA（T-M2-03 / T-M2-08，XCFramework 绑定 + xcodebuild archive / 归档 Payload 直压）
//! - [`harmony`]：HAP（T-M3-03，工程配置生成 + hvigor 调用 + 结构检查）

pub mod android;
pub mod dmg;
pub mod harmony;
pub mod ios;
pub mod linux;
pub mod web;
pub mod windows;
