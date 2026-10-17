//! QNX / Zephyr QEMU 冒烟共用运行器与结果解析（T-M6-09 / T-M6-10）。
//!
//! 两条冒烟腿（`qnx-qemu-smoke` / `zephyr-qemu-smoke`）只负责准备镜像 / 工程并启动
//! QEMU；判定、失败输出与日志归档全部走本模块——**没有第二套判定逻辑**。
//!
//! 自检程序（QNX 镜像内程序、Zephyr `src/main.c`）的输出契约：
//! - 至少一行 `poly_engine version=`；
//! - 每项自检一行 `[POLYPACK-STEP <name>] PASS|FAIL`；
//! - 全部通过时打印 `SMOKE PASS`；
//! - 进程退出码符合板级约定（Zephyr ARM semihosting = 0；Zephyr x86 isa-debug-exit =
//!   `(0 << 1) | 1` = 1；QNX 镜像 shutdown = 0）。
//!
//! 失败输出固定为 `Failed{stage, code, hint}`（`Display` 实现）：`stage` 是具名自检步骤
//! （或 `qemu-run` / `spawn` / `archive`），`code` 是稳定的 `poly::E_SMOKE_*` 码，`hint`
//! 给出复跑路径。串口日志与机器可读结果落在
//! `<root>/.polypack/logs/smoke/<target>/<case>.{log,result.json}`，CI 直接归档整个目录；
//! 日志里的 `[POLYPACK-STEP ...] FAIL` 行即具体自检项。

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use poly_core::diagnostics::codes;
use poly_core::diagnostics::{Code, Diagnostic};

/// 引擎版本行前缀（与 C ABI 自检输出一致）。
pub(crate) const VERSION_MARKER: &str = "poly_engine version=";
/// 具名自检步骤成功标记。
pub(crate) const STEP_PASS: &str = "PASS";
/// 具名自检步骤失败标记。
pub(crate) const STEP_FAIL: &str = "FAIL";
/// 全绿终标记。
pub(crate) const FINAL_MARKER: &str = "SMOKE PASS";

/// 一次冒烟运行的准备参数（命令 + 判定契约 + 归档根）。
#[derive(Debug, Clone)]
pub(crate) struct SmokeRun {
    /// 目标 id（日志目录名）。
    pub target: String,
    /// 板型 / 镜像名（日志文件名）。
    pub case: String,
    /// 可执行文件（west / qemu-system-*）。
    pub program: PathBuf,
    /// 命令参数。
    pub args: Vec<String>,
    /// 工作目录。
    pub cwd: Option<PathBuf>,
    /// 追加环境变量。
    pub env: Vec<(String, String)>,
    /// 板级约定的成功退出码。
    pub expected_exit: i32,
    /// 必需的具名自检步骤（按自检顺序）。
    pub steps: Vec<String>,
    /// `.polypack/logs/smoke` 归档根。
    pub log_root: PathBuf,
    /// 超时（含构建 + QEMU 运行）。
    pub timeout: Duration,
}

/// 冒烟通过后的证据（供 verify 腿写事件 / job summary）。
#[derive(Debug, Clone)]
pub(crate) struct SmokeReport {
    /// 引擎版本行。
    pub version_line: String,
    /// 实际退出码。
    pub exit_code: i32,
    /// 串口日志路径。
    pub log: PathBuf,
    /// 结果 JSON 路径。
    pub result: PathBuf,
}

/// 冒烟失败：`Display` 即验收要求的 `Failed{stage, code, hint}`。
#[derive(Debug, Clone)]
pub(crate) struct SmokeFailure {
    /// 失败阶段（具名自检步骤 / `qemu-run` / `spawn` / `archive`）。
    pub stage: String,
    /// 稳定诊断码。
    pub code: Code,
    /// 可执行的修复 / 复跑提示。
    pub hint: String,
    /// 串口日志路径（可能尚未落盘）。
    pub log: PathBuf,
}

impl fmt::Display for SmokeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Failed{{stage={}, code={}, hint={}}}",
            self.stage, self.code, self.hint
        )
    }
}

impl SmokeFailure {
    fn new(stage: impl Into<String>, code: Code, hint: impl Into<String>, log: PathBuf) -> Self {
        Self {
            stage: stage.into(),
            code,
            hint: hint.into(),
            log,
        }
    }

    /// 转成 CLI 诊断：消息保留 `Failed{...}`，hint 与日志路径同时可见。
    pub(crate) fn diagnostic(&self, target: &str) -> Diagnostic {
        Diagnostic::error(self.code, format!("目标 {target} 的 QEMU 冒烟失败：{self}"))
            .with_hint(format!("{}（串口日志：{}）", self.hint, self.log.display()))
    }
}

impl SmokeRun {
    /// 执行一次冒烟：归档 → 启动 → 退出码断言 → 输出契约断言。
    pub(crate) fn execute(&self) -> Result<SmokeReport, SmokeFailure> {
        let dir = self.log_root.join(&self.target);
        let log = dir.join(format!("{}.log", safe_case(&self.case)));
        let result = dir.join(format!("{}.result.json", safe_case(&self.case)));
        if let Err(err) = fs::create_dir_all(&dir) {
            return Err(SmokeFailure::new(
                "archive",
                codes::E_SMOKE_0005,
                format!("无法创建冒烟日志目录 {}：{err}", dir.display()),
                log,
            ));
        }
        let file = match fs::File::create(&log) {
            Ok(file) => file,
            Err(err) => {
                return Err(SmokeFailure::new(
                    "archive",
                    codes::E_SMOKE_0005,
                    format!("无法创建串口日志 {}：{err}", log.display()),
                    log,
                ));
            }
        };
        let stderr = match file.try_clone() {
            Ok(stderr) => stderr,
            Err(err) => {
                return Err(SmokeFailure::new(
                    "archive",
                    codes::E_SMOKE_0005,
                    format!("无法复制串口日志句柄 {}：{err}", log.display()),
                    log,
                ));
            }
        };
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(file))
            .stderr(Stdio::from(stderr));
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let _ = self.write_result(&result, "spawn-failed", None, &[], None, &log);
                return Err(SmokeFailure::new(
                    "spawn",
                    codes::E_SMOKE_0001,
                    format!(
                        "无法启动 {}：{err}；核对工具链是否在 PATH / 环境变量覆盖是否正确",
                        self.program.display()
                    ),
                    log,
                ));
            }
        };

        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(err) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = self.write_result(&result, "failed", None, &[], None, &log);
                    return Err(SmokeFailure::new(
                        "qemu-run",
                        codes::E_SMOKE_0001,
                        format!("等待 QEMU 进程失败：{err}"),
                        log,
                    ));
                }
            }
            if started.elapsed() >= self.timeout {
                let _ = child.kill();
                let _ = child.wait();
                let _ = self.write_result(&result, "timeout", None, &[], None, &log);
                return Err(SmokeFailure::new(
                    "qemu-run",
                    codes::E_SMOKE_0001,
                    format!(
                        "冒烟在 {:?} 内未退出（板型 / 镜像未正常关机）；日志末段见 {}",
                        self.timeout,
                        log.display()
                    ),
                    log,
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        };

        let exit_code = status.code();
        let text = fs::read(&log)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let states = self.step_states(&text);
        let version = version_line(&text);
        match judge(&text, exit_code, self.expected_exit, &self.steps, &log) {
            Ok((version_line, states)) => {
                if let Err(err) = self.write_result(
                    &result,
                    "passed",
                    exit_code,
                    &states,
                    Some(&version_line),
                    &log,
                ) {
                    return Err(SmokeFailure::new(
                        "archive",
                        codes::E_SMOKE_0005,
                        format!("冒烟通过但结果 JSON 写盘失败：{err}"),
                        log,
                    ));
                }
                Ok(SmokeReport {
                    version_line,
                    exit_code: exit_code.unwrap_or_default(),
                    log,
                    result,
                })
            }
            Err(failure) => {
                let _ = self.write_result(
                    &result,
                    "failed",
                    exit_code,
                    &states,
                    version.as_deref(),
                    &log,
                );
                Err(failure)
            }
        }
    }

    fn step_states(&self, text: &str) -> Vec<(String, Option<bool>)> {
        self.steps
            .iter()
            .map(|name| (name.clone(), step_state(text, name)))
            .collect()
    }

    fn write_result(
        &self,
        path: &Path,
        status: &str,
        exit: Option<i32>,
        steps: &[(String, Option<bool>)],
        version: Option<&str>,
        log: &Path,
    ) -> io::Result<()> {
        let steps_json: Vec<serde_json::Value> = steps
            .iter()
            .map(|(name, state)| {
                serde_json::json!({
                    "name": name,
                    "state": match state {
                        Some(true) => "pass",
                        Some(false) => "fail",
                        None => "missing",
                    },
                })
            })
            .collect();
        let missing: Vec<&String> = steps
            .iter()
            .filter(|(_, state)| state.is_none())
            .map(|(name, _)| name)
            .collect();
        let env: std::collections::BTreeMap<&String, &String> =
            self.env.iter().map(|(key, value)| (key, value)).collect();
        let value = serde_json::json!({
            "target": self.target,
            "case": self.case,
            "status": status,
            "command": self.command_line(),
            "env": env,
            "exit_code": exit,
            "expected_exit": self.expected_exit,
            "version": version,
            "steps": steps_json,
            "missing_steps": missing,
            "log": log.display().to_string(),
        });
        let text = serde_json::to_string_pretty(&value).map_err(io::Error::other)?;
        fs::write(path, format!("{text}\n"))
    }

    fn command_line(&self) -> String {
        let mut parts = vec![self.program.display().to_string()];
        parts.extend(self.args.iter().cloned());
        parts.join(" ")
    }
}

/// 自检步骤状态：`Some(true)` = PASS，`Some(false)` = FAIL，`None` = 缺失。
type StepStates = Vec<(String, Option<bool>)>;
/// 判定成功结果：`(版本行, 步骤状态)`。
type JudgeResult = Result<(String, StepStates), SmokeFailure>;

/// 纯判定：退出码 → 失败步骤 → 输出契约。
fn judge(
    text: &str,
    exit: Option<i32>,
    expected_exit: i32,
    steps: &[String],
    log: &Path,
) -> JudgeResult {
    let states: Vec<(String, Option<bool>)> = steps
        .iter()
        .map(|name| (name.clone(), step_state(text, name)))
        .collect();
    // 具名步骤 FAIL 优先：日志直接指向具体自检项。
    if let Some((name, _)) = states.iter().find(|(_, state)| *state == Some(false)) {
        return Err(SmokeFailure::new(
            name,
            codes::E_SMOKE_0002,
            format!(
                "自检步骤 `{name}` 打印 FAIL；查看 {} 中该步骤前后的输出",
                log.display()
            ),
            log.to_path_buf(),
        ));
    }
    // 进程没按板级约定退出（崩溃 / 提前关机 / 编码漂移）→ qemu-run。
    if exit != Some(expected_exit) {
        return Err(SmokeFailure::new(
            "qemu-run",
            codes::E_SMOKE_0003,
            format!(
                "QEMU 退出码 {exit:?} ≠ 板级约定 {expected_exit}（x86 isa-debug-exit 只产奇数：\
                 成功写 0 → 退出 1；ARM semihosting 成功退出 0）；核对板级 overlay 与退出代码"
            ),
            log.to_path_buf(),
        ));
    }
    // 退出码对但步骤标记缺失：进程可能没跑到该步骤。
    if let Some((name, _)) = states.iter().find(|(_, state)| state.is_none()) {
        return Err(SmokeFailure::new(
            name,
            codes::E_SMOKE_0004,
            format!(
                "串口日志缺步骤标记 `[POLYPACK-STEP {name}] PASS|FAIL`（进程可能未跑到该步骤）；日志：{}",
                log.display()
            ),
            log.to_path_buf(),
        ));
    }
    let Some(version_line) = version_line(text) else {
        return Err(SmokeFailure::new(
            "version",
            codes::E_SMOKE_0004,
            format!(
                "串口日志缺引擎版本行 `{VERSION_MARKER}`（自检程序未链接引擎静态库？）；日志：{}",
                log.display()
            ),
            log.to_path_buf(),
        ));
    };
    if !text.lines().any(|line| line.trim() == FINAL_MARKER) {
        return Err(SmokeFailure::new(
            "smoke",
            codes::E_SMOKE_0004,
            format!(
                "串口日志缺终标记 `{FINAL_MARKER}`（自检未走完全部步骤）；日志：{}",
                log.display()
            ),
            log.to_path_buf(),
        ));
    }
    Ok((version_line, states))
}

fn version_line(text: &str) -> Option<String> {
    text.lines()
        .find(|line| line.contains(VERSION_MARKER))
        .map(|line| line.trim().to_string())
}

fn step_state(text: &str, name: &str) -> Option<bool> {
    let pass = format!("[POLYPACK-STEP {name}] {STEP_PASS}");
    let fail = format!("[POLYPACK-STEP {name}] {STEP_FAIL}");
    text.lines().find_map(|line| {
        let line = line.trim();
        if line.contains(&fail) {
            Some(false)
        } else if line.contains(&pass) {
            Some(true)
        } else {
            None
        }
    })
}

/// 文件名安全化（板型 / 镜像名只应含 `[A-Za-z0-9._-]`；其余替换为 `-`）。
fn safe_case(case: &str) -> String {
    let cleaned: String = case
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.trim_matches('-').is_empty() {
        "case".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
#[path = "smoke_tests.rs"]
mod tests;
