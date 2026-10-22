//! Linux 打包器契约测试：T-M1-07 首版（deb / AppImage）+ T-M5-01..04 成熟化
//! （rpm / Flatpak / Snap、AppStream、L5 发行版目标分派）。
//!
//! 真跑范围：dpkg-deb 组装、`--info`/`--contents` 校验、`dpkg-deb -x` 解包安装冒烟、
//! AppDir 直接执行冒烟、desktop-file-validate；rpmbuild 在场时真产 `.rpm` 并解析
//! 头部（PATH 或 `~/.cache/polypack/tools/rpm`）。Flatpak / Snap 运行时缺席时如实
//! 降级为 `*-stage` 目录，结构自检仍真跑。
//!
//! 工具无关性：图标取仓库内提交的夹具（`tests/fixtures/`，由 T-M1-14 派生管线生成一次
//! 入库），不再依赖宿主 rsvg-convert / magick；dpkg-deb / desktop-file-validate / sh
//! 缺失时**显式 skip**（打印原因后通过），工具齐备的宿主走真跑分支。
//!
//! 阻塞项：`dpkg -i` 系统级安装需 root；AppImage 单文件生成需 appimagetool、deb 验收需
//! lintian（CI ubuntu 腿安装 1.9.1/sha256 校验与发行版 lintian；缺席宿主显式 skip）。

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use polypack_bundle::error::{BundleError, codes};
use polypack_bundle::inputs::{BundleInputs, Icon, IconKind, ProductInfo};
use polypack_bundle::packager::{Packager, builtin, find};
use polypack_bundle::packagers::linux::deb::DEB_RUNTIME_DEPENDS;
use polypack_bundle::packagers::linux::{
    AppImage, Deb, FLATPAK_FINISH_ARGS, Flatpak, Rpm, SNAP_PLUGS, Snap, deb_arch, rpm_arch,
};
use polypack_targets::{Host, PackageSpec, TargetSpec};

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
        // AppStream 的 url-homepage 校验项：验收跑 appstreamcli validate 需要主页。
        homepage: Some("https://example.com/pax-notes".to_string()),
    }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "polypack-linux-{tag}-{}-{:?}",
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
    let bare = polypack_bundle::tauri::bare_product(&inputs, Host::Linux).expect("裸产物");
    inputs.bare = Some(bare);
    inputs
}

fn run(exe: &Path) -> std::process::Output {
    Command::new(exe)
        .arg("--version")
        .output()
        .expect("执行产物")
}

/// appimagetool 1.9.1 无 `ARCH` 时用 AppDir 内 `*.so.*` 的 ELF `e_machine` 猜架构；
/// 夹具核心是 shell 脚本、没有共享库，补一个只带 ELF 头的桩让真跑路径可判架构。
fn add_arch_stub(inputs: &BundleInputs) {
    let machine: u16 = match std::env::consts::ARCH {
        "x86_64" => 62,
        "aarch64" => 183,
        "x86" => 3,
        "arm" => 40,
        other => panic!("appimagetool 架构桩未覆盖 {other}"),
    };
    let mut elf = vec![0u8; 20];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2; // ELFCLASS64（e_machine 字段位置与 class 无关）
    elf[5] = 1; // little-endian
    elf[0x12..0x14].copy_from_slice(&machine.to_le_bytes());
    let root = inputs.bare.as_ref().expect("裸产物").root.join("usr/lib");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("libpax.so.1"), elf).unwrap();
}

#[test]
fn deb_package_builds_and_verifies_with_dpkg_deb() {
    let base = scratch("deb");
    let inputs = inputs(&base);
    let plan = Deb
        .plan(&inputs, &spec("deb"))
        .expect("deb 计划（不执行外部工具）");

    // 宿主 arch 口径：与打包器同一推导（x86_64 → amd64、aarch64 → arm64），
    // 否则 macOS 腿（Apple Silicon 的 Homebrew dpkg-deb）真跑时会误报。
    let arch = deb_arch();
    if cfg!(target_arch = "x86_64") {
        assert_eq!(arch, "amd64", "x86_64 宿主须映射为 amd64");
    } else if cfg!(target_arch = "aarch64") {
        assert_eq!(arch, "arm64", "aarch64 宿主须映射为 arm64");
    }

    // 计划契约：control 内容 + dpkg-deb 命令组装。
    let control = plan
        .scripts
        .iter()
        .find(|s| s.path.ends_with("DEBIAN/control"))
        .expect("control 脚本");
    for field in [
        "Package: pax-notes\n",
        "Version: 1.2.3\n",
        &format!("Architecture: {arch}\n"),
        "Maintainer: PolyPack <noreply@polypack.dev>\n",
        "Installed-Size: ",
        "Section: utils\n",
        "Priority: optional\n",
        "Description: Pax 示例笔记应用\n",
        &format!("Depends: {DEB_RUNTIME_DEPENDS}\n"),
    ] {
        assert!(control.contents.contains(field), "control 缺 {field:?}");
    }
    // 维护者脚本：postinst/postrm 必须随包（dpkg-deb 要求可执行位，组装侧已置）。
    for (name, marker) in [
        ("DEBIAN/postinst", "[ \"$1\" = \"configure\" ]"),
        ("DEBIAN/postrm", "[ \"$1\" = \"remove\" ]"),
    ] {
        let script = plan
            .scripts
            .iter()
            .find(|s| s.path.ends_with(name))
            .unwrap_or_else(|| panic!("缺维护者脚本 {name}"));
        assert!(
            script.contents.contains(marker),
            "{name} 缺分支标记 {marker:?}"
        );
    }
    let lines: Vec<String> = plan.commands.iter().map(|c| c.line()).collect();
    assert_eq!(lines.len(), 3, "命令序列：{lines:?}");
    assert!(
        lines[0].starts_with("dpkg-deb --build -Zgzip --root-owner-group "),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].ends_with(&format!("pax-notes_1.2.3_{arch}.deb")),
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
        artifact
            .path
            .ends_with(format!("pax-notes_1.2.3_{arch}.deb")),
        "产物路径：{}",
        artifact.path.display()
    );
    let evidence = artifact.notes.join("\n");
    assert!(
        evidence.contains(&format!(
            "dpkg-deb --info: Package: pax-notes, Version: 1.2.3, Architecture: {arch}"
        )),
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
        "./usr/share/metainfo/com.polypack.paxnotes.metainfo.xml",
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
    // 纯 Rust 结构面（verify 腿 lintian 的本地兜底）与真实 listing 同源。
    let structure = polypack_bundle::check_deb_structure(&artifact.path).expect("deb 结构检查");
    assert_eq!(structure.package, "pax-notes");
    assert_eq!(structure.version, "1.2.3");
    assert_eq!(structure.architecture, arch);
    assert_eq!(structure.depends, DEB_RUNTIME_DEPENDS);
    assert!(
        structure
            .entries
            .iter()
            .any(|entry| entry.contains("com.polypack.paxnotes.metainfo.xml")),
        "结构面缺 metainfo：{:?}",
        structure.entries
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

/// T-M1-07 lintian 真跑（与 `polypack verify` 同口径 `--fail-on error`）。
///
/// Maintainer 邮箱、changelog、libc6 依赖与描述兜底均已修，CI 上必须零 error。
#[test]
fn deb_lintian_real_run_is_clean() {
    if common::skip_without_any(&["dpkg-deb", "lintian"]) {
        return;
    }
    let base = scratch("lintian");
    let inputs = inputs(&base);
    let artifact = Deb.package(&inputs, &spec("deb")).expect("deb 组装");

    let output = Command::new("lintian")
        .args(["--fail-on", "error", &artifact.path.display().to_string()])
        .output()
        .expect("lintian 真跑");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "lintian --fail-on error 必须零 error：\n{text}"
    );
}

/// T-M1-07 安装验收：passwordless sudo 的 root 宿主上真跑 `dpkg -i` → 启动 → `dpkg -r`。
///
/// 本地无 sudo 时显式 skip；CI ubuntu-latest 提供 passwordless sudo。测试用
/// `--force-depends` 安装，因为夹具核心是 shell 脚本、不依赖 WebView 运行库。
#[test]
fn deb_installs_runs_and_uninstalls_on_root_host() {
    if common::skip_without_any(&["dpkg"]) {
        return;
    }
    if !Command::new("sudo")
        .args(["-n", "true"])
        .status()
        .is_ok_and(|status| status.success())
    {
        eprintln!("SKIP：无 passwordless sudo，跳过 dpkg -i 安装/卸载冒烟");
        return;
    }
    let base = scratch("dpkg-install");
    let inputs = inputs(&base);
    let artifact = Deb.package(&inputs, &spec("deb")).expect("deb 组装");

    let install = Command::new("sudo")
        .args([
            "dpkg",
            "-i",
            "--force-depends",
            &artifact.path.display().to_string(),
        ])
        .status()
        .expect("dpkg -i");
    let smoke = Command::new("pax-notes").arg("--version").output();
    let remove = Command::new("sudo")
        .args(["dpkg", "-r", "pax-notes"])
        .status();

    assert!(install.success(), "dpkg -i 失败");
    let smoke = smoke.expect("启动已安装的可执行");
    assert!(
        smoke.status.success(),
        "安装后启动失败：{}",
        String::from_utf8_lossy(&smoke.stderr)
    );
    assert!(
        String::from_utf8_lossy(&smoke.stdout).contains("pax-notes 1.2.3"),
        "启动输出不符：{}",
        String::from_utf8_lossy(&smoke.stdout)
    );
    assert!(remove.expect("dpkg -r").success(), "dpkg -r 卸载失败");
}

#[test]
fn appimage_builds_single_file_or_labeled_appdir_that_runs() {
    // AppDir 需要根级 AppRun 符号链接，且冒烟要执行 shell 夹具核心。
    if common::skip_unless_unix("AppDir 符号链接与 shell 夹具") || common::skip_without("sh")
    {
        return;
    }
    let base = scratch("appimage");
    let inputs = inputs(&base);
    add_arch_stub(&inputs);
    let plan = AppImage.plan(&inputs, &spec("appimage")).expect("计划");
    assert!(
        plan.commands[0].line().starts_with("appimagetool "),
        "命令组装：{}",
        plan.commands[0].line()
    );
    assert!(
        !plan.commands[0].line().contains("--no-appstream"),
        "AppStream 元数据须交给 appimagetool 校验：{}",
        plan.commands[0].line()
    );
    if !common::has("appimagetool") {
        assert!(
            plan.skipped.iter().any(|s| s.tool == "appimagetool"),
            "缺工具须如实记录：{:?}",
            plan.skipped
        );
    }

    let artifact = AppImage
        .package(&inputs, &spec("appimage"))
        .expect("AppDir");
    if artifact.format == "appimage" {
        // 装了 appimagetool 的真跑路径（CI ubuntu 腿安装 1.9.1）：单文件产物。
        assert_eq!(artifact.kind, polypack_bundle::ArtifactKind::File);
        assert_eq!(
            artifact.path.extension().and_then(|ext| ext.to_str()),
            Some("AppImage"),
            "产物路径：{}",
            artifact.path.display()
        );
        let bytes = std::fs::read(&artifact.path).expect("读 AppImage");
        assert!(
            artifact.bytes > 0 && artifact.bytes == bytes.len() as u64,
            "产物大小：{}（实读 {}）",
            artifact.bytes,
            bytes.len()
        );
        // AppImage = ELF runtime + 偏移 8 处的 `AI` 魔数（appimagetool 产 type 2）。
        assert_eq!(
            &bytes[..4],
            b"\x7fELF",
            "AppImage 须为 ELF：{}",
            artifact.path.display()
        );
        assert_eq!(&bytes[8..10], b"AI", "缺 AppImage 魔数");
        assert!(
            matches!(bytes[10], 1 | 2),
            "未知 AppImage 类型：{}",
            bytes[10]
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert!(
                std::fs::metadata(&artifact.path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o111
                    != 0,
                "AppImage 缺可执行位：{}",
                artifact.path.display()
            );
        }
        // 启动冒烟：extract-and-run 不依赖 FUSE（CI 缺 libfuse2 时同口径）。
        let smoke = Command::new(&artifact.path)
            .arg("--version")
            .env("APPIMAGE_EXTRACT_AND_RUN", "1")
            .output()
            .expect("执行 AppImage");
        assert!(
            smoke.status.success(),
            "AppImage 启动失败：{}",
            String::from_utf8_lossy(&smoke.stderr)
        );
        assert!(
            String::from_utf8_lossy(&smoke.stdout).contains("pax-notes 1.2.3"),
            "AppImage 启动输出：{}",
            String::from_utf8_lossy(&smoke.stdout)
        );
        return;
    }
    assert_eq!(artifact.format, "appdir", "未打包必须如实标注格式");
    assert_eq!(artifact.kind, polypack_bundle::ArtifactKind::Directory);
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
    // T-M5-04：AppStream 元数据随 AppDir 分发，且生成器与结构检查同源。
    let metainfo = appdir.join("usr/share/metainfo/com.polypack.paxnotes.metainfo.xml");
    assert!(metainfo.is_file(), "AppDir 缺 AppStream 元数据");
    let structure = polypack_bundle::check_appdir_structure(appdir).expect("AppDir 结构检查");
    assert_eq!(structure.desktop, "pax-notes.desktop");
    assert!(structure.metainfo.ends_with(".metainfo.xml"));
    assert!(structure.icons >= 1, "hicolor 图标缺失");

    // 真跑：直接执行 AppDir 的 AppRun（符号链接 → usr/bin，资源按 ../lib 解析）。
    let smoke = run(&appdir.join("AppRun"));
    assert!(
        smoke.status.success(),
        "AppRun 冒烟失败：{}",
        String::from_utf8_lossy(&smoke.stderr)
    );
}

#[test]
fn rpm_plan_declares_requires_and_package_builds_or_degrades_honestly() {
    let base = scratch("rpm");
    let inputs = inputs(&base);
    let plan = Rpm.plan(&inputs, &spec("rpm")).expect("rpm 计划");

    // spec 契约：Fedora 运行时依赖、预布置载荷、desktop / AppStream 入 %files。
    let spec_script = plan
        .scripts
        .iter()
        .find(|s| s.path.ends_with("SPECS/pax-notes.spec"))
        .expect("rpm spec 脚本");
    for want in [
        "Name: pax-notes\n",
        "Version: 1.2.3\n",
        "Requires: webkit2gtk4.1, gtk3, libsoup3\n",
        "cp -a \"%{_sourcedir}/payload/.\" \"%{buildroot}/\"\n",
        "/usr/bin/pax-notes\n",
        "\"/usr/lib/Pax Notes\"\n",
        "/usr/share/applications/pax-notes.desktop\n",
        "/usr/share/metainfo/com.polypack.paxnotes.metainfo.xml\n",
        "/usr/share/doc/pax-notes/RUNTIME-DEPENDENCIES.txt\n",
        "%changelog\n* Mon Jan 01 2024",
    ] {
        assert!(spec_script.contents.contains(want), "spec 缺 {want:?}");
    }

    let artifact = Rpm.package(&inputs, &spec("rpm")).expect("rpm 组装");
    assert_eq!(artifact.target, "linux");
    match artifact.format.as_str() {
        // PATH 或无缓存工具链时走真构建。
        "rpm" => {
            let expected = base
                .join("out")
                .join(format!("pax-notes-1.2.3-1.{}.rpm", rpm_arch()));
            assert_eq!(artifact.path, expected, "产物名须与打包器同口径推导");
            let structure = polypack_bundle::check_rpm_structure(&artifact.path).expect("rpm 头部解析");
            assert_eq!(structure.name, "pax-notes");
            assert_eq!(structure.version, "1.2.3");
            assert_eq!(structure.release, "1");
            assert_eq!(structure.arch, rpm_arch());
            assert_eq!(structure.license, "Apache-2.0");
            for suffix in [
                "/bin/pax-notes",
                "/share/applications/pax-notes.desktop",
                "/share/metainfo/com.polypack.paxnotes.metainfo.xml",
            ] {
                assert!(
                    structure.files.iter().any(|path| path.ends_with(suffix)),
                    "头部文件列表缺 {suffix}：{:?}",
                    structure.files
                );
            }
            assert!(
                structure
                    .files
                    .iter()
                    .any(|path| path.starts_with("/usr/lib/")),
                "头部文件列表缺 /usr/lib/：{:?}",
                structure.files
            );
            assert!(artifact.notes.iter().any(|n| n.contains("结构检查")));
        }
        // 工具链缺席：如实降级为载荷目录，不伪造 .rpm。
        "rpm-payload" => {
            assert_eq!(artifact.kind, polypack_bundle::ArtifactKind::Directory);
            assert!(
                artifact
                    .notes
                    .iter()
                    .any(|n| n.contains("未打包：rpmbuild"))
            );
            assert!(
                artifact
                    .path
                    .join("usr/share/metainfo/com.polypack.paxnotes.metainfo.xml")
                    .is_file()
            );
            assert!(artifact.path.join("usr/bin/pax-notes").is_file());
        }
        other => panic!("意外的 rpm 降级格式：{other}"),
    }
}

#[test]
fn flatpak_declares_minimal_sandbox_and_stage_structure_holds() {
    let base = scratch("flatpak");
    let inputs = inputs(&base);
    let plan = Flatpak
        .plan(&inputs, &spec("flatpak"))
        .expect("Flatpak 计划");
    let manifest_script = plan
        .scripts
        .iter()
        .find(|s| s.path.ends_with("com.polypack.paxnotes.json"))
        .expect("Flatpak manifest");
    for want in [
        "\"app-id\": \"com.polypack.paxnotes\"",
        "\"runtime\": \"org.gnome.Platform\"",
        "\"runtime-version\": \"47\"",
        "\"command\": \"pax-notes\"",
        "\"buildsystem\": \"simple\"",
        "\"type\": \"dir\"",
    ] {
        assert!(
            manifest_script.contents.contains(want),
            "manifest 缺 {want:?}"
        );
    }
    for arg in FLATPAK_FINISH_ARGS {
        assert!(
            manifest_script.contents.contains(&format!("\"{arg}\"")),
            "manifest 缺权限 {arg}"
        );
    }
    // 最小沙箱：不申请 home / 网络 / 系统总线。
    assert!(!manifest_script.contents.contains("--filesystem=home"));
    assert!(!manifest_script.contents.contains("--share=network"));
    assert!(!manifest_script.contents.contains("--system-talk-name"));

    let artifact = Flatpak
        .package(&inputs, &spec("flatpak"))
        .expect("Flatpak 组装");
    assert_eq!(artifact.target, "linux");
    if artifact.format == "flatpak" {
        assert!(
            artifact
                .path
                .extension()
                .is_some_and(|ext| ext == "flatpak")
        );
        assert!(artifact.notes.iter().any(|n| n.contains("结构检查")));
    } else {
        assert_eq!(artifact.format, "flatpak-stage", "未打包必须如实标注格式");
        assert_eq!(artifact.kind, polypack_bundle::ArtifactKind::Directory);
        assert!(
            artifact
                .notes
                .iter()
                .any(|n| n.contains("未打包") && n.contains("flatpak-builder"))
        );
        let structure =
            polypack_bundle::check_flatpak_structure(&artifact.path).expect("Flatpak 结构检查");
        assert_eq!(structure.app_id, "com.polypack.paxnotes");
        assert_eq!(structure.runtime, "org.gnome.Platform//47");
        assert_eq!(structure.command, "pax-notes");
        assert_eq!(
            structure.finish_args.into_iter().collect::<BTreeSet<_>>(),
            FLATPAK_FINISH_ARGS
                .iter()
                .map(|arg| arg.to_string())
                .collect::<BTreeSet<_>>(),
            "权限集合必须恰为打包器常量"
        );
        for suffix in [
            "files/bin/pax-notes",
            "files/share/applications/pax-notes.desktop",
            "files/share/metainfo/com.polypack.paxnotes.metainfo.xml",
        ] {
            assert!(
                structure.files.iter().any(|path| path.ends_with(suffix)),
                "载荷缺 {suffix}：{:?}",
                structure.files
            );
        }
    }
}

#[test]
fn snap_declares_strict_confinement_and_stage_structure_holds() {
    let base = scratch("snap");
    let inputs = inputs(&base);
    let plan = Snap.plan(&inputs, &spec("snap")).expect("Snap 计划");
    let yaml_script = plan
        .scripts
        .iter()
        .find(|s| s.path.ends_with("snapcraft.yaml"))
        .expect("snapcraft.yaml");
    for want in [
        "name: pax-notes\n",
        "base: core24\n",
        "version: '1.2.3'\n",
        "confinement: strict\n",
        "grade: stable\n",
        "plugin: dump\n",
        "source: files\n",
        "command: bin/pax-notes\n",
        "      - gnome\n",
    ] {
        assert!(yaml_script.contents.contains(want), "YAML 缺 {want:?}");
    }
    for plug in SNAP_PLUGS {
        assert!(
            yaml_script.contents.contains(&format!("      - {plug}\n")),
            "YAML 缺 plug {plug}"
        );
    }
    assert!(!yaml_script.contents.contains("confinement: classic"));
    assert!(!yaml_script.contents.contains("confinement: devmode"));

    let artifact = Snap.package(&inputs, &spec("snap")).expect("Snap 组装");
    assert_eq!(artifact.target, "linux");
    if artifact.format == "snap" {
        assert!(artifact.path.extension().is_some_and(|ext| ext == "snap"));
        assert!(artifact.notes.iter().any(|n| n.contains("结构检查")));
    } else {
        assert_eq!(artifact.format, "snap-stage", "未打包必须如实标注格式");
        assert_eq!(artifact.kind, polypack_bundle::ArtifactKind::Directory);
        assert!(
            artifact
                .notes
                .iter()
                .any(|n| n.contains("未打包") && n.contains("snapcraft"))
        );
        let structure = polypack_bundle::check_snap_structure(&artifact.path).expect("Snap 结构检查");
        assert_eq!(structure.name, "pax-notes");
        assert_eq!(structure.base, "core24");
        assert_eq!(structure.confinement, "strict");
        assert_eq!(
            structure.plugs.into_iter().collect::<BTreeSet<_>>(),
            SNAP_PLUGS
                .iter()
                .map(|plug| plug.to_string())
                .collect::<BTreeSet<_>>(),
            "plug 集合必须恰为打包器常量"
        );
        for suffix in [
            "files/bin/pax-notes",
            "files/share/applications/pax-notes.desktop",
            "files/share/metainfo/com.polypack.paxnotes.metainfo.xml",
        ] {
            assert!(
                structure.files.iter().any(|path| path.ends_with(suffix)),
                "载荷缺 {suffix}：{:?}",
                structure.files
            );
        }
        // 店端元数据：snapcraft.yaml 同级、以 snap 名命名。
        assert!(artifact.path.join("pax-notes.metainfo.xml").is_file());
    }
}

/// L5 发行版目标共用四个 Linux 打包器；notes 的权限清单必须与打包器常量同源。
#[test]
fn l5_targets_share_linux_packagers_and_notes_match_permissions() {
    let registry = polypack_targets::builtin::builtin();
    let target = |id: &str| {
        registry
            .iter()
            .find(|spec| spec.id == id)
            .unwrap_or_else(|| panic!("内建目标缺 {id}"))
    };
    for (id, format) in [("ubuntu", "deb"), ("fedora", "rpm"), ("mint", "deb")] {
        let spec = target(id);
        assert_eq!(spec.package.format, format, "{id} 主档格式");
        let packager =
            polypack_bundle::packager::for_spec(spec).unwrap_or_else(|| panic!("{id} 无打包器"));
        assert_eq!(packager.id(), "linux", "{id} 应复用 Linux 打包器");
        assert_eq!(packager.format(), format);
        assert!(packager.serves(id), "{id} 分发目标应被 serves 接受");
    }

    // serves 边界：deb 系不吃 fedora；rpm 系不吃 ubuntu/mint；Snap 只在 Ubuntu 目标声明。
    assert!(Deb.serves("ubuntu") && Deb.serves("mint") && !Deb.serves("fedora"));
    assert!(Rpm.serves("fedora") && !Rpm.serves("ubuntu") && !Rpm.serves("mint"));
    for id in ["linux", "ubuntu", "fedora", "mint"] {
        assert!(Flatpak.serves(id), "Flatpak 应服务 {id}");
    }
    assert!(Snap.serves("linux") && Snap.serves("ubuntu") && !Snap.serves("fedora"));

    let flatpak_args = format!("[{}]", FLATPAK_FINISH_ARGS.join(", "));
    let snap_plugs = format!("[{}]", SNAP_PLUGS.join(", "));
    for id in ["ubuntu", "fedora", "mint"] {
        let notes = target(id).notes.as_deref().unwrap_or("");
        assert!(
            notes.contains(&flatpak_args),
            "{id} notes 的 Flatpak 权限与常量不同源：{notes}"
        );
    }
    let ubuntu_notes = target("ubuntu").notes.as_deref().unwrap_or("");
    assert!(
        ubuntu_notes.contains(&snap_plugs),
        "ubuntu notes 的 Snap plugs 与常量不同源：{ubuntu_notes}"
    );
    for id in ["fedora", "mint"] {
        let notes = target(id).notes.as_deref().unwrap_or("");
        assert!(
            !notes.contains(&snap_plugs),
            "{id} 未声明 Snap 发布面，notes 不应给出 plugs"
        );
    }
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
    let cases = [
        (
            Deb.id(),
            Deb.package(&inputs, &spec("appimage")).unwrap_err(),
        ),
        (
            AppImage.id(),
            AppImage.package(&inputs, &spec("deb")).unwrap_err(),
        ),
        (Rpm.id(), Rpm.package(&inputs, &spec("deb")).unwrap_err()),
        (
            Flatpak.id(),
            Flatpak.package(&inputs, &spec("rpm")).unwrap_err(),
        ),
        (Snap.id(), Snap.package(&inputs, &spec("deb")).unwrap_err()),
    ];
    for (packager, err) in cases {
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
fn registry_exposes_all_linux_packagers() {
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
        ("linux", "rpm"),
        ("linux", "flatpak"),
        ("linux", "snap"),
        ("web", "static-site"),
    ] {
        assert!(
            all.contains(&(want.0.to_string(), want.1.to_string())),
            "缺 {want:?}：{all:?}"
        );
    }
    assert!(find("linux", "deb").is_some());
    assert!(find("ubuntu", "deb").is_some(), "L5 ubuntu 目标应分派 Deb");
    assert!(find("fedora", "rpm").is_some(), "L5 fedora 目标应分派 Rpm");
    assert!(
        find("mint", "flatpak").is_some(),
        "L5 mint 目标应可产 Flatpak"
    );
    assert!(
        find("ubuntu", "snap").is_some(),
        "L5 ubuntu 目标应可产 Snap"
    );
    assert!(find("fedora", "snap").is_none(), "Fedora 目标未声明 Snap");
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
        Rpm.package(&inputs, &spec("rpm")).unwrap_err(),
        Flatpak.package(&inputs, &spec("flatpak")).unwrap_err(),
        Snap.package(&inputs, &spec("snap")).unwrap_err(),
    ] {
        assert!(matches!(
            err,
            BundleError::MissingInput { field: "bare", .. }
        ));
        assert_eq!(err.code(), codes::E_BND_0002);
    }
}
