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
    /// 流水线 DAG 定义非法（节点重名 / 依赖缺失 / 成环）。
    pub const E_PLAN_0003: Code = Code::new("poly::E_PLAN_0003");
    /// 核心 crate 编译失败。
    pub const E_COMPILE_0001: Code = Code::new("poly::E_COMPILE_0001");
    /// UI 构建（vite / 内建静态打包）失败。
    pub const E_WEB_0001: Code = Code::new("poly::E_WEB_0001");
    /// PWA manifest 生成失败（缺图标资产 / 入口 HTML 不合法）。
    pub const E_WEB_0002: Code = Code::new("poly::E_WEB_0002");
    /// 构建缓存读写失败。
    pub const E_CACHE_0001: Code = Code::new("poly::E_CACHE_0001");
    /// `poly init` 落盘失败（目标文件已存在 / 写入失败）。
    pub const E_INIT_0001: Code = Code::new("poly::E_INIT_0001");
    /// `poly dev` 失败（端口占用 / 监听失败）。
    pub const E_DEV_0001: Code = Code::new("poly::E_DEV_0001");
    /// 桥生成失败（引擎缺失 / 接口定义非法 / 生成产物缺失）。
    pub const E_BRIDGE_0001: Code = Code::new("poly::E_BRIDGE_0001");
    /// 桥接口版本不受支持（非 `bridge/v1`）。
    pub const E_BRIDGE_0002: Code = Code::new("poly::E_BRIDGE_0002");
    /// AAR 组装失败（ABI 缺失 / 清单写入失败 / 归档失败）。
    pub const E_BRIDGE_0003: Code = Code::new("poly::E_BRIDGE_0003");
    /// wasm 沙箱白名单拒绝（接口或类型越出沙箱可用子集）。
    pub const E_BRIDGE_0004: Code = Code::new("poly::E_BRIDGE_0004");
    /// secret-scan 命中密钥材料。
    pub const E_SCAN_0001: Code = Code::new("poly::E_SCAN_0001");
    /// secret-scan 执行失败（IO / 仓库不可读）。
    pub const E_SCAN_0002: Code = Code::new("poly::E_SCAN_0002");
    /// 农场协议帧非法（序列化格式 / 版本 / 字段缺失）。
    pub const E_FARM_0001: Code = Code::new("poly::E_FARM_0001");
    /// worker 注册被拒（能力声明与任务需求不匹配）。
    pub const E_FARM_0002: Code = Code::new("poly::E_FARM_0002");
    /// worker 执行失败（错误码与修复提示回传）。
    pub const E_FARM_0003: Code = Code::new("poly::E_FARM_0003");
    /// 产物二跑不一致（`poly verify --reproducible`）。
    pub const E_VERIFY_0001: Code = Code::new("poly::E_VERIFY_0001");
    /// 没有可比对的产物 / 未接入该目标的校验器。
    pub const E_VERIFY_0002: Code = Code::new("poly::E_VERIFY_0002");
    /// 校验器判定产物有缺陷（如快应用 `.rpk` 的 hap-toolkit lint / 结构检查命中）。
    pub const E_VERIFY_0003: Code = Code::new("poly::E_VERIFY_0003");
    /// 提审输入未就绪（如未找到 `.rpk` 产物 / 缺必需参数）。
    pub const E_PUBLISH_0001: Code = Code::new("poly::E_PUBLISH_0001");
    /// 渠道客户端未落位（`poly-publish` 侧无该渠道的备位 / 内置实现）。
    pub const E_PUBLISH_0002: Code = Code::new("poly::E_PUBLISH_0002");
    /// 未找到可重签的未签名产物（`poly retry --from sign`）。
    pub const E_RETRY_0001: Code = Code::new("poly::E_RETRY_0001");
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
