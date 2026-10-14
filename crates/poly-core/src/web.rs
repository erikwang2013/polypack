//! T-M1-04：Web 管线 —— vite 构建集成与 PWA manifest 生成。
//!
//! 引擎选择（[`detect_engine`]）：UI 目录内含 `node_modules/vite/bin/vite.js` 时用 node 调起
//! 项目本地 vite；否则回退 PATH 上的 `vite`；两者都无 → 内建最小静态打包（仅拷贝，不打包 /
//! 不转译）并发出 `Warning` 事件如实标注降级——绝不把降级结果当 vite 产物上报。

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::config::DEFAULT_THEME_COLOR;
use crate::diagnostics::{Diagnostic, codes};
use crate::plan::{Project, StageError};

/// UI 构建引擎。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEngine {
    /// 项目本地或 PATH 上的 vite（携带探测到的版本）。
    Vite {
        /// `vite --version` 输出（探测失败为 `None`）。
        version: Option<String>,
    },
    /// 内建最小静态打包。
    Static,
}

impl UiEngine {
    /// 引擎标签（写入缓存键与报告）。
    pub fn label(&self) -> &'static str {
        match self {
            Self::Vite { .. } => "vite",
            Self::Static => "static",
        }
    }
}

/// 一次 UI 构建的产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiBuild {
    /// 产物目录（vite 的 `dist`）。
    pub dist: PathBuf,
    /// 实际使用的引擎。
    pub engine: UiEngine,
    /// 生成的 PWA manifest 路径。
    pub manifest: PathBuf,
}

/// UI 根目录：`polypack.toml` 的 `[ui].entry`（如 `ui/index.html`）所在目录。
pub fn ui_dir(project: &Project) -> Result<PathBuf, StageError> {
    let entry = PathBuf::from(&project.config.ui.entry);
    let dir = entry.parent().filter(|dir| !dir.as_os_str().is_empty());
    dir.map(|dir| project.root.join(dir)).ok_or_else(|| {
        Diagnostic::error(
            codes::E_WEB_0001,
            format!("[ui].entry 缺少目录部分: {}", project.config.ui.entry),
        )
        .with_hint("把 [ui].entry 写成形如 ui/index.html 的路径")
        .into()
    })
}

/// 探测 UI 目录可用的构建引擎（不产生副作用）。
pub fn detect_engine(ui_dir: &Path) -> UiEngine {
    if ui_dir.join("node_modules/vite/bin/vite.js").is_file() {
        return UiEngine::Vite {
            version: vite_version(ui_dir),
        };
    }
    if which("vite").is_some() {
        return UiEngine::Vite {
            version: vite_version(ui_dir),
        };
    }
    UiEngine::Static
}

/// 构建 UI 到 `dist`：按引擎调起 vite 或走内建静态打包，随后生成 manifest 并注入 link。
pub fn build_ui(project: &Project, dist: &Path) -> Result<UiBuild, StageError> {
    let ui = ui_dir(project)?;
    if !ui.is_dir() {
        return Err(Diagnostic::error(
            codes::E_WEB_0001,
            format!("UI 目录不存在: {}", ui.display()),
        )
        .with_hint("核对 polypack.toml 的 [ui].entry 指向的目录")
        .into());
    }
    let engine = detect_engine(&ui);
    match &engine {
        UiEngine::Vite { .. } => run_vite(&ui, dist, &engine)?,
        UiEngine::Static => copy_static(&ui, dist)?,
    }
    let manifest = write_manifest(project, dist)?;
    Ok(UiBuild {
        dist: dist.to_path_buf(),
        engine,
        manifest,
    })
}

/// 调起 vite 构建（失败即 `poly::E_WEB_0001`，不做静默降级）。
fn run_vite(ui: &Path, dist: &Path, engine: &UiEngine) -> Result<(), StageError> {
    let vite_js = ui.join("node_modules/vite/bin/vite.js");
    let mut command = if vite_js.is_file() {
        let mut command = Command::new("node");
        command.arg(&vite_js);
        command
    } else {
        Command::new("vite")
    };
    let output = command
        // `--base ./`：静态托管在任意子路径下都可用（PWA 相对路径资产）
        .args(["build", "--base", "./", "--outDir"])
        .arg(dist)
        .arg("--emptyOutDir")
        .current_dir(ui)
        .output()
        .map_err(|err| {
            Diagnostic::error(
                codes::E_WEB_0001,
                format!("无法调起 vite（{}）: {err}", engine_label(engine)),
            )
            .with_hint("安装 Node.js，并在 UI 目录执行 npm install 以获取 vite")
        })?;
    if !output.status.success() {
        return Err(Diagnostic::error(
            codes::E_WEB_0001,
            format!("vite 构建失败（退出码 {}）", output.status),
        )
        .with_hint(format!(
            "查看 UI 目录的 vite 报错；stderr 尾部: {}",
            stderr_tail(&output.stderr)
        ))
        .into());
    }
    if !dist.is_dir() {
        return Err(Diagnostic::error(
            codes::E_WEB_0001,
            format!("vite 退出码为 0 但未产出 {}", dist.display()),
        )
        .with_hint("检查 vite.config.js 的 build.outDir 与 polypack.toml 的 [ui].entry")
        .into());
    }
    Ok(())
}

/// 内建最小静态打包：拷贝 UI 目录（`public/` 内容提到根），不打包 / 不转译。
///
/// shortcut: 只拷贝，不做打包与转译——无裸模块导入的纯 ESM UI 才能直接跑；模板的 React /
/// JSX 入口（`main.jsx`）必须在 UI 目录 `npm install` 装齐 vite 后由 vite 构建，
/// 否则产物只是原样拷贝，浏览器打不开（[`needs_bundler`] 判据，降级告警会如实说明）。
fn copy_static(ui: &Path, dist: &Path) -> Result<(), StageError> {
    // 构建工具的配置/元数据不进产物（vite 只把构建结果写进 dist）。
    const SKIP: &[&str] = &[
        "node_modules",
        "dist",
        ".polypack",
        "target",
        "public",
        "package.json",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "vite.config.js",
        "vite.config.ts",
        "tsconfig.json",
        ".gitignore",
        "README.md",
    ];
    if dist.exists() {
        std::fs::remove_dir_all(dist).map_err(|err| io_error("清空 dist", dist, err))?;
    }
    copy_tree(ui, dist, SKIP)?;
    // vite 的 publicDir 语义：public/ 的内容落到 dist 根
    let public = ui.join("public");
    if public.is_dir() {
        copy_tree(&public, dist, &[])?;
    }
    Ok(())
}

/// UI 是否含需要打包器处理的源码（JSX / TS 入口）：静态降级只拷贝，产物浏览器跑不了。
///
/// 只看 UI 目录本身（跳过 `node_modules` / `dist` / `target` / `public`：前者是依赖，
/// 后三者 vite 也只是原样拷贝，不算需要转译）。
pub fn needs_bundler(ui: &Path) -> bool {
    const SKIP: &[&str] = &["node_modules", "dist", "target", "public"];
    let Ok(entries) = std::fs::read_dir(ui) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().map(|name| name.to_string_lossy());
            if name.as_deref().is_some_and(|name| SKIP.contains(&name)) {
                continue;
            }
            if needs_bundler(&path) {
                return true;
            }
        } else if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("jsx" | "tsx" | "ts")
        ) {
            return true;
        }
    }
    false
}

/// 递归拷贝目录（`skip` 为任意层级同名的目录 / 文件）。
fn copy_tree(from: &Path, to: &Path, skip: &[&str]) -> Result<(), StageError> {
    std::fs::create_dir_all(to).map_err(|err| io_error("创建目录", to, err))?;
    let entries = std::fs::read_dir(from).map_err(|err| io_error("读取目录", from, err))?;
    for entry in entries {
        let entry = entry.map_err(|err| io_error("读取目录项", from, err))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if skip.contains(&name.as_ref()) {
            continue;
        }
        let path = entry.path();
        let target = to.join(name.as_ref());
        if path.is_dir() {
            copy_tree(&path, &target, skip)?;
        } else {
            std::fs::copy(&path, &target).map_err(|err| io_error("拷贝文件", &path, err))?;
        }
    }
    Ok(())
}

/// 生成 `manifest.webmanifest`（名称 / 图标 / 主题色来自 `polypack.toml`）并注入 link。
pub fn write_manifest(project: &Project, dist: &Path) -> Result<PathBuf, StageError> {
    let index = dist.join("index.html");
    if !index.is_file() {
        return Err(Diagnostic::error(
            codes::E_WEB_0002,
            format!("产物缺少入口 HTML: {}", index.display()),
        )
        .with_hint("确认 [ui].entry 指向的 index.html 存在，且被构建拷贝进 dist")
        .into());
    }
    let icons = collect_icons(dist)?;
    let theme = project
        .config
        .ui
        .theme_color
        .clone()
        .unwrap_or_else(|| DEFAULT_THEME_COLOR.to_string());
    let manifest = Manifest {
        name: project.config.package.name.clone(),
        short_name: short_name(&project.config.package.name),
        start_url: "./".to_string(),
        scope: "./".to_string(),
        display: "standalone".to_string(),
        background_color: "#ffffff".to_string(),
        theme_color: theme,
        icons,
    };
    let path = dist.join("manifest.webmanifest");
    let json = serde_json::to_string_pretty(&manifest).expect("manifest 可序列化");
    std::fs::write(&path, format!("{json}\n"))
        .map_err(|err| io_error("写入 manifest", &path, err))?;
    inject_manifest_link(&index)?;
    Ok(path)
}

/// 注入 `<link rel="manifest">`（已有则不重复注入）。
fn inject_manifest_link(index: &Path) -> Result<(), StageError> {
    let html =
        std::fs::read_to_string(index).map_err(|err| io_error("读取入口 HTML", index, err))?;
    if html.contains("rel=\"manifest\"") {
        return Ok(());
    }
    let Some(head) = html.find("</head>") else {
        return Err(Diagnostic::error(
            codes::E_WEB_0002,
            format!(
                "入口 HTML 缺少 </head>，无法注入 manifest link: {}",
                index.display()
            ),
        )
        .with_hint("补全 <head> 段后重试（PWA manifest 必须由入口 HTML 引用）")
        .into());
    };
    // 插到 `</head>` 所在行的行首并沿用该行缩进：否则 `</head>` 的缩进会被留给新链接；
    // 行首不是纯空白（`</head>` 与其它标签同行）时退化为紧邻 `</head>` 插入。
    let line_start = html[..head].rfind('\n').map_or(0, |index| index + 1);
    let (line_start, indent) = match html[line_start..head].trim().is_empty() {
        true => (line_start, &html[line_start..head]),
        false => (head, ""),
    };
    let link = format!("{indent}<link rel=\"manifest\" href=\"./manifest.webmanifest\">\n");
    let mut patched = String::with_capacity(html.len() + link.len());
    patched.push_str(&html[..line_start]);
    patched.push_str(&link);
    patched.push_str(&html[line_start..]);
    std::fs::write(index, patched).map_err(|err| io_error("写入入口 HTML", index, err))?;
    Ok(())
}

/// 收集图标：`dist/icon.svg` 与 `dist/icons/*.png`（尺寸取自文件名）。
fn collect_icons(dist: &Path) -> Result<Vec<Icon>, StageError> {
    let mut icons = Vec::new();
    if dist.join("icon.svg").is_file() {
        icons.push(Icon {
            src: "icon.svg".to_string(),
            sizes: "any".to_string(),
            kind: "image/svg+xml".to_string(),
            purpose: "any".to_string(),
        });
    }
    let icons_dir = dist.join("icons");
    if icons_dir.is_dir() {
        let entries = std::fs::read_dir(&icons_dir)
            .map_err(|err| io_error("读取图标目录", &icons_dir, err))?;
        let mut pngs: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".png"))
            .collect();
        pngs.sort(); // 目录序与文件系统无关，保证 manifest 可复现
        for name in pngs {
            icons.push(Icon {
                src: format!("icons/{name}"),
                sizes: size_from_name(&name).unwrap_or_else(|| "any".to_string()),
                kind: "image/png".to_string(),
                purpose: "any".to_string(),
            });
        }
    }
    if icons.is_empty() {
        return Err(Diagnostic::error(
            codes::E_WEB_0002,
            format!("产物缺少应用图标资产（{}）", dist.display()),
        )
        .with_hint("把 Pax 派生图标放到 ui/public/icon.svg；位图放 ui/public/icons/icon-<N>.png")
        .into());
    }
    Ok(icons)
}

/// 从文件名取尺寸：`icon-192.png` → `192x192`，`192x192.png` → `192x192`。
fn size_from_name(name: &str) -> Option<String> {
    let digits: Vec<&str> = name
        .trim_end_matches(".png")
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .collect();
    match digits.as_slice() {
        [one] => Some(format!("{one}x{one}")),
        [w, h, ..] => Some(format!("{w}x{h}")),
        [] => None,
    }
}

/// PWA `short_name`：截断到 12 字符（manifest 展示约束）。
fn short_name(name: &str) -> String {
    name.chars().take(12).collect()
}

fn stderr_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines[lines.len().saturating_sub(3)..].join(" / ")
}

fn engine_label(engine: &UiEngine) -> String {
    match engine {
        UiEngine::Vite {
            version: Some(version),
        } => format!("vite {version}"),
        UiEngine::Vite { version: None } => "vite".to_string(),
        UiEngine::Static => "内置静态打包".to_string(),
    }
}

/// 取 vite 版本（探测失败返回 `None`，不影响构建）。
fn vite_version(ui: &Path) -> Option<String> {
    let mut command = if ui.join("node_modules/vite/bin/vite.js").is_file() {
        let mut command = Command::new("node");
        command.arg(ui.join("node_modules/vite/bin/vite.js"));
        command
    } else {
        Command::new("vite")
    };
    let output = command.arg("--version").current_dir(ui).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|version| !version.is_empty())
}

/// PATH 上查找可执行文件（不引入 which 依赖）。
fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(bin))
        .find(|candidate| candidate.is_file())
}

fn io_error(action: &str, path: &Path, err: std::io::Error) -> StageError {
    Diagnostic::error(
        codes::E_IO_0001,
        format!("{action} 失败（{}）: {err}", path.display()),
    )
    .with_hint("检查路径是否存在、权限是否足够")
    .into()
}

/// PWA manifest（§3.2 Web 行：静态站点 + PWA manifest）。
#[derive(Debug, Serialize)]
struct Manifest {
    name: String,
    short_name: String,
    start_url: String,
    scope: String,
    display: String,
    background_color: String,
    theme_color: String,
    icons: Vec<Icon>,
}

/// manifest 图标条目。
#[derive(Debug, Serialize)]
struct Icon {
    src: String,
    sizes: String,
    #[serde(rename = "type")]
    kind: String,
    purpose: String,
}
