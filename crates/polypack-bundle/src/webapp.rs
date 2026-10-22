//! web 系 TV 目标的共享载荷与摘要（T-M4-08 / T-M4-09 / T-M4-10）：一份 UI 产物多目标复用。
//!
//! `tizen-tv` / `webos-tv` / `vidaa-tv` / `smartcast-tv` 四个目标的随包载荷同构：UI dist 整树 +
//! 随包 wasm 出口（`bridge/wasm-bindgen/`，与小程序族同一目录约定）+ 图标（`icon.png`）。
//! 载荷落盘、「同源产物摘要」与归档条目收集都收在这一个模块里——摘要由 [`tree_digest`] 从
//! **源 dist** 计算，四个打包器共用同一函数，「同源同摘要」是构造性事实而非巧合
//! （T-M4-09 验收：`.ipk` 与 `.wgt` 的 UI 载荷摘要一致）。

use std::path::{Path, PathBuf};

use crate::error::{BundleError, io_err};
use crate::fsutil;
use crate::inputs::{BundleInputs, IconKind};
use crate::packagers::harmony::PLACEHOLDER_PNG;
use crate::sri;

/// 应用入口文件名（四个目标的清单都把 `index.html` 指为入口）。
pub const ENTRY: &str = "index.html";

/// wasm 沙箱出口（T-M3-07）在随包根下的相对路径（与工程根侧同名，引用口径统一）。
pub const BRIDGE_REL: &str = "bridge/wasm-bindgen";

/// 包内图标名（Tizen `icon@src` / webOS appinfo `icon` / VIDAA 与 Vizio 的图标位共用一个）。
pub const ICON: &str = "icon.png";

/// 一次 web 系 TV 打包的共享载荷（落盘后的说明面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    /// 源 UI 载荷目录（dist）。
    pub dist: PathBuf,
    /// 源 UI 载荷的 SRI 摘要（`sha384-<base64>`；同源同摘要）。
    pub digest: String,
    /// 源 UI 载荷字节（dist 树文件字节总和）。
    pub ui_bytes: u64,
    /// 随包 wasm 出口说明（目录缺席 = `None`，不占 notes 行）。
    pub bridge_note: Option<String>,
    /// 图标来源说明。
    pub icon_note: String,
}

impl Payload {
    /// 公共 notes 行：同源摘要 + 图标 + wasm 出口（各打包器再补自己的清单面与平台边界）。
    pub fn notes(&self) -> Vec<String> {
        let mut notes = vec![
            format!(
                "UI 载荷摘要：{}（源 {}，{} 字节；tizen-tv / webos-tv / vidaa-tv / smartcast-tv 同源同摘要）",
                self.digest,
                self.dist.display(),
                self.ui_bytes
            ),
            self.icon_note.clone(),
        ];
        if let Some(note) = &self.bridge_note {
            notes.push(note.clone());
        }
        notes
    }
}

/// 组装共享载荷：`dest` ← dist 整树 + 随包 wasm 出口 + 图标；入口缺席即拒（沙箱自检第一条
/// 规则在打包期就守，不留给校验器）。
pub fn stage_payload(
    inputs: &BundleInputs,
    dest: &Path,
    packager: &'static str,
) -> Result<Payload, BundleError> {
    let dist = inputs.require_ui_dist(packager)?;
    let digest = tree_digest(dist)?;
    let ui_bytes = fsutil::tree_bytes(dist)?;
    std::fs::create_dir_all(dest).map_err(io_err(dest))?;
    fsutil::copy_tree(dist, dest)?;
    if !dest.join(ENTRY).is_file() {
        return Err(BundleError::InvalidInput {
            packager,
            field: "ui_dist(index.html)",
            value: dist.display().to_string(),
            reason: format!("入口 {ENTRY} 不在 dist 根——沙箱自检的入口规则在打包期先拦"),
        });
    }
    let bridge_note = stage_bridge(inputs, dest, packager)?;
    let icon_note = stage_icon(inputs, dest)?;
    Ok(Payload {
        dist: dist.to_path_buf(),
        digest,
        ui_bytes,
        bridge_note,
        icon_note,
    })
}

/// wasm 沙箱出口（T-M3-07 的四个随包文件）随载荷分发：`miniapp_bridge` 目录整拷到
/// `dest_root/<BRIDGE_REL>`。
///
/// 目录缺席 = 工程没跑过 `polypack bridge wasm`（或本无 wasm 接口）——不属错误，notes 也不占行；
/// 在场则两个不变式文件必须齐备，缺一即拒（半成品出口目录不许静默进包）。打包期不解析
/// wasm / 胶水内容：白名单与导入面核对是 loader 的加载期闸（T-M3-07），不在这里重复。
pub fn stage_bridge(
    inputs: &BundleInputs,
    dest_root: &Path,
    packager: &'static str,
) -> Result<Option<String>, BundleError> {
    let Some(src) = inputs.miniapp_bridge.as_deref() else {
        return Ok(None);
    };
    if !src.is_dir() {
        return Ok(None);
    }
    for invariant in ["manifest.json", "polypack-loader.js"] {
        if !src.join(invariant).is_file() {
            return Err(BundleError::InvalidInput {
                packager,
                field: "miniapp_bridge",
                value: src.join(invariant).display().to_string(),
                reason: format!(
                    "wasm 出口目录在场但缺 {invariant}——半成品不许随包，重跑 polypack bridge wasm"
                ),
            });
        }
    }
    let dest = dest_root.join(BRIDGE_REL);
    fsutil::copy_tree(src, &dest)?;
    // 只是拷，不重编：随包字节 = 出口目录当前字节（T-M3-08 的裁剪时机在出口层，
    // 「出口目录生成后、随包拷贝前」——打包器不做二次处理）
    let bytes = fsutil::tree_bytes(&dest)?;
    let wasm = match wasm_leaf_bytes(&dest)? {
        Some(wasm) => format!("，其中 wasm {wasm} 字节"),
        None => String::new(),
    };
    Ok(Some(format!(
        "wasm 沙箱出口随包：{BRIDGE_REL}/（{bytes} 字节{wasm}；随包字节即出口层 wasm-opt 裁剪后字节；\
         加载契约 polypack-loader.js 的 load(bytes)）"
    )))
}

/// 出口目录内 `.wasm` 文件的字节数（T-M3-08 体积报告；T-M3-07 四文件形态下恰一个）。
fn wasm_leaf_bytes(dir: &Path) -> Result<Option<u64>, BundleError> {
    let mut total: Option<u64> = None;
    for entry in std::fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "wasm") {
            let len = entry.metadata().map_err(io_err(&path))?.len();
            total = Some(total.unwrap_or(0) + len);
        }
    }
    Ok(total)
}

/// 包内图标：显式 PNG 图标输入优先（原样拷入），否则内联 1×1 占位 PNG。
///
/// 商店素材（Tizen 117×117 等规格）不在本环节；占位足数结构自检（入口 / 清单 / 体积），
/// 来源如实写进 notes，绝不假装是品牌图标。
fn stage_icon(inputs: &BundleInputs, dest: &Path) -> Result<String, BundleError> {
    let target = dest.join(ICON);
    match inputs.icon(IconKind::Png) {
        Some(icon) => {
            fsutil::copy_file(&icon.path, &target, false)?;
            Ok(format!(
                "图标：{}（UI 图标输入原样拷入 {ICON}）",
                icon.path.display()
            ))
        }
        None => {
            std::fs::write(&target, PLACEHOLDER_PNG).map_err(io_err(&target))?;
            Ok(
                "图标：内联 1×1 占位 PNG（未提供 PNG 图标输入；平台图标规格素材不在本环节）"
                    .to_string(),
            )
        }
    }
}

/// 目录树的单一 SRI 摘要：相对路径 + 文件 SRI 的稳定序行再取 SRI（同源 → 同摘要）。
pub fn tree_digest(root: &Path) -> Result<String, BundleError> {
    let manifest = sri::manifest_of(root)?;
    let mut canonical = String::new();
    for (path, entry) in &manifest.files {
        canonical.push_str(path);
        canonical.push(' ');
        canonical.push_str(&entry.integrity);
        canonical.push('\n');
    }
    Ok(sri::integrity_of(canonical.as_bytes()))
}

/// 递归收集目录下的文件（归档条目名：`/` 分隔、按名排序 → zip / tar 条目确定性）。
pub fn collect_files(root: &Path) -> Result<Vec<(String, PathBuf)>, BundleError> {
    let mut out = Vec::new();
    collect_into(root, "", &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// 递归实现（`prefix` 为已归一的相对目录名）。
fn collect_into(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), BundleError> {
    for entry in std::fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.file_type().map_err(io_err(&path))?.is_dir() {
            collect_into(&path, &rel, out)?;
        } else {
            out.push((rel, path));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("polypack-webapp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("临时目录可创建");
        dir
    }

    /// 条目收集：名字 `/` 分隔 + 排序（跨宿主同序，Windows 反斜杠不出现在归档名里）。
    #[test]
    fn collect_files_uses_slash_and_sorts() {
        let base = scratch("collect");
        std::fs::create_dir_all(base.join("sub")).unwrap();
        std::fs::write(base.join("index.html"), "<html></html>").unwrap();
        std::fs::write(base.join("sub").join("app.js"), "1").unwrap();
        let entries = collect_files(&base).expect("收集");
        assert_eq!(
            entries
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["index.html", "sub/app.js"]
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 摘要只随内容走：改名 / 改内容才变，重跑同内容同摘要。
    #[test]
    fn tree_digest_tracks_content() {
        let base = scratch("digest");
        std::fs::write(base.join("index.html"), "a").unwrap();
        let first = tree_digest(&base).unwrap();
        assert_eq!(first, tree_digest(&base).unwrap());
        assert!(first.starts_with("sha384-"), "{first}");
        std::fs::write(base.join("index.html"), "b").unwrap();
        assert_ne!(first, tree_digest(&base).unwrap());
        let _ = std::fs::remove_dir_all(&base);
    }
}
