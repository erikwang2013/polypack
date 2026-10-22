//! T-M5-10：PWA 三腿审计（manifest / 离线能力 / 安装性）。
//!
//! ChromeOS PWA 轨（`chromeos-pwa`）的 `pwa-audit` 校验器直接调用 [`audit_pwa`]；结构
//! 断言在 Rust 侧始终真跑，真浏览器腿（Chrome `Page.getInstallabilityErrors` + 断网重载）
//! 由 `ui/smoke/pwa/audit.mjs` 在 Chrome 在场时另跑（缺席 SKIP-with-print）。
//!
//! shortcut: 只做 Chrome 安装性所需的最小结构集（manifest 必填 / display / start_url /
//! ≥144px 图标 / manifest link / SW 注册与 fetch+cache）；W3C manifest 全量 schema 与真实
//! 离线网络策略不在此层展开。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{BundleError, io_err};
use crate::packagers::web::validate_manifest;

/// 审计器 id（诊断的 packager 字段；与 `TargetSpec.verify` 的 id 一致）。
pub const PWA_AUDIT: &str = "pwa-audit";

/// Chrome 安装性要求的图标最小边长（px）；`sizes: "any"`（SVG）按通过计（实测 Chrome 151）。
const MIN_ICON_PX: u64 = 144;

/// 可安装的 display 模式（`browser` 不可安装）。
const INSTALLABLE_DISPLAY: &[&str] = &["standalone", "fullscreen", "minimal-ui"];

/// 离线能力扫描的文本扩展名（注册既可能在入口 HTML，也可能在打包后的 JS）。
const SCAN_EXTENSIONS: &[&str] = &["html", "js", "mjs"];

/// 三腿审计结果；每条为人读证据，进 verify 告警 / 构建报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PwaAudit {
    /// manifest 结构与站点引用证据。
    pub manifest: String,
    /// 离线能力证据（SW 注册 / fetch 处理器 / Cache Storage）。
    pub offline: String,
    /// 安装性证据（display / start_url / 图标）。
    pub installability: String,
}

/// 审计站点目录（`<packages>/<target>/site`）。
pub fn audit_pwa(site: &Path) -> Result<PwaAudit, BundleError> {
    if !site.is_dir() {
        return Err(defect(
            "site",
            site.display().to_string(),
            "静态站点目录不存在（先跑 polypack build）",
        ));
    }
    if !site.join("index.html").is_file() {
        return Err(defect(
            "index.html",
            site.display().to_string(),
            "站点缺入口 HTML",
        ));
    }
    let manifest = read_manifest(site)?;
    // 打包器已跑过的 manifest 结构 / 引用存在性校验在此复用同一实现。
    validate_manifest(&manifest, site, PWA_AUDIT)?;
    let manifest_evidence = check_manifest_link(site, &manifest)?;
    let installability = check_installability(site, &manifest)?;
    let offline = check_offline(site)?;
    Ok(PwaAudit {
        manifest: manifest_evidence,
        offline,
        installability,
    })
}

/// 读 `manifest.webmanifest`（Web 管线 T-M1-04 的产物，站点打包器原样落盘）。
fn read_manifest(site: &Path) -> Result<Value, BundleError> {
    let path = site.join("manifest.webmanifest");
    if !path.is_file() {
        return Err(defect(
            "manifest.webmanifest",
            site.display().to_string(),
            "PWA manifest 缺失（Web 管线应生成 manifest.webmanifest）",
        ));
    }
    let text = std::fs::read_to_string(&path).map_err(io_err(&path))?;
    serde_json::from_str(&text).map_err(|err| {
        defect(
            "manifest.webmanifest",
            path.display().to_string(),
            format!("JSON 解析失败：{err}"),
        )
    })
}

/// manifest 审计：入口 HTML 必须引用 manifest；证据带 name / display / 图标数。
fn check_manifest_link(site: &Path, manifest: &Value) -> Result<String, BundleError> {
    let index = site.join("index.html");
    let html = std::fs::read_to_string(&index).map_err(io_err(&index))?;
    let lower = html.to_lowercase();
    if !(lower.contains("rel=\"manifest\"") || lower.contains("rel='manifest'")) {
        return Err(defect(
            "index.html(manifest-link)",
            "manifest.webmanifest",
            "入口 HTML 未引用 manifest（须含 <link rel=\"manifest\">）",
        ));
    }
    let name = manifest["name"].as_str().unwrap_or("");
    let display = manifest["display"].as_str().unwrap_or("");
    let icons = manifest["icons"].as_array().map_or(0, Vec::len);
    Ok(format!(
        "name={name}，display={display}，icons={icons}，入口 HTML 已引用 manifest.webmanifest"
    ))
}

/// 安装性审计：Chrome 的三项结构门槛（display / start_url / ≥144px 图标）。
fn check_installability(site: &Path, manifest: &Value) -> Result<String, BundleError> {
    let display = manifest["display"].as_str().unwrap_or("");
    if !INSTALLABLE_DISPLAY.contains(&display) {
        return Err(defect(
            "manifest.display",
            display.to_string(),
            "须为 standalone / fullscreen / minimal-ui（browser 不可安装）",
        ));
    }
    let start_url = manifest["start_url"].as_str().unwrap_or("");
    if start_url_file(site, start_url).is_none() {
        return Err(defect(
            "manifest.start_url",
            start_url.to_string(),
            "start_url 须为站点内可解析的本地路径",
        ));
    }
    let (src, sizes) = installable_icon(manifest).ok_or_else(|| {
        defect(
            "manifest.icons",
            manifest["icons"].to_string(),
            "须至少有 1 枚 sizes 为 `any` 或边长 ≥144px 的图标（Chrome 安装性门槛）",
        )
    })?;
    Ok(format!(
        "display={display}，start_url={start_url} 可解析，图标 {src}（{sizes}）满足 ≥{MIN_ICON_PX}px"
    ))
}

/// 选一枚满足安装性门槛的图标；返回 `(src, sizes)`。
fn installable_icon(manifest: &Value) -> Option<(&str, &str)> {
    manifest["icons"].as_array()?.iter().find_map(|icon| {
        let src = icon["src"].as_str()?;
        let sizes = icon["sizes"].as_str()?;
        let ok = sizes == "any"
            || sizes
                .split_once(['x', 'X'])
                .and_then(|(w, h)| {
                    let w: u64 = w.parse().ok()?;
                    let h: u64 = h.parse().ok()?;
                    Some(w.min(h) >= MIN_ICON_PX)
                })
                .unwrap_or(false);
        ok.then_some((src, sizes))
    })
}

/// `start_url` → 站点内实际文件；远程 / 越界路径返回 `None`。
fn start_url_file(site: &Path, start_url: &str) -> Option<PathBuf> {
    if start_url.contains("://") || start_url.starts_with("//") {
        return None;
    }
    let clean = start_url.split(['?', '#']).next().unwrap_or("");
    let rel = clean.trim_start_matches("./").trim_start_matches('/');
    if rel.split('/').any(|part| part == "..") {
        return None;
    }
    let path = if rel.is_empty() {
        site.join("index.html")
    } else {
        site.join(rel)
    };
    let path = if path.is_dir() {
        path.join("index.html")
    } else {
        path
    };
    path.is_file().then_some(path)
}

/// 离线能力审计：注册了 SW，且 SW 含 fetch 处理器 + Cache Storage。
fn check_offline(site: &Path) -> Result<String, BundleError> {
    for file in text_files(site)? {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Some(url) = registration_url(&text) else {
            continue;
        };
        let sw = resolve_sw(site, &file, &url)?;
        if !sw.is_file() {
            return Err(defect(
                "service-worker",
                url,
                format!("注册脚本在站点内不存在（{}）", sw.display()),
            ));
        }
        let source = std::fs::read_to_string(&sw).map_err(io_err(&sw))?;
        let lower = source.to_lowercase();
        let fetch_ok = lower.contains("addeventlistener(\"fetch\"")
            || lower.contains("addeventlistener('fetch'")
            || lower.contains("onfetch");
        let cache_ok = lower.contains("caches.");
        if !fetch_ok || !cache_ok {
            return Err(defect(
                "service-worker",
                url,
                "SW 须同时含 fetch 事件处理器与 Cache Storage 缓存（离线能力）",
            ));
        }
        let from = relative(&file, site);
        let script = relative(&sw, site);
        return Ok(format!(
            "注册于 {from}，脚本 {script} 含 fetch 处理器与 Cache Storage"
        ));
    }
    Err(defect(
        "service-worker",
        site.display().to_string(),
        "站点未注册 service worker（离线能力缺失；应用须自带 SW 注册与缓存策略）",
    ))
}

/// 从源码里取 `serviceWorker.register(<字面量>)` 的 URL。
///
/// shortcut: 只认字符串字面量；动态拼接（`register(swUrl)`）无法静态解析，按未注册处理，
/// 真浏览器腿会给出实际判决。
fn registration_url(text: &str) -> Option<String> {
    for (index, _) in text.match_indices("register(") {
        let head = &text[..index];
        let Some(pos) = head.rfind("serviceWorker") else {
            continue;
        };
        // `serviceWorker?.register(` / `serviceWorker.register(` 都在 32 字节内。
        if index - pos > 32 {
            continue;
        }
        let rest = text[index + "register(".len()..].trim_start();
        let Some(quote) = rest.chars().next().filter(|ch| *ch == '"' || *ch == '\'') else {
            continue;
        };
        let body = &rest[1..];
        let Some(end) = body.find(quote) else {
            continue;
        };
        let url = body[..end].trim();
        if !url.is_empty() && !url.contains(['\n', '\r']) {
            return Some(url.to_string());
        }
    }
    None
}

/// SW 注册 URL → 站点内文件（HTML 里的相对路径按该 HTML 目录解析，JS 里按站点根）。
fn resolve_sw(site: &Path, from: &Path, url: &str) -> Result<PathBuf, BundleError> {
    if url.contains("://") || url.starts_with("//") {
        return Err(defect(
            "service-worker",
            url.to_string(),
            "注册脚本须为同源本地文件",
        ));
    }
    let clean = url.split(['?', '#']).next().unwrap_or("");
    if clean.split('/').any(|part| part == "..") {
        return Err(defect(
            "service-worker",
            url.to_string(),
            "注册脚本路径越出站点目录",
        ));
    }
    let base = if extension(from) == Some("html") {
        from.parent().unwrap_or(site)
    } else {
        site
    };
    Ok(match clean.strip_prefix('/') {
        Some(absolute) => site.join(absolute),
        None => base.join(clean),
    })
}

/// 递归收集审计扫描的文本文件（排序稳定）。
fn text_files(root: &Path) -> Result<Vec<PathBuf>, BundleError> {
    let mut files = Vec::new();
    collect_text_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_text_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), BundleError> {
    for entry in std::fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        let path = entry.path();
        if entry.file_type().map_err(io_err(&path))?.is_dir() {
            collect_text_files(&path, out)?;
        } else if SCAN_EXTENSIONS.contains(&extension(&path).unwrap_or("")) {
            out.push(path);
        }
    }
    Ok(())
}

fn extension(path: &Path) -> Option<&str> {
    path.extension().and_then(|ext| ext.to_str())
}

fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn defect(field: &'static str, value: impl Into<String>, reason: impl Into<String>) -> BundleError {
    BundleError::InvalidInput {
        packager: PWA_AUDIT,
        field,
        value: value.into(),
        reason: reason.into(),
    }
}
