//! `poly scan`：secret-scan 提交与构建双重检查（T-M2-12）。
//!
//! - 缺省扫工作区目录树；`--history` 追加 git 全历史（非仓库 / 浅克隆 / git 缺失按
//!   「跳过 + 原因」处理，绝不失败）；
//! - `--install-hook` 写 git pre-commit 钩子：双检的第一道（提交时），CI 侧由
//!   `.github/workflows/secret-scan.yml` 在三个宿主上跑同一命令的 `--history` 形态；
//! - 命中 → `poly::E_SCAN_0001`（退出码 1），逐条输出规则 / 位置 / 脱敏上下文与修复提示。

use std::path::{Path, PathBuf};

use poly_core::scan::{Finding, ScanReport, history_skip_reason, scan_dir, scan_history};
use poly_core::{Diagnostic, codes};

/// 钩子文件内的标记：既是「本工具生成」的凭据，也是拒写外部钩子的判据。
const HOOK_MARKER: &str = "poly scan --install-hook";

/// `poly scan` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 扫描根目录（缺省当前目录）。
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,
    /// 追加扫描 git 全历史（全部 ref 可达对象）。
    #[arg(long)]
    pub history: bool,
    /// 安装 git pre-commit 钩子（`<dir>/.git/hooks/pre-commit`），装完即退出。
    #[arg(long = "install-hook")]
    pub install_hook: bool,
}

/// 执行 `poly scan`；返回进程退出码（命中密钥材料 → 1）。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let root = std::fs::canonicalize(&args.dir).unwrap_or_else(|_| args.dir.clone());
    if args.install_hook {
        let path = install_hook(&root)?;
        if json {
            let report = serde_json::json!({
                "event": "scan-hook",
                "installed": path.to_string_lossy(),
            });
            println!(
                "{}",
                serde_json::to_string(&report).expect("钩子报告可序列化")
            );
        } else {
            println!(
                "已安装提交钩子：{}（每次提交自动跑 poly scan）",
                path.display()
            );
        }
        return Ok(0);
    }

    let mut report = scan_dir(&root)?;
    if args.history {
        match history_skip_reason(&root) {
            Some(reason) => report.skipped.push(reason),
            None => merge(&mut report, scan_history(&root)?),
        }
    }
    print_report(&root, args.history, &report, json);
    if report.findings.is_empty() {
        return Ok(0);
    }
    Err(Diagnostic::error(
        codes::E_SCAN_0001,
        format!("检出 {} 处疑似密钥材料（见上方命中清单与修复提示）", report.findings.len()),
    )
    .with_hint(
        "构造样本 / 示例在同一行加白名单标记（POLYPACK_TEST_FIXTURE / EXAMPLE_NOT_A_REAL_KEY / poly:allow-secret）",
    ))
}

/// 合并两份报告：计数累加，命中按 路径 → 行号 → 规则 排序并去重（同一位点工作区与历史各报一次）。
fn merge(report: &mut ScanReport, extra: ScanReport) {
    report.scanned += extra.scanned;
    report.skipped.extend(extra.skipped);
    report.findings.extend(extra.findings);
    report.findings.sort_by(|a, b| key(a).cmp(&key(b)));
    report.findings.dedup_by(|a, b| key(a) == key(b));
}

/// 命中的排序 / 去重键。
fn key(finding: &Finding) -> (&str, usize, &str) {
    (&finding.path, finding.line, finding.rule)
}

/// 报告输出：`--json` 为单行 JSON（stdout 保持纯 JSON Lines），否则人读清单。
fn print_report(root: &Path, history: bool, report: &ScanReport, json: bool) {
    if json {
        let hits: Vec<serde_json::Value> = report
            .findings
            .iter()
            .map(|finding| {
                serde_json::json!({
                    "rule": finding.rule,
                    "path": finding.path,
                    "line": finding.line,
                    "excerpt": finding.excerpt,
                    "hint": finding.hint,
                })
            })
            .collect();
        let out = serde_json::json!({
            "event": "scan",
            "dir": root.to_string_lossy(),
            "history": history,
            "scanned": report.scanned,
            "hits": hits,
            "skipped": report.skipped,
        });
        println!("{}", serde_json::to_string(&out).expect("扫描报告可序列化"));
        return;
    }
    println!(
        "poly scan — {}（扫描 {}{}）",
        root.display(),
        report.scanned,
        if history {
            " 个文件 + 全历史对象"
        } else {
            " 个文件"
        }
    );
    for finding in &report.findings {
        println!(
            "✗ {}:{} [{}] {}",
            finding.path, finding.line, finding.rule, finding.excerpt
        );
        println!("  修复提示：{}", finding.hint);
    }
    for skipped in &report.skipped {
        println!("· 跳过：{skipped}");
    }
    if report.findings.is_empty() {
        println!("未检出密钥材料。");
    }
}

/// 写 `<root>/.git/hooks/pre-commit`（已存在且非本工具生成 → 拒绝覆盖）。
fn install_hook(root: &Path) -> Result<PathBuf, Diagnostic> {
    let hooks = root.join(".git").join("hooks");
    if !hooks.is_dir() {
        return Err(Diagnostic::error(
            codes::E_SCAN_0002,
            format!("不是 git 仓库（缺 {}）", hooks.display()),
        )
        .with_hint("在仓库根目录运行，或先 git init"));
    }
    let path = hooks.join("pre-commit");
    if let Ok(existing) = std::fs::read_to_string(&path)
        && !existing.contains(HOOK_MARKER)
    {
        return Err(Diagnostic::error(
            codes::E_SCAN_0002,
            format!("已存在外部 pre-commit 钩子（{}），不覆盖", path.display()),
        )
        .with_hint("手工把 `poly scan` 一行并入该钩子，或先移除它再重装"));
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("poly"));
    std::fs::write(&path, hook_script(&shell_path(&exe), &shell_path(root)))
        .map_err(|err| crate::project::io_error("写入提交钩子", &path, err))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .map_err(|err| crate::project::io_error("设置钩子可执行位", &path, err))?;
    }
    Ok(path)
}

/// 路径转 shell 文本：`\` → `/`（Git for Windows 的 sh 也认这套分隔符）。
fn shell_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 钩子脚本：poly 不可用时 fail-open（跳过检查），命中才拒绝提交。
fn hook_script(exe: &str, root: &str) -> String {
    format!(
        r#"#!/bin/sh
# {HOOK_MARKER} 生成；删除本文件即可卸载。
# 双检的第一道（提交时）；CI 侧 .github/workflows/secret-scan.yml 跑 `poly scan --history`。
POLY="{exe}"
if [ ! -x "$POLY" ]; then
  echo "poly scan: 未找到 $POLY，跳过提交检查（fail-open）" >&2
  exit 0
fi
"$POLY" scan --dir "{root}" || {{
  echo "poly scan: 检出疑似密钥材料，提交被拒（见上方命中与修复提示）" >&2
  exit 1
}}
"#
    )
}
