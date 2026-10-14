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
