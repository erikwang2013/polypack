//! Linux 家族打包器。
//!
//! - T-M1-07 首版：[`Deb`]（dpkg-deb）与 [`AppImage`]（appimagetool / AppDir）。
//! - T-M5-01 成熟化：deb 的 `Depends` / 维护者脚本 / AppStream 元数据，[`Rpm`]（rpmbuild spec）。
//! - T-M5-04：[`AppImage`] 的 AppStream / desktop / 图标 / `SOURCE_DATE_EPOCH` 可复现。
//! - T-M5-02 / T-M5-03：[`Flatpak`] 与 [`Snap`]（manifest / snapcraft.yaml + 结构自检）。
//!
//! 全部打包器消费 T-M1-03 的裸产物（AppDir 形状：`usr/bin` + `usr/lib/<产品名>` +
//! `usr/share/{applications,icons,doc,metainfo}`）。发行版目标（ubuntu / fedora / mint）
//! 与 L1 `linux` 共用同一实现，按 `Packager::serves` 分派（T-M5-01）。
//!
//! 可复现：`SOURCE_DATE_EPOCH` 存在时注入外部工具并把落盘树的 mtime 归一到该时刻
//! （AppImage / deb / rpm / flatpak / snap 同口径）；未设置则不伪造确定性。

pub mod appimage;
pub mod deb;
pub mod flatpak;
pub mod rpm;
mod rpm_header;
pub mod snap;

pub use appimage::{AppImage, AppdirStructure, check_appdir_structure};
pub use deb::{Deb, DebStructure, check_deb_structure};
pub use flatpak::{FLATPAK_FINISH_ARGS, Flatpak, FlatpakStructure, check_flatpak_structure};
pub use rpm::{Rpm, RpmStructure, check_rpm_structure, rpm_arch};
pub use snap::{SNAP_PLUGS, Snap, SnapStructure, check_snap_structure};

use std::path::{Path, PathBuf};

use polypack_targets::TargetSpec;

use crate::error::{BundleError, io_err};
use crate::fsutil;
use crate::inputs::{BareProduct, BundleInputs, IconKind, ProductInfo};
use crate::packager::Command;

/// 产物格式常量（`check_format` 与 `Packager::format` 共用的唯一字面量）。
pub const DEB: &str = "deb";
/// AppImage 单文件格式。
pub const APPIMAGE: &str = "appimage";
/// rpm 格式。
pub const RPM: &str = "rpm";
/// Flatpak 单文件格式。
pub const FLATPAK: &str = "flatpak";
/// Snap 单文件格式。
pub const SNAP: &str = "snap";

/// 服务 `linux` L1 目标的发行版目标（T-M5-01：Ubuntu / Mint 走 deb，Fedora 走 rpm；
/// T-M5-10：ChromeOS Crostini 复用 deb 链，不新写打包器）。
pub(crate) const DEB_TARGETS: &[&str] = &["linux", "ubuntu", "mint", "chromeos"];
/// 服务 rpm 的目标。
pub(crate) const RPM_TARGETS: &[&str] = &["linux", "fedora"];
/// 服务 Flatpak 的 Linux 家族目标（ROADMAP L5：Ubuntu / Fedora / Mint 均可出 Flatpak）。
pub(crate) const FLATPAK_TARGETS: &[&str] = &["linux", "ubuntu", "fedora", "mint"];
/// 服务 Snap 的目标（ROADMAP：Ubuntu 的 Snapcraft 渠道）。
pub(crate) const SNAP_TARGETS: &[&str] = &["linux", "ubuntu"];

/// deb `Architecture` 字段（本机打包；交叉打包由 T-M2+ 的 triple 决定）。
///
/// 公开给契约测试按**同一口径**推导期望值（x86_64 → amd64、aarch64 → arm64 …），
/// 避免测试写死宿主 arch 而在 Apple Silicon 上误报。
pub fn deb_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "i386",
        "arm" => "armhf",
        "riscv64" => "riscv64",
        other => other,
    }
}

/// 打包格式与打包器声明必须一致（防「按 appimage 目标产出 deb」）。
pub(crate) fn check_format(spec: &TargetSpec, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: "linux",
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}

/// deb 需要 256 PNG 图标（金样条目 `usr/share/icons/hicolor/256x256/apps/*`）：缺失响亮拒绝。
///
/// 图标由 T-M1-14 派生管线（[`crate::icons::derive`]）从品牌 SVG 产出后填入
/// `BundleInputs.icons`；其它尺寸可有可无，256 是发行包桌面条目的最低要求。
pub(crate) fn require_png256(inputs: &BundleInputs) -> Result<(), BundleError> {
    if inputs.icon(IconKind::Png).is_none() {
        return Err(BundleError::MissingInput {
            packager: DEB,
            field: "icons(PNG)",
        });
    }
    if !inputs
        .icons
        .iter()
        .any(|i| i.kind == IconKind::Png && i.size == Some(256))
    {
        return Err(BundleError::MissingInput {
            packager: DEB,
            field: "icons(PNG 256)",
        });
    }
    Ok(())
}

/// 中间暂存目录。
pub(crate) fn stage_dir(inputs: &BundleInputs, tag: &str) -> PathBuf {
    inputs
        .out_dir
        .join(format!("{}-{tag}.stage", inputs.product.kebab_name()))
}

/// AppDir 目录。
pub(crate) fn appdir_path(inputs: &BundleInputs) -> PathBuf {
    inputs
        .out_dir
        .join(format!("{}.AppDir", inputs.product.kebab_name()))
}

/// AppImage 文件路径。
pub(crate) fn appimage_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join(format!(
        "{}-{}-{}.AppImage",
        inputs.product.name.replace(' ', "-"),
        inputs.product.version,
        deb_arch()
    ))
}

/// 参数形态（路径一律显式传参，避免拼接成 shell 串）。
pub(crate) fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

/// AppStream / Flatpak 的应用 ID：反向域名，至少三段（AppStream 与 Flatpak 的共同硬约束）。
///
/// `ProductInfo.identifier` 只要求「至少一个点」；不足三段时补 `app.` 前缀使其合法，
/// 超过三段原样使用。每段必须以字母开头（Flatpak 校验规则），否则响亮拒绝。
pub fn app_id(product: &ProductInfo) -> Result<String, BundleError> {
    let mut id = product.identifier.clone();
    if id.split('.').count() < 3 {
        id = format!("app.{id}");
    }
    let valid = id.split('.').all(|part| {
        !part.is_empty()
            && part.starts_with(|ch: char| ch.is_ascii_alphabetic())
            && part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    });
    if !valid {
        return Err(BundleError::InvalidInput {
            packager: "linux",
            field: "product.identifier",
            value: product.identifier.clone(),
            reason: "AppStream / Flatpak 应用 ID 须为反向域名（每段字母开头，仅字母数字/-/_）"
                .to_string(),
        });
    }
    Ok(id)
}

/// AppStream 元数据（`<id>.metainfo.xml`）：deb / rpm / AppImage / Flatpak / Snap 同一份。
///
/// AppStream 校验器的硬约束：`id` 与文件名一致、`metadata_license` 为 SPDX、
/// `release` 带日期、`launchable` 指向 desktop 文件。`homepage` 缺省时省略
/// `<url>`（`appstreamcli` 会报 warning，不是 error——不伪造 URL）。
pub fn appstream_metainfo(product: &ProductInfo) -> Result<String, BundleError> {
    let id = app_id(product)?;
    let kebab = product.kebab_name();
    let summary = product.description.trim_end_matches(['.', '。']).trim();
    let mut url = String::new();
    if let Some(homepage) = &product.homepage {
        url.push_str(&format!(
            "  <url type=\"homepage\">{}</url>\n",
            xml_escape(homepage)
        ));
    }
    // 发行日期取 SOURCE_DATE_EPOCH（可复现）或固定 1970-01-01：AppStream 要求 release 带日期。
    let date = source_date_epoch()
        .map(date_of)
        .unwrap_or_else(|| "1970-01-01".to_string());
    let developer = developer_id(product);
    let developer = match developer {
        Some(vendor) => format!(
            "  <developer id=\"{}\">\n    <name>{}</name>\n  </developer>\n",
            xml_escape(&vendor),
            xml_escape(&product.manufacturer)
        ),
        None => String::new(),
    };
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <component type=\"desktop-application\">\n\
         \x20 <id>{id}</id>\n\
         \x20 <metadata_license>CC0-1.0</metadata_license>\n\
         \x20 <project_license>Apache-2.0</project_license>\n\
         \x20 <name>{name}</name>\n\
         \x20 <summary>{summary}</summary>\n\
         \x20 <description>\n    <p>{description}</p>\n  </description>\n\
         \x20 <launchable type=\"desktop-id\">{kebab}.desktop</launchable>\n\
         {url}{developer}\
         \x20 <categories>\n    <category>Utility</category>\n  </categories>\n\
         \x20 <content_rating type=\"oars-1.1\"/>\n\
         \x20 <releases>\n    <release version=\"{version}\" date=\"{date}\"/>\n  </releases>\n\
         </component>\n",
        name = xml_escape(&product.name),
        summary = xml_escape(summary),
        description = xml_escape(&product.description),
        version = xml_escape(&product.version),
    ))
}

/// 写 AppStream 元数据到 `share/metainfo/<id>.metainfo.xml`（`share` 是 `usr/share` 或载荷
/// 的 `share/`）。返回落盘路径。
pub(crate) fn install_appstream(
    share: &Path,
    product: &ProductInfo,
) -> Result<PathBuf, BundleError> {
    let id = app_id(product)?;
    let path = share.join("metainfo").join(format!("{id}.metainfo.xml"));
    fsutil::write_text(&path, &appstream_metainfo(product)?)?;
    Ok(path)
}

/// `<developer id>`：取 identifier 去掉末段（至少两段才合法；不足则省略）。
fn developer_id(product: &ProductInfo) -> Option<String> {
    let mut parts: Vec<&str> = product.identifier.split('.').collect();
    if parts.len() < 3 {
        return None;
    }
    parts.pop();
    Some(parts.join("."))
}

/// AppStream 元数据的结构检查结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppstreamMeta {
    /// 组件 id（等于文件名去 `.metainfo.xml`）。
    pub id: String,
    /// 组件名。
    pub name: String,
    /// 摘要。
    pub summary: String,
}

/// 校验 `<id>.metainfo.xml` 的硬约束（与 `appstreamcli validate` 的 error 面同源）。
///
/// 官方校验器在场时 verify 腿再叠加真跑判词；本检查纯文本解析，跨宿主成立。
pub fn check_appstream_metainfo(path: &Path) -> Result<AppstreamMeta, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: path.to_path_buf(),
        reason,
    };
    let text = std::fs::read_to_string(path).map_err(io_err(path))?;
    let field = |tag: &str| -> Option<String> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start = text.find(&open)? + open.len();
        let end = text[start..].find(&close)? + start;
        Some(text[start..end].trim().to_string())
    };
    if !text.contains("<component type=\"desktop-application\">") {
        return Err(bad(
            "缺 `<component type=\"desktop-application\">`".to_string()
        ));
    }
    let id = field("id")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| bad("缺 <id>".to_string()))?;
    for required in ["metadata_license", "project_license", "name", "summary"] {
        if field(required).is_none_or(|value| value.is_empty()) {
            return Err(bad(format!("缺必填元素 <{required}>")));
        }
    }
    let summary = field("summary").unwrap_or_default();
    if summary.ends_with('.') {
        return Err(bad("summary 不得以句点结尾（AppStream 规则）".to_string()));
    }
    if !text.contains("<description>") || !text.contains("<p>") {
        return Err(bad("description 须含 <p> 段落".to_string()));
    }
    if !text.contains("<launchable type=\"desktop-id\">") {
        return Err(bad("缺 <launchable type=\"desktop-id\">".to_string()));
    }
    let file_id = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".metainfo.xml"));
    if file_id != Some(id.as_str()) {
        return Err(bad(format!(
            "文件名须为 <id>.metainfo.xml（id = {id}，实际 {}）",
            path.display()
        )));
    }
    Ok(AppstreamMeta {
        id,
        name: field("name").unwrap_or_default(),
        summary,
    })
}

/// XML 文本转义（产品字段过了白名单，但 `&` / `<` 仍在描述字符集里）。
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `SOURCE_DATE_EPOCH`（秒）；未设置 / 非法值为 `None`（不伪造确定性）。
pub(crate) fn source_date_epoch() -> Option<i64> {
    std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|text| text.trim().parse::<i64>().ok())
        .filter(|epoch| *epoch >= 0)
}

/// 给命令注入 `SOURCE_DATE_EPOCH`（存在时；工具自会按其规则钳制时间戳）。
pub(crate) fn with_epoch(mut command: Command, epoch: Option<i64>) -> Command {
    if let Some(epoch) = epoch {
        command = command.with_env("SOURCE_DATE_EPOCH", epoch.to_string());
    }
    command
}

/// 把目录树的 mtime / atime 归一到 `epoch`（可复现的外部工具输入）。
///
/// 符号链接不动（AppRun / .DirIcon 的链接本身不带可影响产物的时间）；非 Unix 宿主为空操作
/// （AppImage / deb / rpm 本身也只拿 Unix 工具产出）。
pub(crate) fn normalize_mtime(path: &Path, epoch: i64) -> Result<(), BundleError> {
    #[cfg(unix)]
    {
        use std::time::{Duration, UNIX_EPOCH};

        let time = UNIX_EPOCH + Duration::from_secs(epoch as u64);
        let times = std::fs::FileTimes::new()
            .set_accessed(time)
            .set_modified(time);
        let meta = std::fs::symlink_metadata(path).map_err(io_err(path))?;
        if meta.file_type().is_symlink() {
            return Ok(());
        }
        if meta.is_dir() {
            for entry in std::fs::read_dir(path).map_err(io_err(path))? {
                normalize_mtime(&entry.map_err(io_err(path))?.path(), epoch)?;
            }
        }
        let file = std::fs::File::open(path).map_err(io_err(path))?;
        file.set_times(times).map_err(io_err(path))?;
    }
    #[cfg(not(unix))]
    let _ = (path, epoch);
    Ok(())
}

/// 归一目录 / 文件权限（目录 0755；文件保留可执行位，其余 0644）。
///
/// 构建树常带 umask 002 的 775/664；deb / rpm 会原样收录，发行包权限惯例要求去掉
/// 组 / 其他可写位（lintian / rpmlint 的权限判词）。符号链接不动。
pub(crate) fn normalize_modes(path: &Path) -> Result<(), BundleError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(path).map_err(io_err(path))?;
        if meta.file_type().is_symlink() {
            return Ok(());
        }
        if meta.is_dir() {
            for entry in std::fs::read_dir(path).map_err(io_err(path))? {
                normalize_modes(&entry.map_err(io_err(path))?.path())?;
            }
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
                .map_err(io_err(path))?;
        } else {
            let executable = meta.permissions().mode() & 0o111 != 0;
            let mode = if executable { 0o755 } else { 0o644 };
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                .map_err(io_err(path))?;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// 归一 mtime（`SOURCE_DATE_EPOCH` 存在时）；返回是否真的归了。
pub(crate) fn apply_epoch(path: &Path) -> Result<bool, BundleError> {
    match source_date_epoch() {
        Some(epoch) => {
            normalize_mtime(path, epoch)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Tauri 载荷的 Flatpak / Snap 布局：`usr/bin` → `bin`、`usr/lib` → `lib`、`usr/share` → `share`。
///
/// 应用在沙箱里装到 `/app` / prime 根：资源目录按 `<exe>/../lib/<产品名>`（Tauri 规则）解析，
/// 故可执行必须落 `bin/`、资源必须落 `lib/`——直接照搬 `usr/` 会让资源目录解析失败。
pub(crate) fn portable_payload(bare: &BareProduct, payload: &Path) -> Result<(), BundleError> {
    let _ = std::fs::remove_dir_all(payload);
    std::fs::create_dir_all(payload).map_err(io_err(payload))?;
    for (from, to) in [
        ("usr/bin", "bin"),
        ("usr/lib", "lib"),
        ("usr/share", "share"),
    ] {
        let src = bare.root.join(from);
        if src.is_dir() {
            fsutil::copy_tree(&src, &payload.join(to))?;
        }
    }
    Ok(())
}

/// 工具缓存路径（`~/.cache/polypack/tools/<rel>`，与 kotlinc / bundletool 的约定同源）。
pub(crate) fn tool_cache(rel: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    let path = base.join("polypack").join("tools").join(rel);
    path.exists().then_some(path)
}

/// `YYYY-MM-DD`（UTC；Howard Hinnant 的 civil-from-days，不引日期库）。
fn date_of(epoch: i64) -> String {
    let days = epoch.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
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
            description: "Pax 示例笔记应用。".to_string(),
            homepage: None,
        }
    }

    #[test]
    fn deb_arch_is_a_debian_name() {
        assert!(
            ["amd64", "arm64", "i386", "armhf", "riscv64"].contains(&deb_arch()),
            "未映射的架构 {}",
            deb_arch()
        );
    }

    /// 元数据必须过 AppStream 的硬约束：id = 文件名、summary 无句末点、release 带日期。
    #[test]
    fn metainfo_has_appstream_required_shape() {
        let meta = appstream_metainfo(&product()).unwrap();
        assert!(meta.contains("<id>com.polypack.paxnotes</id>"));
        assert!(meta.contains("<metadata_license>CC0-1.0</metadata_license>"));
        assert!(meta.contains("<launchable type=\"desktop-id\">pax-notes.desktop</launchable>"));
        assert!(meta.contains("<summary>Pax 示例笔记应用</summary>"));
        assert!(meta.contains("date=\"1970-01-01\"") || meta.contains("date=\"20"));
        assert!(meta.contains("<developer id=\"com.polypack\">"));
    }

    #[test]
    fn app_id_fills_short_identifiers_and_rejects_bad_parts() {
        let mut short = product();
        short.identifier = "com.paxnotes".to_string();
        assert_eq!(app_id(&short).unwrap(), "app.com.paxnotes");
        short.identifier = "com.7zip.pax".to_string();
        assert_eq!(
            app_id(&short).unwrap_err().code(),
            crate::error::codes::E_BND_0003
        );
    }

    #[test]
    fn date_of_epoch_zero_is_unix_epoch() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(1_700_000_000), "2023-11-14");
    }
}
