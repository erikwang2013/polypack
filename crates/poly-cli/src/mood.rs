//! T-M1-14（CLI 半）：Pax 五态表情 —— 启动横幅与运行结束状态。
//!
//! 五态（笑 / 哭 / 惊恐 / 疑惑 / 眨眼）取自品牌单一源 `tools/build-pax.mjs`，此处是其
//! 文本化演绎：终端图形通道（内联图片）未实现，M1 一律「文字 + ANSI 256 色」；非 TTY /
//! 设了 `NO_COLOR` / `--json` 时进一步降级为纯文字（不带颜色码）。

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicUsize, Ordering};

use poly_core::{BuildEvent, EventSink, HumanSink};

/// Pax 五态表情。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    /// 成功（笑）。
    Happy,
    /// 失败（哭）。
    Cry,
    /// 异常（惊恐）：非零退出等未预期结果。
    Shock,
    /// 待确认（疑惑）：跑完了但有告警。
    Confused,
    /// 空闲（眨眼）：启动横幅。
    Blink,
}

impl Mood {
    /// 文本面容。
    pub fn face(self) -> &'static str {
        match self {
            Self::Happy => "(^‿^)",
            Self::Cry => "(;﹏;)",
            Self::Shock => "(°o°)",
            Self::Confused => "(・_・?)",
            Self::Blink => "(-_-)",
        }
    }

    /// 256 色前景码；Pax 品牌橙 208 = `tokens.css` 的 `--poly-color-brand`（#ea580c）。
    fn color(self) -> u8 {
        match self {
            Self::Happy => 208,
            Self::Cry => 39,
            Self::Shock => 203,
            Self::Confused => 220,
            Self::Blink => 244,
        }
    }
}

/// 运行结束的（表情, 状态词）：成功=笑 / 有告警=疑惑（待确认）/ 非零退出=惊恐（异常）。
///
/// 失败（诊断错误）由调用方直接给 [`Mood::Cry`]：错误详情走 stderr，不在这里复述。
pub fn for_run(code: i32, warnings: usize) -> (Mood, String) {
    match (code, warnings) {
        (0, 0) => (Mood::Happy, "完成".to_string()),
        (0, n) => (Mood::Confused, format!("待确认（{n} 条告警）")),
        (code, _) => (Mood::Shock, format!("异常（退出码 {code}）")),
    }
}

/// 渲染一行 `<面容> <文字>`；`color = false` 即降级形态（纯文字，不带转义序列）。
pub fn line(mood: Mood, text: &str, color: bool) -> String {
    if color {
        format!(
            "\u{1b}[38;5;{}m{}\u{1b}[0m {text}",
            mood.color(),
            mood.face()
        )
    } else {
        format!("{} {text}", mood.face())
    }
}

/// 启动横幅（空闲 = 眨眼）。
pub fn banner(color: bool) -> String {
    line(Mood::Blink, "PolyPack —— 一次编写、全端打包", color)
}

/// 颜色可用性：stdout 是 TTY 且未设 `NO_COLOR`（管道 / 日志里不该混入转义序列）。
pub fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

static WARNINGS: AtomicUsize = AtomicUsize::new(0);

/// 本次进程累计的告警数（结束表情「待确认」的判据）。
pub fn warnings() -> usize {
    WARNINGS.load(Ordering::Relaxed)
}

/// 人读事件出口：转发 [`HumanSink`]，顺带累计告警数。
pub struct MoodSink<W: Write> {
    inner: HumanSink<W>,
}

impl<W: Write> MoodSink<W> {
    /// 包装一个人读事件出口。
    pub fn new(out: W) -> Self {
        Self {
            inner: HumanSink { out },
        }
    }
}

impl<W: Write> EventSink for MoodSink<W> {
    fn emit(&mut self, event: &BuildEvent) {
        if matches!(event, BuildEvent::Warning { .. }) {
            WARNINGS.fetch_add(1, Ordering::Relaxed);
        }
        self.inner.emit(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 状态 → 表情映射（成功笑 / 待确认疑惑 / 异常惊恐）。
    #[test]
    fn run_outcome_maps_to_mood() {
        assert_eq!(for_run(0, 0).0, Mood::Happy);
        assert_eq!(for_run(0, 2).0, Mood::Confused);
        assert!(for_run(0, 2).1.contains("2 条告警"), "{}", for_run(0, 2).1);
        assert_eq!(for_run(3, 0).0, Mood::Shock);
        assert!(for_run(3, 0).1.contains('3'), "{}", for_run(3, 0).1);
    }

    /// 降级分支：无色 = 纯文字无转义序列；有色 = 带 256 色前景码。
    #[test]
    fn color_off_degrades_to_plain_text() {
        let plain = line(Mood::Happy, "完成", false);
        assert!(
            !plain.contains('\u{1b}'),
            "降级形态不得含转义序列：{plain:?}"
        );
        assert!(plain.contains(Mood::Happy.face()));
        assert!(plain.contains("完成"));

        let colored = line(Mood::Happy, "完成", true);
        assert!(
            colored.contains("\u{1b}[38;5;208m"),
            "有色形态：{colored:?}"
        );
        assert!(colored.contains(Mood::Happy.face()));
        assert!(
            colored.ends_with("\u{1b}[0m 完成"),
            "复位后接文字：{colored:?}"
        );
    }

    /// 五态面容互不相同（映射表本身不重复）。
    #[test]
    fn faces_are_distinct() {
        let faces = [
            Mood::Happy.face(),
            Mood::Cry.face(),
            Mood::Shock.face(),
            Mood::Confused.face(),
            Mood::Blink.face(),
        ];
        let unique: std::collections::BTreeSet<&str> = faces.iter().copied().collect();
        assert_eq!(unique.len(), faces.len(), "五态面容重合：{faces:?}");
    }

    #[test]
    fn banner_uses_idle_face() {
        assert!(banner(false).starts_with(Mood::Blink.face()));
    }
}
