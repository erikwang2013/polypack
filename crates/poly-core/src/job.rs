//! `JobSpec`：一次构建请求的完整描述（ROADMAP §4.2）。

use std::time::Duration;

use poly_targets::{ExcludedTarget, TargetSpec};
use serde::{Deserialize, Serialize};

/// 构建 profile。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// CI 环境（更严格、非交互）。
    Ci,
    /// 本地开发。
    Local,
}

/// 凭据来源（仅引用，绝不内联明文）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CredentialSource {
    /// 系统密钥库（Keychain / Credential Manager / Secret Service）。
    Keyring,
    /// 外部 Vault。
    Vault,
    /// 环境变量注入（CI 场景）。
    Env {
        /// 环境变量名。
        variable: String,
    },
}

/// 对密钥库条目的引用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef {
    /// 凭据档案名（= TargetSpec.signing）。
    pub name: String,
    /// 来源。
    pub source: CredentialSource,
}

/// 产物预算（§2 原则 4）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// 产物体积上限（字节）。
    pub size: Option<u64>,
    /// 启动时延上限（毫秒）。
    pub startup_ms: Option<u64>,
}

/// 构建时限预算（ROADMAP §5.4 的默认值）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeBudgets {
    /// 缓存全命中（单目标）。
    pub cache_hit: Duration,
    /// 增量构建（单目标）。
    pub incremental: Duration,
    /// L1 冷构建（本地五目标矩阵）。
    pub cold: Duration,
}

impl Default for TimeBudgets {
    fn default() -> Self {
        Self {
            cache_hit: Duration::from_secs(30),
            incremental: Duration::from_secs(180),
            cold: Duration::from_secs(1200),
        }
    }
}

/// 一次构建请求。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobSpec {
    /// 请求标识。
    pub id: String,
    /// 构建 profile。
    pub profile: Profile,
    /// 已解析的目标（含排除清单的决策结果，见 §4.3 `--target all` 语义）。
    pub targets: Vec<TargetSpec>,
    /// `--target all` 解析时被排除的目标与原因。
    #[serde(default)]
    pub excluded: Vec<ExcludedTarget>,
    /// 本次构建需要的凭据引用。
    #[serde(default)]
    pub secrets: Vec<CredentialRef>,
    /// 产物预算。
    #[serde(default)]
    pub budget: Budget,
}
