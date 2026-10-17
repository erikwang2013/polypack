//! AppImage 打包器（T-M1-07 首版；T-M5-04 成熟化）。
//!
//! 产物 = `<out_dir>/<Name>-<version>-<arch>.AppImage`（appimagetool）。AppDir 根级
//! desktop / 图标 / `AppRun` 之外，T-M5-04 补齐 `usr/share/metainfo` 的 AppStream 元数据，
//! 并在 `SOURCE_DATE_EPOCH` 存在时归一树内 mtime 后传给 appimagetool（可复现构建）。
//! appimagetool 缺席时产出 AppDir 目录并如实标注 `appdir`（不是假 AppImage）。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use super::{
    APPIMAGE, appdir_path, appimage_path, check_appstream_metainfo, check_format,
    install_appstream, path_arg, source_date_epoch, with_epoch,
};
use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, IconKind};
use crate::packager::{Artifact, BundlePlan, Command, Packager};
use crate::tauri;

/// AppImage 打包器（appimagetool；工具缺失时产出 AppDir 并如实标注未打包）。
pub struct AppImage;

/// appimagetool 的安装建议（`exec::install_hint` 同文案）。
const APPIMAGETOOL_HINT: &str = "从 github.com/AppImage/appimagetool 下载官方 release 二进制，\
     或把 AppDir 直接托管 / 用 deb / rpm / Flatpak / Snap 打包器";

impl Packager for AppImage {
    fn id(&self) -> &'static str {
        "linux"
    }

    fn format(&self) -> &'static str {
        APPIMAGE
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, APPIMAGE)?;
        inputs.require_bare(APPIMAGE)?;
        let appdir = appdir_path(inputs);
        let out = appimage_path(inputs);
        let mut plan = BundlePlan::new().with_command(with_epoch(
            // 不传 `--no-appstream`：AppDir 已带 `usr/share/metainfo`，让 appimagetool
            // 在 appstreamcli 在场时真校验（T-M5-04）。
            Command::new("appimagetool", [path_arg(&appdir), path_arg(&out)]),
            source_date_epoch(),
        ));
        if !exec::tool_present("appimagetool") {
            plan = plan.skipping("appimagetool", APPIMAGETOOL_HINT);
        }
        Ok(plan)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let bare = inputs.require_bare(APPIMAGE)?;
        // AppDir 根级图标取最大 PNG（appimagetool 用它作 `.DirIcon` 的源）。
        let icon = inputs
            .icons
            .iter()
            .filter(|icon| icon.kind == IconKind::Png)
            .max_by_key(|icon| icon.size)
            .ok_or(BundleError::MissingInput {
                packager: APPIMAGE,
                field: "icons(PNG)",
            })?;
        let appdir = appdir_path(inputs);
        let _ = std::fs::remove_dir_all(&appdir);
        fsutil::copy_tree(&bare.root, &appdir)?;

        // AppDir 根级三件套：desktop / 图标 / AppRun（AppImage 约定）。
        let name = inputs.product.kebab_name();
        let bin = inputs.product.binary_name();
        fsutil::write_text(
            &appdir.join(format!("{name}.desktop")),
            &tauri::desktop_file(&inputs.product, &bin),
        )?;
        fsutil::copy_file(&icon.path, &appdir.join(format!("{name}.png")), false)?;
        fsutil::symlink(Path::new(&format!("{name}.png")), &appdir.join(".DirIcon"))?;
        // AppRun 指向可执行：AppImage runtime 挂载时会设置 APPDIR，直接运行 AppDir 时
        // Tauri 走 `../lib/<产品名>` 分支，两者都落到同一资源目录。
        fsutil::symlink(Path::new(&format!("usr/bin/{bin}")), &appdir.join("AppRun"))?;
        // T-M5-04：AppStream 元数据随 AppDir 分发（appimagetool 可校验、商店可抓取）。
        install_appstream(&appdir.join("usr/share"), &inputs.product)?;
        super::normalize_modes(&appdir)?;
        super::apply_epoch(&appdir)?;

        if plan.skipped.is_empty() {
            exec::run_all(&plan.commands)?;
            let out = appimage_path(inputs);
            let structure = check_appdir_structure(&appdir)?;
            return fsutil::file_artifact(
                &spec.id,
                APPIMAGE,
                &out,
                vec![
                    format!(
                        "结构检查：{} 条目，desktop={}，metainfo={}，图标 {} 枚",
                        structure.entries, structure.desktop, structure.metainfo, structure.icons
                    ),
                    "启动冒烟：chmod +x <AppImage> && ./<AppImage> --version".to_string(),
                ],
            );
        }
        let notes = plan
            .skipped
            .iter()
            .map(|s| format!("未打包：{}（{}）", s.tool, s.reason))
            .collect();
        fsutil::dir_artifact(&spec.id, "appdir", &appdir, notes)
    }
}

/// AppDir 结构检查结果（打包器自检与 verify 腿 `appimage-structure` 共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppdirStructure {
    /// 根级 desktop 文件名。
    pub desktop: String,
    /// AppStream 元数据相对路径。
    pub metainfo: String,
    /// hicolor 图标枚数。
    pub icons: usize,
    /// 树内条目总数。
    pub entries: usize,
}

/// AppDir 结构检查：`AppRun` / 根级 desktop+图标 / hicolor 图标 / AppStream 元数据。
///
/// T-M5-04 的本地真跑面：appimagetool 缺席时 AppDir 仍是可运行产物，结构缺陷必须响亮。
pub fn check_appdir_structure(appdir: &Path) -> Result<AppdirStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: appdir.to_path_buf(),
        reason,
    };
    if std::fs::symlink_metadata(appdir.join("AppRun")).is_err() {
        return Err(bad("缺 AppRun（AppImage 入口）".to_string()));
    }
    let desktop =
        single_file(appdir, "desktop").ok_or_else(|| bad("根级缺唯一的 *.desktop".to_string()))?;
    let text = std::fs::read_to_string(&desktop)
        .map_err(|err| bad(format!("desktop 读取失败（{}）：{err}", desktop.display())))?;
    for key in ["[Desktop Entry]", "Type=Application", "Exec=", "Icon="] {
        if !text.contains(key) {
            return Err(bad(format!("desktop 缺 {key}")));
        }
    }
    let icon_name = text
        .lines()
        .find_map(|line| line.strip_prefix("Icon="))
        .unwrap_or_default()
        .trim()
        .to_string();
    if !appdir.join(format!("{icon_name}.png")).is_file() {
        return Err(bad(format!("根级缺 desktop 声明的图标 {icon_name}.png")));
    }
    let metainfo = list_files(&appdir.join("usr/share/metainfo"), "metainfo.xml")
        .into_iter()
        .next()
        .ok_or_else(|| bad("缺 usr/share/metainfo/*.metainfo.xml".to_string()))?;
    check_appstream_metainfo(&metainfo)?;
    let icons = count_icons(&appdir.join("usr/share/icons/hicolor"));
    if icons == 0 {
        return Err(bad("缺 usr/share/icons/hicolor/*/apps/*.png".to_string()));
    }
    Ok(AppdirStructure {
        desktop: file_name(&desktop),
        metainfo: format!("usr/share/metainfo/{}", file_name(&metainfo)),
        icons,
        entries: count_entries(appdir),
    })
}

/// 目录下后缀匹配的唯一文件（多份即取排序首件，调用方自行判重）。
fn single_file(dir: &Path, suffix: &str) -> Option<PathBuf> {
    list_files(dir, suffix).into_iter().next()
}

/// 目录下后缀匹配的文件（排序，确定性）。
fn list_files(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(suffix))
        })
        .collect();
    files.sort();
    files
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `hicolor/<size>x<size>/apps/*.png` 枚数。
fn count_icons(hicolor: &Path) -> usize {
    std::fs::read_dir(hicolor)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("apps"))
        .map(|apps| list_files(&apps, ".png").len())
        .sum()
}

/// 树内条目总数（文件 + 目录 + 符号链接）。
fn count_entries(root: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            1 + if is_dir { count_entries(&path) } else { 0 }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_entries_walks_tree() {
        let base = std::env::temp_dir().join(format!("poly-appdir-count-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        fsutil::write_text(&base.join("a"), "1").unwrap();
        fsutil::write_text(&base.join("nested/b"), "2").unwrap();
        assert_eq!(count_entries(&base), 3);
        let _ = std::fs::remove_dir_all(&base);
    }
}
