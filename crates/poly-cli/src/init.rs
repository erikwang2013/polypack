//! `poly init`：生成示例应用骨架（T-M1-11）或 watchOS 伴生模板（T-M7-05，实验）。
//!
//! 模板资产随二进制内嵌（`crates/poly-cli/templates/`），落盘即得到可 `poly doctor` /
//! `poly build` 的最小工程：app = Rust 核心 crate + 前端骨架 + 四端 L1 目标声明 + Pax 图标；
//! watchos = SwiftUI 伴生工程（核心 API 调用桩 + WatchConnectivity 说明，macOS 宿主
//! `xcodebuild` 构建）。

use std::path::{Path, PathBuf};

use poly_core::{Diagnostic, codes};

/// 模板形态（`--target`；缺省 app，保持 T-M1-11 行为不变）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Template {
    /// 全栈示例应用（React UI + Rust 核心 + 四端 L1 目标）。
    App,
    /// watchOS SwiftUI 伴生模板（实验；核心 API 调用桩 + WatchConnectivity 说明）。
    Watchos,
}

impl Template {
    /// 报告字段 / 提示用的稳定名称。
    fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Watchos => "watchos",
        }
    }

    /// 本形态的模板文件清单。
    fn files(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::App => APP_FILES,
            Self::Watchos => WATCHOS_FILES,
        }
    }
}

/// 全栈示例应用模板：`(相对路径, 内容)`；UI 为 React 骨架（vite + @vitejs/plugin-react），
/// 图标为 `docs/assets/pax-mark.svg` 的副本（单一品牌源，见 tests/init.rs 的逐字节断言）。
const APP_FILES: &[(&str, &str)] = &[
    (".gitignore", include_str!("../templates/app/.gitignore")),
    ("README.md", include_str!("../templates/app/README.md")),
    (
        "polypack.toml",
        include_str!("../templates/app/polypack.toml"),
    ),
    (
        "core/Cargo.toml",
        include_str!("../templates/app/core/Cargo.toml"),
    ),
    (
        "core/src/main.rs",
        include_str!("../templates/app/core/src/main.rs"),
    ),
    (
        "ui/index.html",
        include_str!("../templates/app/ui/index.html"),
    ),
    ("ui/main.jsx", include_str!("../templates/app/ui/main.jsx")),
    (
        "ui/package.json",
        include_str!("../templates/app/ui/package.json"),
    ),
    (
        "ui/tokens.css",
        include_str!("../templates/app/ui/tokens.css"),
    ),
    (
        "ui/vite.config.js",
        include_str!("../templates/app/ui/vite.config.js"),
    ),
    (
        "ui/public/icon.svg",
        include_str!("../templates/app/ui/public/icon.svg"),
    ),
    (
        "ui/public/sw.js",
        include_str!("../templates/app/ui/public/sw.js"),
    ),
];

/// watchOS 伴生模板（T-M7-05，实验）：最小可 `xcodebuild archive` 的 SwiftUI 工程。
///
/// 手表端 runtime = none：核心不直接运行，`CoreBridge` 只是核心 API 调用桩，
/// `WatchConnectivity` 给出经 iOS 伴生 App 代理的通道说明；不含任何明文凭据
/// （签名档案 `apple-codesign` 只在注册表 / polypack.toml 里以引用名出现）。
const WATCHOS_FILES: &[(&str, &str)] = &[
    (
        ".gitignore",
        include_str!("../templates/watchos/.gitignore"),
    ),
    ("README.md", include_str!("../templates/watchos/README.md")),
    (
        "polypack.toml",
        include_str!("../templates/watchos/polypack.toml"),
    ),
    (
        "WatchApp/PolyPackWatch.swift",
        include_str!("../templates/watchos/WatchApp/PolyPackWatch.swift"),
    ),
    (
        "WatchApp/ContentView.swift",
        include_str!("../templates/watchos/WatchApp/ContentView.swift"),
    ),
    (
        "WatchApp/CoreBridge.swift",
        include_str!("../templates/watchos/WatchApp/CoreBridge.swift"),
    ),
    (
        "WatchApp/WatchConnectivity.swift",
        include_str!("../templates/watchos/WatchApp/WatchConnectivity.swift"),
    ),
    (
        "WatchApp/Info.plist",
        include_str!("../templates/watchos/WatchApp/Info.plist"),
    ),
    (
        "PolyPackWatch.xcodeproj/project.pbxproj",
        include_str!("../templates/watchos/PolyPackWatch.xcodeproj/project.pbxproj"),
    ),
    (
        "PolyPackWatch.xcodeproj/xcshareddata/xcschemes/PolyPackWatch.xcscheme",
        include_str!(
            "../templates/watchos/PolyPackWatch.xcodeproj/xcshareddata/xcschemes/PolyPackWatch.xcscheme"
        ),
    ),
];

/// `poly init` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 生成目录（缺省当前目录）。
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// 模板形态（app | watchos）。
    #[arg(long, value_enum, default_value = "app")]
    pub target: Template,
    /// 覆盖已存在的模板文件。
    #[arg(long)]
    pub force: bool,
}

/// 写入模板；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let root = &args.path;
    std::fs::create_dir_all(root).map_err(|err| crate::project::io_error("创建目录", root, err))?;
    let files = args.target.files();

    if !args.force {
        let existing: Vec<&str> = files
            .iter()
            .map(|(path, _)| *path)
            .filter(|path| root.join(path).exists())
            .collect();
        if !existing.is_empty() {
            return Err(Diagnostic::error(
                codes::E_INIT_0001,
                format!("目标目录已存在模板文件: {}", existing.join("、")),
            )
            .with_hint("在空目录执行 poly init，或加 --force 覆盖已有文件"));
        }
    }

    let mut written = Vec::new();
    for (path, content) in files {
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| crate::project::io_error("创建目录", parent, err))?;
        }
        std::fs::write(&target, content)
            .map_err(|err| crate::project::io_error("写入模板文件", &target, err))?;
        written.push(*path);
    }

    if json {
        let report = serde_json::json!({
            "event": "init",
            "target": args.target.as_str(),
            "root": display_path(root),
            "files": written,
        });
        println!(
            "{}",
            serde_json::to_string(&report).expect("init 报告可序列化")
        );
    } else if args.target == Template::Watchos {
        println!(
            "已生成 PolyPack watchOS 伴生模板（实验，{} 个文件）：",
            written.len()
        );
        for path in &written {
            println!("  {path}");
        }
        println!(
            "下一步：poly doctor → macOS 宿主 xcodebuild -project PolyPackWatch.xcodeproj \
             -scheme PolyPackWatch -destination 'generic/platform=watchOS' build → \
             poly build --target watchos（Linux / Windows 走 poly farm submit --target watchos）"
        );
    } else {
        println!("已生成 PolyPack 示例应用（{} 个文件）：", written.len());
        for path in &written {
            println!("  {path}");
        }
        println!("下一步：poly doctor → poly build --target all → poly dev");
    }
    Ok(0)
}

fn display_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}
