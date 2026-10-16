//! 发布阶段的稳定错误码与错误类型（T-M3-09 / T-M3-10）。
//!
//! 组织风格与 `poly-sign::error` 对齐：码值集中在 [`codes`] 常量表，以 `&'static str`
//! 暴露，CLI 侧用 `poly_core::Code::new(err.code())` 转成 `Diagnostic`。
//!
//! **脱敏硬要求**：错误只携带渠道 / 档案名 / 端点（脱敏后）/ 平台自述码与截断正文，
//! 绝不携带凭据值——所有取自平台响应 / 传输层的文本在构造错误前须经
//! [`crate::secret::Redactor`]。

use std::path::PathBuf;

/// 错误码常量表（发布域，新增码在此登记）。
pub mod codes {
    /// 凭据缺失 / 不可用 / 非法（引用错误、后端载荷解析失败）。
    pub const E_PUB_0001: &str = "poly::E_PUB_0001";
    /// HTTP 传输失败 / 非 2xx / 响应非法 JSON。
    pub const E_PUB_0002: &str = "poly::E_PUB_0002";
    /// 平台拒绝（业务错误码，如微信 errcode / 支付宝 code / 抖音 err_no）。
    pub const E_PUB_0003: &str = "poly::E_PUB_0003";
    /// 输入 / 回执归档 IO 或形态非法。
    pub const E_PUB_0004: &str = "poly::E_PUB_0004";
    /// 轮询超出预算仍未到终态（状态流水已落回执，R4：不承诺全自动过审）。
    pub const E_PUB_0005: &str = "poly::E_PUB_0005";
}

/// 发布失败。
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    /// 凭据不可用：无可用后端 / 条目不存在 / 环境变量缺失。
    #[error("凭据 {name} 不可用：{reason}")]
    CredentialUnavailable {
        /// 凭据档案名（= `CredentialRef.name`）。
        name: String,
        /// 尝试过的后端 id 清单（`memory` / `env` / 桥接实现自报）。
        providers: String,
        /// 后端自述原因（不含凭据值）。
        reason: String,
    },
    /// 凭据载荷非法：非 base64、非 JSON、字段缺失或空。
    #[error("凭据 {name} 载荷非法：{reason}")]
    CredentialInvalid {
        /// 凭据档案名。
        name: String,
        /// 后端 id。
        provider: &'static str,
        /// 非法原因（不含载荷内容）。
        reason: String,
    },
    /// HTTP 传输失败 / 非 2xx / 响应形态非法（文本已脱敏）。
    #[error("HTTP 失败（{endpoint}）：{reason}")]
    Http {
        /// 端点（URL 含凭据时已被 [`crate::secret::Redactor`] 脱敏）。
        endpoint: String,
        /// 脱敏后的失败原因（含状态码与正文尾部）。
        reason: String,
    },
    /// 平台拒绝（业务错误码；文本已脱敏）。
    #[error("平台 {channel} 拒绝：code={code}：{message}")]
    Platform {
        /// 渠道 id（`wechat-open-platform` 等）。
        channel: &'static str,
        /// 平台错误码（原样透传：微信 errcode / 支付宝 code / 抖音 err_no）。
        code: String,
        /// 平台错误信息（脱敏后）。
        message: String,
        /// 修复提示（如微信 85009 的「已有审核中版本」语义）。
        hint: Option<String>,
    },
    /// 输入（代码包目录）IO / 形态非法。
    ///
    /// 原因（`source`）**直出到显示里**：CLI 只渲染 `Diagnostic` 的 message，不走 source
    /// 链（poly-core 无该通路）——不带上原因，用户只能看到路径、看不到「根缺 app.json」。
    #[error("代码包非法：{path}（{source}）")]
    Input {
        /// 出错的代码包路径。
        path: PathBuf,
        /// 底层原因。
        #[source]
        source: std::io::Error,
    },
    /// 回执归档 IO 失败（原因直出，同 [`Self::Input`]）。
    #[error("回执归档失败：{path}（{source}）")]
    Archive {
        /// 出错的归档路径。
        path: PathBuf,
        /// 底层原因。
        #[source]
        source: std::io::Error,
    },
    /// 提审材料缺失 / 形态非法（平台要求材料齐全；本客户端不猜平台侧配置，如微信类目）。
    #[error("提审材料不合法（{channel}）：{reason}")]
    Materials {
        /// 渠道 id。
        channel: &'static str,
        /// 缺失 / 非法原因。
        reason: String,
    },
    /// 轮询超预算未终态（回执已带状态流水落盘）。
    #[error("轮询 {channel} 超时：最后状态 {last}（回执已归档，R4：审核结果以平台为准）")]
    PollTimeout {
        /// 渠道 id。
        channel: &'static str,
        /// 最后一次轮询的规范化状态。
        last: String,
    },
}

impl PublishError {
    /// 稳定错误码（见 [`codes`]）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::CredentialUnavailable { .. } | Self::CredentialInvalid { .. } => {
                codes::E_PUB_0001
            }
            Self::Http { .. } => codes::E_PUB_0002,
            Self::Platform { .. } => codes::E_PUB_0003,
            Self::Input { .. } | Self::Archive { .. } | Self::Materials { .. } => codes::E_PUB_0004,
            Self::PollTimeout { .. } => codes::E_PUB_0005,
        }
    }

    /// 修复提示（供 `Diagnostic::with_hint` 使用）。
    pub fn hint(&self) -> Option<String> {
        match self {
            Self::CredentialUnavailable { .. } => Some(
                "headless 无密钥库 / Vault 不可达时改用 Env 或内存后端注入凭据（T-M2-11 模型同款），\
                 载荷须为 base64(JSON)（见各平台客户端的字段约定）"
                    .to_string(),
            ),
            Self::CredentialInvalid { .. } => Some(
                "后端载荷须为 base64(JSON)：微信 {appid, secret, private_key} / \
                 支付宝 {app_id, private_key[, app_auth_token]} / 抖音 {appid, secret}"
                    .to_string(),
            ),
            Self::Http { .. } => Some(
                "核对端点可达性 / 代理 / 证书，重跑同键命令即从回执断点续走（上传与提审不重复）"
                    .to_string(),
            ),
            Self::Platform { hint, .. } => hint.clone(),
            Self::Input { .. } => {
                Some("代码包应为目录（T-M3-06 产物：<...>/miniapp/dist）且含 app.json".to_string())
            }
            Self::Archive { .. } => Some("核对归档目录（<root>/.polypack/publish）的权限与磁盘空间".to_string()),
            Self::Materials { .. } => Some(
                "材料是平台侧配置的如实转述：微信 materials = item_list 数组（1–5 项，每项 \
                 {address, tag, first_class, second_class, third_class, first_id, second_id, \
                 third_id}；类目须已在账号后台配置）"
                    .to_string(),
            ),
            Self::PollTimeout { .. } => Some(
                "R4：不承诺全自动过审；到平台后台查看审核结果，重跑同键命令只续轮询、不重复提审"
                    .to_string(),
            ),
        }
    }
}
