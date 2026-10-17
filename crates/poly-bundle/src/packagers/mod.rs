//! 各平台打包器实现（每个打包器一个 `Packager` impl）。
//!
//! - [`windows`]：WiX MSI 与 NSIS（T-M1-05，生成 .wxs / .nsi 与调用命令）
//! - [`dmg`]：macOS DMG（T-M1-06，hdiutil 组装 + 拖拽安装布局）
//! - [`linux`]：deb 与 AppImage（T-M1-07）
//! - [`web`]：Web 静态站点（T-M1-08，含 SRI 清单）
//! - [`android`]：AAB 与 TV 变体（T-M2-06 / T-M2-07 / T-M4-04，AAPT2 免 Gradle + bundletool）
//! - [`ios`]：IPA（T-M2-03 / T-M2-08，XCFramework 绑定 + xcodebuild archive / 归档 Payload 直压）
//! - [`harmony`]：HAP（T-M3-03，工程配置生成 + hvigor 调用 + 结构检查）
//! - [`miniapp`]：小程序族（T-M3-06，Taro 编译 + 代码包组装；快应用编译工程）
//! - [`tizen`]：Tizen TV `.wgt`（T-M4-08，config.xml + UI 载荷 + 结构检查）
//! - [`webos`]：webOS TV `.ipk`（T-M4-09，Debian 形态 ar 归档 + 结构检查）
//! - [`webtv`]：VIDAA / Vizio SmartCast 应用包（T-M4-10，两目标 + 沙箱自检）
//! - [`openbsd`]：OpenBSD ports 骨架（T-M5-07，Makefile / PLIST / DESCR + signify / portcheck）
//! - [`roku`]：Roku 频道 `.zip`（T-M4-15，复用 SceneGraph 发射器 + 侧载结构自检）

pub mod android;
pub mod dmg;
pub mod harmony;
pub mod ios;
pub mod linux;
pub mod miniapp;
pub mod openbsd;
pub mod roku;
pub mod tizen;
pub mod web;
pub mod webos;
pub mod webtv;
pub mod windows;
