//! `poly` 命令行入口（ROADMAP §4.1）。

mod bridge;
mod build;
mod build_artifacts;
mod clean;
mod dev;
mod doctor;
mod farm;
mod history;
mod init;
mod mood;
mod package;
mod plugin_new;
mod preview;
mod project;
mod providers;
mod publish;
mod release;
mod retry;
mod sbom;
mod scan;
mod sign;
mod smoke;
mod targets;
mod update;
mod verify;
mod verify_embedded;
mod verify_freertos;
mod verify_rtos;
mod verify_smoke;

use clap::{Parser, Subcommand};

/// PolyPack —— 一次编写、全端打包。
#[derive(Debug, Parser)]
#[command(name = "poly", version, about = "PolyPack —— 一次编写、全端打包")]
struct Cli {
    /// 以 JSON 事件流输出（机器可读）。
    #[arg(long, global = true)]
    json: bool,

    /// 子命令。
    #[command(subcommand)]
    command: Command,
}

/// 子命令集合。
#[derive(Debug, Subcommand)]
enum Command {
    /// 探测本机工具链并输出报告。
    Doctor(doctor::Args),
    /// 生成示例应用骨架（poly init [目录]）。
    Init(init::Args),
    /// 按 polypack.toml 构建（compile 与 bundle(ui) 双支线，带内容寻址缓存）。
    Build(build::Args),
    /// 构建历史：按目标查看最近 N 次状态 / 耗时 / 产物（`.polypack/history/events.jsonl`）。
    History(history::Args),
    /// 启动 Web 宿主热预览（整页重载形态）。
    Dev(dev::Args),
    /// 清理构建缓存与产物快照（默认 dry-run）。
    Clean(clean::Args),
    /// 跨语言桥：由单处接口定义生成 Kotlin / Swift 绑定，并组装 Android AAR。
    Bridge(bridge::Args),
    /// 远程农场：worker 注册与拉取-执行-回传循环。
    Farm(farm::Args),
    /// 设备预览：生成设备租约计划（`--dry-run` 不接触设备或网络）。
    Preview(preview::Args),
    /// secret-scan：提交与构建双重密钥检查。
    Scan(scan::Args),
    /// 从签名阶段独立重跑（重新拉取凭据、重签未签名 APK）。
    Retry(retry::Args),
    /// 产物校验：缺省跑目标声明的校验器（codesign / spctl），`--reproducible` 做二跑比对。
    Verify(verify::Args),
    /// 内建目标清单（CI 矩阵派生：`poly targets --tier 6 --headless --json`）。
    Targets(targets::Args),
    /// 插件开发：生成工程、校验工程 / WIT 同步、输出制品 SRI 摘要。
    Plugin(plugin_new::Args),
    /// 商店提审（R4 止步提审）：产物定位 → 凭据引用解析 → 上传 / 提审 / 轮询（幂等重跑）。
    Publish(publish::Args),
    /// 多目标仓库发布：产物集合 → 仓库元数据刷新与签名 → 回执归档（幂等重跑）。
    Release(release::Args),
    /// OTA 更新通道：生成并签名 Tauri v2 更新清单 / 校验清单与签名。
    Update(update::Args),
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    // 启动横幅与结束表情只走人读通道（T-M1-14）：--json 的 stdout 必须是纯 JSON Lines
    if !cli.json {
        println!("{}", mood::banner(mood::color_enabled()));
    }
    let result = match cli.command {
        Command::Doctor(args) => doctor::run(&args, cli.json),
        Command::Init(args) => init::run(&args, cli.json),
        Command::Build(args) => build::run(&args, cli.json),
        Command::History(args) => history::run(&args, cli.json),
        Command::Dev(args) => dev::run(&args, cli.json),
        Command::Clean(args) => clean::run(&args, cli.json),
        Command::Bridge(args) => bridge::run(&args, cli.json),
        Command::Farm(args) => farm::run(&args, cli.json),
        Command::Preview(args) => preview::run(&args, cli.json),
        Command::Scan(args) => scan::run(&args, cli.json),
        Command::Retry(args) => retry::run(&args, cli.json),
        Command::Verify(args) => verify::run(&args, cli.json),
        Command::Targets(args) => targets::run(&args, cli.json),
        Command::Plugin(args) => plugin_new::run(&args, cli.json),
        Command::Publish(args) => publish::run(&args, cli.json),
        Command::Release(args) => release::run(&args, cli.json),
        Command::Update(args) => update::run(&args, cli.json),
    };
    if !cli.json {
        let (face, text) = match &result {
            Ok(code) => mood::for_run(*code, mood::warnings()),
            Err(_) => (mood::Mood::Cry, "失败（见上方错误）".to_string()),
        };
        println!("{}", mood::line(face, &text, mood::color_enabled()));
    }
    match result {
        Ok(code) => std::process::ExitCode::from(code as u8),
        Err(diagnostic) => {
            eprintln!("{diagnostic}");
            std::process::ExitCode::FAILURE
        }
    }
}
