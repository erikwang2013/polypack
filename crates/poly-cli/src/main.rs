//! `poly` 命令行入口（ROADMAP §4.1）。

mod doctor;

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
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Doctor(args) => doctor::run(&args, cli.json),
    };
    match result {
        Ok(code) => std::process::ExitCode::from(code as u8),
        Err(diagnostic) => {
            eprintln!("{diagnostic}");
            std::process::ExitCode::FAILURE
        }
    }
}
