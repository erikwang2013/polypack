//! wasm 出口的体积裁剪（T-M3-08，R10 对策）：wasm-opt（binaryen）对 wasm-bindgen 产物
//! 做体积优化，前后对比写进出口清单的体积元数据（[`super::wasm::Manifest::size`]）。
//!
//! ## 裁剪时机（接口契约）
//!
//! ```text
//! wasm-bindgen 生成出口目录 →【wasm-opt -Oz 在此裁剪】→ taro/打包器随包「在场即拷」
//! ```
//!
//! 裁剪发生在**出口目录生成后、随包拷贝前**：随包条目就是优化后的字节（打包器只做整目录
//! 拷贝，不感知 wasm-opt）。
//!
//! ## skip-if-missing（不阻断，但绝不静默）
//!
//! wasm-opt 缺失 / 裁剪失败都**不**让出口失败：告警放行、`wasm_opt.applied=false` 与原因
//! 写进清单——元数据必须能区分「没裁」与「裁了没收益」。
//!
//! ## 定位顺序
//!
//! `$POLYPACK_WASM_OPT` → `PATH` → 工具缓存 `~/.cache/polypack/tools/wasm-opt/`
//! （官方发行包解包形态，与 wasm-bindgen / kotlinc 的约定同源）。

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, codes};
use crate::events::{BuildEvent, EventSink};
use crate::plan::StageError;

/// wasm-opt 的优化参数：`-Oz`（体积优先；wasm-bindgen 官方文档同款用法）。
const WASM_OPT_ARGS: [&str; 1] = ["-Oz"];

/// 建议的 binaryen 发行版本（提示文案用；不强制——任何近期版本都能裁）。
pub const WASM_OPT_RELEASE: &str = "version_133";

/// 出口体积元数据（T-M3-08）：随包 wasm 的落盘字节 + wasm-opt 前后对比。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SizeInfo {
    /// 落盘 wasm 字节数（= `wasm_opt.after`；消费方免去再 stat 一次）。
    pub wasm_bytes: u64,
    /// wasm-opt 裁剪记录。
    pub wasm_opt: WasmOptInfo,
}

/// wasm-opt 裁剪记录。
///
/// 工具缺失或裁剪失败时 `applied=false`、`before == after`，`note` 记原因——
/// 元数据必须能区分「没裁」与「裁了没收益」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmOptInfo {
    /// 是否真的裁剪并替换了 wasm 字节。
    pub applied: bool,
    /// 工具标识（`wasm-opt@<版本>`；缺失为 `None`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// 优化参数（[`WASM_OPT_ARGS`] 拼串）。
    pub args: String,
    /// 裁剪前字节（原 wasm；`applied=false` 时与 `after` 相同）。
    pub before: u64,
    /// 裁剪后字节（随包字节）。
    pub after: u64,
    /// 未裁剪的原因（工具缺失 / 失败报文尾）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// wasm-opt CLI 的定位结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmOpt {
    /// 可执行文件。
    pub bin: PathBuf,
}

impl WasmOpt {
    /// 定位顺序：`$POLYPACK_WASM_OPT` → `PATH` → 工具缓存（见模块文档）。
    pub fn discover() -> Option<Self> {
        if let Some(path) = std::env::var_os("POLYPACK_WASM_OPT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
        {
            return Some(Self { bin: path });
        }
        if let Some(path) = wasm_opt_on_path() {
            return Some(Self { bin: path });
        }
        cached_wasm_opt().map(|bin| Self { bin })
    }

    /// 实际版本号（跑 `--version` 取 `wasm-opt version <n>` 的 `<n>`）。
    pub fn version(&self) -> Result<String, StageError> {
        let output = Command::new(&self.bin)
            .arg("--version")
            .output()
            .map_err(|err| {
                Diagnostic::error(
                    codes::E_BRIDGE_0003,
                    format!("无法执行 wasm-opt（{}）: {err}", self.bin.display()),
                )
                .with_hint(wasm_opt_hint())
            })?;
        let text = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() {
            return Err(Diagnostic::error(
                codes::E_BRIDGE_0003,
                format!(
                    "wasm-opt --version 退出非零（{}）: {}",
                    self.bin.display(),
                    text.trim()
                ),
            )
            .with_hint(wasm_opt_hint())
            .into());
        }
        parse_wasm_opt_version(&text).ok_or_else(|| {
            Diagnostic::error(
                codes::E_BRIDGE_0003,
                format!("wasm-opt --version 输出不可解析: {}", text.trim()),
            )
            .with_hint(wasm_opt_hint())
            .into()
        })
    }

    /// 工具标识（缓存键输入与元数据）：`wasm-opt@<版本>`；版本取不到时 `wasm-opt:unknown`
    /// （不因报告取数失败挡住缓存键计算）。
    pub fn fingerprint(&self) -> String {
        match self.version() {
            Ok(version) => format!("wasm-opt@{version}"),
            Err(_) => "wasm-opt:unknown".to_string(),
        }
    }

    /// 裁剪 `wasm`：`wasm-opt -Oz <wasm> -o <tmp>` 后原子替换（失败不碰原文件）。
    pub fn optimize(&self, wasm: &Path) -> Result<WasmOptInfo, StageError> {
        let before = file_bytes(wasm)?;
        // 先写旁路文件再整文件替换：中途失败时原 wasm 保持不动（出口目录不会出现半截产物）
        let tmp = wasm.with_extension("opt");
        let _ = std::fs::remove_file(&tmp);
        let output = Command::new(&self.bin)
            .args(WASM_OPT_ARGS)
            .arg(wasm)
            .arg("-o")
            .arg(&tmp)
            .output()
            .map_err(|err| {
                Diagnostic::error(
                    codes::E_BRIDGE_0003,
                    format!("无法启动 wasm-opt（{}）: {err}", self.bin.display()),
                )
                .with_hint(wasm_opt_hint())
            })?;
        if !output.status.success() {
            let _ = std::fs::remove_file(&tmp);
            return Err(Diagnostic::error(
                codes::E_BRIDGE_0003,
                format!(
                    "wasm-opt 裁剪失败（{}）: {}",
                    wasm.display(),
                    stderr_tail(&output.stderr)
                ),
            )
            .with_hint(
                "产物可能是新提案的 wasm（工具版本偏旧）；换更新版 binaryen，\
                 或去掉 wasm-opt 依赖（体积报告将如实记 applied=false）",
            )
            .into());
        }
        let after = file_bytes(&tmp)?;
        std::fs::rename(&tmp, wasm).map_err(|err| {
            Diagnostic::error(
                codes::E_BRIDGE_0003,
                format!("wasm-opt 产物替换失败（{}）: {err}", wasm.display()),
            )
            .with_hint("确认出口目录可写；重跑 poly bridge wasm")
        })?;
        Ok(WasmOptInfo {
            applied: true,
            tool: Some(self.fingerprint()),
            args: WASM_OPT_ARGS.join(" "),
            before,
            after,
            note: None,
        })
    }
}

/// 出口目录生成后的裁剪步骤（skip-if-missing / 失败降级，绝不静默）：
///
/// - 工具在场且成功：替换 wasm 字节 + 告警级体积报告（前后对比进事件流）；
/// - 工具缺失：**不阻断**，告警 + `applied=false`（原因写进元数据）；
/// - 裁剪失败：**不阻断**，保留原字节，告警 + `applied=false` + 失败尾。
pub fn optimize_wasm(wasm: &Path, events: &mut dyn EventSink) -> Result<SizeInfo, StageError> {
    optimize_with(WasmOpt::discover(), wasm, events)
}

/// [`optimize_wasm`] 的本体：工具定位结果显式传入——单测据此直接覆盖「缺失 / 失败」两条
/// 降级路径（改进程环境跑不了：`unsafe_code = forbid`）。
fn optimize_with(
    tool: Option<WasmOpt>,
    wasm: &Path,
    events: &mut dyn EventSink,
) -> Result<SizeInfo, StageError> {
    let before = file_bytes(wasm)?;
    let Some(tool) = tool else {
        let note = "未找到 wasm-opt，wasm 未裁剪".to_string();
        events.emit(&BuildEvent::Warning {
            stage: "bridge".to_string(),
            message: format!("wasm 未裁剪（{before} 字节）：{note}——{}", wasm_opt_hint()),
        });
        return Ok(skipped(&note, None, before));
    };
    match tool.optimize(wasm) {
        Ok(info) => {
            events.emit(&BuildEvent::Warning {
                stage: "bridge".to_string(),
                message: format!(
                    "wasm-opt 裁剪（{}）：{} → {} 字节（-{}%）",
                    info.tool.as_deref().unwrap_or("wasm-opt"),
                    info.before,
                    info.after,
                    saved_percent(info.before, info.after)
                ),
            });
            Ok(SizeInfo {
                wasm_bytes: info.after,
                wasm_opt: info,
            })
        }
        Err(err) => {
            let note = err.diagnostic.message.clone();
            events.emit(&BuildEvent::Warning {
                stage: "bridge".to_string(),
                message: format!("wasm-opt 裁剪失败（保留原字节 {before}，不阻断）：{note}"),
            });
            Ok(skipped(&note, Some(tool.fingerprint()), before))
        }
    }
}

/// 回读出口清单里的体积元数据（缓存命中路径：清单随树还原，不必重新生成）。
pub fn read_size(dir: &Path) -> Option<SizeInfo> {
    let text = std::fs::read_to_string(dir.join(super::wasm::MANIFEST_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    serde_json::from_value(value.get("size")?.clone()).ok()
}

/// 未裁剪的体积元数据（`applied=false`，`before == after`）。
fn skipped(note: &str, tool: Option<String>, bytes: u64) -> SizeInfo {
    SizeInfo {
        wasm_bytes: bytes,
        wasm_opt: WasmOptInfo {
            applied: false,
            tool,
            args: WASM_OPT_ARGS.join(" "),
            before: bytes,
            after: bytes,
            note: Some(note.to_string()),
        },
    }
}

/// `wasm-opt --version` 的输出解析（`wasm-opt version 133` / `wasm-opt version 133 (version_133-…)`）。
///
/// pub 供测试直达（纯函数，无副作用）。
pub fn parse_wasm_opt_version(text: &str) -> Option<String> {
    let mut tokens = text.split_whitespace();
    while let Some(token) = tokens.next() {
        if token != "version" {
            continue;
        }
        let raw = tokens.next()?;
        let raw = raw.strip_prefix("version_").unwrap_or(raw);
        let digits: String = raw
            .chars()
            .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
            .collect();
        return (!digits.is_empty()).then_some(digits);
    }
    None
}

/// `$POLYPACK_WASM_OPT` 之外的 PATH 查找（Windows 认 `.exe`）。
fn wasm_opt_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let candidates = ["wasm-opt", "wasm-opt.exe"];
    for dir in std::env::split_paths(&path) {
        for candidate in candidates {
            let full = dir.join(candidate);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

/// 缓存里找 CLI：官方发行包解包形态（`<版本>/<发行目录>/bin/wasm-opt` 或
/// `<发行目录>/bin/wasm-opt`），下探 ≤3 层收集候选；优先路径含 [`WASM_OPT_RELEASE`] 的，
/// 其次取字典序最大（与 wasm-bindgen 的取法同风格）。
fn cached_wasm_opt() -> Option<PathBuf> {
    let root = crate::bridge::wasm::tool_cache_dir("wasm-opt")?;
    let candidates = wasm_opt_candidates(&root);
    candidates
        .iter()
        .rev()
        .find(|path| path.to_string_lossy().contains(WASM_OPT_RELEASE))
        .or_else(|| candidates.last())
        .cloned()
}

/// 目录树下收集名为 `wasm-opt` 的可执行候选（升序；根目录不存在 → 空）。
fn wasm_opt_candidates(root: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 3 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push((path, depth + 1));
            } else if path
                .file_name()
                .is_some_and(|name| name == "wasm-opt" || name == "wasm-opt.exe")
            {
                candidates.push(path);
            }
        }
    }
    candidates.sort();
    candidates
}

/// CLI 缺失 / 失败的修复提示（官方发行包落缓存目录，无 npm / cargo install 的供应链面）。
fn wasm_opt_hint() -> String {
    format!(
        "取官方发行包：binaryen-{WASM_OPT_RELEASE}-<宿主平台>.tar.gz\
         （github.com/WebAssembly/binaryen/releases），解到 ~/.cache/polypack/tools/wasm-opt/；\
         或设 $POLYPACK_WASM_OPT 指向 wasm-opt 可执行文件"
    )
}

/// 节省百分比（整数；`before == 0` 记 0）。
fn saved_percent(before: u64, after: u64) -> u64 {
    if before == 0 {
        return 0;
    }
    before.saturating_sub(after) * 100 / before
}

/// 文件字节数（IO 失败 → `poly::E_BRIDGE_0003`）。
fn file_bytes(path: &Path) -> Result<u64, StageError> {
    std::fs::metadata(path)
        .map(|meta| meta.len())
        .map_err(|err| {
            Diagnostic::error(
                codes::E_BRIDGE_0003,
                format!("读取 wasm 体积失败（{}）: {err}", path.display()),
            )
            .with_hint("确认 wasm-bindgen 已产出 <lib>_wasm_bg.wasm；重跑 poly bridge wasm")
            .into()
        })
}

/// stderr 尾部关键行（≤3 行；与 wasm 出口的取法同形）。
fn stderr_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let mut lines: Vec<&str> = text
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(3)
        .collect();
    lines.reverse();
    lines.join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_output_forms_are_parsed() {
        assert_eq!(
            parse_wasm_opt_version("wasm-opt version 133"),
            Some("133".to_string())
        );
        assert_eq!(
            parse_wasm_opt_version("wasm-opt version 133 (version_133-abcdef)\n"),
            Some("133".to_string())
        );
        assert_eq!(
            parse_wasm_opt_version("wasm-opt version version_120\n"),
            Some("120".to_string()),
            "发行串形态（version_<n>）也应解析"
        );
        assert_eq!(parse_wasm_opt_version("binaryen toolchain"), None);
        assert_eq!(parse_wasm_opt_version(""), None);
    }

    #[test]
    fn cache_scan_prefers_pinned_release_and_finds_flat_layouts() {
        let base = std::env::temp_dir().join(format!("poly-wasm-opt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        // 官方解包：<版本>/<发行目录>/bin/wasm-opt（cache 下探 ≤3 层的真实形态）
        let nested = base
            .join("version_133")
            .join("binaryen-version_133-x86_64-linux")
            .join("bin");
        std::fs::create_dir_all(&nested).expect("建缓存目录");
        std::fs::write(nested.join("wasm-opt"), b"#!/bin/sh\n").expect("写候选");
        // 另一版本（非钉住）：存在也不优先
        let other = base
            .join("version_99")
            .join("binaryen-version_99-x86_64-linux")
            .join("bin");
        std::fs::create_dir_all(&other).expect("建缓存目录");
        std::fs::write(other.join("wasm-opt"), b"#!/bin/sh\n").expect("写候选");

        let candidates = wasm_opt_candidates(&base);
        assert_eq!(candidates.len(), 2, "两个候选都收集到：{candidates:?}");
        let preferred = candidates
            .iter()
            .rev()
            .find(|path| path.to_string_lossy().contains(WASM_OPT_RELEASE))
            .or_else(|| candidates.last())
            .expect("应有候选");
        assert!(
            preferred.to_string_lossy().contains("version_133"),
            "优先钉住的发行版：{preferred:?}"
        );
        // 不存在的根目录：空候选（CLI 缺失路径不 panic）
        assert!(wasm_opt_candidates(&base.join("absent")).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn saved_percent_is_integer_and_zero_safe() {
        assert_eq!(saved_percent(1000, 750), 25);
        assert_eq!(saved_percent(1000, 1000), 0);
        assert_eq!(saved_percent(0, 0), 0);
        assert_eq!(saved_percent(3, 4), 0, "变大时不下溢（saturating）");
    }

    /// 单测用的临时目录（drop 靠测试自己清）。
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poly-wasm-opt-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 假工具脚本（可执行；`exit <code>`）。
    #[cfg(unix)]
    fn fake_tool(dir: &Path, name: &str, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, script).expect("写假工具");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("置执行位");
        path
    }

    /// 工具缺失：降级为「未裁剪」元数据 + 告警（不阻断、不静默）。
    #[test]
    fn missing_tool_skips_with_reason() {
        let dir = scratch("skip");
        let wasm = dir.join("pax_core_wasm_bg.wasm");
        std::fs::write(&wasm, vec![0u8; 42]).expect("写 wasm");
        let mut sink = crate::events::HumanSink { out: Vec::new() };

        let size = optimize_with(None, &wasm, &mut sink).expect("缺工具必须放行（不阻断）");
        assert!(!size.wasm_opt.applied, "缺工具时 applied=false");
        assert_eq!(size.wasm_bytes, 42);
        assert_eq!((size.wasm_opt.before, size.wasm_opt.after), (42, 42));
        assert_eq!(size.wasm_opt.tool, None);
        let note = size.wasm_opt.note.unwrap_or_default();
        assert!(note.contains("未找到 wasm-opt"), "原因必须可读：{note}");
        assert_eq!(size.wasm_opt.args, "-Oz", "参数如实记录，便于比对报告");
        assert_eq!(
            std::fs::metadata(&wasm).expect("wasm 仍在").len(),
            42,
            "原字节不动"
        );
        let log = String::from_utf8_lossy(&sink.out).to_string();
        assert!(log.contains("wasm 未裁剪"), "告警绝不静默：{log}");
        assert!(log.contains("POLYPACK_WASM_OPT"), "告警给出修复路径：{log}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 工具在场但裁剪失败：保留原字节 + `applied=false` + 失败原因（不阻断）。
    #[cfg(unix)]
    #[test]
    fn failing_tool_keeps_original_bytes() {
        let dir = scratch("fail");
        let wasm = dir.join("pax_core_wasm_bg.wasm");
        std::fs::write(&wasm, vec![7u8; 64]).expect("写 wasm");
        let bin = fake_tool(
            &dir,
            "wasm-opt",
            "#!/bin/sh\necho 'boom: bad module' >&2\nexit 3\n",
        );
        let mut sink = crate::events::HumanSink { out: Vec::new() };

        let size = optimize_with(Some(WasmOpt { bin }), &wasm, &mut sink)
            .expect("裁剪失败必须放行（不阻断）");
        assert!(!size.wasm_opt.applied);
        assert_eq!(size.wasm_bytes, 64, "失败时随包字节 = 原字节");
        assert_eq!((size.wasm_opt.before, size.wasm_opt.after), (64, 64));
        // 版本取不到（脚本只认 `--version` 之外的一切）→ 工具标识退化为 wasm-opt:unknown
        assert!(size.wasm_opt.tool.is_some());
        let note = size.wasm_opt.note.unwrap_or_default();
        assert!(note.contains("裁剪失败"), "{note}");
        assert!(note.contains("boom"), "失败尾进元数据：{note}");
        assert_eq!(std::fs::metadata(&wasm).expect("wasm 仍在").len(), 64);
        assert!(!wasm.with_extension("opt").exists(), "旁路文件必须清掉");
        let log = String::from_utf8_lossy(&sink.out).to_string();
        assert!(log.contains("wasm-opt 裁剪失败"), "{log}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
