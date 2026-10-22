//! 签名阶段的稳定错误码与错误类型（T-M2-09 / T-M2-11）。
//!
//! 组织风格与 `polypack-bundle::error` 对齐：码值集中在 [`codes`] 常量表，以 `&'static str`
//! 暴露，CLI 侧用 `polypack_core::Code::new(err.code())` 转成 `Diagnostic`（见 `polypack-cli`）。
//!
//! **脱敏硬要求**：错误只携带凭据名 / 后端 id / 路径与后端自述原因，绝不携带凭据值。

use std::path::PathBuf;

/// 错误码常量表（签名域，新增码在此登记）。
pub mod codes {
    /// 缺少外部签名工具（apksigner / hap-sign-tool 及其 java 运行时）。
    pub const E_SIG_0001: &str = "polypack::E_SIG_0001";
    /// 凭据缺失 / 不可用 / 非法（引用错误、后端载荷解析失败）。
    pub const E_SIG_0002: &str = "polypack::E_SIG_0002";
    /// 外部命令（签名 / 校验）非零退出。
    pub const E_SIG_0003: &str = "polypack::E_SIG_0003";
    /// 输入 / 产物 / 临时密钥材料 IO 失败。
    pub const E_SIG_0004: &str = "polypack::E_SIG_0004";
}

/// 签名失败。
#[derive(Debug, thiserror::Error)]
pub enum SignError {
    /// 宿主缺少外部工具（绝不静默降级）。
    #[error("签名缺少外部工具 {tool}：{hint}")]
    ToolMissing {
        /// 缺失的工具（可执行名或显式配置的路径）。
        tool: String,
        /// 安装 / 替代建议。
        hint: String,
    },
    /// 凭据不可用：后端不可达（headless 无密钥库）、条目不存在、环境变量缺失。
    #[error("凭据 {name}（{backend} 后端）不可用：{reason}")]
    CredentialUnavailable {
        /// 凭据档案名（= `CredentialRef.name`）。
        name: String,
        /// 后端 id（`keyring` / `vault` / `env` / `memory`）。
        backend: &'static str,
        /// 后端自述原因（不含凭据值）。
        reason: String,
    },
    /// 凭据载荷非法：非 base64、非 JSON、字段缺失或空。
    #[error("凭据 {name}（{backend} 后端）载荷非法：{reason}")]
    CredentialInvalid {
        /// 凭据档案名。
        name: String,
        /// 后端 id。
        backend: &'static str,
        /// 非法原因（不含载荷内容）。
        reason: String,
    },
    /// 外部命令非零退出。
    ///
    /// stderr 尾部**直出到显示里**（同 `polypack-bundle::BundleError::ToolFailed`）：命令失败的
    /// 根因只在工具自己的 stderr，只给「查看 stderr」而不带内容，日志里就什么都没有。
    #[error("外部命令失败（退出码 {status}）：{command}\nstderr 尾部：{stderr}")]
    ToolFailed {
        /// 命令展示形态（apksigner 口令经 `env:VAR` 引用；hap-sign-tool 口令只能进 argv，
        /// 展示串已由 `polypack-sign::harmony` 脱敏为 `****`）。
        command: String,
        /// 退出状态。
        status: String,
        /// 诊断所用的 stderr 尾部。
        stderr: String,
    },
    /// 文件系统操作失败。
    #[error("签名 IO 失败：{path}")]
    Io {
        /// 出错路径。
        path: PathBuf,
        /// 底层错误。
        #[source]
        source: std::io::Error,
    },
}

impl SignError {
    /// 稳定错误码（见 [`codes`]）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::ToolMissing { .. } => codes::E_SIG_0001,
            Self::CredentialUnavailable { .. } | Self::CredentialInvalid { .. } => {
                codes::E_SIG_0002
            }
            Self::ToolFailed { .. } => codes::E_SIG_0003,
            Self::Io { .. } => codes::E_SIG_0004,
        }
    }

    /// 修复提示（供 `Diagnostic::with_hint` 使用）。
    pub fn hint(&self) -> Option<&str> {
        match self {
            Self::ToolMissing { hint, .. } => Some(hint),
            Self::CredentialUnavailable { .. } => Some(
                "headless 无 Secret Service / Vault 不可达时改用 Env 或 Memory 后端注入凭据\
                 （T-M2-11），或先用 `polypack doctor` 核对密钥库配置",
            ),
            Self::CredentialInvalid { .. } => Some(
                "后端载荷须为 base64(JSON)：{\"keystore\":\"<base64>\",\"store_password\":\"…\",\
                 \"key_alias\"?,\"key_password\"?}；HAP 签名另需 \"app_cert\"（证书链）与 \
                 \"profile\"（已签名 p7b）两个 base64 字段",
            ),
            Self::ToolFailed { .. } => Some(
                "命令与 stderr 已随本错直出；apksigner 路径按 POLYPACK_APKSIGNER → PATH → \
                 ANDROID_HOME / ANDROID_SDK_ROOT 解析，hap-sign-tool.jar 按 POLYPACK_HAP_SIGN_TOOL \
                 → $HARMONYOS_TOOLS 解析",
            ),
            Self::Io { .. } => Some("核对输入 APK / 输出目录 / 临时目录的路径、权限与磁盘空间"),
        }
    }
}

/// 构造 [`SignError::Io`] 的便捷函数（路径由调用方补齐）。
pub(crate) fn io_err(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> SignError {
    let path = path.into();
    move |source| SignError::Io { path, source }
}
