//! 构建事件总线（ROADMAP §4.2）：驱动进度、报告与缓存写入。
//!
//! 除实时事件外，本模块定义落盘的构建历史 schema（M9-E6 / T-M9-16）：[`HistoryRecord`]
//! 是 `poly build` 写入 `.polypack/history/events.jsonl` 与 `poly history` 读回的同一份
//! 契约。落盘路径一律脱敏：工程内只留相对路径，工程外绝对路径写 [`REDACTED_PATH`]。

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// 一次构建中发布的全部事件。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum BuildEvent {
    /// 计划完成。
    Planned {
        /// 入选目标 id。
        targets: Vec<String>,
        /// 被排除目标与原因（`--target all` 语义）。
        excluded: Vec<String>,
    },
    /// 工具链探测完成。
    Probed {
        /// 已找到的工具链 id。
        found: Vec<String>,
        /// 缺失的工具链 id。
        missing: Vec<String>,
    },
    /// Rust 库编译完成。
    Compiled {
        /// crate 名。
        crate_name: String,
        /// 目标三元组。
        triple: String,
    },
    /// UI 打包完成。
    UiBundled {
        /// 渲染适配器。
        adapter: String,
    },
    /// 签名完成。
    Signed {
        /// 凭据档案。
        profile: String,
    },
    /// 封装完成。
    Packaged {
        /// 产物格式。
        format: String,
        /// 产物体积（字节）。
        bytes: u64,
    },
    /// 体积预算判定（T-M3-08）：每个打包产物一条（`[budget].size` 未设时不发）。
    Budgeted {
        /// 目标 id。
        target: String,
        /// 产物格式。
        format: String,
        /// 产物体积（字节）。
        bytes: u64,
        /// 预算上限（字节）。
        limit: u64,
        /// 超预算的处理策略（`warn` / `fail`）。
        policy: String,
        /// 超预算时的稳定错误码（在预算内为 `None`）。
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    /// 校验完成。
    Verified {
        /// 校验器。
        checker: String,
        /// 是否通过。
        ok: bool,
    },
    /// 发布完成（提审到终态且通过 / 已上架）。
    Published {
        /// 渠道。
        channel: String,
        /// 平台提交号（提审单号；平台未给时为本地幂等键）。
        submission: String,
    },
    /// 失败（携带稳定错误码与提示）。
    Failed {
        /// 阶段名。
        stage: String,
        /// 错误码。
        code: String,
        /// 修复提示。
        #[serde(skip_serializing_if = "Option::is_none")]
        hint: Option<String>,
    },
    /// 插件能力授权清单（T-M8-01）：每个 `[[plugins]]` 一条，`poly build --json` 可见。
    PluginGranted {
        /// 插件 id。
        plugin: String,
        /// 只读目录。
        fs_read: Vec<String>,
        /// 可写目录。
        fs_write: Vec<String>,
        /// 出网域名白名单。
        net: Vec<String>,
    },
    /// 缓存查询结果。
    Cached {
        /// 缓存键。
        key: String,
        /// 是否命中。
        hit: bool,
    },
    /// 非阻断告警（阶段继续执行）：降级路径、淘汰、缺失项说明。
    Warning {
        /// 阶段名。
        stage: String,
        /// 告警文本。
        message: String,
    },
}

/// 事件接收端。
pub trait EventSink {
    /// 发布一个事件。
    fn emit(&mut self, event: &BuildEvent);
}

/// 人类可读输出（每行一条）。
pub struct HumanSink<W: Write> {
    /// 输出目标。
    pub out: W,
}

impl<W: Write> EventSink for HumanSink<W> {
    fn emit(&mut self, event: &BuildEvent) {
        let line = match event {
            BuildEvent::Planned { targets, excluded } => {
                format!(
                    "• 计划：{} 个目标入选，{} 个排除",
                    targets.len(),
                    excluded.len()
                )
            }
            BuildEvent::Probed { found, missing } => {
                format!("• 探测：{} 个就绪，{} 个缺失", found.len(), missing.len())
            }
            BuildEvent::Compiled { crate_name, triple } => {
                format!("• 编译：{crate_name} ({triple})")
            }
            BuildEvent::UiBundled { adapter } => format!("• UI 打包：{adapter}"),
            BuildEvent::Signed { profile } => format!("• 签名：{profile}"),
            BuildEvent::Packaged { format, bytes } => format!("• 封装：{format}（{bytes} 字节）"),
            BuildEvent::Budgeted {
                target,
                format,
                bytes,
                limit,
                policy,
                code,
            } => match code {
                Some(code) => format!(
                    "⚠ 预算：{target}（{format}）超预算 {bytes}/{limit} 字节 [{code}]（策略 {policy}）"
                ),
                None => format!("• 预算：{target}（{format}）{bytes}/{limit} 字节（在预算内）"),
            },
            BuildEvent::Verified { checker, ok } => format!("• 校验：{checker} → {ok}"),
            BuildEvent::Published {
                channel,
                submission,
            } => format!("• 发布：{channel}（submission={submission}）"),
            BuildEvent::Failed { stage, code, hint } => match hint {
                Some(h) => format!("✗ 失败：{stage} [{code}]（{h}）"),
                None => format!("✗ 失败：{stage} [{code}]"),
            },
            BuildEvent::PluginGranted {
                plugin,
                fs_read,
                fs_write,
                net,
            } => format!(
                "• 插件授权：{plugin}（读 {} 目录，写 {} 目录，出网 {} 域名）",
                fs_read.len(),
                fs_write.len(),
                net.len()
            ),
            BuildEvent::Cached { key, hit } => {
                format!("• 缓存：{key} → {}", if *hit { "命中" } else { "未命中" })
            }
            BuildEvent::Warning { stage, message } => format!("⚠ {stage}：{message}"),
        };
        let _ = writeln!(self.out, "{line}");
    }
}

/// 机器可读事件流（JSON Lines，`--json`）。
pub struct JsonSink<W: Write> {
    /// 输出目标。
    pub out: W,
}

impl<W: Write> EventSink for JsonSink<W> {
    fn emit(&mut self, event: &BuildEvent) {
        if let Ok(line) = serde_json::to_string(event) {
            let _ = writeln!(self.out, "{line}");
        }
    }
}

/// 构建历史 schema 版本（`HistoryRecord.schema_version`；不兼容变更时递增）。
pub const HISTORY_SCHEMA_VERSION: u32 = 1;

/// 工程外路径在落盘记录中的占位符（绝不写宿主绝对路径）。
pub const REDACTED_PATH: &str = "<redacted>";

/// 历史记录的事件类型（`HistoryRecord.event`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryEventType {
    /// 一次构建完成（成功、告警或失败）。
    Build,
}

/// 历史记录的构建状态（`poly history` 的 STATUS 列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryStatus {
    /// 构建成功且该目标有本次产出物。
    Success,
    /// 构建完成但有告警，或该目标本次没有产出物（跳过 / 降级）。
    Warning,
    /// 构建失败（`error_code` 给出稳定错误码）。
    Failed,
}

/// 一条持久化的构建历史记录（JSON Lines，一行一条）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRecord {
    /// schema 版本（当前为 [`HISTORY_SCHEMA_VERSION`]）。
    pub schema_version: u32,
    /// 构建 id（一次 `poly build` 内所有目标共用）。
    pub build_id: String,
    /// 完成时间（Unix 毫秒时间戳，UTC）。
    pub ts: u64,
    /// 目标 id。
    pub target: String,
    /// 事件类型。
    pub event: HistoryEventType,
    /// 构建状态。
    pub status: HistoryStatus,
    /// 该次构建耗时（毫秒）。
    pub duration_ms: u64,
    /// 产物路径（工程相对路径；无法相对化时为 [`REDACTED_PATH`]）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
    /// 产物摘要（SHA-256，`sha256:<hex>`；目录产物为确定性树摘要）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
    /// 稳定错误码（失败时；如 `poly::E_COMPILE_0001`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

impl HistoryRecord {
    /// 构造一条构建完成记录（`event = build`）。
    pub fn build(
        build_id: impl Into<String>,
        target: impl Into<String>,
        status: HistoryStatus,
        ts: u64,
        duration_ms: u64,
    ) -> Self {
        Self {
            schema_version: HISTORY_SCHEMA_VERSION,
            build_id: build_id.into(),
            ts,
            target: target.into(),
            event: HistoryEventType::Build,
            status,
            duration_ms,
            artifact: None,
            artifact_digest: None,
            error_code: None,
        }
    }

    /// 附产物：`path` 落盘前脱敏为工程相对路径（工程外 → [`REDACTED_PATH`]）。
    #[must_use]
    pub fn with_artifact(
        mut self,
        project_root: &Path,
        path: &Path,
        digest: Option<String>,
    ) -> Self {
        self.artifact = Some(redact_path(project_root, path));
        self.artifact_digest = digest;
        self
    }

    /// 附稳定错误码。
    #[must_use]
    pub fn with_error_code(mut self, code: impl Into<String>) -> Self {
        self.error_code = Some(code.into());
        self
    }
}

/// 把路径脱敏为工程相对路径。
///
/// - 工程内路径 → `/` 分隔的相对路径（`.` 表示工程根）；
/// - 工程外绝对路径、越界相对路径（含 `..`）→ [`REDACTED_PATH`]。
///
/// 已存在的路径先 canonicalize 再比较，避免符号链接 / `..` 绕过相对化。
pub fn redact_path(project_root: &Path, path: &Path) -> String {
    let root = std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let candidate = std::fs::canonicalize(&candidate).unwrap_or(candidate);
    let Ok(relative) = candidate.strip_prefix(&root) else {
        return REDACTED_PATH.to_string();
    };
    let text = relative.to_string_lossy().replace('\\', "/");
    if text.is_empty() {
        return ".".to_string();
    }
    // 双保险：canonicalize 失败时仍拦住绝对路径与父目录穿越。
    if Path::new(&text).is_absolute() || text.split('/').any(|part| part == "..") {
        return REDACTED_PATH.to_string();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JSON 事件流与人读输出同源于一个事件：渠道与提交号两边都在（口径一致）。
    #[test]
    fn published_event_renders_same_fields_in_both_channels() {
        let event = BuildEvent::Published {
            channel: "wechat-open-platform".to_string(),
            submission: "audit-42".to_string(),
        };
        let mut json = Vec::new();
        JsonSink { out: &mut json }.emit(&event);
        let line = String::from_utf8(json).expect("UTF-8");
        let value: serde_json::Value = serde_json::from_str(line.trim()).expect("JSON 行");
        assert_eq!(value["event"], "published");
        assert_eq!(value["channel"], "wechat-open-platform");
        assert_eq!(value["submission"], "audit-42");

        let mut human = Vec::new();
        HumanSink { out: &mut human }.emit(&event);
        let text = String::from_utf8(human).expect("UTF-8");
        assert!(text.contains("wechat-open-platform"), "{text}");
        assert!(text.contains("submission=audit-42"), "{text}");
    }

    /// T-M9-16：落盘 schema 字段齐全，产物路径只留工程相对路径。
    #[test]
    fn history_record_serializes_schema_and_redacts_absolute_paths() {
        let root =
            std::env::temp_dir().join(format!("polypack-history-redaction-{}", std::process::id()));
        let inside = root.join(".polypack/packages/linux/app.AppImage");
        let outside = std::env::temp_dir().join("polypack-history-outside/app.AppImage");

        let record = HistoryRecord::build(
            "build-1700000000000",
            "linux",
            HistoryStatus::Success,
            1_700_000_000_000,
            1_234,
        )
        .with_artifact(&root, &inside, Some("sha256:abc".to_string()))
        .with_error_code("poly::E_COMPILE_0001");

        let json = serde_json::to_string(&record).expect("历史记录可序列化");
        assert!(json.contains("\"schema_version\":1"), "{json}");
        assert!(json.contains("\"event\":\"build\""), "{json}");
        assert!(json.contains("\"status\":\"success\""), "{json}");
        assert!(
            json.contains("\"artifact\":\".polypack/packages/linux/app.AppImage\""),
            "{json}"
        );
        assert!(
            json.contains("\"artifact_digest\":\"sha256:abc\""),
            "{json}"
        );
        assert!(
            json.contains("\"error_code\":\"poly::E_COMPILE_0001\""),
            "{json}"
        );
        assert!(
            !json.contains(&root.to_string_lossy().into_owned()),
            "不得写出工程绝对路径: {json}"
        );
        assert_eq!(
            serde_json::from_str::<HistoryRecord>(&json).expect("历史记录可反序列化"),
            record
        );

        let redacted = record.with_artifact(&root, &outside, None);
        assert_eq!(redacted.artifact.as_deref(), Some(REDACTED_PATH));
        assert_eq!(redacted.artifact_digest, None);
    }

    /// 相对路径保留；父目录穿越与工程外绝对路径一律脱敏。
    #[test]
    fn redact_path_keeps_relative_and_rejects_traversal() {
        let root =
            std::env::temp_dir().join(format!("polypack-history-relative-{}", std::process::id()));
        assert_eq!(
            redact_path(&root, Path::new("dist/app.bin")),
            "dist/app.bin"
        );
        assert_eq!(redact_path(&root, &root), ".");
        assert_eq!(
            redact_path(&root, Path::new("../escape/app.bin")),
            REDACTED_PATH
        );
        assert_eq!(
            redact_path(&root, &std::env::temp_dir().join("outside.bin")),
            REDACTED_PATH
        );
    }
}
