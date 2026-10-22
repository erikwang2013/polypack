//! T-M5-10：PWA 三腿审计（manifest / 离线能力 / 安装性）契约测试。
//!
//! 全部纯结构真跑（不 mock）：夹具站点写在 temp_dir，逐条改坏一个维度，断言
//! [`polypack_bundle::audit_pwa`] 给出可定位的缺陷；happy path 与 Chrome 实测口径对齐
//! （`sizes: "any"` 的 SVG 图标过安装性门槛，`browser` display / <144px 图标不过）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use polypack_bundle::BundleError;
use polypack_bundle::pwa::audit_pwa;

/// 合法 manifest（`display: standalone` + SVG any 图标 + 可解析 start_url）。
const MANIFEST_OK: &str = r##"{
  "name": "Pax Notes",
  "short_name": "Pax",
  "start_url": "./",
  "scope": "./",
  "display": "standalone",
  "background_color": "#ffffff",
  "theme_color": "#0f172a",
  "icons": [
    { "src": "icon.svg", "sizes": "any", "type": "image/svg+xml", "purpose": "any" }
  ]
}"##;

/// 含 registration + fetch + caches 的最小 service worker。
const SW_OK: &str = r#"const CACHE = "pax-v1";
self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(["./", "./index.html", "./manifest.webmanifest"])));
  self.skipWaiting();
});
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));
self.addEventListener("fetch", (event) => {
  event.respondWith(caches.match(event.request).then((hit) => hit ?? fetch(event.request)));
});
"#;

/// 测试临时站点：进程内自增序号（并行安全），Drop 时整树删除。
struct Site(PathBuf);

impl Site {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "polypack-bundle-pwa-{}-{tag}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建临时站点");
        let site = Self(dir);
        site.write("index.html", &index_html());
        site.write("manifest.webmanifest", MANIFEST_OK);
        site.write("sw.js", SW_OK);
        site.write("icon.svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
        site
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, rel: &str, contents: &str) -> PathBuf {
        let path = self.0.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("创建站点子目录");
        }
        std::fs::write(&path, contents).expect("写站点文件");
        path
    }
}

impl Drop for Site {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 入口 HTML：manifest link + SW 注册（与 Web 管线注入后的形态一致）。
fn index_html() -> String {
    r#"<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8">
    <link rel="manifest" href="./manifest.webmanifest">
    <title>Pax Notes</title>
  </head>
  <body>
    <main id="app">pax</main>
    <script>navigator.serviceWorker?.register("./sw.js");</script>
  </body>
</html>"#
        .to_string()
}

/// 断言错误是 PWA 审计的 `InvalidInput`，字段与原因包含给定片段。
fn assert_defect(result: Result<polypack_bundle::PwaAudit, BundleError>, field: &str, needle: &str) {
    match result {
        Err(BundleError::InvalidInput {
            packager,
            field: got_field,
            reason,
            ..
        }) => {
            assert_eq!(packager, "pwa-audit", "诊断须标记审计器 id");
            assert_eq!(got_field, field, "缺陷字段：{reason}");
            assert!(
                reason.contains(needle),
                "原因应包含 {needle:?}，实际：{reason}"
            );
        }
        other => panic!("期望 {field} 的 InvalidInput，实际：{other:?}"),
    }
}

#[test]
fn valid_site_passes_all_three_legs() {
    let site = Site::new("ok");
    let audit = audit_pwa(site.path()).expect("合法站点应通过三腿审计");
    assert!(audit.manifest.contains("standalone"), "{}", audit.manifest);
    assert!(
        audit.manifest.contains("manifest.webmanifest"),
        "{}",
        audit.manifest
    );
    assert!(audit.offline.contains("fetch"), "{}", audit.offline);
    assert!(audit.offline.contains("Cache Storage"), "{}", audit.offline);
    assert!(
        audit.installability.contains("144px"),
        "{}",
        audit.installability
    );
}

#[test]
fn missing_manifest_fails_manifest_leg() {
    let site = Site::new("no-manifest");
    std::fs::remove_file(site.path().join("manifest.webmanifest")).unwrap();
    assert_defect(audit_pwa(site.path()), "manifest.webmanifest", "缺失");
}

#[test]
fn manifest_must_be_linked_from_html() {
    let site = Site::new("no-link");
    site.write(
        "index.html",
        "<!DOCTYPE html><html><body>no link</body></html>",
    );
    assert_defect(
        audit_pwa(site.path()),
        "index.html(manifest-link)",
        "未引用 manifest",
    );
}

#[test]
fn browser_display_is_not_installable() {
    let site = Site::new("browser-display");
    site.write(
        "manifest.webmanifest",
        &MANIFEST_OK.replace("\"standalone\"", "\"browser\""),
    );
    assert_defect(audit_pwa(site.path()), "manifest.display", "standalone");
}

#[test]
fn start_url_must_resolve_inside_site() {
    let site = Site::new("start-url");
    site.write(
        "manifest.webmanifest",
        &MANIFEST_OK.replace("\"./\"", "\"./missing.html\""),
    );
    assert_defect(audit_pwa(site.path()), "manifest.start_url", "本地路径");
}

#[test]
fn icon_below_144px_fails_installability() {
    let site = Site::new("small-icon");
    site.write("icon-128.png", "not-a-real-png");
    site.write(
        "manifest.webmanifest",
        &MANIFEST_OK.replace(
            r#"{ "src": "icon.svg", "sizes": "any", "type": "image/svg+xml", "purpose": "any" }"#,
            r#"{ "src": "icon-128.png", "sizes": "128x128", "type": "image/png" }"#,
        ),
    );
    assert_defect(audit_pwa(site.path()), "manifest.icons", "144");
}

#[test]
fn any_size_svg_icon_passes_installability() {
    let site = Site::new("svg-any");
    // MANIFEST_OK 本身就是 SVG `sizes: "any"`；Chrome 151 实测可安装。
    let audit = audit_pwa(site.path()).expect("SVG any 图标应通过");
    assert!(
        audit.installability.contains("icon.svg"),
        "{}",
        audit.installability
    );
}

#[test]
fn site_without_service_worker_fails_offline_leg() {
    let site = Site::new("no-sw");
    site.write(
        "index.html",
        "<!DOCTYPE html><html><head><link rel=\"manifest\" href=\"./manifest.webmanifest\"></head><body>static</body></html>",
    );
    assert_defect(audit_pwa(site.path()), "service-worker", "未注册");
}

#[test]
fn service_worker_needs_fetch_and_cache() {
    let site = Site::new("sw-no-cache");
    site.write(
        "sw.js",
        "self.addEventListener(\"install\", () => self.skipWaiting());\n",
    );
    assert_defect(audit_pwa(site.path()), "service-worker", "Cache Storage");
}

#[test]
fn service_worker_registration_must_point_at_existing_file() {
    let site = Site::new("sw-missing");
    site.write(
        "index.html",
        "<!DOCTYPE html><html><head><link rel=\"manifest\" href=\"./manifest.webmanifest\"></head>\
         <body><script>navigator.serviceWorker.register('./missing-sw.js');</script></body></html>",
    );
    assert_defect(audit_pwa(site.path()), "service-worker", "不存在");
}

#[test]
fn remote_service_worker_is_rejected() {
    let site = Site::new("sw-remote");
    site.write(
        "index.html",
        "<!DOCTYPE html><html><head><link rel=\"manifest\" href=\"./manifest.webmanifest\"></head>\
         <body><script>navigator.serviceWorker.register('https://cdn.example.com/sw.js');</script></body></html>",
    );
    assert_defect(audit_pwa(site.path()), "service-worker", "同源");
}

#[test]
fn missing_site_directory_is_a_defect() {
    let site = Site::new("missing-site");
    let missing = site.path().join("nope");
    assert_defect(audit_pwa(&missing), "site", "不存在");
}
