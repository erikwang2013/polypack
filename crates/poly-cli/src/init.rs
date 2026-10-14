//! `poly init`：生成示例应用骨架（T-M1-11）。
//!
//! 模板资产随二进制内嵌（`crates/poly-cli/templates/app/`），落盘即得到可 `poly doctor` /
//! `poly build` 的最小工程：Rust 核心 crate + 前端骨架 + 四端 L1 目标声明 + Pax 图标。

use std::path::{Path, PathBuf};

use poly_core::{Diagnostic, codes};

/// 模板文件清单：`(相对路径, 内容)`；UI 为 React 骨架（vite + @vitejs/plugin-react），
/// 图标为 `docs/assets/pax-mark.svg` 的副本（单一品牌源，见 tests/init.rs 的逐字节断言）。
const FILES: &[(&str, &str)] = &[
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
];

/// `poly init` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 生成目录（缺省当前目录）。
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// 覆盖已存在的模板文件。
    #[arg(long)]
    pub force: bool,
}

/// 写入模板；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let root = &args.path;
    std::fs::create_dir_all(root).map_err(|err| crate::project::io_error("创建目录", root, err))?;

    if !args.force {
        let existing: Vec<&str> = FILES
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
    for (path, content) in FILES {
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
            "root": display_path(root),
            "files": written,
        });
        println!(
            "{}",
            serde_json::to_string(&report).expect("init 报告可序列化")
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
