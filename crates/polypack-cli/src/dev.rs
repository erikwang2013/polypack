//! `polypack dev`：Web 宿主热预览 v0（T-M1-10，整页重载形态）。
//!
//! 形态（TASKS 允许的降级预案）：内建静态服务器托管 `dist/`，向返回的 HTML 注入一段
//! 轮询脚本——UI 源文件变化 → 重打包 → 版本号 +1 → 浏览器整页重载；核心源码变化 →
//! 重编译（`cargo build --release`）→ 同样刷新，并在页面右下角的状态条上显示核心状态。
//! 不做核心热注入（M2+）；不常驻任何子进程，退出即无残留。

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use polypack_cache::Digest;
use polypack_core::{Diagnostic, Project, codes, compile, web};
use polypack_targets::{Bridge, Runtime, TargetSpec};

use crate::project;

/// 文件轮询间隔。
const POLL: Duration = Duration::from_millis(400);

/// `polypack dev` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// 监听端口。
    #[arg(long, default_value_t = 5173)]
    pub port: u16,
    /// 监听地址。
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
}

/// dev 服务器共享状态。
struct DevState {
    /// 产物目录。
    dist: PathBuf,
    /// 版本号：每次重建 +1，浏览器据此整页重载。
    version: u64,
    /// UI 子状态（状态条用）。
    ui: serde_json::Value,
    /// 核心子状态（状态条用）。
    core: serde_json::Value,
}

/// 起服务并进入监听循环（正常情况下不返回）。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let project = project::load(&args.config)?;
    let ui = web::ui_dir(&project)?;
    let core_dir = project.root.join(&project.config.core.crate_path);
    let dist = project.root.join("dist");

    let mut state = DevState {
        dist: dist.clone(),
        version: 0,
        ui: serde_json::json!({ "engine": "unknown", "built_at": null }),
        core: serde_json::json!({ "state": "pending", "message": null, "built_at": null }),
    };
    rebuild_ui(&project, &ui, &mut state, json);
    rebuild_core(&project, &mut state);
    state.version = 1;

    let listener = TcpListener::bind((args.host.as_str(), args.port)).map_err(|err| {
        Diagnostic::error(
            codes::E_DEV_0001,
            format!("无法监听 {}:{}（{err}）", args.host, args.port),
        )
        .with_hint("换一个 --port，或确认该端口未被占用")
    })?;
    let state = Arc::new(Mutex::new(state));
    {
        let state = state.clone();
        std::thread::spawn(move || serve(listener, state));
    }

    let url = format!("http://{}:{}", args.host, args.port);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "event": "dev_started",
                "url": url,
                "ui": ui.display().to_string(),
                "core": core_dir.display().to_string(),
            })
        );
    } else {
        println!("polypack dev — {url}（Ctrl-C 退出；整页重载形态）");
        println!("  监听 UI: {}；核心: {}", ui.display(), core_dir.display());
    }

    // 主线程做文件轮询：内容摘要变化即重建（UI → 重打包；核心 → 重编译）。
    let mut ui_digest = digest_of(&ui, &["node_modules", "dist", ".polypack"]);
    let mut core_digest = digest_of(&core_dir, &["target", ".polypack"]);
    loop {
        std::thread::sleep(POLL);
        let next_ui = digest_of(&ui, &["node_modules", "dist", ".polypack"]);
        let next_core = digest_of(&core_dir, &["target", ".polypack"]);
        if next_ui == ui_digest && next_core == core_digest {
            continue;
        }
        let ui_changed = next_ui != ui_digest;
        let core_changed = next_core != core_digest;
        ui_digest = next_ui;
        core_digest = next_core;

        let mut guard = state.lock().expect("dev 状态锁未被毒化");
        if ui_changed {
            rebuild_ui(&project, &ui, &mut guard, json);
        }
        if core_changed {
            rebuild_core(&project, &mut guard);
        }
        guard.version += 1;
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "event": "dev_rebuilt",
                    "build": guard.version,
                    "ui": ui_changed,
                    "core": core_changed,
                })
            );
        } else {
            println!(
                "· 变更已应用（build {}）：UI {}，核心 {}",
                guard.version,
                if ui_changed { "已重打包" } else { "未变" },
                if core_changed {
                    "已重编译"
                } else {
                    "未变"
                }
            );
        }
    }
}

/// 重打包 UI（失败只记状态，不终止 dev 会话）。
fn rebuild_ui(project: &Project, ui: &Path, state: &mut DevState, json: bool) {
    let engine = web::detect_engine(ui);
    match web::build_ui(project, &state.dist) {
        Ok(build) => {
            state.ui = serde_json::json!({
                "engine": build.engine.label(),
                "built_at": timestamp(),
            });
            if !json {
                println!("· UI 已构建（{}）", build.engine.label());
            }
        }
        Err(err) => {
            state.ui = serde_json::json!({
                "engine": engine.label(),
                "built_at": null,
                "error": err.diagnostic.to_string(),
            });
            eprintln!("✗ UI 构建失败（dev 继续）: {err}");
        }
    }
}

/// 重编译核心（失败只记状态，不终止 dev 会话）。
fn rebuild_core(project: &Project, state: &mut DevState) {
    let Some(target) = native_desktop_target(project) else {
        state.core = serde_json::json!({
            "state": "skipped",
            "message": "无可编译的宿主原生桌面目标",
            "built_at": null,
        });
        return;
    };
    match compile::compile_core(project, &target) {
        Ok(result) => {
            state.core = serde_json::json!({
                "state": "ok",
                "message": format!("{} ({}) {:.2}s", result.crate_name, result.triple, result.duration.as_secs_f64()),
                "built_at": timestamp(),
            });
            println!(
                "· 核心已重编译（{}，{:.2}s）",
                result.triple,
                result.duration.as_secs_f64()
            );
        }
        Err(err) => {
            state.core = serde_json::json!({
                "state": "failed",
                "message": err.diagnostic.to_string(),
                "built_at": null,
            });
            eprintln!("✗ 核心编译失败（dev 继续）: {err}");
        }
    }
}

/// 首个宿主原生桌面目标（与 pipeline 的编译判据一致）。
fn native_desktop_target(project: &Project) -> Option<TargetSpec> {
    let registry = polypack_targets::Registry::builtin();
    project
        .config
        .targets
        .keys()
        .filter_map(|id| registry.get(id).cloned())
        .find(|spec: &TargetSpec| {
            spec.runtime == Runtime::Native && spec.bridge == Bridge::TauriIpc
        })
}

/// 目录内容摘要（复用内容寻址哈希：只读、无副作用）。
fn digest_of(dir: &Path, exclude: &[&str]) -> Option<String> {
    Digest::of_tree(dir, exclude)
        .ok()
        .map(|digest| digest.as_str().to_string())
}

fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    format!("{now}")
}

/// 连接处理循环。
fn serve(listener: TcpListener, state: Arc<Mutex<DevState>>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let state = state.clone();
        std::thread::spawn(move || {
            let _ = handle(stream, &state);
        });
    }
}

/// 单个 HTTP 请求：`/__polypack_version`、`/__polypack_status` 为 dev 端点，其余按 `dist/` 静态文件。
fn handle(mut stream: TcpStream, state: &Arc<Mutex<DevState>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    // 丢弃请求头（本服务器只用请求行）
    let mut header = String::new();
    while reader.read_line(&mut header)? > 2 {}
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();

    match path.as_str() {
        "/__polypack_version" => {
            let version = state.lock().expect("dev 状态锁未被毒化").version;
            respond(
                &mut stream,
                200,
                "text/plain; charset=utf-8",
                version.to_string().as_bytes(),
            )
        }
        "/__polypack_status" => {
            let status = {
                let guard = state.lock().expect("dev 状态锁未被毒化");
                status_json(&guard)
            };
            respond(
                &mut stream,
                200,
                "application/json; charset=utf-8",
                status.as_bytes(),
            )
        }
        _ => serve_file(&mut stream, state, &path),
    }
}

/// 状态条载荷：`version` 与 `build` 同为当前版本号，注入脚本据此判断是否整页重载。
fn status_json(state: &DevState) -> String {
    serde_json::json!({
        "build": state.version,
        "version": state.version,
        "ui": state.ui,
        "core": state.core,
    })
    .to_string()
}

/// 静态文件响应（HTML 注入 dev 脚本；路径穿越被拒绝）。
fn serve_file(
    stream: &mut TcpStream,
    state: &Arc<Mutex<DevState>>,
    path: &str,
) -> std::io::Result<()> {
    let (dist, _) = {
        let guard = state.lock().expect("dev 状态锁未被毒化");
        (guard.dist.clone(), guard.version)
    };
    let relative = path.trim_start_matches('/');
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    if relative.split('/').any(|segment| segment == "..") {
        return respond(stream, 403, "text/plain; charset=utf-8", b"forbidden");
    }
    let mut file = dist.join(relative);
    if file.is_dir() {
        file = file.join("index.html");
    }
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(_) => {
            let hint = format!("404 {}（polypack dev 服务 {}）", path, dist.display());
            return respond(stream, 404, "text/plain; charset=utf-8", hint.as_bytes());
        }
    };
    let mime = mime_of(relative);
    if mime.starts_with("text/html") {
        let injected = inject_dev_script(&bytes);
        return respond(stream, 200, mime, &injected);
    }
    respond(stream, 200, mime, &bytes)
}

/// 注入 dev 脚本：轮询版本号（变化即整页重载）+ 右下角状态条。
fn inject_dev_script(html: &[u8]) -> Vec<u8> {
    const SCRIPT: &str = r#"<script>
(function () {
  var bar = document.createElement("div");
  bar.style.cssText = "position:fixed;right:8px;bottom:8px;z-index:99999;font:12px/1.5 ui-monospace,SFMono-Regular,monospace;background:#0f172a;color:#fdba74;padding:6px 10px;border-radius:8px;opacity:.92;max-width:60vw";
  bar.textContent = "polypack dev";
  document.body.appendChild(bar);
  var seen = null;
  function tick() {
    fetch("/__polypack_status").then(function (r) { return r.json(); }).then(function (s) {
      bar.textContent = "polypack dev · build " + s.build + " · ui:" + s.ui.engine + " · core:" + s.core.state + (s.core.built_at ? " @" + s.core.built_at : "");
      var v = String(s.version);
      if (seen === null) { seen = v; } else if (seen !== v) { seen = v; location.reload(); }
    }).catch(function () {});
  }
  tick();
  setInterval(tick, 700);
})();
</script>
</body>"#;
    let text = String::from_utf8_lossy(html).into_owned();
    match text.rfind("</body>") {
        Some(index) => {
            let mut patched = String::with_capacity(text.len() + SCRIPT.len());
            patched.push_str(&text[..index]);
            patched.push_str(SCRIPT);
            patched.push_str(&text[index + "</body>".len()..]);
            patched.into_bytes()
        }
        None => html.to_vec(),
    }
}

/// 写一个最小 HTTP 响应（`Connection: close`，不保持连接）。
fn respond(stream: &mut TcpStream, code: u16, mime: &str, body: &[u8]) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {code} {}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        if code == 200 { "OK" } else { "Error" },
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// 扩展名 → Content-Type（覆盖模板与常见前端产物）。
fn mime_of(path: &str) -> &'static str {
    const TABLE: &[(&str, &str)] = &[
        ("html", "text/html; charset=utf-8"),
        ("js", "text/javascript; charset=utf-8"),
        ("mjs", "text/javascript; charset=utf-8"),
        ("css", "text/css; charset=utf-8"),
        ("json", "application/json; charset=utf-8"),
        ("webmanifest", "application/manifest+json"),
        ("svg", "image/svg+xml"),
        ("png", "image/png"),
        ("ico", "image/x-icon"),
        ("wasm", "application/wasm"),
        ("txt", "text/plain; charset=utf-8"),
    ];
    let extension = path.rsplit('.').next().unwrap_or("");
    TABLE
        .iter()
        .find(|(name, _)| *name == extension)
        .map(|(_, mime)| *mime)
        .unwrap_or("application/octet-stream")
}
