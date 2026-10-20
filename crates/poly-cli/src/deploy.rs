//! `poly deploy`：部署模式（非 TargetSpec，M9-P3 / T-M9-26/27）。
//!
//! `line-liff` 只在本地校验 LIFF 托管配置与静态目录，然后输出 LINE Developers
//! Console 人工步骤。命令始终是 dry-run：不触网、不上传、不生成平台包，也不声明
//! TargetSpec。LIFF 官方没有本地上传包格式或 CI 上传 API，把它做成 TargetSpec 会
//! 伪造不存在的平台产物；设计与边界见 `docs/decisions/0009-line-deploy.md`。

use std::path::{Path, PathBuf};

use poly_core::codes;
use poly_core::diagnostics::Diagnostic;
use serde::Deserialize;

/// `poly deploy` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 子命令。
    #[command(subcommand)]
    pub command: Command,
}

/// `poly deploy` 子命令。
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// 校验 LINE LIFF 托管配置并输出 Console 人工步骤（dry-run，不触网）。
    LineLiff(LineLiffArgs),
}

/// `poly deploy line-liff` 的参数。
#[derive(Debug, clap::Args)]
pub struct LineLiffArgs {
    /// Web 构建产物目录（只校验存在，不上传）。
    #[arg(long)]
    pub dir: PathBuf,
    /// LIFF 配置文件（line-liff.toml）。
    #[arg(long, default_value = "line-liff.toml")]
    pub config: PathBuf,
}

/// `line-liff.toml` schema（schema = 1；未知字段直接拒绝）。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiffConfig {
    schema: u32,
    liff_id: String,
    endpoint: String,
    channel_id: String,
    scopes: Vec<String>,
}

/// 执行 `poly deploy`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    match &args.command {
        Command::LineLiff(args) => line_liff(args, json),
    }
}

/// 校验配置与目录，输出人工步骤；不读写网络、不写任何文件。
fn line_liff(args: &LineLiffArgs, json: bool) -> Result<i32, Diagnostic> {
    if !args.dir.is_dir() {
        return Err(
            input_error(format!("--dir 不是目录：{}", args.dir.display()))
                .with_hint("先运行 poly build --target web，再用 --dir 指向构建产物目录"),
        );
    }
    let config = load_config(&args.config)?;
    let steps = console_steps(&config, &args.dir);

    if json {
        let event = serde_json::json!({
            "event": "deploy_dry_run",
            "mode": "line-liff",
            "config": args.config.display().to_string(),
            "dir": args.dir.display().to_string(),
            "liff_id": config.liff_id,
            "channel_id": config.channel_id,
            "endpoint": config.endpoint,
            "scopes": config.scopes,
            "dry_run": true,
            "network": false,
            "target_spec": false,
            "steps": steps,
        });
        println!(
            "{}",
            serde_json::to_string(&event).expect("deploy 事件可序列化")
        );
    } else {
        println!("poly deploy line-liff — dry-run（不触网、不上传、不生成平台包）");
        println!("  配置: {}", args.config.display());
        println!("  静态目录: {}", args.dir.display());
        println!(
            "  LIFF ID: {}；Channel ID: {}；Endpoint: {}",
            config.liff_id, config.channel_id, config.endpoint
        );
        println!("  Scopes: {}", config.scopes.join(", "));
        for (index, step) in steps.iter().enumerate() {
            println!("  {}. {step}", index + 1);
        }
    }
    Ok(0)
}

/// 读取并校验配置；解析失败与字段非法都归一到 `E_DEPLOY_0001`。
fn load_config(path: &Path) -> Result<LiffConfig, Diagnostic> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        input_error(format!("读取 LIFF 配置失败：{}（{err}）", path.display()))
            .with_hint("复制 docs/LINE-DEPLOY.md 的 line-liff.toml 示例，再用 --config 指向它")
    })?;
    let config: LiffConfig = toml::from_str(&text)
        .map_err(|err| input_error(format!("解析 LIFF 配置失败：{}（{err}）", path.display())))?;
    validate_config(&config)?;
    Ok(config)
}

/// 必填字段、schema 版本、Endpoint 规则与 Channel ID 形态。
fn validate_config(config: &LiffConfig) -> Result<(), Diagnostic> {
    if config.schema != 1 {
        return Err(input_error(format!(
            "不支持的 line-liff schema {}（当前只支持 1）",
            config.schema
        )));
    }
    for (name, value) in [
        ("liff_id", &config.liff_id),
        ("endpoint", &config.endpoint),
        ("channel_id", &config.channel_id),
    ] {
        if value.trim().is_empty() {
            return Err(input_error(format!("line-liff 缺少必填字段：{name}")));
        }
        if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(input_error(format!(
                "line-liff 字段 {name} 不能包含空白或控制字符"
            )));
        }
    }
    if !config.channel_id.chars().all(|c| c.is_ascii_digit()) {
        return Err(input_error(format!(
            "channel_id 必须是数字：{}",
            config.channel_id
        )));
    }
    validate_endpoint(&config.endpoint)?;
    if config.scopes.is_empty() || config.scopes.iter().any(|scope| scope.trim().is_empty()) {
        return Err(input_error("line-liff 的 scopes 不能为空，且每项必须非空"));
    }
    Ok(())
}

/// LIFF Endpoint URL：HTTPS、有主机名、不含 fragment（官方 Console 规则）。
fn validate_endpoint(endpoint: &str) -> Result<(), Diagnostic> {
    let rest = endpoint
        .split_once("://")
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("https"))
        .map(|(_, rest)| rest)
        .ok_or_else(|| input_error(format!("endpoint 必须是 HTTPS URL：{endpoint}")))?;
    if endpoint.contains('#') {
        return Err(input_error(format!(
            "endpoint 不能包含 fragment（#…）：{endpoint}"
        )));
    }
    let authority = rest.split(['/', '?']).next().unwrap_or_default();
    if authority.is_empty() {
        return Err(input_error(format!("endpoint 缺少主机名：{endpoint}")));
    }
    Ok(())
}

/// Console 人工步骤；人读与 JSON 输出共用，避免两份漂移。
fn console_steps(config: &LiffConfig, dir: &Path) -> Vec<String> {
    vec![
        format!(
            "将 {} 的内容部署到 {}（HTTPS；URL 不得含 fragment）。",
            dir.display(),
            config.endpoint
        ),
        format!(
            "打开 LINE Developers Console → Provider → Channel {} → LINE Login → LIFF → Add。",
            config.channel_id
        ),
        format!(
            "Endpoint URL 填 {}；Scopes 填 {}；LIFF app name 按应用填写。",
            config.endpoint,
            config.scopes.join(", ")
        ),
        "保存后打开 Console 给出的 LIFF URL，确认页面加载正常。".to_string(),
        "上传 / 发布由人工在 Console 完成；本命令不调用 LINE API、不触网。".to_string(),
    ]
}

/// 稳定的输入错误（`poly::E_DEPLOY_0001`）。
fn input_error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(codes::E_DEPLOY_0001, message).with_hint(
        "检查 line-liff.toml：schema = 1，liff_id / endpoint / channel_id / scopes 必填，endpoint 须为 HTTPS 且不含 fragment",
    )
}
