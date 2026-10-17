//! 各平台打包器实现（每个打包器一个 `Packager` impl）。
//!
//! - [`windows`]：WiX MSI 与 NSIS（T-M1-05，生成 .wxs / .nsi 与调用命令）
//! - [`dmg`]：macOS DMG（T-M1-06，hdiutil 组装 + 拖拽安装布局）
//! - [`linux`]：deb / AppImage / rpm / Flatpak / Snap（T-M1-07、T-M5-01..04）
//! - [`web`]：Web 静态站点（T-M1-08，含 SRI 清单）
//! - [`android`]：AAB 与 TV / AAOS 变体（T-M2-06 / T-M2-07 / T-M4-04 / T-M6-11，AAPT2 免 Gradle + bundletool）
//! - [`ios`]：IPA（T-M2-03 / T-M2-08，XCFramework 绑定 + xcodebuild archive / 归档 Payload 直压）
//! - [`harmony`]：HAP（T-M3-03，工程配置生成 + hvigor 调用 + 结构检查）
//! - [`miniapp`]：小程序族（T-M3-06，Taro 编译 + 代码包组装；快应用编译工程）
//! - [`tizen`]：Tizen TV `.wgt`（T-M4-08，config.xml + UI 载荷 + 结构检查）
//! - [`webos`]：webOS TV `.ipk`（T-M4-09，Debian 形态 ar 归档 + 结构检查）
//! - [`webtv`]：VIDAA / Vizio SmartCast 应用包（T-M4-10，两目标 + 沙箱自检）
//! - [`openbsd`]：OpenBSD ports 骨架（T-M5-07，Makefile / PLIST / DESCR + signify / portcheck）
//! - [`freebsd`]：FreeBSD pkg（T-M5-06，`pkg create` + pkg 暂存树 / manifest 结构自检）
//! - [`pkgsrc`]：NetBSD / DragonFly pkgsrc 骨架（T-M5-08，Makefile / PLIST / DESCR + 结构自检）
//! - [`roku`]：Roku 频道 `.zip`（T-M4-15，复用 SceneGraph 发射器 + 侧载结构自检）
//! - [`qnx`]：QNX 静态库 + 部署镜像（T-M6-03，QNX SDP 交叉；无 SDP 按环境槽 SKIP）
//! - [`autosar`]：AUTOSAR Adaptive 静态库 + ARXML 服务清单（T-M6-04，C ABI 单一来源）
//! - [`embedded`]：嵌入式共享实现（T-M6-05/06，引擎静态库构建 / C ABI 断言 / §5.3 路径审计）
//! - [`vxworks`]：VxWorks 库 / 镜像 kit（T-M6-05，wr-cc 链接腿 + nm 符号断言）
//! - [`zephyr`]：Zephyr west 工程模板（T-M6-06，module 链入 no_std 引擎 + 两板构建）
//! - [`freertos`]：FreeRTOS QEMU 示例板（T-M6-07，mps2-an385 + 内核按 tag/sha256 现拉）
//! - [`fitbit`]：Fitbit OS 应用包 `.fba`（T-M7-07，SDK 6.1 结构 + 三层结构自检）
//! - [`zepp`]：Zepp OS 小程序 `.zab`（T-M7-08，manifest + md5 命名 zpk + 内层 device.zip）

pub mod android;
pub mod autosar;
pub mod dmg;
pub mod embedded;
pub mod fitbit;
pub mod freebsd;
pub mod freertos;
pub mod harmony;
pub mod ios;
pub mod linux;
pub mod miniapp;
pub mod openbsd;
pub mod pkgsrc;
pub mod qnx;
pub mod roku;
pub mod tizen;
pub mod vxworks;
pub mod web;
pub mod webos;
pub mod webtv;
pub mod windows;
pub mod zephyr;
pub mod zepp;
