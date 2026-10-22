//! T-M1-08 Web 静态站点打包器契约测试：真跑打包 + 真跑静态服务器托管。
//!
//! 全部在本机 Linux 真跑（不 mock）：产物落在 temp_dir 下的独立子目录，
//! 静态托管起 `python3 -m http.server` 子进程并用 `TcpStream` 发最小 HTTP/1.1 GET，
//! 子进程由 Drop 守卫回收（测试失败 / panic 也不留孤儿）。

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as Proc, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use polypack_bundle::packager::{ArtifactKind, Packager};
use polypack_bundle::packagers::web::StaticSite;
use polypack_bundle::{BundleError, BundleInputs, ProductInfo, exec, fsutil, sri};
use polypack_targets::{Bridge, Host, PackageSpec, Runtime, TargetSpec};

/// 写进 index.html 的标记文本（静态服务器真跑时按响应体断言）。
const MARKER: &str = "polypack-static-site-marker";

/// 几字节的「图标」：打包器只校验引用存在性，不解析 PNG。
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";

/// 合法 PWA manifest：图标引用站点内真实文件 + 一条 `any` 尺寸 + 一条远程引用。
const MANIFEST_OK: &str = r##"{
  "name": "Pax Notes",
  "short_name": "Pax",
  "start_url": "/",
  "display": "standalone",
  "theme_color": "#0f172a",
  "background_color": "#ffffff",
  "icons": [
    { "src": "icon-192.png", "sizes": "192x192", "type": "image/png" },
    { "src": "icon-512.png", "sizes": "any", "type": "image/png" },
    { "src": "https://cdn.example.com/remote.png", "sizes": "512x512" }
  ]
}"##;

/// 静态服务器候选端口（高位，避开常用端口）。
const PORTS: [u16; 4] = [38173, 38174, 38175, 38176];

// ---------------------------------------------------------------- 夹具

/// 测试用临时目录：进程内自增序号保证各测试子目录互不冲突（并行安全），Drop 时整树删除。
struct Temp(PathBuf);

impl Temp {
    /// 新建独立子目录。
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "polypack-bundle-web-{}-{tag}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建临时目录");
        Self(dir)
    }

    /// 子路径。
    fn join(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 产品元数据。
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

/// 目标声明（web / static-site；按需覆盖格式以测守卫）。
fn spec(format: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub("web", 1, &[Host::Linux]);
    spec.runtime = Runtime::Wasm;
    spec.bridge = Bridge::WasmBindgen;
    spec.ui = vec!["react-dom".to_string()];
    spec.package = PackageSpec {
        format: format.to_string(),
        builder: "web-static".to_string(),
    };
    spec
}

/// 标准夹具：dist（index.html + assets + 图标）+ 合法 manifest，返回可直接打包的输入。
fn inputs(t: &Temp) -> BundleInputs {
    let dist = t.join("dist");
    fsutil::write_text(
        &dist.join("index.html"),
        &format!(
            "<!doctype html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n\
             <link rel=\"manifest\" href=\"manifest.webmanifest\">\n<title>Pax Notes</title>\n\
             </head>\n<body>\n<div id=\"root\">{MARKER}</div>\n\
             <script src=\"assets/app.js\"></script>\n</body>\n</html>\n"
        ),
    )
    .expect("写 index.html");
    fsutil::write_text(&dist.join("assets/app.js"), "console.log(\"Pax Notes\");\n")
        .expect("写 app.js");
    fsutil::write_text(
        &dist.join("assets/style.css"),
        "body { font-family: system-ui; }\n",
    )
    .expect("写 style.css");
    std::fs::write(dist.join("icon-192.png"), PNG).expect("写 192 图标");
    std::fs::write(dist.join("icon-512.png"), PNG).expect("写 512 图标");
    fsutil::write_text(&t.join("manifest.json"), MANIFEST_OK).expect("写 manifest");

    let mut inputs = BundleInputs::new(product(), t.join("out"));
    inputs.ui_dist = Some(dist);
    inputs.web_manifest = Some(t.join("manifest.json"));
    inputs
}

// ---------------------------------------------------------------- 静态服务器真跑

/// 静态服务器子进程（Drop 时 kill + wait）。
struct Server {
    child: Child,
    port: u16,
}

impl Server {
    /// 启动 `python3 -m http.server`，轮询至就绪（最多约 5 秒）；端口被占用即退避下一个。
    fn start(root: &Path) -> Self {
        for port in PORTS {
            let mut cmd = Proc::new("python3");
            cmd.args(["-m", "http.server", "--bind", "127.0.0.1", "--directory"])
                .arg(root)
                .arg(port.to_string());
            let mut child = cmd
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("静态服务器真跑需要 python3");
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if child.try_wait().expect("探测子进程状态").is_some() {
                    break; // 启动即退出（端口占用等）：试下一个端口
                }
                if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                    return Self { child, port };
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        panic!("{PORTS:?} 均无法启动静态服务器（python3 不可用或端口被占用）");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 最小 HTTP/1.1 GET（`Connection: close` 保证能读到响应末尾）。
fn get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("连接静态服务器");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("设置读超时");
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .expect("发送请求");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("读取响应");
    response
}

// ---------------------------------------------------------------- 用例

/// 1. 结构与 SRI：产物形态、目录内容、清单算法与摘要。
#[test]
fn package_builds_site_with_manifest_and_sri() {
    let t = Temp::new("structure");
    let inputs = inputs(&t);
    let artifact = StaticSite
        .package(&inputs, &spec("static-site"))
        .expect("打包必须成功");

    assert_eq!(StaticSite.id(), "web");
    assert_eq!(StaticSite.format(), "static-site");
    assert_eq!(artifact.kind, ArtifactKind::Directory);
    assert_eq!(artifact.target, "web");
    assert_eq!(artifact.format, "static-site");
    assert_eq!(artifact.path, t.join("out/site"));
    assert!(artifact.bytes > 0, "目录产物须有体积");

    let site = artifact.path;
    for rel in [
        "index.html",
        "assets/app.js",
        "assets/style.css",
        "manifest.webmanifest",
        "integrity.json",
    ] {
        assert!(site.join(rel).is_file(), "站点缺少 {rel}");
    }
    assert_eq!(
        std::fs::read_to_string(site.join("manifest.webmanifest"))
            .expect("读 manifest.webmanifest"),
        MANIFEST_OK,
        "manifest 须原样写入（不做无谓改写）"
    );

    let manifest: sri::Manifest = serde_json::from_slice(
        &std::fs::read(site.join("integrity.json")).expect("读 integrity.json"),
    )
    .expect("integrity.json 须为合法 sri::Manifest");
    assert_eq!(manifest.algorithm, "sha384");
    let bytes = std::fs::read(site.join("index.html")).expect("读 index.html");
    assert_eq!(
        manifest.files["index.html"].integrity,
        sri::integrity_of(&bytes),
        "清单摘要须与文件字节一致"
    );
    assert_eq!(manifest.files["index.html"].bytes, bytes.len() as u64);
    assert!(
        manifest.files.contains_key("manifest.webmanifest"),
        "manifest 也纳入 SRI"
    );
    assert!(
        !manifest.files.contains_key("integrity.json"),
        "清单不得自指"
    );
    assert!(
        artifact.notes.iter().any(|note| note.contains("SRI")),
        "notes 须含 SRI 说明：{:?}",
        artifact.notes
    );
}

/// 2. 静态托管真跑：python3 起服务器 → HTTP/1.1 GET /index.html → 200 + 标记文本。
#[test]
fn site_is_served_by_a_static_http_server() {
    // 静态托管真跑要 python3（三宿主不保证预装；缺失即显式跳过）。
    if common::skip_without("python3") {
        return;
    }
    let t = Temp::new("serve");
    let inputs = inputs(&t);
    let artifact = StaticSite
        .package(&inputs, &spec("static-site"))
        .expect("打包必须成功");

    let server = Server::start(&artifact.path);
    let response = get(server.port, "/index.html");
    assert!(!response.is_empty(), "空响应");
    let status = response.lines().next().unwrap_or_default().to_string();
    assert!(status.contains("200"), "响应首行须含 200，实得 {status:?}");
    assert!(
        response.contains(MARKER),
        "响应体须含标记文本 {MARKER}：{response}"
    );
    // 真跑证据（--nocapture 可见）：端口与响应首行。
    eprintln!("静态托管真跑 127.0.0.1:{} → {status}", server.port);
}

/// 3. manifest 失败路径：引用缺失 / 缺 name / 颜色非法，一律 InvalidInput（E_BND_0003）。
#[test]
fn manifest_violations_are_invalid_input() {
    let cases = [
        (
            "icons-missing-file",
            r#"{"name":"Pax Notes","start_url":"/","display":"standalone",
                "icons":[{"src":"missing-icon.png","sizes":"192x192"}]}"#,
        ),
        (
            "name-missing",
            r#"{"start_url":"/","display":"standalone",
                "icons":[{"src":"icon-192.png","sizes":"192x192"}]}"#,
        ),
        (
            "theme-color-invalid",
            r#"{"name":"Pax Notes","start_url":"/","display":"standalone","theme_color":"blue",
                "icons":[{"src":"icon-192.png","sizes":"192x192"}]}"#,
        ),
    ];
    for (tag, manifest) in cases {
        let t = Temp::new(tag);
        let inputs = inputs(&t);
        fsutil::write_text(&t.join("manifest.json"), manifest).expect("覆盖 manifest");
        let err = StaticSite
            .package(&inputs, &spec("static-site"))
            .expect_err("非法 manifest 必须报错");
        assert!(
            matches!(err, BundleError::InvalidInput { .. }),
            "{tag}：期望 InvalidInput，实得 {err:?}"
        );
        assert_eq!(err.code(), "polypack::E_BND_0003", "{tag}");
        assert!(
            !inputs.out_dir.join("site/integrity.json").exists(),
            "{tag}：校验失败不得留下 SRI 清单"
        );
    }
}

/// manifest 完全缺失：MissingInput（PWA 资产是验收项，不静默跳过）。
#[test]
fn missing_web_manifest_is_reported() {
    let t = Temp::new("no-manifest");
    let mut inputs = inputs(&t);
    inputs.web_manifest = None;
    let err = StaticSite
        .package(&inputs, &spec("static-site"))
        .expect_err("缺 manifest 必须报错");
    assert!(
        matches!(
            err,
            BundleError::MissingInput {
                field: "web_manifest",
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(err.code(), "polypack::E_BND_0002");
}

/// 4. 格式守卫：按 appimage 目标调用 web 打包器 → InvalidInput。
#[test]
fn format_guard_rejects_other_formats() {
    let t = Temp::new("format");
    let inputs = inputs(&t);
    let err = StaticSite
        .package(&inputs, &spec("appimage"))
        .expect_err("格式不符必须报错");
    match err {
        BundleError::InvalidInput { field, .. } => assert_eq!(field, "spec.package.format"),
        other => panic!("期望 InvalidInput，实得 {other:?}"),
    }
    assert_eq!(err.code(), "polypack::E_BND_0003");
    assert!(
        StaticSite.plan(&inputs, &spec("appimage")).is_err(),
        "plan 同样守卫"
    );
}

/// 5. HTML 结构失败：页面缺 `<body>` → InvalidInput（field 指到具体页面）。
#[test]
fn html_without_body_is_rejected() {
    let t = Temp::new("html");
    let inputs = inputs(&t);
    fsutil::write_text(
        &t.join("dist/index.html"),
        "<!doctype html>\n<html><head><title>x</title></head></html>\n",
    )
    .expect("覆盖 index.html");
    let err = StaticSite
        .package(&inputs, &spec("static-site"))
        .expect_err("缺 body 必须报错");
    let code = err.code();
    match err {
        BundleError::InvalidInput { field, value, .. } => {
            assert_eq!(field, "ui_dist(html)");
            assert_eq!(value, "index.html");
        }
        other => panic!("期望 InvalidInput，实得 {other:?}"),
    }
    assert_eq!(code, "polypack::E_BND_0003");
}

/// 6. html-validate 分支：在场时逐文件出命令，缺席时记 skipped；两条路径 package 都成功。
#[test]
fn html_validate_branch_follows_tool_presence() {
    let t = Temp::new("validate");
    let inputs = inputs(&t);
    let plan = StaticSite
        .plan(&inputs, &spec("static-site"))
        .expect("计划必须生成");
    assert!(!t.join("out").exists(), "plan() 不得写盘");

    if exec::tool_present("html-validate") {
        assert!(!plan.commands.is_empty(), "工具在场须给出校验命令");
        assert!(plan.commands.iter().all(|c| c.program == "html-validate"));
        assert!(plan.skipped.is_empty());
        assert!(
            plan.commands
                .iter()
                .any(|c| c.args.iter().any(|a| a.ends_with("index.html"))),
            "命令须指向 dist 下的 HTML：{:?}",
            plan.commands
        );
    } else {
        assert!(plan.commands.is_empty());
        assert!(
            plan.skipped.iter().any(|s| s.tool == "html-validate"),
            "工具缺席须如实记录 skipped：{:?}",
            plan.skipped
        );
    }

    let artifact = StaticSite
        .package(&inputs, &spec("static-site"))
        .expect("两种分支都必须打包成功");
    assert!(artifact.path.join("index.html").is_file());
}
