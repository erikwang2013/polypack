//! 外部命令执行：工具探测、运行与错误归因。

use std::path::PathBuf;
use std::process::Output;

use crate::error::BundleError;
use crate::packager::Command;

/// 工具是否可在 PATH 上找到（仅用于计划阶段的 `skipped` 记录；执行仍以实际调用为准）。
pub fn tool_present(program: &str) -> bool {
    tool_path(program).is_some()
}

/// 在 PATH 中解析可执行文件（Windows 额外尝试 PATHEXT 扩展名）。
pub fn tool_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".to_string())
            .split(';')
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        Vec::new()
    };
    for dir in std::env::split_paths(&path) {
        let direct = dir.join(program);
        if direct.is_file() {
            return Some(direct);
        }
        for ext in &exts {
            let with_ext = dir.join(format!("{program}{ext}"));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }
    }
    None
}

/// 执行一条命令（捕获 stdout / stderr）。
///
/// 缺席的工具 → [`BundleError::ToolMissing`]（附安装建议）；非零退出 → [`BundleError::ToolFailed`]。
pub fn run(command: &Command) -> Result<Output, BundleError> {
    let mut cmd = std::process::Command::new(&command.program);
    cmd.args(&command.args);
    if let Some(cwd) = &command.cwd {
        cmd.current_dir(cwd);
    }
    let output = cmd.output().map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            BundleError::ToolMissing {
                packager: "extern",
                tool: command.program.clone(),
                hint: install_hint(&command.program),
            }
        } else {
            BundleError::ToolFailed {
                command: command.line(),
                status: "spawn-failed".to_string(),
                stderr: source.to_string(),
            }
        }
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BundleError::ToolFailed {
            command: command.line(),
            status: output
                .status
                .code()
                .map_or_else(|| "signal".to_string(), |c| c.to_string()),
            stderr: tail(&stderr, 800),
        });
    }
    Ok(output)
}

/// 依次执行命令序列，任一失败即中止。
pub fn run_all(commands: &[Command]) -> Result<(), BundleError> {
    for command in commands {
        run(command)?;
    }
    Ok(())
}

/// 内部工具缺失时的安装建议。
pub(crate) fn install_hint(tool: &str) -> String {
    match tool {
        "dpkg-deb" => "安装 dpkg（Debian / Ubuntu 宿主通常已预装）".to_string(),
        "appimagetool" => "从 github.com/AppImage/appimagetool 下载，或改用 deb 打包器".to_string(),
        "hdiutil" => "hdiutil 仅随 macOS 提供；DMG 打包需 macOS 宿主".to_string(),
        "candle" | "light" => "安装 WiX Toolset v3（windows-2022 runner 已预装）".to_string(),
        "makensis" => {
            "安装 NSIS ≥ 3.08（Linux：apt install nsis；Windows：choco install nsis）".to_string()
        }
        "rsvg-convert" => {
            "安装 librsvg2-bin（Debian / Ubuntu：apt install librsvg2-bin）".to_string()
        }
        "magick" => {
            "安装 ImageMagick（Debian / Ubuntu：apt install imagemagick）——ICO 派生所需".to_string()
        }
        other => format!("安装 {other} 后重试"),
    }
}

/// 取字符串尾部（诊断信息限长，避免报告被截断日志淹没）。
fn tail(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let skip = trimmed.chars().count() - limit;
    trimmed.chars().skip(skip).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_tool_is_reported_not_ignored() {
        let err = run(&Command::new("poly-definitely-missing-tool", ["--version"]))
            .expect_err("缺失工具必须报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0001);
        assert!(matches!(err, BundleError::ToolMissing { .. }));
    }

    #[test]
    fn failing_tool_carries_stderr() {
        // 三宿主通用：Unix 用 sh，Windows 用 cmd（CI 三腿都要跑）。
        let failing = if cfg!(windows) {
            Command::new("cmd", ["/C", "echo boom 1>&2 & exit 3"])
        } else {
            Command::new("sh", ["-c", "echo boom >&2; exit 3"])
        };
        let err = run(&failing).expect_err("非零退出");
        match err {
            BundleError::ToolFailed { status, stderr, .. } => {
                assert_eq!(status, "3");
                assert!(stderr.contains("boom"), "stderr 须带诊断：{stderr}");
            }
            other => panic!("期望 ToolFailed，实得 {other:?}"),
        }
    }

    #[test]
    fn tool_present_matches_path_lookup() {
        // 正向用例取各宿主必有的 shell（Unix: sh / Windows: cmd.exe）。
        let present = if cfg!(windows) { "cmd.exe" } else { "sh" };
        assert!(tool_present(present), "{present} 应可解析");
        assert!(!tool_present("poly-definitely-missing-tool"));
    }

    #[test]
    fn tail_keeps_end() {
        let long = "x".repeat(2000) + "END";
        let out = tail(&long, 10);
        assert!(out.ends_with("END"));
        assert_eq!(out.chars().count(), 10);
    }
}
