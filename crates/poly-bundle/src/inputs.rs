//! 打包阶段输入：产品元数据、裸产物（T-M1-03）、UI 产物与图标。
//!
//! 所有进入打包脚本或产物路径的文本都在此处校验（构建脚本注入边界）：
//! 产品名 / 厂商 / 描述走 Unicode 字母数字 + 有限标点的白名单，版本号走
//! 语义化形态，标识符走反向域名形态。校验失败即 [`BundleError::InvalidInput`]。

use std::path::{Path, PathBuf};

use poly_targets::Host;

use crate::error::BundleError;

/// 产品元数据（来自 `polypack.toml`，由 `poly init` 模板填充）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductInfo {
    /// 产品名（用户可见，如 `Pax Notes`）。
    pub name: String,
    /// 语义化版本（`1.2.3` / `1.2.3-beta.1`）。
    pub version: String,
    /// 反向域名标识（如 `com.polypack.paxnotes`）。
    pub identifier: String,
    /// 厂商 / 维护者。
    pub manufacturer: String,
    /// 单行产品描述。
    pub description: String,
    /// 主页（可选）。
    pub homepage: Option<String>,
}

/// 允许出现在产品名 / 厂商名中的字符：Unicode 字母数字 + ` -_.+()`。
const NAME_ALLOWED: &[char] = &[' ', '-', '_', '.', '+', '(', ')'];

/// 允许出现在描述中的字符（比名称多常见标点，仍排除引号与脚本元字符）。
const DESC_ALLOWED: &[char] = &[
    ' ', '-', '_', '.', ',', ':', ';', '!', '?', '(', ')', '[', ']', '/', '\'', '+', '#', '@', '&',
];

impl ProductInfo {
    /// 校验并返回产物用名（kebab-case，供 deb 包名与文件名派生）。
    pub fn validate(&self, packager: &'static str) -> Result<(), BundleError> {
        text_field(packager, "product.name", &self.name, NAME_ALLOWED)?;
        text_field(
            packager,
            "product.manufacturer",
            &self.manufacturer,
            NAME_ALLOWED,
        )?;
        text_field(
            packager,
            "product.description",
            &self.description,
            DESC_ALLOWED,
        )?;
        if self.name.trim() != self.name
            || self.name.starts_with(['.', '-'])
            || self.name.ends_with(['.', '-'])
        {
            return Err(BundleError::InvalidInput {
                packager,
                field: "product.name",
                value: self.name.clone(),
                reason: "名称不得含首尾空白，也不得以 `.` / `-` 开头或结尾（产物路径安全）"
                    .to_string(),
            });
        }
        if !version_is_valid(&self.version) {
            return Err(BundleError::InvalidInput {
                packager,
                field: "product.version",
                value: self.version.clone(),
                reason: "版本号须为 `主.次.补丁`（可选 `-预发布` / `+构建`）".to_string(),
            });
        }
        if !identifier_is_valid(&self.identifier) {
            return Err(BundleError::InvalidInput {
                packager,
                field: "product.identifier",
                value: self.identifier.clone(),
                reason: "标识符须为小写反向域名（`[a-z0-9.-]`，至少一个点）".to_string(),
            });
        }
        Ok(())
    }

    /// deb 包名 / 文件名派生的 kebab-case 形态（`Pax Notes` → `pax-notes`）。
    pub fn kebab_name(&self) -> String {
        let mut out = String::with_capacity(self.name.len());
        let mut prev_dash = false;
        for ch in self.name.chars() {
            if ch.is_alphanumeric() {
                out.extend(ch.to_lowercase());
                prev_dash = false;
            } else if !prev_dash && !out.is_empty() {
                out.push('-');
                prev_dash = true;
            }
        }
        while out.ends_with('-') {
            out.pop();
        }
        out
    }

    /// 执行文件名的 snake_case 形态（`Pax Notes` → `pax-notes`）。
    ///
    /// 与 [`Self::kebab_name`] 同形：Tauri 侧可执行名沿用产品名 kebab 化。
    pub fn binary_name(&self) -> String {
        self.kebab_name()
    }
}

/// 图标类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    /// PNG（Linux hicolor / Web）。
    Png,
    /// ICO（Windows MSI / NSIS）。
    Ico,
    /// ICNS（macOS .app）。
    Icns,
}

/// 一枚图标（由 T-M1-14 派生管线从 `docs/assets/pax-mark.svg` 生成）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Icon {
    /// 图标文件路径。
    pub path: PathBuf,
    /// 图标类别。
    pub kind: IconKind,
    /// 位图边长（PNG 必填，用于 hicolor 目录）。
    pub size: Option<u32>,
}

/// 原生库（T-M2-02 Android AAR / T-M3 NAPI 桥产物的一枚 `.so`，按 ABI 目录名归档）。
///
/// Android 与 HarmonyOS 共用 ABI 目录命名（`arm64-v8a` / `armeabi-v7a` / `x86_64` / `x86`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeLib {
    /// ABI 目录名（`arm64-v8a` / `armeabi-v7a` / `x86_64` / `x86`）。
    pub abi: String,
    /// `.so` 文件路径。
    pub path: PathBuf,
}

/// Android 打包输入（T-M2-06）。
///
/// 可选字段在缺省时由产品元数据 / 图标 / UI 产物派生（最小清单、mipmap 图标、assets），
/// 使 `poly build --target android` 在只有 UI 产物的工程上也能直出产物；显式字段优先。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidInputs {
    /// `AndroidManifest.xml`（缺省 → 由 [`ProductInfo`] 生成最小清单）。
    pub manifest: Option<PathBuf>,
    /// `res/` 资源目录（可多个：宿主 + 桥产物；缺省 → 用 PNG 图标生成 mipmap）。
    pub res: Vec<PathBuf>,
    /// `assets/` 目录（RN bundle 等；缺省 → 用 `BundleInputs.ui_dist`）。
    pub assets: Option<PathBuf>,
    /// 原生库（T-M2-02 AAR / jniLibs）。
    pub native_libs: Vec<NativeLib>,
    /// Java 侧输入：`classes.jar` / `*.dex`（无 Java 侧时为空，产物无 dex 属正常）。
    pub dex_inputs: Vec<PathBuf>,
    /// ABI 裁剪（空 → 取 [`Self::native_libs`] 出现的 ABI，顺序固定）。
    pub abi_filters: Vec<String>,
    /// 语言裁剪（aapt2 `--config`；空 → 不裁，保留全部语言资源）。
    pub locales: Vec<String>,
    /// `minSdkVersion`。
    pub min_sdk: u32,
    /// `targetSdkVersion`（`0` → 取 SDK 内最新 `platforms/android-NN`）。
    pub target_sdk: u32,
    /// `versionCode`（缺省 → 由语义化版本派生 `主*10000+次*100+补`）。
    pub version_code: Option<u32>,
    /// Android SDK 根覆盖（缺省 → `$ANDROID_HOME` / `$ANDROID_SDK_ROOT` / `~/Android/Sdk`）。
    pub sdk_root: Option<PathBuf>,
    /// `bundletool-all.jar` 覆盖（缺省 → `$POLYPACK_BUNDLETOOL` / PATH / 本地工具缓存）。
    pub bundletool: Option<PathBuf>,
}

impl Default for AndroidInputs {
    fn default() -> Self {
        Self {
            manifest: None,
            res: Vec::new(),
            assets: None,
            native_libs: Vec::new(),
            dex_inputs: Vec::new(),
            abi_filters: Vec::new(),
            locales: Vec::new(),
            min_sdk: 24,
            target_sdk: 0,
            version_code: None,
            sdk_root: None,
            bundletool: None,
        }
    }
}

/// iOS 打包输入（T-M2-03 / T-M2-08）。
///
/// `core_crate` + `lib_name` 是 XCFramework 静态库切片的构建输入（cargo 在核心 crate 上
/// 按 Apple triple 构建 `lib<lib_name>.a`）；`host_project` 指向 RN 宿主工程根，其中的
/// `ios/<产品名>.xcodeproj` 存在即复用，否则由打包器生成工程骨架；
/// `bindings_dir` 是 T-M2-01 生成器的 Swift 三件套落点（`<工程根>/bridge/swift`），
/// 缺省回落到 `<out_dir>/bridge/swift`（无生成器的老路径 / 测试）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IosInputs {
    /// 核心 crate 根目录（cargo 构建静态库切片的 cwd）。
    pub core_crate: Option<PathBuf>,
    /// 核心库基础名（`lib<name>.a` 的 `<name>`，crate lib 名的下划线形态）。
    pub lib_name: Option<String>,
    /// 宿主工程根（含 RN 宿主模板产出的 `ios/` 目录；缺省 → 生成工程骨架）。
    pub host_project: Option<PathBuf>,
    /// Swift 绑定目录（T-M2-01 生成器落点 `<工程根>/bridge/swift`；缺省见结构体文档）。
    pub bindings_dir: Option<PathBuf>,
}

/// HarmonyOS 打包输入（T-M3-03）。
///
/// `host_project` 指向用户工程根：其 `harmony/` 目录（含 `build-profile.json5`）存在即复用
/// （T-M3-01 宿主模板实例 / 用户自置工程），否则打包器在 `<host_project>/harmony/` 生成
/// 最小工程骨架（探针实证可构建）。工程恒落在 out_dir **之外**：out_dir 每次构建被清空，
/// hvigor 增量缓存（`entry/build` 与 `.hvigor`）跨构建留存是「二跑缓存命中」的前提。
/// `native_libs` 是 `poly bridge ohos` 的 `.so`（`<工程根>/bridge/harmonyos/<abi>/`），
/// 打包器注入 `<harmony>/entry/libs/<abi>/`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HarmonyInputs {
    /// 宿主工程根（含 `harmony/` 模板实例；缺省 → 在其下生成工程骨架）。
    pub host_project: Option<PathBuf>,
    /// 原生库（按 ABI 目录名归档；注入 `entry/libs/<abi>/`）。
    pub native_libs: Vec<NativeLib>,
    /// `hvigorw` 覆盖（缺省 → `$HARMONYOS_TOOLS` / PATH）。
    pub hvigorw: Option<PathBuf>,
    /// `ohpm` 覆盖（复用工程装依赖用；骨架工程无依赖，不需要）。
    pub ohpm: Option<PathBuf>,
    /// `versionCode` 覆盖（缺省 → 由语义化版本派生，与 Android 同口径）。
    pub version_code: Option<u32>,
}

/// 裸产物（T-M1-03 输出）：Linux AppDir 根 / Windows 产物目录 / macOS `.app`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BareProduct {
    /// 产物根目录。
    pub root: PathBuf,
    /// 可执行文件（Linux `usr/bin/<bin>` / Windows `<bin>.exe` / macOS `.app/Contents/MacOS/<bin>`）。
    pub executable: PathBuf,
    /// 产出该裸产物的宿主。
    pub host: Host,
}

/// 一次打包的全部输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleInputs {
    /// 产品元数据。
    pub product: ProductInfo,
    /// 核心可执行（T-M1-02 编译产物），[`crate::tauri::bare_product`] 的输入。
    pub core_binary: Option<PathBuf>,
    /// 裸产物（桌面目标的必需输入）。
    pub bare: Option<BareProduct>,
    /// UI 产物目录（Web 目标的必需输入；Tauri 目标由裸产物内已注入资源）。
    pub ui_dist: Option<PathBuf>,
    /// 图标集合。
    pub icons: Vec<Icon>,
    /// PWA manifest（T-M1-04 生成，Web 目标）。
    pub web_manifest: Option<PathBuf>,
    /// Android 打包输入（T-M2-06；`None` → 打包器按产品元数据 + 图标 + `ui_dist` 派生）。
    pub android: Option<AndroidInputs>,
    /// iOS 打包输入（T-M2-03 / T-M2-08；`None` → iOS 打包器报缺输入）。
    pub ios: Option<IosInputs>,
    /// 小程序族宿主工程根（T-M3-06；Taro 工程候选链见 `packagers::miniapp`，缺省即缺输入）。
    pub miniapp_host: Option<PathBuf>,
    /// wasm 沙箱出口目录（T-M3-07 `poly bridge wasm` 的落点 `<工程根>/bridge/wasm-bindgen`；
    /// 在场即整目录随代码包分发，缺席不报错——无 wasm 出口的工程照常打包）。
    pub miniapp_bridge: Option<PathBuf>,
    /// HarmonyOS 打包输入（T-M3-03；`None` → 按产品元数据在其宿主根下落骨架工程）。
    pub harmony: Option<HarmonyInputs>,
    /// 输出目录。
    pub out_dir: PathBuf,
}

impl BundleInputs {
    /// 最小输入（裸产物 / UI / 图标留待调用方补齐）。
    pub fn new(product: ProductInfo, out_dir: impl Into<PathBuf>) -> Self {
        Self {
            product,
            core_binary: None,
            bare: None,
            ui_dist: None,
            icons: Vec::new(),
            web_manifest: None,
            android: None,
            ios: None,
            miniapp_host: None,
            miniapp_bridge: None,
            harmony: None,
            out_dir: out_dir.into(),
        }
    }

    /// 校验产品元数据并取裸产物（缺失即报错）。
    pub fn require_bare(&self, packager: &'static str) -> Result<&BareProduct, BundleError> {
        self.product.validate(packager)?;
        self.bare.as_ref().ok_or(BundleError::MissingInput {
            packager,
            field: "bare",
        })
    }

    /// 校验产品元数据并取 UI 产物目录（缺失即报错）。
    pub fn require_ui_dist(&self, packager: &'static str) -> Result<&Path, BundleError> {
        self.product.validate(packager)?;
        self.ui_dist.as_deref().ok_or(BundleError::MissingInput {
            packager,
            field: "ui_dist",
        })
    }

    /// 按类别取第一枚图标。
    pub fn icon(&self, kind: IconKind) -> Option<&Icon> {
        self.icons.iter().find(|icon| icon.kind == kind)
    }
}

/// 白名单文本校验：拒绝控制字符与白名单外的字符（防脚本 / 路径注入）。
fn text_field(
    packager: &'static str,
    field: &'static str,
    value: &str,
    allowed: &[char],
) -> Result<(), BundleError> {
    if value.trim().is_empty() {
        return Err(BundleError::InvalidInput {
            packager,
            field,
            value: value.to_string(),
            reason: "不能为空".to_string(),
        });
    }
    if let Some(bad) = value
        .chars()
        .find(|ch| ch.is_control() || !(ch.is_alphanumeric() || allowed.contains(ch)))
    {
        return Err(BundleError::InvalidInput {
            packager,
            field,
            value: value.to_string(),
            reason: format!(
                "含不允许的字符 {bad:?}（仅允许字母数字与 `{}`）",
                allowed.iter().collect::<String>()
            ),
        });
    }
    Ok(())
}

/// 版本号形态：`主.次.补丁`（可选 `-预发布` / `+构建`，段内 `[0-9A-Za-z.-]`）。
pub fn version_is_valid(version: &str) -> bool {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    if !parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

/// 标识符形态：小写反向域名（`[a-z0-9.-]`，至少两个段）。
pub fn identifier_is_valid(identifier: &str) -> bool {
    identifier.contains('.')
        && identifier.split('.').all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// 语义化版本的三段数字（WiX `Version` 字段只接受 `主.次.补丁[.构建]`）。
pub fn version_parts(version: &str) -> (u64, u64, u64) {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let mut it = core.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

/// WiX `Version` 字段形态：`主.次.补丁[.构建]`。
pub fn wix_version(version: &str) -> String {
    let (major, minor, patch) = version_parts(version);
    format!("{major}.{minor}.{patch}")
}

/// NSIS `VIProductVersion` 形态：四段数字 `主.次.补丁.构建`。
pub fn nsis_version(version: &str) -> String {
    let (major, minor, patch) = version_parts(version);
    let build = version
        .split_once('+')
        .map(|(_, b)| b.chars().filter(char::is_ascii_digit).collect::<String>())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "0".to_string());
    format!("{major}.{minor}.{patch}.{build}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product() -> ProductInfo {
        ProductInfo {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "Pax 示例笔记应用".to_string(),
            homepage: None,
        }
    }

    #[test]
    fn valid_product_passes() {
        assert!(product().validate("test").is_ok());
    }

    #[test]
    fn injection_in_name_is_rejected() {
        for bad in [
            "Pax\"; rm -rf /",
            "Pax`boom`",
            "Pax$(id)",
            "Pax\nName",
            "Pax%TEMP%",
            "Pax\\x",
        ] {
            let mut p = product();
            p.name = bad.to_string();
            assert!(
                matches!(p.validate("test"), Err(BundleError::InvalidInput { .. })),
                "应拒绝 {bad:?}"
            );
        }
    }

    #[test]
    fn version_and_identifier_forms() {
        assert!(version_is_valid("1.2.3"));
        assert!(version_is_valid("1.2.3-beta.1"));
        assert!(!version_is_valid("0.0.0+x86_64")); // `_` 不在允许集内
        assert!(!version_is_valid("1.2"));
        assert!(!version_is_valid("v1.2.3"));
        assert!(!version_is_valid("1.2.3; rm"));

        assert!(identifier_is_valid("com.polypack.paxnotes"));
        assert!(!identifier_is_valid("PaxNotes"));
        assert!(!identifier_is_valid("com/pax"));
    }

    #[test]
    fn name_derivations() {
        assert_eq!(product().kebab_name(), "pax-notes");
        assert_eq!(product().binary_name(), "pax-notes");
        let mut p = product();
        p.name = "Pax  Notes++".to_string();
        assert_eq!(p.kebab_name(), "pax-notes");
    }

    #[test]
    fn tool_version_forms() {
        assert_eq!(wix_version("1.2.3"), "1.2.3");
        assert_eq!(wix_version("1.2.3-beta.1"), "1.2.3");
        assert_eq!(nsis_version("1.2.3"), "1.2.3.0");
        assert_eq!(nsis_version("1.2.3+45"), "1.2.3.45");
    }
}
