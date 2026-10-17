//! T-M5-10 ChromeOS 双轨契约测试的共用夹具（mock / real 两个测试目标各以 `mod common;` 引入）。
//!
//! 集成测试的每个顶层 `.rs` 是独立 crate：把 `poly` 调用、临时工程、站点与 SW 夹具放这里，
//! 避免两份漂移。各目标只用到其中一部分，故允许未使用项。
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// 真实模式开关：`POLY_CONTRACT_REAL=1`（或 `true`）；默认 mock。
pub fn real_mode() -> bool {
    matches!(
        std::env::var("POLY_CONTRACT_REAL").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// 运行 `poly` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_poly`）。
pub fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

/// stdout + stderr（JSON 事件与诊断分别落在两处，断言取并集）。
pub fn combined(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// 命令是否可用（`--version` 成功）。
pub fn command_available(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// 临时工程：目录 + `poly init`（mock 的 verify 腿与 real 链路共用）。
pub fn init_project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "poly-contract-m5-chromeos-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("临时工程目录可建");
    let init = poly(&["init"], &dir);
    assert!(
        init.status.success(),
        "poly init 失败（{}）：{}",
        init.status,
        combined(&init)
    );
    dir
}

/// 合法 manifest（`display: standalone` + SVG any 图标；Chrome 151 实测可安装）。
pub const MANIFEST: &str = r##"{
  "name": "Chromebook Notes",
  "short_name": "CrNotes",
  "start_url": "./",
  "scope": "./",
  "display": "standalone",
  "background_color": "#ffffff",
  "theme_color": "#0f172a",
  "icons": [
    { "src": "icon.svg", "sizes": "any", "type": "image/svg+xml", "purpose": "any" }
  ]
}"##;

/// 应用自带的最小 service worker：install 预缓存 + activate 接管 + fetch 缓存优先。
pub const SERVICE_WORKER: &str = r#"const CACHE = "chromeos-contract-v1";
self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(["./", "./index.html", "./manifest.webmanifest", "./icon.svg"]))
      .then(() => self.skipWaiting()),
  );
});
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));
self.addEventListener("fetch", (event) => {
  event.respondWith(caches.match(event.request).then((hit) => hit ?? fetch(event.request)));
});
"#;

/// 打包后站点入口（已含 build 注入的 manifest link；verify 直接吃这份形态）。
pub fn site_index(with_sw: bool) -> String {
    let registration = if with_sw {
        r#"<script>navigator.serviceWorker?.register("./sw.js");</script>"#
    } else {
        "<!-- 应用未注册 service worker：离线能力腿应硬失败 -->"
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8">
    <link rel="manifest" href="./manifest.webmanifest">
    <title>Chromebook Notes</title>
  </head>
  <body>
    <main id="app">chromeos-pwa</main>
    {registration}
  </body>
</html>
"#
    )
}

/// 写 `packages/chromeos-pwa/site`（mock 的 verify 输入；布局与 StaticSite 打包器一致）。
pub fn write_site(root: &Path, with_sw: bool) {
    let site = root.join(".polypack/packages/chromeos-pwa/site");
    std::fs::create_dir_all(&site).expect("站点目录可建");
    std::fs::write(site.join("index.html"), site_index(with_sw)).expect("入口可写");
    std::fs::write(site.join("manifest.webmanifest"), MANIFEST).expect("manifest 可写");
    std::fs::write(site.join("sw.js"), SERVICE_WORKER).expect("SW 可写");
    std::fs::write(
        site.join("icon.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"512\" height=\"512\"/>",
    )
    .expect("图标可写");
}

/// `poly init` 后注入静态入口 + 应用自带 SW（替换 React 骨架，避免静态降级拷贝 JSX）。
pub fn write_ui(root: &Path) {
    std::fs::write(root.join("ui/index.html"), site_index_without_link()).expect("UI 入口可写");
    let public = root.join("ui/public");
    std::fs::create_dir_all(&public).expect("public 目录可建");
    std::fs::write(public.join("sw.js"), SERVICE_WORKER).expect("UI SW 可写");
    // 模板的 JSX 入口不再被引用；移除后静态降级产物更干净（不影响审计口径）。
    let _ = std::fs::remove_file(root.join("ui/main.jsx"));
}

/// build 前入口：**不带** manifest link（由 Web 管线 T-M1-04 注入，顺带验证注入腿）。
fn site_index_without_link() -> String {
    r#"<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Chromebook Notes</title>
    <link rel="icon" href="./icon.svg" type="image/svg+xml">
  </head>
  <body>
    <main id="app">chromeos-pwa</main>
    <script>navigator.serviceWorker?.register("./sw.js");</script>
  </body>
</html>
"#
    .to_string()
}
