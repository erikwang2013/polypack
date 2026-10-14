//! Tauri 捆绑支持（T-M1-03）：前端 dist 注入 WebView 资源，产出宿主裸产物。
//!
//! 布局依据 tauri-utils `platform.rs::resource_dir_from` 与 tauri-bundler
//! `bundle/linux/debian.rs`（2026-10 版源码）：
//!
//! | 宿主 | 可执行 | WebView 资源目录（`resource_dir()` 解析目标） |
//! |---|---|---|
//! | Linux | `<root>/usr/bin/<bin>` | `<root>/usr/lib/<产品名>`（AppImage 内为 `$APPDIR/usr/lib/<产品名>`） |
//! | Windows | `<root>/<bin>.exe` | 可执行同目录 |
//! | macOS | `<产品名>.app/Contents/MacOS/<bin>` | `<产品名>.app/Contents/Resources` |
//!
//! 前端是否内嵌进二进制由核心构建决定（Tauri 默认 `frontendDist` 内嵌）；本模块负责
//! 把 dist 作为**可分发的 WebView 资源**注入上述目录，使同一布局被 deb / AppImage /
//! MSI / NSIS / DMG 直接复用。
//!
//! macOS 宿主额外要求 `IconKind::Icns` 输入（缺失即 `MissingInput{icons.icns}`）：`.app`
//! 静默无图标不可接受，icns 由 macOS 打包机的 iconutil 从 PNG 派生（T-M1-14）。

use std::path::{Path, PathBuf};

use poly_targets::Host;

use crate::error::BundleError;
use crate::fsutil;
use crate::inputs::{BareProduct, BundleInputs, IconKind, ProductInfo};

/// 随产物分发的运行时依赖说明文件名。
pub const DEPS_FILE: &str = "RUNTIME-DEPENDENCIES.txt";

/// 宿主运行时依赖（写入产物说明；T-M1-03 验收要求 WebView2 等依赖见诸产物）。
/// macOS 另附 icns 构建前置说明。
pub fn runtime_dependencies(host: Host) -> Vec<&'static str> {
    match host {
        Host::Win => vec![
            "WebView2 Runtime（Windows 11 与 Win10 1803+ 常已预装；缺失时需随安装器分发引导器）",
            "Microsoft Visual C++ 2015-2022 运行库（MSVC 目标）",
        ],
        Host::Macos => vec![
            "无外部运行时（WebKit 随系统提供，最低 macOS 10.15）",
            "构建前置：icns 图标须在 macOS 打包机以 iconutil 预生成（Contents/Resources/icon.icns）",
        ],
        Host::Linux => vec![
            "webkit2gtk-4.1（Debian/Ubuntu 包名 libwebkit2gtk-4.1-0）",
            "GTK 3（libgtk-3-0）与 libsoup-3.0",
        ],
    }
}

/// 产出宿主裸产物：核心可执行 + WebView 资源（dist 注入）+ 图标 + 平台元数据。
///
/// 输出目录：`<out_dir>/<产品名 kebab>-<版本>-<宿主>`。
pub fn bare_product(inputs: &BundleInputs, host: Host) -> Result<BareProduct, BundleError> {
    const P: &str = "tauri";
    inputs.product.validate(P)?;
    let core = inputs
        .core_binary
        .as_deref()
        .ok_or(BundleError::MissingInput {
            packager: P,
            field: "core_binary",
        })?;
    if !core.is_file() {
        return Err(BundleError::MissingInput {
            packager: P,
            field: "core_binary",
        });
    }

    let product = &inputs.product;
    let bin = product.binary_name();
    let meta = format!("{}-{}-{}", bin, product.version, host_slug(host));
    let root = inputs.out_dir.join(meta);
    let _ = std::fs::remove_dir_all(&root);

    let (executable, deps_path) = match host {
        Host::Linux => {
            let exe = root.join("usr/bin").join(&bin);
            fsutil::copy_file(core, &exe, true)?;
            if let Some(dist) = &inputs.ui_dist {
                inject_webview_resources(dist, &linux_resource_dir(&root, product))?;
            }
            linux_metadata(inputs, &root, &bin)?;
            let deps = root.join(format!(
                "usr/share/doc/{}/{}",
                product.kebab_name(),
                DEPS_FILE
            ));
            (exe, deps)
        }
        Host::Win => {
            let exe = root.join(format!("{bin}.exe"));
            fsutil::copy_file(core, &exe, false)?;
            if let Some(dist) = &inputs.ui_dist {
                // tauri-utils：Windows 的 `resource_dir()` 恒为可执行所在目录。
                inject_webview_resources(dist, &root)?;
            }
            (exe, root.join(DEPS_FILE))
        }
        Host::Macos => {
            // 无 icns 的 .app 在 macOS 上静默无图标：响亮拒绝，不静默省略。
            let icns = inputs
                .icon(IconKind::Icns)
                .ok_or(BundleError::MissingInput {
                    packager: P,
                    field: "icons.icns",
                })?;
            let app = macos_app_root(&root, product);
            let exe = app.join("Contents/MacOS").join(&bin);
            fsutil::copy_file(core, &exe, true)?;
            fsutil::copy_file(&icns.path, &app.join("Contents/Resources/icon.icns"), false)?;
            if let Some(dist) = &inputs.ui_dist {
                inject_webview_resources(dist, &app.join("Contents/Resources"))?;
            }
            fsutil::write_text(&app.join("Contents/Info.plist"), &info_plist(product, &bin))?;
            fsutil::write_text(&app.join("Contents/PkgInfo"), "APPL????")?;
            (exe, app.join("Contents/Resources").join(DEPS_FILE))
        }
    };

    let deps = runtime_dependencies(host)
        .into_iter()
        .map(|line| format!("- {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    fsutil::write_text(&deps_path, &format!("运行时依赖：\n{deps}\n"))?;

    Ok(BareProduct {
        root,
        executable,
        host,
    })
}

/// 前端 dist 注入 WebView 资源目录（递归拷贝；dist 缺失即快速失败）。
pub fn inject_webview_resources(dist: &Path, resources: &Path) -> Result<(), BundleError> {
    if !dist.is_dir() {
        return Err(BundleError::MissingInput {
            packager: "tauri",
            field: "ui_dist",
        });
    }
    fsutil::copy_tree(dist, resources)
}

/// Linux 资源目录（`resource_dir()` 的 `../lib/<产品名>` 分支）。
pub fn linux_resource_dir(root: &Path, product: &ProductInfo) -> PathBuf {
    root.join("usr/lib").join(&product.name)
}

/// macOS `.app` 根（`<root>/<产品名>.app`）。
pub fn macos_app_root(root: &Path, product: &ProductInfo) -> PathBuf {
    root.join(format!("{}.app", product.name))
}

/// freedesktop desktop 文件（Linux 裸产物与 AppDir 共用同一内容）。
pub fn desktop_file(product: &ProductInfo, bin: &str) -> String {
    let mut out = String::from("[Desktop Entry]\n");
    out.push_str("Type=Application\n");
    out.push_str("Version=1.1\n");
    out.push_str(&format!("Name={}\n", product.name));
    out.push_str(&format!("Comment={}\n", product.description));
    out.push_str(&format!("Exec={bin} %U\n"));
    out.push_str(&format!("Icon={}\n", product.kebab_name()));
    out.push_str("Terminal=false\n");
    out.push_str("Categories=Utility;\n");
    out.push_str(&format!("StartupWMClass={}\n", product.kebab_name()));
    if let Some(home) = &product.homepage {
        out.push_str(&format!("Homepage={home}\n"));
    }
    out
}

/// macOS `Info.plist`（最小可用集：标识、可执行、版本、类型）。
pub fn info_plist(product: &ProductInfo, bin: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>{name}</string>
    <key>CFBundleDisplayName</key><string>{name}</string>
    <key>CFBundleIdentifier</key><string>{identifier}</string>
    <key>CFBundleExecutable</key><string>{bin}</string>
    <key>CFBundleVersion</key><string>{version}</string>
    <key>CFBundleShortVersionString</key><string>{version}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleIconFile</key><string>icon.icns</string>
    <key>LSMinimumSystemVersion</key><string>10.15</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#,
        name = product.name,
        identifier = product.identifier,
        version = product.version,
    )
}

/// Linux 平台元数据：desktop 文件 + hicolor 图标。
fn linux_metadata(inputs: &BundleInputs, root: &Path, bin: &str) -> Result<(), BundleError> {
    let product = &inputs.product;
    let name = product.kebab_name();
    fsutil::write_text(
        &root.join(format!("usr/share/applications/{name}.desktop")),
        &desktop_file(product, bin),
    )?;
    for icon in inputs.icons.iter().filter(|i| i.kind == IconKind::Png) {
        let size = icon.size.ok_or(BundleError::InvalidInput {
            packager: "tauri",
            field: "icons[].size",
            value: icon.path.display().to_string(),
            reason: "Linux hicolor 布局需要 PNG 图标边长".to_string(),
        })?;
        fsutil::copy_file(
            &icon.path,
            &root.join(format!(
                "usr/share/icons/hicolor/{size}x{size}/apps/{name}.png"
            )),
            false,
        )?;
    }
    Ok(())
}

/// 宿主目录名片段。
fn host_slug(host: Host) -> &'static str {
    match host {
        Host::Win => "windows",
        Host::Linux => "linux",
        Host::Macos => "macos",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_file_has_required_keys() {
        let product = ProductInfo {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "示例笔记应用".to_string(),
            homepage: None,
        };
        let desktop = desktop_file(&product, "pax-notes");
        for key in [
            "Type=Application",
            "Name=Pax Notes",
            "Exec=pax-notes",
            "Icon=pax-notes",
        ] {
            assert!(desktop.contains(key), "desktop 缺少 {key}");
        }
        assert!(desktop.starts_with("[Desktop Entry]\n"));
    }

    #[test]
    fn info_plist_carries_identity() {
        let product = ProductInfo {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "示例笔记应用".to_string(),
            homepage: None,
        };
        let plist = info_plist(&product, "pax-notes");
        assert!(
            plist.contains("<key>CFBundleIdentifier</key><string>com.polypack.paxnotes</string>")
        );
        assert!(plist.contains("<key>CFBundleExecutable</key><string>pax-notes</string>"));
    }
}
