//! T-M1-14 契约测试：图标派生管线（Pax 品牌 SVG → PNG / ICO / ICNS）与打包器侧消费。
//!
//! Linux 段**真跑**：rsvg-convert 由 `docs/assets/pax-mark.svg` 产出 PNG 集、magick 产 ICO，
//! 派生资产进 deb 真产物并按字节比对（尺寸断言 + 品牌资产同源）。
//! ICNS 段真机阻塞（宿主无 iconutil，仅 macOS 提供）：断言缺口被**如实记录**为 Note。

use std::path::{Path, PathBuf};
use std::process::Command;

use poly_bundle::error::{BundleError, codes};
use poly_bundle::icons::{PNG_SIZES, derive};
use poly_bundle::inputs::{BundleInputs, Icon, IconKind, ProductInfo};
use poly_bundle::packager::Packager;
use poly_bundle::packagers::linux::{AppImage, Deb};
use poly_targets::{Host, PackageSpec, TargetSpec};

fn brand_svg() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/assets/pax-mark.svg")
}

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

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "poly-icons-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

fn spec(format: &str) -> TargetSpec {
    spec_for("linux", Host::Linux, format)
}

fn spec_for(target: &str, host: Host, format: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub(target, 1, &[host]);
    spec.package = PackageSpec {
        format: format.to_string(),
        builder: format!("{target}-{format}"),
    };
    spec
}

/// PNG IHDR 尺寸（签名 8 + 长度 4 + "IHDR" 4 = 偏移 16 起，大端 u32 宽/高）。
fn png_size(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).expect("读 PNG");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "PNG 签名");
    assert_eq!(&bytes[12..16], b"IHDR", "IHDR 块");
    let u32_at = |o: usize| u32::from_be_bytes(bytes[o..o + 4].try_into().unwrap());
    (u32_at(16), u32_at(20))
}

/// 真跑：派生 PNG 集（尺寸齐全）+ ICO（魔数）+ ICNS 缺口 Note。
#[test]
fn derive_produces_png_set_and_ico_with_honest_icns_note() {
    let base = scratch("derive");
    let set = derive(&brand_svg(), &base).expect("派生（rsvg-convert + magick 真跑）");

    for size in PNG_SIZES {
        let icon = set
            .icons
            .iter()
            .find(|i| i.kind == IconKind::Png && i.size == Some(size))
            .unwrap_or_else(|| panic!("缺 {size}x{size} PNG：{:?}", set.icons));
        assert_eq!(
            png_size(&icon.path),
            (size, size),
            "{} 尺寸不符",
            icon.path.display()
        );
    }
    // 品牌资产同源：派生 256 PNG 与 SVG 派生结果一致（同输入同工具 ⇒ 同字节）。
    let png256 = set
        .icons
        .iter()
        .find(|i| i.kind == IconKind::Png && i.size == Some(256))
        .expect("256 PNG");
    let redo = derive(&brand_svg(), &base.join("redo")).expect("二次派生");
    let png256b = redo
        .icons
        .iter()
        .find(|i| i.kind == IconKind::Png && i.size == Some(256))
        .expect("256 PNG");
    assert_eq!(
        std::fs::read(&png256.path).unwrap(),
        std::fs::read(&png256b.path).unwrap(),
        "同一 SVG 派生必须可复现"
    );

    let ico = set
        .icons
        .iter()
        .find(|i| i.kind == IconKind::Ico)
        .expect("ICO（magick 真跑）");
    let ico_bytes = std::fs::read(&ico.path).expect("读 ICO");
    assert_eq!(&ico_bytes[..4], b"\x00\x00\x01\x00", "ICO 魔数");
    assert!(ico_bytes.len() > 1024, "ICO 体积异常：{}", ico_bytes.len());

    // ICNS 真机阻塞（宿主）：必须留下明确 Note，不得静默省略。
    assert!(
        set.notes.iter().any(|n| n.contains("iconutil")),
        "ICNS 缺口须如实记录：{:?}",
        set.notes
    );
    if Command::new("iconutil").arg("-h").output().is_err() {
        assert!(
            set.icons.iter().all(|i| i.kind != IconKind::Icns),
            "本机无 iconutil，不得伪造 icns"
        );
    }
}

/// 真跑：派生资产进 deb 真产物，且尺寸断言端到端成立（hicolor 256 与派生 PNG 字节相同）。
#[test]
fn deb_carries_pax_derived_icons_end_to_end() {
    let base = scratch("deb-icons");
    let set = derive(&brand_svg(), &base).expect("派生");
    let png256 = set
        .icons
        .iter()
        .find(|i| i.kind == IconKind::Png && i.size == Some(256))
        .expect("256 PNG")
        .clone();

    let inputs = bare_inputs(&base, set.icons.clone(), Host::Linux);
    let artifact = Deb.package(&inputs, &spec("deb")).expect("deb 组装");

    let listing = String::from_utf8_lossy(
        &Command::new("dpkg-deb")
            .args(["--contents", &artifact.path.display().to_string()])
            .output()
            .expect("dpkg-deb --contents")
            .stdout,
    )
    .to_string();
    for size in PNG_SIZES {
        let entry = format!("./usr/share/icons/hicolor/{size}x{size}/apps/pax-notes.png");
        assert!(listing.contains(&entry), "deb 缺 {entry}：\n{listing}");
    }

    // 解包后逐字节比对：产物内图标 == 派生产物（Pax 品牌资产，非占位图）。
    let prefix = base.join("install-root");
    assert!(
        Command::new("dpkg-deb")
            .args(["-x", &artifact.path.display().to_string()])
            .arg(&prefix)
            .status()
            .expect("dpkg-deb -x")
            .success(),
        "解包失败"
    );
    let installed = prefix.join("usr/share/icons/hicolor/256x256/apps/pax-notes.png");
    assert_eq!(
        std::fs::read(&installed).expect("产物内图标"),
        std::fs::read(&png256.path).expect("派生图标"),
        "产物图标必须与派生资产逐字节相同"
    );
    assert_eq!(png_size(&installed), (256, 256));
}

/// deb 与 AppImage 一致强校验：无 PNG / 无 256 均响亮拒绝。
#[test]
fn deb_requires_256_png_icon() {
    let err = Deb
        .package(
            &bare_inputs(&scratch("deb-no-png"), Vec::new(), Host::Linux),
            &spec("deb"),
        )
        .expect_err("无 PNG 须报错");
    assert_eq!(err.code(), codes::E_BND_0002);
    assert!(matches!(
        err,
        BundleError::MissingInput {
            field: "icons(PNG)",
            ..
        }
    ));
    assert!(
        err.hint().unwrap_or_default().contains("icons::derive"),
        "修复提示须指向派生管线：{:?}",
        err.hint()
    );

    // 只有 128：仍缺 256（金样 hicolor/256x256 条目要求）。
    let base = scratch("deb-128-only");
    let set = derive(&brand_svg(), &base).expect("派生");
    let small: Vec<Icon> = set
        .icons
        .iter()
        .filter(|i| i.kind == IconKind::Png && i.size == Some(128))
        .cloned()
        .collect();
    let err = Deb
        .package(&bare_inputs(&base, small, Host::Linux), &spec("deb"))
        .expect_err("缺 256 须报错");
    assert!(matches!(
        err,
        BundleError::MissingInput {
            field: "icons(PNG 256)",
            ..
        }
    ));

    // 补齐整套派生资产后通过。
    let base = scratch("deb-full");
    let set = derive(&brand_svg(), &base).expect("派生");
    assert!(
        Deb.package(&bare_inputs(&base, set.icons, Host::Linux), &spec("deb"))
            .is_ok(),
        "派生资产应满足 deb 要求"
    );
}

/// AppImage 既有 PNG 强校验保持（T-M1-07 不回退）。
#[test]
fn appimage_keeps_its_png_requirement() {
    let err = AppImage
        .package(
            &bare_inputs(&scratch("ai-no-png"), Vec::new(), Host::Linux),
            &spec("appimage"),
        )
        .expect_err("无 PNG 须报错");
    assert!(matches!(
        err,
        BundleError::MissingInput {
            field: "icons(PNG)",
            ..
        }
    ));

    let base = scratch("ai-full");
    let set = derive(&brand_svg(), &base).expect("派生");
    assert!(
        AppImage
            .package(
                &bare_inputs(&base, set.icons, Host::Linux),
                &spec("appimage")
            )
            .is_ok(),
        "派生资产应满足 AppImage 的 PNG 要求"
    );
}

/// MSI 段（真机阻塞：Linux 宿主无 candle / light）：契约断言安装器图标是**派生资产**。
#[test]
fn msi_installer_icon_is_the_derived_ico() {
    use poly_bundle::packagers::windows::WixMsi;

    let base = scratch("msi-ico");
    let set = derive(&brand_svg(), &base).expect("派生");
    let ico = set
        .icons
        .iter()
        .find(|i| i.kind == IconKind::Ico)
        .expect("ICO")
        .clone();
    let inputs = bare_inputs(&base, set.icons, Host::Win);

    let plan = WixMsi
        .plan(&inputs, &spec_for("windows", Host::Win, "msi"))
        .expect("MSI 计划");
    assert!(
        !plan.skipped.iter().any(|s| s.tool == "icons(ICO)"),
        "派生 ICO 供上后不得再报缺图标：{:?}",
        plan.skipped
    );
    let wxs = &plan
        .scripts
        .iter()
        .find(|s| s.path.extension().is_some_and(|e| e == "wxs"))
        .expect("wxs 脚本")
        .contents;
    assert!(
        wxs.contains("ARPPRODUCTICON") && wxs.contains(&ico.path.display().to_string()),
        "MSI 图标须指向派生 ICO（{}）：\n{wxs}",
        ico.path.display()
    );

    // 真机阻塞（宿主）：Linux 无 WiX —— 断言如实报缺工具而非伪造成功。
    if !poly_bundle::exec::tool_present("candle") {
        let err = WixMsi
            .package(&inputs, &spec_for("windows", Host::Win, "msi"))
            .expect_err("无 WiX 须报 ToolMissing");
        assert_eq!(err.code(), codes::E_BND_0001);
    }
}

/// 造输入：假核心 + dist + 给定图标 → 指定宿主的 T-M1-03 裸产物。
fn bare_inputs(base: &Path, icons: Vec<Icon>, host: Host) -> BundleInputs {
    let core = base.join("pax-notes");
    std::fs::write(&core, "#!/bin/sh\nprintf 'pax-notes 1.2.3\\n'\n").expect("写夹具核心");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&core, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let dist = base.join("dist");
    std::fs::create_dir_all(dist.join("assets")).expect("建 dist");
    std::fs::write(dist.join("index.html"), "<!doctype html><body>pax</body>").unwrap();
    std::fs::write(dist.join("assets/app.js"), "console.log('pax')").unwrap();

    let mut inputs = BundleInputs::new(product(), base.join("out"));
    inputs.core_binary = Some(core);
    inputs.ui_dist = Some(dist);
    inputs.icons = icons;
    let bare = poly_bundle::tauri::bare_product(&inputs, host).expect("裸产物");
    inputs.bare = Some(bare);
    inputs
}
