//! T-M1-14 图标派生管线：品牌 SVG → PNG 集 / ICO / ICNS（构建期生成，不入库）。
//!
//! 单一源：模板项目 `ui/public/icon.svg` 与 `docs/assets/pax-mark.svg` 逐字节相同
//! （测试断言），本管线只消费该 SVG，自身不携带、不复制任何品牌资产。
//!
//! 派生结果的消费方：`usr/share/icons/hicolor/<N>x<N>/apps`（deb / AppImage，PNG）、
//! MSI / NSIS（ICO，可选）、macOS `.app`（ICNS，必需——icns 由 macOS 打包机的 iconutil
//! 从本管线给出的 PNG 集派生，本机无 iconutil 时如实记录缺口，不伪造）。

use std::path::{Path, PathBuf};

use crate::error::{BundleError, io_err};
use crate::exec;
use crate::inputs::{Icon, IconKind};
use crate::packager::Command;

/// 派生的 PNG 边长集合（hicolor / AppImage 根图标 / ICO 与 iconset 的源）。
pub const PNG_SIZES: [u32; 6] = [16, 32, 48, 64, 128, 256];

/// 派生结果：可直接填入 `BundleInputs.icons` 的图标输入 + 如实记录的缺口说明。
#[derive(Debug)]
pub struct IconSet {
    /// 派生出的图标（PNG 集、可选 ICO）。
    pub icons: Vec<Icon>,
    /// 未派生的项（工具缺失等），逐条写明原因，绝不静默省略。
    pub notes: Vec<String>,
}

/// 从品牌 SVG 派生图标到 `<out_dir>/icons/`。
///
/// PNG 集由 `rsvg-convert` 生成——它是 deb / AppImage 的必需输入，工具缺失即
/// [`BundleError::ToolMissing`]；ICO（`magick`）与 ICNS（`iconutil`）缺失时只记
/// [`IconSet::notes`]，由消费端按各自严格度决定是否拒绝（Deb 要求 PNG，MSI 的 ICO 可选）。
pub fn derive(svg: &Path, out_dir: &Path) -> Result<IconSet, BundleError> {
    const P: &str = "icons";
    if !svg.is_file() {
        return Err(BundleError::MissingInput {
            packager: P,
            field: "icon_svg",
        });
    }
    if !exec::tool_present("rsvg-convert") {
        return Err(BundleError::ToolMissing {
            packager: P,
            tool: "rsvg-convert".to_string(),
            hint: exec::install_hint("rsvg-convert"),
        });
    }

    let dir = out_dir.join("icons");
    std::fs::create_dir_all(&dir).map_err(io_err(&dir))?;
    let stem = svg
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "icon".to_string());
    let mut set = IconSet {
        icons: Vec::new(),
        notes: Vec::new(),
    };

    for size in PNG_SIZES {
        let png = dir.join(format!("{stem}-{size}.png"));
        exec::run(&Command::new(
            "rsvg-convert",
            [
                "-w".to_string(),
                size.to_string(),
                "-h".to_string(),
                size.to_string(),
                "-o".to_string(),
                png.display().to_string(),
                svg.display().to_string(),
            ],
        ))?;
        set.icons.push(Icon {
            path: png,
            kind: IconKind::Png,
            size: Some(size),
        });
    }

    let png256 = icon_path(&set, IconKind::Png, 256);
    if exec::tool_present("magick") {
        let ico = dir.join(format!("{stem}.ico"));
        exec::run(&Command::new(
            "magick",
            [
                png256.display().to_string(),
                "-define".to_string(),
                "icon:auto-resize=256,128,64,48,32,16".to_string(),
                ico.display().to_string(),
            ],
        ))?;
        set.icons.push(Icon {
            path: ico,
            kind: IconKind::Ico,
            size: Some(256),
        });
    } else {
        set.notes.push(
            "未派生 ICO：ImageMagick(magick) 未安装（Windows 打包机：apt install imagemagick 或安装 ImageMagick）"
                .to_string(),
        );
    }

    // shortcut: iconutil 仅随 macOS 提供，本机不可能真实执行；此处只给出手把手的 macOS
    // 步骤而不写无法验证的分支。macOS 打包任务落地时按注释实现并加上真跑验收。
    set.notes.push(format!(
        "未派生 ICNS：需 macOS 打包机执行 iconutil —— 由 PNG 集组织 `{stem}.iconset/`（icon_16x16.png / icon_16x16@2x.png=32 / icon_32x32.png / icon_32x32@2x.png=64 / icon_128x128.png / icon_128x128@2x.png=256 / icon_256x256.png），再 `iconutil -c icns {stem}.iconset -o {stem}.icns`"
    ));
    Ok(set)
}

/// 取已派生图标的路径（缺项由调用方先行校验）。
fn icon_path(set: &IconSet, kind: IconKind, size: u32) -> PathBuf {
    set.icons
        .iter()
        .find(|i| i.kind == kind && i.size == Some(size))
        .map(|i| i.path.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 模板项目的图标与仓库品牌源逐字节相同（T-M1-14 单一源断言）。
    #[test]
    fn template_icon_is_byte_identical_to_brand_source() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let brand = root.join("docs/assets/pax-mark.svg");
        let template = root.join("crates/polypack-cli/templates/app/ui/public/icon.svg");
        let (a, b) = (
            std::fs::read(&brand).expect("品牌 SVG"),
            std::fs::read(&template).expect("模板图标 SVG"),
        );
        assert_eq!(a, b, "模板图标必须与 docs/assets/pax-mark.svg 逐字节相同");
    }

    #[test]
    fn missing_svg_is_reported() {
        let out = std::env::temp_dir().join(format!("polypack-icons-{}", std::process::id()));
        let err = derive(&out.join("nope.svg"), &out).expect_err("缺 SVG 须报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0002);
        assert!(matches!(
            err,
            BundleError::MissingInput {
                field: "icon_svg",
                ..
            }
        ));
    }
}
