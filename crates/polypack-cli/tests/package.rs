//! T-M1-05..08 / T-M1-13（CLI 侧）：`package` 阶段接线契约。
//!
//! `polypack build` 在 compile / bundle 之后按**目标注册表声明的格式**调 polypack-bundle 打包器
//! （`[targets.*]` 只声明平台参数，格式以注册表为准）：成功发 `Packaged{format,bytes}`，
//! 环境性缺失（宿主无打包工具 / 派生图标未就绪）如实告警并跳过本目标；产物随构建快照落
//! `.polypack/artifacts/<target>/build-<millis>/packages/`。金样比对与四端深检见 tests/contract_m1.rs。
//!
//! 整份文件是 **Linux 宿主专属**：全部用例都 `polypack build --target linux`，而显式目标在非 Linux
//! 宿主上是结构性不可达（`Registry::resolve_all` 返回 `Unreachable`，见 build.rs 只喂
//! `current_host()`），加上 deb / AppDir / hicolor 断言本身也只对 Linux 打包器成立——故整文件
//! 一次性 gate 掉，省得每个用例各带一条 `cfg` 又在别处漏掉辅助函数的 gate（死代码）。
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 1x1 PNG 的合法字节：手工图标替身（边长取文件名，只测「放置优先于派生」这条分支）。
const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xFC, 0xCF, 0xC0, 0x50,
    0x0F, 0x00, 0x04, 0x85, 0x01, 0x80, 0x84, 0xA9, 0x8C, 0x21, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

// ---------------------------------------------------------------- 测试支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时工程目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可创建");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// 运行 `polypack` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_polypack`）。
fn polypack(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_polypack"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("polypack 可执行")
}

/// 按 JSON Lines 解析 stdout（逐行必须合法 JSON）。
fn json_lines(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("stdout 每行应为合法 JSON"))
        .collect()
}

/// `polypack init` 模板工程（linux 目标声明格式 = 注册表默认 `appimage`）。
fn init_project(tag: &str) -> TempDir {
    let temp = TempDir::new(tag);
    let out = polypack(&["init", "."], temp.path());
    assert!(
        out.status.success(),
        "polypack init 失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    temp
}

/// 本次构建的产物快照（`.polypack/artifacts/linux/build-<millis>/`，取最新一份）。
fn latest_snapshot(root: &Path) -> PathBuf {
    let target_dir = root.join(".polypack").join("artifacts").join("linux");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&target_dir)
        .unwrap_or_else(|err| panic!("产物快照目录应存在（{}）: {err}", target_dir.display()))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs.pop().expect("至少一份快照")
}

/// 宿主是否可执行该工具（spawn 成功即算有）。
fn tool_present(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
}

/// 目录树内是否有该相对路径。
fn contains(dir: &Path, relative: &str) -> bool {
    dir.join(relative).exists()
}

// ---------------------------------------------------------------- 契约

/// 只有 Web 位图图标、没有可派生源图（无 `icons/`、无 `ui/public/icon.svg`）时：如实告警、
/// 跳过桌面打包，**不出半成品包**（Web 目标另有 PWA manifest 的位图，不受影响）。
#[test]
fn packaging_skips_without_derivable_icon_and_reports_why() {
    let temp = init_project("pkg-skip");
    let root = temp.path();
    // 换成「只有位图」的图标集：PWA manifest 认 `dist/icons/*.png`，桌面打包器要 256 PNG 源
    std::fs::remove_file(root.join("ui/public/icon.svg")).expect("删掉项目图标 svg");
    let bitmaps = root.join("ui/public/icons");
    std::fs::create_dir_all(&bitmaps).expect("位图图标目录可建");
    std::fs::write(bitmaps.join("icon-192.png"), PNG_1X1).expect("位图图标可写");

    let out = polypack(&["build", "--target", "linux", "--json"], root);
    assert!(
        out.status.success(),
        "缺可派生图标不得让构建失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let events = json_lines(&out);

    let warning = events
        .iter()
        .find(|event| event["event"] == "warning" && event["stage"] == "package")
        .expect("应发出 package 告警");
    let message = warning["message"].as_str().expect("告警文本");
    assert!(
        message.contains("图标") && message.contains("icon.svg"),
        "告警应说明缺图标与源图路径: {message}"
    );

    assert!(
        events.iter().all(|event| event["event"] != "packaged"),
        "无图标时不得产出安装包事件"
    );
    assert!(
        !latest_snapshot(root).join("packages").exists(),
        "跳过打包时快照内不得有 packages 目录"
    );
    assert!(
        !root
            .join(".polypack")
            .join("packages")
            .join("linux")
            .exists(),
        "跳过打包时不得建出打包暂存目录"
    );
}

/// PNG 尺寸（IHDR 宽高）：`\x89PNG\r\n\x1a\n` + 长度 + `IHDR` 后为大端 u32 宽、高。
fn png_size(path: &Path) -> (u32, u32) {
    let bytes = std::fs::read(path).unwrap_or_else(|err| panic!("读 {}: {err}", path.display()));
    assert_eq!(
        &bytes[..8],
        b"\x89PNG\r\n\x1a\n",
        "{} 不是 PNG",
        path.display()
    );
    let be = |offset: usize| {
        u32::from_be_bytes(
            bytes
                .get(offset..offset + 4)
                .unwrap_or_else(|| panic!("{} 缺少 IHDR", path.display()))
                .try_into()
                .expect("4 字节"),
        )
    };
    (be(16), be(20))
}

/// 宿主 Linux：真实端到端出 Linux 包（AppDir；宿主有 appimagetool 则为单文件 AppImage）。
///
/// 图标走 T-M1-14 派生管线（模板的 `ui/public/icon.svg` → PNG 集 + ICO），故前置条件是宿主有
/// `rsvg-convert`；尺寸按 IHDR 断言，不看文件名。本测试真编 crate（模板 crate 零依赖）。
#[test]
fn packaging_derives_icons_and_produces_linux_artifact() {
    if !tool_present("rsvg-convert") {
        eprintln!("跳过：宿主无 rsvg-convert（图标派生管线的外部依赖）");
        return;
    }
    let temp = init_project("pkg-linux");
    let root = temp.path();

    let out = polypack(&["build", "--target", "linux", "--json"], root);
    assert!(
        out.status.success(),
        "构建失败: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let events = json_lines(&out);
    let packaged = events
        .iter()
        .find(|event| event["event"] == "packaged")
        .unwrap_or_else(|| panic!("应有 Packaged 事件: {events:?}"));
    assert!(packaged["bytes"].as_u64().expect("产物体积") > 0);

    // 缺 appimagetool（宿主现实）→ 打包器如实降级为 AppDir 目录；有则出 AppImage 单文件
    let format = packaged["format"].as_str().expect("产物格式");
    if tool_present("appimagetool") {
        assert_eq!(format, "appimage");
    } else {
        assert_eq!(format, "appdir", "无 appimagetool 时不得谎报 AppImage");
        // 告警里既有图标派生的说明（如缺 ICNS 工具），也有本条的降级说明——按名字找后者
        assert!(
            events.iter().any(|event| {
                event["event"] == "warning"
                    && event["stage"] == "package"
                    && event["message"]
                        .as_str()
                        .is_some_and(|message| message.contains("appimagetool"))
            }),
            "降级告警应点名缺失工具: {events:?}"
        );
    }

    // 产物随构建快照落盘，且 AppDir 形状与金样一致（`linux.artifact.json` 的 appimage 条目）
    let snapshot = latest_snapshot(root);
    let appdir = snapshot.join("packages").join("polypack-app.AppDir");
    assert!(appdir.is_dir(), "AppDir 应在快照内: {}", appdir.display());
    for entry in [
        "AppRun",
        "polypack-app.desktop",
        "polypack-app.png",
        "usr/bin/polypack-app",
        "usr/share/applications/polypack-app.desktop",
    ] {
        assert!(contains(&appdir, entry), "AppDir 缺少 {entry}");
    }
    // T-M1-14 验收的尺寸断言：包内 256 图标确实是 256×256 的 Pax 派生位图
    let icon = appdir.join("usr/share/icons/hicolor/256x256/apps/polypack-app.png");
    assert!(icon.is_file(), "图标应进 hicolor 目录: {}", icon.display());
    assert_eq!(png_size(&icon), (256, 256), "包内 256 图标尺寸不符");
    for size in [16, 32, 48, 64, 128, 256] {
        let derived = root
            .join(".polypack")
            .join("packages")
            .join("linux")
            .join("icons")
            .join(format!("icon-{size}.png"));
        assert_eq!(
            png_size(&derived),
            (size, size),
            "派生 PNG 尺寸不符: {}",
            derived.display()
        );
    }
    assert!(
        contains(
            &appdir,
            "usr/share/doc/polypack-app/RUNTIME-DEPENDENCIES.txt"
        ),
        "宿主运行时依赖应随产物分发"
    );
    // 暂存目录 = 「最新一份产物」的稳定位置（打包器告警里的路径即它），快照另存副本
    let latest = root
        .join(".polypack")
        .join("packages")
        .join("linux")
        .join("polypack-app.AppDir");
    assert!(
        latest.join("AppRun").exists(),
        "最新产物应留在 {}",
        latest.display()
    );

    // 人读 sink 的封装行（二跑走缓存，避免重复编译）
    let human = polypack(&["build", "--target", "linux"], root);
    assert!(human.status.success());
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(stdout.contains("封装："), "人读输出应有封装行: {stdout}");
}

/// 配置驱动的格式覆盖（§3.4「目标即配置」）：`[targets.linux] format = "deb"` 真的出 `.deb`，
/// 且产物过 `dpkg-deb` 深检（包名 + 包内 256 图标来自派生管线）。
#[test]
fn target_format_override_packages_deb() {
    for tool in ["dpkg-deb", "rsvg-convert"] {
        if !tool_present(tool) {
            eprintln!("跳过：宿主无 {tool}");
            return;
        }
    }
    let temp = init_project("pkg-deb");
    let root = temp.path();
    let config = std::fs::read_to_string(root.join("polypack.toml")).expect("配置可读");
    std::fs::write(
        root.join("polypack.toml"),
        config.replace("format = \"appimage\"", "format = \"deb\""),
    )
    .expect("配置可写");

    let out = polypack(&["build", "--target", "linux", "--json"], root);
    assert!(
        out.status.success(),
        "构建失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let events = json_lines(&out);
    let packaged = events
        .iter()
        .find(|event| event["event"] == "packaged")
        .unwrap_or_else(|| panic!("应有 Packaged 事件: {events:?}"));
    assert_eq!(
        packaged["format"].as_str().expect("格式"),
        "deb",
        "配置覆盖未生效（仍是注册表的 appimage）"
    );

    let staging = root.join(".polypack").join("packages").join("linux");
    let deb = std::fs::read_dir(&staging)
        .expect("打包暂存目录可读")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "deb"))
        .unwrap_or_else(|| panic!("暂存目录应有 .deb: {}", staging.display()));
    let info = Command::new("dpkg-deb")
        .arg("--info")
        .arg(&deb)
        .output()
        .expect("dpkg-deb 可执行");
    assert!(
        info.status.success(),
        "dpkg-deb --info 失败: {}",
        String::from_utf8_lossy(&info.stderr)
    );
    assert!(
        String::from_utf8_lossy(&info.stdout).contains("Package: polypack-app"),
        "包元数据缺包名: {}",
        String::from_utf8_lossy(&info.stdout)
    );
    let contents = Command::new("dpkg-deb")
        .arg("--contents")
        .arg(&deb)
        .output()
        .expect("dpkg-deb 可执行");
    assert!(
        String::from_utf8_lossy(&contents.stdout)
            .contains("usr/share/icons/hicolor/256x256/apps/polypack-app.png"),
        "包内应有派生 256 图标: {}",
        String::from_utf8_lossy(&contents.stdout)
    );
}

/// 图标源回退链的末档：没有 `<root>/icons/`、`ui/public/icon.svg` 也被移走时，改从工程根
/// `icon.svg` 派生（UI 目录之外的落点）；PWA manifest 用另一枚位图，与桌面图标互不影响。
#[test]
fn root_icon_svg_fallback_derives_for_desktop_packaging() {
    if !tool_present("rsvg-convert") {
        eprintln!("跳过：宿主无 rsvg-convert（图标派生管线的外部依赖）");
        return;
    }
    let temp = init_project("pkg-root-icon");
    let root = temp.path();
    // 移走 UI 目录里的图标源，改放工程根；顺带给 PWA manifest 留一枚位图
    std::fs::rename(root.join("ui/public/icon.svg"), root.join("icon.svg"))
        .expect("图标源应可移到工程根");
    let bitmaps = root.join("ui/public/icons");
    std::fs::create_dir_all(&bitmaps).expect("位图图标目录可建");
    std::fs::write(bitmaps.join("icon-192.png"), PNG_1X1).expect("位图图标可写");

    let out = polypack(&["build", "--target", "linux", "--json"], root);
    assert!(
        out.status.success(),
        "构建失败: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let events = json_lines(&out);
    assert!(
        events.iter().any(|event| event["event"] == "packaged"),
        "根图标源应能派生出安装包: {events:?}"
    );
    let derived = root
        .join(".polypack")
        .join("packages")
        .join("linux")
        .join("icons")
        .join("icon-256.png");
    assert_eq!(
        png_size(&derived),
        (256, 256),
        "应是从根 icon.svg 派生的 256 位图: {}",
        derived.display()
    );
}

/// 手工放置的 `<root>/icons/` 优先于派生：放了就不为该项目图标起派生管线
/// （暂存目录里没有派生落点 `icons/`），包内用放置的那枚。
#[test]
fn manual_icons_take_precedence_over_derivation() {
    let temp = init_project("pkg-manual-icons");
    let root = temp.path();
    let icons = root.join("icons");
    std::fs::create_dir_all(&icons).expect("图标目录可建");
    std::fs::write(icons.join("256x256.png"), PNG_1X1).expect("图标可写");

    let out = polypack(&["build", "--target", "linux", "--json"], root);
    assert!(
        out.status.success(),
        "构建失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let events = json_lines(&out);
    assert!(
        events.iter().any(|event| event["event"] == "packaged"),
        "有手工图标就应出包: {events:?}"
    );
    assert!(
        !root
            .join(".polypack")
            .join("packages")
            .join("linux")
            .join("icons")
            .exists(),
        "手工图标在场时不应跑派生（无派生落点）"
    );
}
