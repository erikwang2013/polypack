//! secret-scan 扫描引擎（T-M2-12）：提交钩子与 CI 作业共用的同一实现；规则集见 `rules`。
//!
//! - 规则面：JKS 密钥库魔数（二进制）、PEM 私钥头、主流 API key 形态（AWS / GitHub /
//!   Google / Slack）与「关键字 + 高熵长串」启发式；
//! - 白名单：命中行内含 [`ALLOW_MARKERS`] 任一标记即不报告——构造样本（测试夹具）可以
//!   带标记安全留存，而**真实形态**的串（无标记）照常命中；
//! - 脱敏：报告只保留命中区段前后的上下文，绝不回显命中内容本身；
//! - 扫描面：[`scan_dir`]（工作区目录树）与 [`scan_history`]（git 全历史 blob：一次
//!   `rev-list` 列对象 + 单个 `cat-file --batch` 批量回读，不逐 commit / 逐 blob 起进程）；
//! - 跳过语义：git 缺失 / 非仓库 / 浅克隆由 [`history_skip_reason`] 判定，调用方跳过并
//!   给出原因，绝不失败（与 `compile.rs` / `doctor.rs` 既有约定一致）。

mod rules;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::diagnostics::{Diagnostic, codes};
use rules::{ARTIFACT_RULES, BINARY_RULES, TEXT_RULES, find_sub, line_has_allow_marker};

/// 白名单标记：命中行内含任一标记即视为构造样本 / 示例，不报告。
pub const ALLOW_MARKERS: &[&str] = &[
    "POLYPACK_TEST_FIXTURE",
    "EXAMPLE_NOT_A_REAL_KEY",
    "polypack:allow-secret",
];

/// 工作区扫描跳过的目录（构建产物 / 依赖 / VCS 内部）。
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", "dist", ".polypack"];
/// 文本面扫描上限（超出部分只查二进制规则，避免大文件整体入内存）。
const TEXT_SCAN_CAP: usize = 1 << 20;
/// 历史 blob 的读取上限（超限只查二进制规则——JKS 魔数在文件头，不受影响）。
const BLOB_SCAN_CAP: usize = 8 << 20;

/// 一条命中（脱敏：`excerpt` 不含命中内容本身）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// 规则 id（如 `pem-private-key`）。
    pub rule: &'static str,
    /// 文件路径（工作区为相对路径；历史为 rev-list 给出的已知路径）。
    pub path: String,
    /// 行号（1 起）。
    pub line: usize,
    /// 脱敏上下文（命中区段以 `⟨已隐去 N 字节⟩` 占位）。
    pub excerpt: String,
    /// 修复提示（命中即失败时与错误码一同输出）。
    pub hint: &'static str,
}

/// 一次扫描的报告。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanReport {
    /// 命中清单（按 路径 → 行号 → 规则 排序）。
    pub findings: Vec<Finding>,
    /// 已扫描单元数（工作区 = 文件数；历史 = blob 数）。
    pub scanned: u64,
    /// 跳过项与原因（超限截断等）。
    pub skipped: Vec<String>,
}

/// 扫描一段字节（工作区文件与 git blob 通用）；`path` 仅用于报告。
pub fn scan_bytes(path: &str, bytes: &[u8]) -> ScanReport {
    let mut report = ScanReport {
        scanned: 1,
        ..ScanReport::default()
    };
    scan_into(path, bytes, &mut report);
    sort_findings(&mut report);
    report
}

/// 把 `bytes` 的命中追加进 `report`。
fn scan_into(path: &str, bytes: &[u8], report: &mut ScanReport) {
    // 首行是二进制规则的白名单判定面（魔数在文件头，标记也在文件头）
    let first = &bytes[..bytes
        .iter()
        .position(|&byte| byte == b'\n')
        .unwrap_or(bytes.len())];
    for rule in BINARY_RULES {
        if bytes.starts_with(rule.magic) && !line_has_allow_marker(first) {
            report.findings.push(Finding {
                rule: rule.id,
                path: path.to_string(),
                line: 1,
                excerpt: format!("文件头魔数 {}", hex(rule.magic)),
                hint: rule.hint,
            });
        }
    }
    // T-M8-03：插件制品（`.wasm`）按字节扫宿主路径 / 环境变量痕迹 / 调试符号；
    // 制品规则不受下方「含 NUL 跳过文本规则」影响。
    scan_artifact(path, first, bytes, report);
    // 含 NUL 视为二进制：文本规则不适用（避免在二进制里误报）；二进制规则已在上方处理
    let window = &bytes[..bytes.len().min(TEXT_SCAN_CAP)];
    if window.contains(&0) {
        return;
    }
    if bytes.len() > TEXT_SCAN_CAP {
        report.skipped.push(format!(
            "{path}: 超文本扫描上限，仅扫前 {TEXT_SCAN_CAP} 字节"
        ));
    }
    for (index, line) in window.split(|&byte| byte == b'\n').enumerate() {
        if line_has_allow_marker(line) {
            continue;
        }
        for rule in TEXT_RULES {
            if let Some((start, len)) = (rule.detect)(line) {
                report.findings.push(Finding {
                    rule: rule.id,
                    path: path.to_string(),
                    line: index + 1,
                    excerpt: redact(line, start, len),
                    hint: rule.hint,
                });
                break; // 一行只报优先级最高的规则
            }
        }
    }
}

/// 插件制品扫描（T-M8-03）：`.wasm` 内的宿主绝对路径 / 环境变量痕迹 / 调试符号段。
///
/// 同一规则只报首次命中；excerpt 只给脱敏描述与偏移，绝不回显路径本身（§7）。
fn scan_artifact(path: &str, first: &[u8], bytes: &[u8], report: &mut ScanReport) {
    let is_wasm = path
        .rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wasm"));
    if !is_wasm || line_has_allow_marker(first) {
        return;
    }
    for rule in ARTIFACT_RULES {
        let duplicate = report
            .findings
            .iter()
            .any(|finding| finding.path == path && finding.rule == rule.id);
        if duplicate {
            continue;
        }
        let Some(offset) = find_sub(bytes, rule.needle) else {
            continue;
        };
        report.findings.push(Finding {
            rule: rule.id,
            path: path.to_string(),
            line: 1,
            excerpt: format!("{}（偏移 {offset}，具体内容已隐去）", rule.label),
            hint: rule.hint,
        });
    }
}

/// 扫描工作区目录树（跳过 [`SKIP_DIRS`]）；IO 失败 → `polypack::E_SCAN_0002`。
pub fn scan_dir(root: &Path) -> Result<ScanReport, Diagnostic> {
    let mut report = ScanReport::default();
    walk(root, root, &mut report)?;
    sort_findings(&mut report);
    Ok(report)
}

/// 递归扫描：`root` 为报告路径基准，`dir` 为当前目录。
fn walk(root: &Path, dir: &Path, report: &mut ScanReport) -> Result<(), Diagnostic> {
    let entries = std::fs::read_dir(dir).map_err(|err| io_error("读取目录", dir, err))?;
    for entry in entries {
        let entry = entry.map_err(|err| io_error("读取目录项", dir, err))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| io_error("读取文件类型", &path, err))?;
        if file_type.is_dir() {
            let skip = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| SKIP_DIRS.contains(&name));
            if !skip {
                walk(root, &path, report)?;
            }
        } else if file_type.is_file() {
            let bytes = std::fs::read(&path).map_err(|err| io_error("读取文件", &path, err))?;
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            report.scanned += 1;
            scan_into(&rel, &bytes, report);
        }
    }
    Ok(())
}

/// 全历史扫描的跳过判定：返回 `Some(原因)` 时调用方跳过（绝不失败）。
///
/// 覆盖：git 不可用 / 非 git 仓库 / 浅克隆（历史不完整，`git fetch --unshallow` 后可扫）。
pub fn history_skip_reason(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--is-shallow-repository"])
        .output();
    match output {
        Ok(out) if !out.status.success() => {
            Some("不是 git 仓库（git rev-parse 失败）：全历史扫描跳过".to_string())
        }
        Ok(out) if String::from_utf8_lossy(&out.stdout).trim() == "true" => {
            Some("浅克隆（历史不完整）：先 git fetch --unshallow 再跑全历史扫描".to_string())
        }
        Ok(_) => None,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Some(format!("git 不可用（{err}）：全历史扫描跳过"))
        }
        Err(err) => Some(format!("git 执行失败（{err}）：全历史扫描跳过")),
    }
}

/// 扫描 git 全历史（全部 ref 可达对象）。
///
/// 批量读取：`rev-list --objects --all` 一次列出对象与路径，随后把对象 id 一次性喂给
/// **单个** `cat-file --batch` 进程流式回读；只扫 blob（commit / tree 跳过）。
pub fn scan_history(root: &Path) -> Result<ScanReport, Diagnostic> {
    let listed = git_stdout(root, &["rev-list", "--objects", "--all"])?;
    let mut paths: BTreeMap<String, String> = BTreeMap::new();
    for line in listed.lines() {
        let (sha, path) = match line.split_once(' ') {
            Some((sha, path)) => (sha, path),
            None => (line, "(路径未知)"),
        };
        if sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            paths
                .entry(sha.to_string())
                .or_insert_with(|| path.to_string());
        }
    }
    // BTreeMap 键序 = 字典序：喂入顺序确定，报告与遍历顺序无关
    let shas: Vec<String> = paths.keys().cloned().collect();

    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|err| io_error("启动 git cat-file", root, err))?;
    let mut stdin = child.stdin.take().expect("stdin 已管道");
    // 写入放独立线程：全部对象 id 喂入后关闭 stdin，避免双向管道互等死锁
    let writer = std::thread::spawn(move || {
        for sha in &shas {
            if writeln!(stdin, "{sha}").is_err() {
                break;
            }
        }
    });

    let mut reader = BufReader::new(child.stdout.take().expect("stdout 已管道"));
    let mut report = ScanReport::default();
    let mut header = String::new();
    loop {
        header.clear();
        match reader.read_line(&mut header) {
            Ok(0) => break,
            Ok(_) => {}
            Err(err) => return Err(io_error("读取 git cat-file 输出", root, err)),
        }
        let mut parts = header.trim_end().split(' ');
        let (Some(sha), Some(kind)) = (parts.next(), parts.next()) else {
            break;
        };
        if kind == "missing" {
            continue;
        }
        let size: usize = parts
            .next()
            .and_then(|size| size.parse().ok())
            .ok_or_else(|| {
                Diagnostic::error(
                    codes::E_SCAN_0002,
                    format!("git cat-file 头部非法: {}", header.trim_end()),
                )
                .with_hint("对象库输出与协议不符：用 git fsck 检查仓库完整性")
            })?;
        let sha = sha.to_string();
        let kept = size.min(BLOB_SCAN_CAP);
        let mut bytes = vec![0u8; kept];
        reader
            .read_exact(&mut bytes)
            .map_err(|err| io_error("读取 git cat-file 载荷", root, err))?;
        if size > kept {
            let mut drain = reader.by_ref().take((size - kept) as u64);
            std::io::copy(&mut drain, &mut std::io::sink())
                .map_err(|err| io_error("丢弃 git cat-file 超限载荷", root, err))?;
        }
        reader // 帧尾换行
            .read_exact(&mut [0u8; 1])
            .map_err(|err| io_error("读取 git cat-file 帧尾", root, err))?;
        if kind == "blob" {
            let path = paths
                .get(&sha)
                .cloned()
                .unwrap_or_else(|| "(未跟踪对象)".to_string());
            if size > kept {
                report.skipped.push(format!(
                    "{path}: blob 超读取上限（{size} 字节），只查了文件头规则"
                ));
            }
            report.scanned += 1;
            scan_into(&path, &bytes, &mut report);
        }
    }
    let _ = writer.join();
    let status = child
        .wait()
        .map_err(|err| io_error("回收 git cat-file", root, err))?;
    if !status.success() {
        return Err(Diagnostic::error(
            codes::E_SCAN_0002,
            format!("git cat-file 退出码非零: {status}"),
        )
        .with_hint("用 git fsck 检查对象库完整性后重试"));
    }
    sort_findings(&mut report);
    Ok(report)
}

/// 执行 git 并捕获 stdout（失败 → `polypack::E_SCAN_0002` + 修复提示）。
fn git_stdout(root: &Path, args: &[&str]) -> Result<String, Diagnostic> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|err| io_error("执行 git", root, err))?;
    if !output.status.success() {
        return Err(Diagnostic::error(
            codes::E_SCAN_0002,
            format!(
                "git {} 失败: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        )
        .with_hint("确认在 git 仓库内运行（polypack scan --history），且 git 在 PATH 中"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 命中区段脱敏：只保留前后各 ≤40 字节上下文，命中内容以占位符替代。
fn redact(line: &[u8], start: usize, len: usize) -> String {
    let before = line[..start.min(line.len())].trim_ascii();
    let after = line[(start + len).min(line.len())..].trim_ascii();
    let head = String::from_utf8_lossy(&before[before.len().saturating_sub(40)..]);
    let tail = String::from_utf8_lossy(&after[..after.len().min(40)]);
    format!("{head}⟨已隐去 {len} 字节⟩{tail}")
}

/// 大写十六进制渲染（二进制规则的报告文本）。
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 排序：路径 → 行号 → 规则（与文件系统 / 对象库遍历顺序无关，报告确定）。
fn sort_findings(report: &mut ScanReport) {
    report
        .findings
        .sort_by(|a, b| (&a.path, a.line, a.rule).cmp(&(&b.path, b.line, b.rule)));
}

/// IO 失败 → 稳定诊断。
fn io_error(action: &str, path: &Path, err: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        codes::E_SCAN_0002,
        format!("{action}失败（{}）: {err}", path.display()),
    )
    .with_hint("确认路径存在且当前用户可读")
}
