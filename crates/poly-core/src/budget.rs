//! 产物体积预算门禁（T-M3-08，ROADMAP §2 原则 4「预算式构建」）。
//!
//! `[budget].size` 是**每个打包产物**的体积上限：打包器出产物后即判定（骑在 DAG 的
//! `package` 节点上，逐目标一次），判定结果走事件流（[`BuildEvent::Budgeted`]）——
//! 这是「体积报告」的机器可读通道。超预算按 [`SizePolicy`] 处理：
//!
//! - `warn`（缺省）：告警放行，报文带 `poly::E_BUDGET_0001`（码可定位，不阻断）；
//! - `fail`：`package` 节点以 `poly::E_BUDGET_0001` 失败——DAG 语义下 `sign` / `verify` /
//!   `publish` 不再执行，进程退出码 1。
//!
//! 预算未设（`None`）不设门禁，但体积仍照常进事件流（`Packaged` + 本模块的判定不发）。
//!
//! 「按目标裁剪」（R10 对策）落在两处：wasm 出口层的 wasm-opt 裁剪在**出口目录生成后、
//! 随包拷贝前**做一次（四端共享同一出口目录，逐端差异化裁剪无实际对象——出口是共享的）；
//! 逐目标的部分是**判定与报告**：四端各自的代码包体积、预算占用与判定逐目标给出，谁也
//! 不替谁背书。

use crate::diagnostics::{Diagnostic, codes};
use crate::events::BuildEvent;
use crate::job::{Budget, SizePolicy};

/// 单个产物的体积预算判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeVerdict {
    /// 产物体积（字节）。
    pub bytes: u64,
    /// 预算上限（字节）。
    pub limit: u64,
    /// 是否超预算。
    pub exceeded: bool,
    /// 超预算策略（决定告警还是失败）。
    pub policy: SizePolicy,
}

/// 判定体积预算：`budget.size` 未设（不设门禁）→ `None`。
pub fn size_verdict(bytes: u64, budget: &Budget) -> Option<SizeVerdict> {
    let limit = budget.size?;
    Some(SizeVerdict {
        bytes,
        limit,
        exceeded: bytes > limit,
        policy: budget.policy,
    })
}

impl SizeVerdict {
    /// 事件流里的一条预算判定（目标 / 格式 / 体积 / 上限 / 策略 / 超预算时的稳定错误码）。
    pub fn event(&self, target: &str, format: &str) -> BuildEvent {
        BuildEvent::Budgeted {
            target: target.to_string(),
            format: format.to_string(),
            bytes: self.bytes,
            limit: self.limit,
            policy: self.policy.to_string(),
            code: self.exceeded.then(|| codes::E_BUDGET_0001.to_string()),
        }
    }

    /// 超预算且策略为 `fail` 时的中止诊断（`poly::E_BUDGET_0001`）：报文点名产物与差量，
    /// 提示给出可执行的下一步（收窄接口 / 分包 / 放宽预算）。
    pub fn failure(&self, target: &str, format: &str) -> Diagnostic {
        Diagnostic::error(
            codes::E_BUDGET_0001,
            format!(
                "目标 {target} 的产物（{format}）超体积预算：{} > {} 字节（超 {} 字节）",
                self.bytes,
                self.limit,
                self.bytes.saturating_sub(self.limit)
            ),
        )
        .with_hint(
            "收窄随包面（wasm 白名单函数 / 资源），或把 [budget].size 调到平台上限；\
             策略 `warn` 只告警不拦（[budget].policy）",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Budget;

    fn budget(size: Option<u64>, policy: SizePolicy) -> Budget {
        Budget {
            size,
            startup_ms: None,
            policy,
        }
    }

    #[test]
    fn unset_size_means_no_gate() {
        assert!(size_verdict(10, &budget(None, SizePolicy::Warn)).is_none());
    }

    #[test]
    fn verdict_marks_only_over_budget() {
        // 恰好等于上限不算超（`>` 而非 `>=`：预算是「不超过」）
        let within = size_verdict(100, &budget(Some(100), SizePolicy::Warn)).expect("有预算即判定");
        assert!(!within.exceeded);
        let over = size_verdict(101, &budget(Some(100), SizePolicy::Fail)).expect("有预算即判定");
        assert!(over.exceeded);
        assert_eq!((over.bytes, over.limit), (101, 100));
    }

    #[test]
    fn event_carries_code_only_when_exceeded() {
        let within = size_verdict(1, &budget(Some(2), SizePolicy::Warn)).expect("判定");
        let BuildEvent::Budgeted {
            code,
            policy,
            bytes,
            ..
        } = within.event("wechat", "weapp")
        else {
            panic!("应为 Budgeted 事件");
        };
        assert!(code.is_none(), "在预算内不带错误码");
        assert_eq!(policy, "warn");
        assert_eq!(bytes, 1);

        let over = size_verdict(3, &budget(Some(2), SizePolicy::Fail)).expect("判定");
        let BuildEvent::Budgeted { code, policy, .. } = over.event("wechat", "weapp") else {
            panic!("应为 Budgeted 事件");
        };
        assert_eq!(code.as_deref(), Some("poly::E_BUDGET_0001"));
        assert_eq!(policy, "fail");
    }

    #[test]
    fn failure_diagnostic_names_artifact_and_delta() {
        let over = size_verdict(4096, &budget(Some(2048), SizePolicy::Fail)).expect("判定");
        let diagnostic = over.failure("quickapp", "quickapp");
        assert_eq!(diagnostic.code, codes::E_BUDGET_0001);
        assert_eq!(diagnostic.severity, crate::Severity::Error);
        assert_eq!(diagnostic.exit_code(), 1);
        let text = diagnostic.to_string();
        for needle in ["quickapp", "4096", "2048", "超 2048 字节"] {
            assert!(text.contains(needle), "报文缺 {needle}：{text}");
        }
        assert!(
            diagnostic
                .hint
                .as_deref()
                .unwrap_or("")
                .contains("[budget].size"),
            "提示应点名配置项"
        );
    }
}
