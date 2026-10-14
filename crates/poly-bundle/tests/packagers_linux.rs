//! T-M1-07 契约测试：deb 与 AppImage 打包器（Linux 宿主**真跑**）。
//!
//! 真跑范围：dpkg-deb 组装 + `--info`/`--contents` 校验 + `dpkg-deb -x` 解包安装冒烟
//! + AppDir 直接执行冒烟 + desktop-file-validate。
//!
//! 工具无关性：图标取仓库内提交的夹具（`tests/fixtures/`，由 T-M1-14 派生管线生成一次
//! 入库），不再依赖宿主 rsvg-convert / magick；dpkg-deb / desktop-file-validate / sh
//! 缺失时**显式 skip**（打印原因后通过），工具齐备的宿主走真跑分支。
//!
//! 阻塞项：`dpkg -i` 系统级安装需 root；AppImage 单文件生成需 appimagetool（本宿主未装）。

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use poly_bundle::error::{BundleError, codes};
use poly_bundle::inputs::{BundleInputs, Icon, IconKind, ProductInfo};
use poly_bundle::packager::{Packager, builtin, find};
use poly_bundle::packagers::linux::{AppImage, Deb};
use poly_targets::{Host, PackageSpec, TargetSpec};

const FAKE_CORE: &str = r#"#!/bin/sh
# PolyPack 测试夹具：模拟 Tauri 核心（Linux 资源解析规则同 tauri-utils）。
self=$(readlink -f -- "$0" 2>/dev/null || echo "$0")
self_dir=$(dirname -- "$self")
resources="$self_dir/../lib/Pax Notes"
case "$1" in
  --version) printf 'pax-notes %s\n' "1.2.3"; [ -f "$resources/index.html" ] || exit 1 ;;
  *) exit 2 ;;
esac
"#;

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
        "poly-linux-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

fn spec(format: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub("linux", 1, &[Host::Linux]);
    spec.package = PackageSpec {
        format: format.to_string(),
        builder: format!("linux-{format}"),
    };
    spec
}

/// Pax 图标（仓库内夹具：T-M1-14 派生管线产出的 256 / 128 PNG，跨宿主可用）。
fn pax_icons() -> Vec<Icon> {
    vec![
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
    ]
}

/// 造输入：假核心 + dist + 256/128 PNG 图标 → T-M1-03 裸产物。
fn inputs(base: &Path) -> BundleInputs {
    let core = base.join("pax-notes");
    std::fs::write(&core, FAKE_CORE).expect("写夹具核心");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&core, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let dist = base.join("dist");
    std::fs::create_dir_all(dist.join("assets")).unwrap();
    std::fs::write(dist.join("index.html"), "<!doctype html><body>pax</body>").unwrap();
    std::fs::write(dist.join("assets/app.js"), "console.log('pax')").unwrap();

    let mut inputs = BundleInputs::new(product(), base.join("out"));
    inputs.core_binary = Some(core);
    inputs.ui_dist = Some(dist);
    inputs.icons = pax_icons();
    let bare = poly_bundle::tauri::bare_product(&inputs, Host::Linux).expect("裸产物");
    inputs.bare = Some(bare);
    inputs
}

fn run(exe: &Path) -> std::process::Output {
    Command::new(exe)
        .arg("--version")
        .output()
        .expect("执行产物")
}

#[test]
fn deb_package_builds_and_verifies_with_dpkg_deb() {
    let base = scratch("deb");
    let inputs = inputs(&base);
    let plan = Deb
        .plan(&inputs, &spec("deb"))
        .expect("deb 计划（不执行外部工具）");

    // 计划契约：control 内容 + dpkg-deb 命令组装。
    let control = plan
        .scripts
        .iter()
        .find(|s| s.path.ends_with("DEBIAN/control"))
        .expect("control 脚本");
    for field in [
        "Package: pax-notes\n",
        "Version: 1.2.3\n",
        "Architecture: amd64\n",
        "Maintainer: PolyPack\n",
        "Installed-Size: ",
        "Section: utils\n",
        "Priority: optional\n",
        "Description: Pax 示例笔记应用\n",
    ] {
        assert!(control.contents.contains(field), "control 缺 {field:?}");
    }
    let lines: Vec<String> = plan.commands.iter().map(|c| c.line()).collect();
    assert_eq!(lines.len(), 3, "命令序列：{lines:?}");
    assert!(
        lines[0].starts_with("dpkg-deb --build --root-owner-group "),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].ends_with("pax-notes_1.2.3_amd64.deb"),
        "{}",
        lines[0]
    );
    assert!(lines[1].starts_with("dpkg-deb --info "), "{}", lines[1]);
    assert!(lines[2].starts_with("dpkg-deb --contents "), "{}", lines[2]);

    // 以下为真跑段：需 dpkg-deb（组装/校验/解包）、desktop-file-validate 与 sh（冒烟）。
    // 计划契约已在上面无条件断言，故工具缺失的宿主（macOS / Windows 腿）仍覆盖计划面。
    if common::skip_without_any(&["dpkg-deb", "desktop-file-validate", "sh"]) {
        return;
    }

    // 真跑：组装 + --info / --contents 校验。
    let artifact = Deb.package(&inputs, &spec("deb")).expect("deb 组装");
    assert_eq!(artifact.format, "deb");
    assert_eq!(artifact.target, "linux");
    assert!(artifact.bytes > 0, "产物为空");
    assert!(
        artifact.path.ends_with("pax-notes_1.2.3_amd64.deb"),
        "产物路径：{}",
        artifact.path.display()
    );
    let evidence = artifact.notes.join("\n");
    assert!(
        evidence
            .contains("dpkg-deb --info: Package: pax-notes, Version: 1.2.3, Architecture: amd64"),
        "校验证据：{evidence}"
    );

    let contents = Command::new("dpkg-deb")
        .args(["--contents", &artifact.path.display().to_string()])
        .output()
        .expect("dpkg-deb --contents");
    let listing = String::from_utf8_lossy(&contents.stdout).to_string();
    for entry in [
        "./usr/bin/pax-notes",
        "./usr/lib/Pax Notes/index.html",
        "./usr/share/applications/pax-notes.desktop",
        "./usr/share/icons/hicolor/256x256/apps/pax-notes.png",
        "./usr/share/doc/pax-notes/RUNTIME-DEPENDENCIES.txt",
        "./usr/share/doc/pax-notes/copyright",
    ] {
        assert!(listing.contains(entry), "deb 缺少 {entry}：\n{listing}");
    }
    // 发行包权限惯例：不带组 / 其他可写位（构建树 umask 002 的 664/775 不得泄漏）。
    for line in listing.lines() {
        let mode = line.split_whitespace().next().unwrap_or("");
        if mode.len() == 10 {
            let bits = mode.as_bytes();
            assert!(
                bits[5] != b'w' && bits[8] != b'w',
                "权限非常规（组/其他可写）：{line}"
            );
        }
    }
    assert!(
        listing.contains("-rwxr-xr-x root/root") && listing.contains("./usr/bin/pax-notes"),
        "可执行位丢失：\n{listing}"
    );

    // 安装冒烟（免 root 形态）：dpkg-deb -x 解包 → 运行解包后的可执行。
    let prefix = base.join("install-root");
    let status = Command::new("dpkg-deb")
        .args(["-x", &artifact.path.display().to_string()])
        .arg(&prefix)
        .status()
        .expect("dpkg-deb -x");
    assert!(status.success(), "解包失败");
    let smoke = run(&prefix.join("usr/bin/pax-notes"));
    assert!(
        smoke.status.success(),
        "解包后的产物启动失败：{}",
        String::from_utf8_lossy(&smoke.stderr)
    );
    assert!(String::from_utf8_lossy(&smoke.stdout).contains("pax-notes 1.2.3"));

    // desktop 文件合规（本机 desktop-file-validate）。
    let desktop = prefix.join("usr/share/applications/pax-notes.desktop");
    let validate = Command::new("desktop-file-validate")
        .arg(&desktop)
        .output()
        .expect("desktop-file-validate");
    assert!(
        validate.status.success(),
        "desktop 文件不合法：{}",
        String::from_utf8_lossy(&validate.stderr)
    );
}

#[test]
fn appimage_without_tool_yields_labeled_appdir_that_runs() {
    // AppDir 需要根级 AppRun 符号链接，且冒烟要执行 shell 夹具核心。
    if common::skip_unless_unix("AppDir 符号链接与 shell 夹具") || common::skip_without("sh")
    {
        return;
    }
    let base = scratch("appimage");
    let inputs = inputs(&base);
    let plan = AppImage.plan(&inputs, &spec("appimage")).expect("计划");
    assert!(
        plan.commands[0]
            .line()
            .starts_with("appimagetool --no-appstream "),
        "命令组装：{}",
        plan.commands[0].line()
    );
    assert!(
        plan.skipped.iter().any(|s| s.tool == "appimagetool"),
        "缺工具须如实记录：{:?}",
        plan.skipped
    );

    let artifact = AppImage
        .package(&inputs, &spec("appimage"))
        .expect("AppDir");
    if artifact.format == "appimage" {
        // 真机上装了 appimagetool 的路径：单文件产物。
        assert!(artifact.path.extension().unwrap() == "AppImage");
        return;
    }
    assert_eq!(artifact.format, "appdir", "未打包必须如实标注格式");
    assert_eq!(artifact.kind, poly_bundle::ArtifactKind::Directory);
    assert!(artifact.notes.iter().any(|n| n.contains("未打包")));

    let appdir = &artifact.path;
    assert!(
        appdir.join("pax-notes.desktop").is_file(),
        "AppDir 根级 desktop"
    );
    assert!(appdir.join("pax-notes.png").is_file(), "AppDir 根级图标");
    assert!(
        std::fs::symlink_metadata(appdir.join("AppRun"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(appdir.join("usr/bin/pax-notes").is_file());

    // 真跑：直接执行 AppDir 的 AppRun（符号链接 → usr/bin，资源按 ../lib 解析）。
    let smoke = run(&appdir.join("AppRun"));
    assert!(
        smoke.status.success(),
        "AppRun 冒烟失败：{}",
        String::from_utf8_lossy(&smoke.stderr)
    );
}

#[test]
fn missing_png_icon_blocks_appimage() {
    let base = scratch("no-icon");
    let mut inputs = inputs(&base);
    inputs.icons.clear();
    let err = AppImage
        .package(&inputs, &spec("appimage"))
        .expect_err("缺 PNG 图标须报错");
    assert_eq!(err.code(), codes::E_BND_0002);
    assert!(matches!(
        err,
        BundleError::MissingInput {
            field: "icons(PNG)",
            ..
        }
    ));
}

#[test]
fn format_mismatch_is_rejected() {
    let base = scratch("guard");
    let inputs = inputs(&base);
    let deb_on_appimage = Deb.package(&inputs, &spec("appimage")).unwrap_err();
    let appimage_on_deb = AppImage.package(&inputs, &spec("deb")).unwrap_err();
    for (packager, err) in [
        (Deb.id(), deb_on_appimage),
        (AppImage.id(), appimage_on_deb),
    ] {
        assert_eq!(err.code(), codes::E_BND_0003, "{packager}");
        assert!(matches!(
            err,
            BundleError::InvalidInput {
                field: "spec.package.format",
                ..
            }
        ));
    }
}

#[test]
fn registry_exposes_all_m1_packagers() {
    let all: Vec<(String, String)> = builtin()
        .iter()
        .map(|p| (p.id().to_string(), p.format().to_string()))
        .collect();
    for want in [
        ("windows", "msi"),
        ("windows", "nsis"),
        ("macos", "dmg"),
        ("linux", "deb"),
        ("linux", "appimage"),
        ("web", "static-site"),
    ] {
        assert!(
            all.contains(&(want.0.to_string(), want.1.to_string())),
            "缺 {want:?}：{all:?}"
        );
    }
    assert!(find("linux", "deb").is_some());
    assert!(find("linux", "msi").is_none());
    assert!(find("nope", "deb").is_none());
}

#[test]
fn missing_bare_product_is_reported() {
    let base = scratch("no-bare");
    let mut inputs = inputs(&base);
    inputs.bare = None;
    for err in [
        Deb.package(&inputs, &spec("deb")).unwrap_err(),
        AppImage.package(&inputs, &spec("appimage")).unwrap_err(),
    ] {
        assert!(matches!(
            err,
            BundleError::MissingInput { field: "bare", .. }
        ));
        assert_eq!(err.code(), codes::E_BND_0002);
    }
}
