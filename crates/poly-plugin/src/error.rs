//! 插件宿主错误：稳定错误码 + 修复提示（进 `poly::E_PLUGIN_xxxx` 诊断表）。

use poly_core::{Code, Diagnostic};

/// 插件加载 / 能力拒绝 / 执行失败。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("[{code}] {message}")]
pub struct PluginError {
    /// 稳定错误码（见 [`poly_core::codes`]）。
    pub code: Code,
    /// 人类可读描述（不含宿主绝对路径 / 环境变量）。
    pub message: String,
    /// 修复提示。
    pub hint: Option<String>,
}

impl PluginError {
    /// 构造错误。
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
        }
    }

    /// 附加修复提示。
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// 转为统一诊断（CLI 的 `Failed` 事件携带同一错误码）。
    pub fn diagnostic(&self) -> Diagnostic {
        let diagnostic = Diagnostic::error(self.code, self.message.clone());
        match &self.hint {
            Some(hint) => diagnostic.with_hint(hint.clone()),
            None => diagnostic,
        }
    }
}
