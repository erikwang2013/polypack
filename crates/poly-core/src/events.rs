//! 构建事件总线（ROADMAP §4.2）：驱动进度、报告与缓存写入。

use std::io::Write;

use serde::Serialize;

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
    /// 发布完成。
    Published {
        /// 渠道。
        channel: String,
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
            BuildEvent::Published { channel } => format!("• 发布：{channel}"),
            BuildEvent::Failed { stage, code, hint } => match hint {
                Some(h) => format!("✗ 失败：{stage} [{code}]（{h}）"),
                None => format!("✗ 失败：{stage} [{code}]"),
            },
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
