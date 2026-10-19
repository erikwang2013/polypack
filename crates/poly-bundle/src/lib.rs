//! PolyPack 打包器矩阵（M1+ 实现）。
//!
//! 首批打包器：MSI / NSIS（T-M1-05）、DMG（T-M1-06）、AppImage / deb（T-M1-07）、
//! Web 静态站点（T-M1-08）。契约见 ROADMAP §4.1 `Packager`。
//!
//! 入口：
//! - [`packager::Packager`]：`id` / `format` / `plan`（生成脚本与命令，纯函数）/ `package`（执行）
//! - [`packager::find`]：按（目标 id，产物格式）取内建打包器
//! - [`tauri::bare_product`]：T-M1-03 裸产物布局与 WebView 资源注入
//! - [`webapp`]：web 系 TV 目标的共享载荷与同源摘要（T-M4-08 / T-M4-09 / T-M4-10）
//! - [`pwa`]：PWA 三腿审计（manifest / 离线能力 / 安装性，T-M5-10）
//! - [`arwrite`]：`.ipk` 的 ar / ustar tar / gzip 读写（T-M4-09，确定性字节）

pub mod arwrite;
pub mod embedded;
pub mod error;
pub mod exec;
pub mod fsutil;
pub mod icons;
pub mod inputs;
pub mod packager;
pub mod packagers;
pub mod provider;
pub mod pwa;
pub mod sri;
pub mod tauri;
pub mod tools;
pub mod webapp;
pub mod zipwrite;

pub use embedded::{EmbeddedInputs, EmbeddedLib, QnxEnv};
pub use error::{BundleError, codes};
pub use icons::{IconSet, derive as derive_icons};
pub use inputs::{
    AndroidInputs, BareProduct, BundleInputs, HarmonyInputs, Icon, IconKind, IosInputs, NativeLib,
    ProductInfo, RokuInputs,
};
pub use packager::{
    Artifact, ArtifactKind, BundlePlan, Command, Packager, ScriptFile, Skipped, builtin, find,
    for_spec,
};
pub use packagers::autosar::{
    ArxmlStructure, AutosarAdaptive, SCHEMA_FILE as AUTOSAR_SCHEMA_FILE, check_arxml,
    xmllint_command, xmllint_tool,
};
pub use packagers::fitbit::{FitbitStructure, check_fitbit_structure};
pub use packagers::freebsd::{PkgStructure, check_pkg_structure};
pub use packagers::freertos::{FreeRtosStructure, check_freertos_structure};
pub use packagers::garmin::{GarminIqStructure, check_garmin_iq_structure};
pub use packagers::harmony::{HapStructure, check_hap_structure};
pub use packagers::html5::{
    FacebookInstantGamesStructure, TiktokMinisStructure, check_facebook_instant_games_structure,
    check_tiktok_minis_structure,
};
pub use packagers::kaios::{KaiosStructure, check_kaios_structure};
pub use packagers::linux::{
    AppdirStructure, DebStructure, FlatpakStructure, RpmStructure, SnapStructure,
    check_appdir_structure, check_deb_structure, check_flatpak_structure, check_rpm_structure,
    check_snap_structure,
};
pub use packagers::miniapp::quickapp_hap_root;
pub use packagers::openbsd::{PortsStructure, check_ports_structure};
pub use packagers::pkgsrc::{PkgsrcStructure, check_pkgsrc_structure};
pub use packagers::qnx::{QnxImage, QnxLibrary, QnxStructure, check_qnx_structure};
pub use packagers::roku::{RokuStructure, check_roku_structure};
pub use packagers::tizen::{WgtStructure, check_wgt_structure, wgt_lint_tool};
pub use packagers::vxworks::{VxWorksStructure, check_vxworks_structure};
pub use packagers::webos::{IpkStructure, ares_package_tool, check_ipk_structure};
pub use packagers::webtv::{
    SMARTCAST, VIDAA, WebAppPlatform, WebAppStructure, check_web_app_structure,
};
pub use packagers::zephyr::{ZephyrStructure, check_zephyr_structure};
pub use packagers::zepp::{ZeppStructure, check_zepp_structure};
pub use provider::{BuiltinProvider, official_providers, official_registry};
pub use pwa::{PwaAudit, audit_pwa};
