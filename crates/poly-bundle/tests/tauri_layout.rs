//! T-M1-03 契约测试：Tauri 裸产物布局、WebView 资源注入与宿主产物结构。
//!
//! Linux 段真跑（产出裸产物并执行 `<exe> --version` 冒烟）；Windows / macOS 段在本
//! 宿主上真实产出布局与资源注入（布局生成与宿主参数无关），但**执行**冒烟真机阻塞
//! ——测试只做结构断言，不冒充真机执行。

mod common;

use std::path::{Path, PathBuf};

use poly_bundle::error::{BundleError, codes};
use poly_bundle::inputs::{BundleInputs, Icon, IconKind, ProductInfo};
use poly_bundle::tauri;

/// 假核心可执行：按 tauri-utils `resource_dir_from` 的 Linux 规则解析资源目录，
/// `--version` 打印版本并校验资源就位（真实 Tauri 二进制的行为代理）。
const FAKE_CORE: &str = r#"#!/bin/sh
# PolyPack 测试夹具：模拟 Tauri 核心可执行（Linux 资源解析规则同 tauri-utils）。
self=$(readlink -f -- "$0" 2>/dev/null || echo "$0")
self_dir=$(dirname -- "$self")
resources="$self_dir/../lib/Pax Notes"
case "$1" in
  --version)
    printf 'pax-notes %s\n' "1.2.3"
    if [ -f "$resources/index.html" ] && [ -f "$resources/assets/app.js" ]; then
      echo "resources: ok ($resources)"
    else
      echo "resources: MISSING ($resources)" >&2
      exit 1
    fi
    ;;
  *) echo "usage: $0 --version" >&2; exit 2 ;;
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

/// 每个测试一个独立目录（并行安全）。
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "poly-tauri-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

/// 造输入：假核心 + 前端 dist（含子目录）。
fn inputs(base: &Path) -> BundleInputs {
    let core = base.join("pax-notes");
    std::fs::write(&core, FAKE_CORE).expect("写夹具核心");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&core, std::fs::Permissions::from_mode(0o755)).expect("置执行位");
    }
    let dist = base.join("dist");
    std::fs::create_dir_all(dist.join("assets")).expect("建 dist");
    std::fs::write(dist.join("index.html"), "<!doctype html><body>pax</body>").unwrap();
    std::fs::write(dist.join("assets/app.js"), "console.log('pax')").unwrap();

    let mut inputs = BundleInputs::new(product(), base.join("out"));
    inputs.core_binary = Some(core);
    inputs.ui_dist = Some(dist);
    inputs
}

fn png_icon(base: &Path) -> Icon {
    let path = base.join("pax-256.png");
    std::fs::write(&path, b"\x89PNG\r\n\x1a\n fake").unwrap();
    Icon {
        path,
        kind: IconKind::Png,
        size: Some(256),
    }
}

/// 最小 icns 样本（`icns` 魔数 + 长度 + 一条哑负载；布局只校验输入存在，不解析格式）。
fn icns_icon(base: &Path) -> Icon {
    let path = base.join("icon.icns");
    std::fs::write(&path, b"icns\x00\x00\x00\x10PAX \x00\x00\x00\x00").unwrap();
    Icon {
        path,
        kind: IconKind::Icns,
        size: None,
    }
}

fn run(exe: &Path) -> std::process::Output {
    std::process::Command::new(exe)
        .arg("--version")
        .output()
        .expect("执行裸产物")
}

#[test]
fn linux_bare_product_layout_and_version_smoke() {
    // 冒烟需执行 shell 夹具核心（`--version` 走 POSIX shell）。
    if common::skip_unless_unix("shell 夹具核心的执行冒烟") || common::skip_without("sh") {
        return;
    }
    let base = scratch("linux");
    let bare = tauri::bare_product(&inputs(&base), poly_targets::Host::Linux).expect("裸产物");
    let stdout = run(&bare.executable);
    let out = String::from_utf8_lossy(&stdout.stdout).to_string();
    assert!(
        stdout.status.success(),
        "`--version` 冒烟失败：{out}{}",
        String::from_utf8_lossy(&stdout.stderr)
    );
    assert!(out.contains("pax-notes 1.2.3"), "版本输出：{out}");
    assert!(
        out.contains("resources: ok"),
        "WebView 资源未按 tauri-utils 规则就位：{out}"
    );

    let root = &bare.root;
    assert!(root.join("usr/bin/pax-notes").is_file());
    assert_eq!(bare.executable, root.join("usr/bin/pax-notes"));
    // 资源目录 = tauri-utils `resource_dir_from` 的 `../lib/<产品名>` 分支。
    assert_eq!(
        tauri::linux_resource_dir(root, &product()),
        root.join("usr/lib/Pax Notes")
    );
    assert!(root.join("usr/lib/Pax Notes/index.html").is_file());
    assert!(root.join("usr/lib/Pax Notes/assets/app.js").is_file());
    assert!(
        root.join("usr/share/applications/pax-notes.desktop")
            .is_file()
    );
    assert!(
        root.join("usr/share/doc/pax-notes/RUNTIME-DEPENDENCIES.txt")
            .is_file()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&bare.executable)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "可执行位缺失");
    }
}

#[test]
fn linux_layout_is_appdir_ready() {
    let base = scratch("appdir-ready");
    let mut inputs = inputs(&base);
    inputs.icons.push(png_icon(&base));
    let bare = tauri::bare_product(&inputs, poly_targets::Host::Linux).expect("裸产物");
    // AppDir 三要素（T-M1-07 的输入契约）：usr/bin、usr/lib 资源、hicolor 图标。
    assert!(bare.root.join("usr/bin/pax-notes").is_file());
    assert!(bare.root.join("usr/lib/Pax Notes").is_dir());
    assert!(
        bare.root
            .join("usr/share/icons/hicolor/256x256/apps/pax-notes.png")
            .is_file()
    );
    let deps = std::fs::read_to_string(
        bare.root
            .join("usr/share/doc/pax-notes/RUNTIME-DEPENDENCIES.txt"),
    )
    .expect("产物说明");
    assert!(deps.contains("webkit2gtk-4.1"), "Linux 运行时依赖：{deps}");
}

#[test]
fn windows_layout_puts_resources_next_to_exe() {
    let base = scratch("windows");
    let bare = tauri::bare_product(&inputs(&base), poly_targets::Host::Win).expect("裸产物");
    assert!(bare.root.join("pax-notes.exe").is_file());
    assert_eq!(bare.executable, bare.root.join("pax-notes.exe"));
    // tauri-utils：Windows 的 resource_dir() 恒为可执行所在目录。
    assert!(bare.root.join("index.html").is_file());
    assert!(bare.root.join("assets/app.js").is_file());
    let deps =
        std::fs::read_to_string(bare.root.join("RUNTIME-DEPENDENCIES.txt")).expect("产物说明");
    assert!(
        deps.contains("WebView2"),
        "WebView2 依赖须写入产物说明：{deps}"
    );
}

#[test]
fn macos_layout_builds_app_bundle() {
    let base = scratch("macos");
    let mut inputs = inputs(&base);
    inputs.icons.push(icns_icon(&base));
    let bare = tauri::bare_product(&inputs, poly_targets::Host::Macos).expect("裸产物");
    let app = bare.root.join("Pax Notes.app");
    assert!(app.join("Contents/MacOS/pax-notes").is_file());
    assert!(app.join("Contents/Resources/index.html").is_file());
    assert!(
        app.join("Contents/Resources/RUNTIME-DEPENDENCIES.txt")
            .is_file()
    );
    // 金样要求 `*.app/Contents/Resources/*.icns` 严格存在（.app 不允许静默无图标）。
    assert!(app.join("Contents/Resources/icon.icns").is_file());
    let deps = std::fs::read_to_string(app.join("Contents/Resources/RUNTIME-DEPENDENCIES.txt"))
        .expect("产物说明");
    assert!(
        deps.contains("iconutil"),
        "产物说明须写明 icns 构建前置：{deps}"
    );
    assert_eq!(bare.executable, app.join("Contents/MacOS/pax-notes"));
    let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).expect("Info.plist");
    assert!(plist.contains("<key>CFBundleIdentifier</key><string>com.polypack.paxnotes</string>"));
    assert_eq!(
        std::fs::read_to_string(app.join("Contents/PkgInfo")).unwrap(),
        "APPL????"
    );
}

/// macOS 缺 icns 即响亮拒绝（Linux / Windows 布局不受此要求约束）。
#[test]
fn macos_layout_requires_icns_icon() {
    let base = scratch("macos-no-icns");
    let inputs = inputs(&base);
    assert!(
        tauri::bare_product(&inputs, poly_targets::Host::Linux).is_ok(),
        "Linux 布局不应要求 icns"
    );
    assert!(
        tauri::bare_product(&inputs, poly_targets::Host::Win).is_ok(),
        "Windows 布局不应要求 icns"
    );
    let err = tauri::bare_product(&inputs, poly_targets::Host::Macos).expect_err("缺 icns 须报错");
    assert!(matches!(
        err,
        BundleError::MissingInput {
            field: "icons.icns",
            ..
        }
    ));
    assert_eq!(err.code(), codes::E_BND_0002);
    assert!(
        err.hint().unwrap_or_default().contains("iconutil"),
        "修复提示须指向 iconutil 派生：{:?}",
        err.hint()
    );
}

#[test]
fn missing_core_binary_is_reported() {
    let base = scratch("missing-core");
    let mut inputs = BundleInputs::new(product(), base.join("out"));
    inputs.ui_dist = Some(base.join("nope"));
    let err = tauri::bare_product(&inputs, poly_targets::Host::Linux).expect_err("缺核心须报错");
    assert!(matches!(
        err,
        BundleError::MissingInput {
            field: "core_binary",
            ..
        }
    ));
    assert_eq!(err.code(), codes::E_BND_0002);
}

#[test]
fn injected_dist_must_exist() {
    let base = scratch("no-dist");
    let mut inputs = inputs(&base);
    inputs.ui_dist = Some(base.join("absent"));
    let err = tauri::bare_product(&inputs, poly_targets::Host::Linux).expect_err("缺 dist 须报错");
    assert!(matches!(
        err,
        BundleError::MissingInput {
            field: "ui_dist",
            ..
        }
    ));
}

#[test]
fn injection_is_idempotent_over_existing_tree() {
    let base = scratch("reinject");
    let inputs = inputs(&base);
    let first = tauri::bare_product(&inputs, poly_targets::Host::Linux).expect("首次");
    let second = tauri::bare_product(&inputs, poly_targets::Host::Linux).expect("重跑");
    assert_eq!(first.root, second.root);
    assert!(second.root.join("usr/lib/Pax Notes/index.html").is_file());
}

/// 真实 Tauri 二进体验收：`POLY_TAURI_CORE_BIN=<tauri 应用可执行>` 时对**真实核心**
/// 走一遍裸产物布局，并用其 `resource_dir()` 解析结果验证注入位置。
///
/// 未设置该变量即跳过（不冒充真机执行）；核心的 `productName` 必须等于
/// `BundleInputs.product.name`（Tauri 用产品名解析 `../lib/<产品名>`）。
#[test]
fn real_tauri_core_binary_resolves_injected_resources() {
    let Ok(core) = std::env::var("POLY_TAURI_CORE_BIN") else {
        eprintln!("跳过：未设置 POLY_TAURI_CORE_BIN（真实 Tauri 核心二进制）");
        return;
    };
    let base = scratch("real-tauri");
    let mut inputs = inputs(&base);
    inputs.product.name = "Pax Probe".to_string();
    inputs.core_binary = Some(PathBuf::from(core));
    let bare = tauri::bare_product(&inputs, poly_targets::Host::Linux).expect("裸产物");
    let smoke = run(&bare.executable);
    let out = String::from_utf8_lossy(&smoke.stdout).to_string();
    assert!(smoke.status.success(), "真实核心 --version 冒烟失败：{out}");
    assert!(
        out.contains("exists=true"),
        "真实 Tauri resource_dir() 未解析到注入资源：{out}"
    );
}
