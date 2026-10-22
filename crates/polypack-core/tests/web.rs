//! T-M1-04 验收：UI 构建引擎选择、内建静态打包的降级语义、PWA manifest 与 link 注入。

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use polypack_core::web::{self, UiEngine};
use polypack_core::{PolypackConfig, Project};

/// 测试临时树：Drop 时整树删除。
struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "polypack-web-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("创建临时目录");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project(root: &Path) -> Project {
    let config = PolypackConfig::parse(
        "[package]\nname = \"web-fixture\"\nversion = \"0.1.0\"\n\
         [ui]\nframework = \"vanilla\"\nentry = \"ui/index.html\"\n\
         [core]\ncrate = \"core\"\n",
    )
    .expect("配置合法");
    Project::new(root.to_path_buf(), config)
}

/// 写一份最小 UI：入口 HTML + 脚本 + 会被构建工具消费 / 不该进产物的文件。
fn write_ui(root: &Path) {
    let ui = root.join("ui");
    std::fs::create_dir_all(ui.join("node_modules")).expect("创建 node_modules");
    std::fs::create_dir_all(ui.join("public")).expect("创建 public");
    std::fs::write(
        ui.join("index.html"),
        "<!doctype html>\n<html lang=\"zh-CN\">\n  <head>\n    <meta charset=\"utf-8\" />\n    <title>fixture</title>\n  </head>\n  <body><main id=\"app\"></main></body>\n</html>\n",
    )
    .expect("写 index.html");
    std::fs::write(ui.join("main.js"), "console.log(\"fixture\");\n").expect("写 main.js");
    std::fs::write(ui.join("public/icon.svg"), "<svg></svg>\n").expect("写 icon.svg");
    for tooling in [
        "package.json",
        "package-lock.json",
        "vite.config.js",
        "tsconfig.json",
        ".gitignore",
        "README.md",
    ] {
        std::fs::write(ui.join(tooling), "tooling\n").expect("写构建工具文件");
    }
    std::fs::write(ui.join("node_modules/leftover.js"), "leftover\n").expect("写依赖目录");
}

/// 内建静态打包：产物齐备（`public/` 提到根）、构建工具文件与依赖目录不进产物、
/// manifest 生成且 link 注入保持原缩进、重复调用不重复注入。
#[test]
fn static_fallback_produces_site_and_manifest() {
    let tree = TempTree::new("static");
    write_ui(tree.path());
    let project = project(tree.path());
    let dist = tree.path().join("dist");

    let build = web::build_ui(&project, &dist).expect("静态打包必须成功");
    // 前置条件：PATH 上无全局 vite（本仓库 CI 亦未安装），故走内建静态打包
    assert_eq!(build.engine, UiEngine::Static, "无 vite 时必须走静态打包");
    assert_eq!(build.manifest, dist.join("manifest.webmanifest"));

    for present in ["index.html", "main.js", "icon.svg", "manifest.webmanifest"] {
        assert!(dist.join(present).is_file(), "产物缺少 {present}");
    }
    for absent in [
        "package.json",
        "package-lock.json",
        "vite.config.js",
        "tsconfig.json",
        ".gitignore",
        "README.md",
        "node_modules",
        "public",
    ] {
        assert!(
            !dist.join(absent).exists(),
            "构建工具文件 / 依赖目录不应进产物: {absent}"
        );
    }

    let html = std::fs::read_to_string(dist.join("index.html")).expect("读产物 HTML");
    assert_eq!(
        html.matches("rel=\"manifest\"").count(),
        1,
        "manifest link 必须恰好注入一次：{html}"
    );
    assert!(
        html.contains("  <link rel=\"manifest\" href=\"./manifest.webmanifest\">\n  </head>"),
        "注入须沿用 </head> 所在行的缩进：{html}"
    );

    // 幂等：再次生成 manifest 不重复注入
    web::write_manifest(&project, &dist).expect("再次生成 manifest 必须成功");
    let again = std::fs::read_to_string(dist.join("index.html")).expect("读产物 HTML");
    assert_eq!(again, html, "重复调用不得改变产物 HTML");
}

/// manifest 必含 PWA 关键键；主题色缺省时回落到 `DEFAULT_THEME_COLOR`。
#[test]
fn manifest_carries_pwa_keys_and_default_theme_color() {
    let tree = TempTree::new("manifest");
    write_ui(tree.path());
    let project = project(tree.path());
    let dist = tree.path().join("dist");
    web::build_ui(&project, &dist).expect("构建必须成功");

    let raw = std::fs::read_to_string(dist.join("manifest.webmanifest")).expect("读 manifest");
    let manifest: serde_json::Value = serde_json::from_str(&raw).expect("manifest 为合法 JSON");
    for key in [
        "name",
        "short_name",
        "start_url",
        "scope",
        "display",
        "background_color",
        "theme_color",
        "icons",
    ] {
        assert!(manifest.get(key).is_some(), "manifest 缺少 {key}：{raw}");
    }
    assert_eq!(manifest["name"], "web-fixture");
    assert_eq!(manifest["display"], "standalone");
    assert_eq!(manifest["start_url"], "./");
    assert_eq!(
        manifest["theme_color"],
        polypack_core::DEFAULT_THEME_COLOR,
        "未配置 theme_color 时必须回落缺省值"
    );
    assert_eq!(manifest["icons"][0]["src"], "icon.svg");
    assert_eq!(manifest["icons"][0]["type"], "image/svg+xml");
}

/// 配置了 `[ui].theme_color` 时以配置为准。
#[test]
fn manifest_honours_configured_theme_color() {
    let tree = TempTree::new("theme");
    write_ui(tree.path());
    let config = PolypackConfig::parse(
        "[package]\nname = \"web-fixture\"\nversion = \"0.1.0\"\n\
         [ui]\nframework = \"vanilla\"\nentry = \"ui/index.html\"\ntheme_color = \"#123456\"\n\
         [core]\ncrate = \"core\"\n",
    )
    .expect("配置合法");
    let project = Project::new(tree.path().to_path_buf(), config);
    let dist = tree.path().join("dist");
    web::build_ui(&project, &dist).expect("构建必须成功");
    let raw = std::fs::read_to_string(dist.join("manifest.webmanifest")).expect("读 manifest");
    let manifest: serde_json::Value = serde_json::from_str(&raw).expect("manifest 为合法 JSON");
    assert_eq!(manifest["theme_color"], "#123456");
}

/// 引擎探测：UI 目录含本地 vite 即判为 vite。
#[test]
fn detect_engine_prefers_local_vite() {
    let tree = TempTree::new("engine");
    write_ui(tree.path());
    let ui = tree.path().join("ui");
    assert_eq!(
        web::detect_engine(&ui),
        UiEngine::Static,
        "无本地 vite 时为静态引擎（前置条件：PATH 上无全局 vite）"
    );

    std::fs::create_dir_all(ui.join("node_modules/vite/bin")).expect("创建 vite 目录");
    std::fs::write(ui.join("node_modules/vite/bin/vite.js"), "// stub\n").expect("写 vite stub");
    assert!(
        matches!(web::detect_engine(&ui), UiEngine::Vite { .. }),
        "存在本地 vite 时必须判为 vite 引擎"
    );
}

/// vite 失败必须报 `polypack::E_WEB_0001`，绝不静默降级为静态产物。
#[test]
fn vite_failure_reports_web_error() {
    let tree = TempTree::new("vite-fail");
    write_ui(tree.path());
    let ui = tree.path().join("ui");
    std::fs::create_dir_all(ui.join("node_modules/vite/bin")).expect("创建 vite 目录");
    // 让 node 调起的「vite」立刻抛错：模拟构建失败
    std::fs::write(
        ui.join("node_modules/vite/bin/vite.js"),
        "throw new Error(\"fixture failure\");\n",
    )
    .expect("写 vite stub");

    let error = web::build_ui(&project(tree.path()), &tree.path().join("dist"))
        .expect_err("vite 失败时不得返回成功");
    assert_eq!(error.diagnostic.code, polypack_core::codes::E_WEB_0001);
    assert!(
        !tree.path().join("dist/index.html").exists(),
        "失败时不得留下产物"
    );
}

/// 入口 HTML 缺失 → `polypack::E_WEB_0002`；UI 目录缺失 → `polypack::E_WEB_0001`。
#[test]
fn missing_entry_and_missing_ui_dir_report_stable_codes() {
    let tree = TempTree::new("missing-entry");
    write_ui(tree.path());
    std::fs::remove_file(tree.path().join("ui/index.html")).expect("删除入口 HTML");
    let error = web::build_ui(&project(tree.path()), &tree.path().join("dist"))
        .expect_err("缺入口 HTML 必须失败");
    assert_eq!(error.diagnostic.code, polypack_core::codes::E_WEB_0002);

    let bare = TempTree::new("missing-ui");
    let error = web::build_ui(&project(bare.path()), &bare.path().join("dist"))
        .expect_err("UI 目录缺失必须失败");
    assert_eq!(error.diagnostic.code, polypack_core::codes::E_WEB_0001);
}

/// 降级告警的判据 [`web::needs_bundler`]：JSX / TS 入口必须走 vite（模板的 React 骨架即此类）；
/// 只看 UI 目录本身——`node_modules` 是依赖，`public/` 与 `dist/` vite 也只是原样拷贝。
#[test]
fn bundler_requirement_only_counts_ui_sources() {
    let tree = TempTree::new("needs-bundler");
    write_ui(tree.path());
    let ui = tree.path().join("ui");
    assert!(!web::needs_bundler(&ui), "纯 ESM fixture 不需要打包器");

    std::fs::write(ui.join("main.jsx"), "export default 1;\n").expect("写 main.jsx");
    assert!(web::needs_bundler(&ui), "JSX 入口需要打包器");
    std::fs::remove_file(ui.join("main.jsx")).expect("删 main.jsx");

    std::fs::write(ui.join("public/app.tsx"), "x\n").expect("写 public 下的 tsx");
    std::fs::write(ui.join("node_modules/dep.tsx"), "x\n").expect("写依赖里的 tsx");
    assert!(
        !web::needs_bundler(&ui),
        "public/ 与 node_modules 不算 UI 源码"
    );

    std::fs::create_dir_all(ui.join("src")).expect("建 src");
    std::fs::write(ui.join("src/util.ts"), "export const a = 1;\n").expect("写 util.ts");
    assert!(web::needs_bundler(&ui), "子目录里的 TS 需要打包器");
    assert!(
        !web::needs_bundler(&tree.path().join("nope")),
        "目录不存在按不需要处理"
    );
}
