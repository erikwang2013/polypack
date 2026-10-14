//! 诊断框架：稳定错误码、严重级别与修复提示（ROADMAP §4.2）。
//!
//! 序列化是单向的（只 `Serialize`）：诊断是「产生侧」产物，报告读取方解析
//! JSON 字符串即可，理由见 [`Code`]。

use serde::Serialize;

/// 稳定错误码：`poly::E_<模块>_<四位序号>`。
///
/// 故意不实现 `Deserialize`：码值绑定编译期常量表（`codes`），而反序列化要求
/// `&'static str`，只能靠泄漏内存伪造，且会放过未登记的码。报告读取方按普通
/// 字符串解析 `code` 字段，再与 `codes` 常量比对即可；确有回读需求时（M1+）
/// 再补常量表查表构造。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Code(&'static str);

impl Code {
    /// 以字符串构造错误码（应使用 `codes` 模块中的常量）。
    pub const fn new(code: &'static str) -> Self {
        Self(code)
    }
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// 错误码常量表（新增码在此登记，避免散落字符串）。
pub mod codes {
    use super::Code;

    /// 配置 schema 版本不受支持。
    pub const E_CFG_0001: Code = Code::new("poly::E_CFG_0001");
    /// 配置文件解析失败。
    pub const E_CFG_0002: Code = Code::new("poly::E_CFG_0002");
    /// 目标不存在。
    pub const E_TGT_0001: Code = Code::new("poly::E_TGT_0001");
    /// 目标不可达（宿主 / 授权 / 凭据）。
    pub const E_TGT_0002: Code = Code::new("poly::E_TGT_0002");
    /// 目标声明非法。
    pub const E_TGT_0003: Code = Code::new("poly::E_TGT_0003");
    /// 缺少必需工具链。
    pub const E_PROBE_0001: Code = Code::new("poly::E_PROBE_0001");
    /// JobSpec 非法。
    pub const E_PLAN_0001: Code = Code::new("poly::E_PLAN_0001");
    /// 缺少凭据引用。
    pub const E_PLAN_0002: Code = Code::new("poly::E_PLAN_0002");
    /// IO 错误。
    pub const E_IO_0001: Code = Code::new("poly::E_IO_0001");
}

/// 严重级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// 信息。
    Info,
    /// 警告（不阻断）。
    Warn,
    /// 错误（阻断）。
    Error,
}

/// 一条诊断。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Diagnostic {
    /// 稳定错误码。
    pub code: Code,
    /// 严重级别。
    pub severity: Severity,
    /// 人类可读描述。
    pub message: String,
    /// 修复提示。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Diagnostic {
    /// 构造错误级诊断。
    pub fn error(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            message: message.into(),
            hint: None,
        }
    }

    /// 构造警告级诊断。
    pub fn warn(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Warn,
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

    /// 进程退出码：错误 → 1，其余 → 0。
    pub fn exit_code(&self) -> i32 {
        match self.severity {
            Severity::Error => 1,
            Severity::Warn | Severity::Info => 0,
        }
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, "（修复提示：{hint}）")?;
        }
        Ok(())
    }
}
