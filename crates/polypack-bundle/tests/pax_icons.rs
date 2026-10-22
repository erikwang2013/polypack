//! T-M1-14 契约测试：图标派生管线（Pax 品牌 SVG → PNG / ICO / ICNS）与打包器侧消费。
//!
//! Linux 段**真跑**：rsvg-convert 由 `docs/assets/pax-mark.svg` 产出 PNG 集、magick 产 ICO，
//! 派生资产进 deb 真产物并按字节比对（尺寸断言 + 品牌资产同源）。
//! ICNS 段真机阻塞（宿主无 iconutil，仅 macOS 提供）：断言缺口被**如实记录**为 Note。
//!
//! 工具无关性：派生管线自身依赖 rsvg-convert / magick / dpkg-deb，缺失时**显式 skip**
//! （打印原因后通过）；打包器侧的契约断言改用仓库内夹具（`tests/fixtures/`），任意宿主
//! 恒可运行。
//!
//! 夹具出处：`tests/fixtures/{pax-notes-128.png,pax-notes-256.png,pax-notes.ico}` 由本管线
//! 在 Linux 上生成一次后入库（256 PNG 供 `rsvg-convert`，ICO 供 `magick -define
//! icon:auto-resize=256,128,64,48,32,16`）。**不做跨宿主逐字节比对**：PNG/ICO 编码随
//! librsvg / ImageMagick 版本变化，字节相等不是契约；契约是**结构**（尺寸集、帧数、位深、
//! 数据区完整），由 `committed_fixtures…` 恒跑断言，工具齐备时另与现派生输出比对结构。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use polypack_bundle::error::{BundleError, codes};
use polypack_bundle::icons::{PNG_SIZES, derive};
use polypack_bundle::inputs::{BundleInputs, Icon, IconKind, ProductInfo};
use polypack_bundle::packager::Packager;
use polypack_bundle::packagers::linux::{AppImage, Deb};
use polypack_targets::{Host, PackageSpec, TargetSpec};

fn brand_svg() -> PathBuf {
    common::brand_svg()
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
        "polypack-icons-{tag}-{}-{:?}",
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

/// ICO 目录表：帧数 + 每帧 `(宽, 高, 位深, 数据字节数)`（宽/高 0 记作 256，ICO 惯例）。
fn ico_frames(path: &Path) -> Vec<(u32, u32, u16, u32)> {
    let bytes = std::fs::read(path).expect("读 ICO");
    assert_eq!(&bytes[..4], b"\x00\x00\x01\x00", "ICO 魔数");
    let u16_at = |o: usize| u16::from_le_bytes(bytes[o..o + 2].try_into().unwrap());
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let count = u16_at(4) as usize;
    let frames: Vec<(u32, u32, u16, u32)> = (0..count)
        .map(|i| {
            let e = 6 + i * 16;
            let dim = |b: u8| if b == 0 { 256 } else { u32::from(b) };
            let (w, h, bpp, size, offset) = (
                dim(bytes[e]),
                dim(bytes[e + 1]),
                u16_at(e + 6),
                u32_at(e + 8),
                u32_at(e + 12) as usize,
            );
            let data = &bytes[offset..];
            assert!(data.len() >= size as usize, "第 {i} 帧数据区越界");
            assert_eq!(
                &data[..4],
                &(40u32).to_le_bytes(),
                "第 {i} 帧须为 BITMAPINFOHEADER 起始",
            );
            (w, h, bpp, size)
        })
        .collect();
    let sorted = frames.windows(2).all(|p| p[0].0 > p[1].0);
    assert!(sorted, "帧尺寸须由大到小：{frames:?}");
    frames
}

/// 真跑：派生 PNG 集（尺寸齐全）+ ICO（魔数）+ ICNS 缺口 Note。
#[test]
fn derive_produces_png_set_and_ico_with_honest_icns_note() {
    // 派生管线真跑需 rsvg-convert；ICO 断言另需 magick（缺失则只断言如实记录的缺口）。
    if common::skip_without("rsvg-convert") {
        return;
    }
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

    if common::has("magick") {
        let ico = set
            .icons
            .iter()
            .find(|i| i.kind == IconKind::Ico)
            .expect("ICO（magick 真跑）");
        let ico_bytes = std::fs::read(&ico.path).expect("读 ICO");
        assert!(ico_bytes.len() > 1024, "ICO 体积异常：{}", ico_bytes.len());
        let derived_frames = ico_frames(&ico.path);
        let want: Vec<(u32, u32)> = PNG_SIZES.iter().rev().map(|s| (*s, *s)).collect();
        let got: Vec<(u32, u32)> = derived_frames.iter().map(|f| (f.0, f.1)).collect();
        assert_eq!(got, want, "ICO 帧尺寸集须等于 PNG 尺寸集");
        assert!(
            derived_frames.iter().all(|f| f.2 == 32),
            "帧位深须为 32：{derived_frames:?}"
        );

        // 夹具的出处契约：与现派生输出**结构**相同（字节随工具版本变，不作契约）。
        let fixture_ico = common::fixture("pax-notes.ico");
        assert_eq!(
            ico_frames(&fixture_ico),
            derived_frames,
            "tests/fixtures/pax-notes.ico 须与派生 ICO 同结构（帧尺寸/位深/体积）"
        );
        let fixture_png = common::fixture("pax-notes-256.png");
        assert_eq!(
            png_size(&fixture_png),
            png_size(&png256.path),
            "tests/fixtures/pax-notes-256.png 须与派生 256 PNG 同尺寸"
        );
    } else {
        // 无 magick：不得伪造 ICO，须留下如实的缺口记录。
        eprintln!("SKIP：宿主缺少 magick（仅断言 ICO 缺口被如实记录）");
        assert!(
            set.icons.iter().all(|i| i.kind != IconKind::Ico),
            "无 magick 不得产出 ICO：{:?}",
            set.icons
        );
        assert!(
            set.notes.iter().any(|n| n.contains("ICO")),
            "ICO 缺口须如实记录：{:?}",
            set.notes
        );
    }

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

/// 夹具结构（**无需任何工具**，任意宿主恒跑）：打包器测试依赖的图标夹具确为 Pax 派生资产。
///
/// 体积下限是「非占位图」的稳健代理：纯色占位图 PNG 压缩后仅数百字节，且 ICO 不会到 10 万级。
#[test]
fn committed_fixtures_are_structurally_pax_icons() {
    for (name, size) in [("pax-notes-128.png", 128u32), ("pax-notes-256.png", 256u32)] {
        let path = common::fixture(name);
        assert_eq!(png_size(&path), (size, size), "{name} 尺寸");
        let len = std::fs::metadata(&path).expect("夹具存在").len();
        assert!(len > 4_000, "{name} 体积极小（疑似占位图）：{len} 字节");
    }

    let frames = ico_frames(&common::fixture("pax-notes.ico"));
    let got: Vec<(u32, u32)> = frames.iter().map(|f| (f.0, f.1)).collect();
    let want: Vec<(u32, u32)> = PNG_SIZES.iter().rev().map(|s| (*s, *s)).collect();
    assert_eq!(got, want, "ICO 须含全部 {PNG_SIZES:?} 尺寸帧");
    assert!(frames.iter().all(|f| f.2 == 32), "帧位深须为 32");
}

/// 真跑：派生资产进 deb 真产物，且尺寸断言端到端成立（hicolor 256 与派生 PNG 字节相同）。
#[test]
fn deb_carries_pax_derived_icons_end_to_end() {
    if common::skip_without_any(&["rsvg-convert", "dpkg-deb"]) {
        return;
    }
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

    // 只有 128：仍缺 256（金样 hicolor/256x256 条目要求）——用仓库夹具，免宿主工具。
    let base = scratch("deb-128-only");
    let small = vec![Icon {
        path: common::fixture("pax-notes-128.png"),
        kind: IconKind::Png,
        size: Some(128),
    }];
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

    // 补齐 256 后通过（deb 组装需 dpkg-deb）。
    if common::skip_without("dpkg-deb") {
        return;
    }
    let base = scratch("deb-full");
    let full = vec![
        Icon {
            path: common::fixture("pax-notes-256.png"),
            kind: IconKind::Png,
            size: Some(256),
        },
        Icon {
            path: common::fixture("pax-notes-128.png"),
            kind: IconKind::Png,
            size: Some(128),
        },
    ];
    assert!(
        Deb.package(&bare_inputs(&base, full, Host::Linux), &spec("deb"))
            .is_ok(),
        "夹具图标应满足 deb 要求"
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

    // AppImage 打包路径要建根级 AppRun 符号链接（非 Unix 宿主跳过）。
    if common::skip_unless_unix("AppDir 符号链接") {
        return;
    }
    let base = scratch("ai-full");
    let full = vec![Icon {
        path: common::fixture("pax-notes-256.png"),
        kind: IconKind::Png,
        size: Some(256),
    }];
    assert!(
        AppImage
            .package(&bare_inputs(&base, full, Host::Linux), &spec("appimage"))
            .is_ok(),
        "夹具图标应满足 AppImage 的 PNG 要求"
    );
}

/// MSI 段（真机阻塞：Linux 宿主无 candle / light）：契约断言安装器图标是**派生资产**。
///
/// 图标取仓库内夹具（与派生管线输出逐字节相同，`derive_produces_…` 断言该同一性），
/// 使本契约在任意宿主恒可运行。
#[test]
fn msi_installer_icon_is_the_derived_ico() {
    use polypack_bundle::packagers::windows::WixMsi;

    let base = scratch("msi-ico");
    let ico = Icon {
        path: common::fixture("pax-notes.ico"),
        kind: IconKind::Ico,
        size: Some(256),
    };
    let inputs = bare_inputs(&base, vec![ico.clone()], Host::Win);

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
    if !polypack_bundle::exec::tool_present("candle") {
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
    let bare = polypack_bundle::tauri::bare_product(&inputs, host).expect("裸产物");
    inputs.bare = Some(bare);
    inputs
}
