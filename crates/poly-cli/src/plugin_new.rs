//! `poly plugin new` / `poly plugin digest`（T-M8-04）：插件模板脚手架与 SRI 摘要。
//!
//! 模板资产在编译期以 `include_str!` 内嵌（`templates/plugin-target/`），
//! 安装后的 `poly` 二进制不依赖源码仓库路径；生成时替换 id / 显示名 / 包名占位符。

use std::path::{Path, PathBuf};

use poly_core::{Diagnostic, codes};

/// 插件 id 占位符。
const ID_TOKEN: &str = "__polypack_plugin_id__";
/// 显示名占位符。
const NAME_TOKEN: &str = "__polypack_plugin_name__";
/// Cargo 包名占位符。
const CRATE_TOKEN: &str = "__polypack_plugin_crate__";

/// 模板文件清单：`(相对路径, 内容)`。
const FILES: &[(&str, &str)] = &[
    (
        ".gitignore",
        include_str!("../../../templates/plugin-target/.gitignore"),
    ),
    (
        "Cargo.toml",
        include_str!("../../../templates/plugin-target/Cargo.toml"),
    ),
    (
        "README.md",
        include_str!("../../../templates/plugin-target/README.md"),
    ),
    (
        "build.sh",
        include_str!("../../../templates/plugin-target/build.sh"),
    ),
    (
        ".github/workflows/ci.yml",
        include_str!("../../../templates/plugin-target/.github/workflows/ci.yml"),
    ),
    (
        "plugin/Cargo.toml",
        include_str!("../../../templates/plugin-target/plugin/Cargo.toml"),
    ),
    (
        "plugin/src/lib.rs",
        include_str!("../../../templates/plugin-target/plugin/src/lib.rs"),
    ),
    (
        "plugin/wit/polypack-target.wit",
        include_str!("../../../templates/plugin-target/plugin/wit/polypack-target.wit"),
    ),
    (
        "componentize/Cargo.toml",
        include_str!("../../../templates/plugin-target/componentize/Cargo.toml"),
    ),
    (
        "componentize/src/main.rs",
        include_str!("../../../templates/plugin-target/componentize/src/main.rs"),
    ),
    (
        "contract/Cargo.toml",
        include_str!("../../../templates/plugin-target/contract/Cargo.toml"),
    ),
    (
        "contract/src/lib.rs",
        include_str!("../../../templates/plugin-target/contract/src/lib.rs"),
    ),
    (
        "contract/tests/contract.rs",
        include_str!("../../../templates/plugin-target/contract/tests/contract.rs"),
    ),
];

/// `poly plugin` 子命令。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 子命令。
    #[command(subcommand)]
    pub command: Command,
}

/// `poly plugin` 子命令集合。
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// 从内嵌模板生成插件工程（poly plugin new <id> [--dir <path>]）。
    New(NewArgs),
    /// 输出插件 wasm 制品摘要（blake3-<hex>，与宿主同口径）。
    Digest(DigestArgs),
}

/// `poly plugin new` 参数。
#[derive(Debug, clap::Args)]
pub struct NewArgs {
    /// 插件 id（kebab-case，如 my-target）。
    pub id: String,
    /// 生成目录（缺省 ./<id>）。
    #[arg(long)]
    pub dir: Option<PathBuf>,
    /// 目标目录非空时仍生成（覆盖同名模板文件）。
    #[arg(long)]
    pub force: bool,
}

/// `poly plugin digest` 参数。
#[derive(Debug, clap::Args)]
pub struct DigestArgs {
    /// wasm 组件路径（build.sh 产物）。
    pub wasm: PathBuf,
}

/// 分发 `poly plugin` 子命令；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    match &args.command {
        Command::New(args) => new(args, json),
        Command::Digest(args) => digest(args, json),
    }
}

/// 生成插件工程。
fn new(args: &NewArgs, json: bool) -> Result<i32, Diagnostic> {
    let id = args.id.as_str();
    validate_id(id)?;
    let root = args.dir.clone().unwrap_or_else(|| PathBuf::from(id));
    let crate_name = format!("polypack-target-{id}");
    let display = display_name(id);

    if !args.force {
        let entries = dir_entries(&root)?;
        if entries > 0 {
            return Err(Diagnostic::error(
                codes::E_INIT_0001,
                format!("目标目录非空（{} 个条目）: {}", entries, root.display()),
            )
            .with_hint("选空目录，或加 --force 覆盖同名模板文件"));
        }
    }

    std::fs::create_dir_all(&root)
        .map_err(|err| crate::project::io_error("创建目录", &root, err))?;
    let mut written = Vec::new();
    for (path, content) in FILES {
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| crate::project::io_error("创建目录", parent, err))?;
        }
        let rendered = render(content, id, &display, &crate_name);
        std::fs::write(&target, rendered)
            .map_err(|err| crate::project::io_error("写入模板文件", &target, err))?;
        written.push(*path);
    }
    make_executable(&root.join("build.sh"));

    if json {
        let report = serde_json::json!({
            "event": "plugin-new",
            "id": id,
            "dir": display_path(&root),
            "files": written,
        });
        println!(
            "{}",
            serde_json::to_string(&report).expect("plugin new 报告可序列化")
        );
    } else {
        println!(
            "已生成 PolyPack 插件工程（{} 个文件）: {}",
            written.len(),
            root.display()
        );
        for path in &written {
            println!("  {path}");
        }
        println!("下一步：");
        println!(
            "  1. cd {} && ./build.sh   # 产物 dist/plugin.wasm",
            root.display()
        );
        println!("  2. 在应用 polypack.toml 注册：");
        println!("     [[plugins]]");
        println!("     id = \"{id}\"");
        println!("     path = \"plugins/{id}.wasm\"");
        println!("     sri = \"blake3-<hex>\"   # 可选；设置后加载前强制校验");
        println!(
            "  3. SRI 来源：poly plugin digest dist/plugin.wasm → blake3-<hex>（与宿主加载同口径）"
        );
        println!(
            "  4. 复制产物到应用：mkdir -p <app>/plugins && cp dist/plugin.wasm <app>/plugins/{id}.wasm"
        );
        println!(
            "     自定义 describe/package 后同步 contract/src/lib.rs 的 EXPECTED_FORMAT/EXPECTED_PACKAGE"
        );
    }
    Ok(0)
}

/// 输出制品摘要（宿主 `content_digest` 同口径）。
fn digest(args: &DigestArgs, json: bool) -> Result<i32, Diagnostic> {
    let bytes = std::fs::read(&args.wasm)
        .map_err(|err| crate::project::io_error("读取插件制品", &args.wasm, err))?;
    let digest = poly_plugin::content_digest(&bytes);
    if json {
        let report = serde_json::json!({
            "event": "plugin-digest",
            "path": display_path(&args.wasm),
            "digest": digest,
        });
        println!(
            "{}",
            serde_json::to_string(&report).expect("plugin digest 报告可序列化")
        );
    } else {
        println!("{digest}");
    }
    Ok(0)
}

/// 校验插件 id：kebab-case，1..=64 字符。
fn validate_id(id: &str) -> Result<(), Diagnostic> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if valid {
        Ok(())
    } else {
        Err(
            Diagnostic::error(codes::E_INIT_0001, format!("插件 id 非法: {id:?}"))
                .with_hint("id 用 kebab-case（小写字母 / 数字 / 连字符，如 my-target）"),
        )
    }
}

/// 显示名：`my-target` → `My Target`。
fn display_name(id: &str) -> String {
    id.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 替换模板占位符。
fn render(content: &str, id: &str, display: &str, crate_name: &str) -> String {
    content
        .replace(ID_TOKEN, id)
        .replace(NAME_TOKEN, display)
        .replace(CRATE_TOKEN, crate_name)
}

/// 目录条目数；不存在按 0 计。
fn dir_entries(dir: &Path) -> Result<usize, Diagnostic> {
    match std::fs::read_dir(dir) {
        Ok(entries) => Ok(entries.filter_map(Result::ok).count()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(err) => Err(crate::project::io_error("读取目标目录", dir, err)),
    }
}

/// 脚手架脚本保持可执行（include_str! 不带文件模式）。
fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn display_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_validation_accepts_kebab_and_rejects_bad_input() {
        assert!(validate_id("my-target").is_ok());
        assert!(validate_id("t2").is_ok());
        for bad in ["", "-x", "x-", "x--y", "My-Target", "a/b", "a_b", "x y"] {
            assert!(validate_id(bad).is_err(), "应拒绝: {bad:?}");
        }
        assert!(validate_id(&"a".repeat(65)).is_err());
    }

    #[test]
    fn display_name_title_cases_words() {
        assert_eq!(display_name("my-target"), "My Target");
        assert_eq!(display_name("t2"), "T2");
    }

    #[test]
    fn render_leaves_no_placeholder_in_any_template_file() {
        for (path, content) in FILES {
            let rendered = render(
                content,
                "demo-target",
                "Demo Target",
                "polypack-target-demo-target",
            );
            for token in [ID_TOKEN, NAME_TOKEN, CRATE_TOKEN] {
                assert!(!rendered.contains(token), "{path} 残留占位符 {token}");
            }
        }
    }

    #[test]
    fn template_wit_matches_host_contract() {
        let template =
            include_str!("../../../templates/plugin-target/plugin/wit/polypack-target.wit");
        let host = include_str!("../../poly-plugin/wit/polypack-target.wit");
        assert_eq!(template, host, "模板 WIT 与宿主 target-spec/v1 漂移");
    }
}
