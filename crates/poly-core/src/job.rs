//! `JobSpec`：一次构建请求的完整描述（ROADMAP §4.2）。
//!
//! T-M6-01：本模块属于引擎层，`std` 与 `no_std + alloc` 两种构建共用同一份实现。

use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;

use poly_targets::{ExcludedTarget, SpecError, TargetSpec};
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

/// 插件能力授权清单（T-M8-01）：空列表 = 沙箱默认（无文件系统、无网络）。
///
/// 路径相对工程根；域名写 `host` 或 `host:port`。清单随 `JobSpec` 持久化并进入
/// `poly build --json` 事件流（`plugin_granted`），宿主按此逐项放行。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginGrant {
    /// 插件 id（= `polypack.toml` `[[plugins]]` 的 `id`）。
    pub id: String,
    /// 插件组件路径（相对工程根）。
    pub path: String,
    /// 只读目录（相对工程根）。
    #[serde(default)]
    pub fs_read: Vec<String>,
    /// 可写目录（相对工程根）。
    #[serde(default)]
    pub fs_write: Vec<String>,
    /// 允许出网的域名（`host` 或 `host:port`）。
    #[serde(default)]
    pub net: Vec<String>,
}

/// 对密钥库条目的引用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef {
    /// 凭据档案名（= TargetSpec.signing）。
    pub name: String,
    /// 来源。
    pub source: CredentialSource,
}

/// 超预算策略（`[budget].policy`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "std", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum SizePolicy {
    /// 超预算仅告警（默认：本地开发不拦）。
    #[default]
    Warn,
    /// 超预算即失败（CI 门禁；`poly::E_BUDGET_0001`）。
    Fail,
}

impl core::fmt::Display for SizePolicy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            SizePolicy::Warn => "warn",
            SizePolicy::Fail => "fail",
        })
    }
}

/// 产物预算（§2 原则 4）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    /// 产物体积上限（字节）。
    pub size: Option<u64>,
    /// 启动时延上限（毫秒）。
    pub startup_ms: Option<u64>,
    /// 超预算策略（缺省 `warn`；`fail` 时超预算中止，见 [`SizePolicy`]）。
    #[serde(default)]
    pub policy: SizePolicy,
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
    /// 插件能力授权清单（T-M8-01；缺省为空 = 无文件系统、无网络）。
    #[serde(default)]
    pub plugins: Vec<PluginGrant>,
}

/// `JobSpec` 校验失败原因（引擎层，no_std 可用）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobError {
    /// 一个目标都没有。
    #[error("JobSpec 未包含任何目标")]
    EmptyTargets,
    /// 同一目标 id 出现多次。
    #[error("目标重复: {0}")]
    DuplicateTarget(String),
    /// 目标声明本身非法（id / tier / 宿主 / 打包字段）。
    #[error("目标 {id} 声明非法: {reason}")]
    InvalidTarget {
        /// 目标 id。
        id: String,
        /// `TargetSpec::validate` 给出的原因。
        reason: SpecError,
    },
    /// 目标引用的凭据档案在 `secrets` 中没有对应引用。
    #[error("目标 {target} 引用凭据档案 {name}，但 JobSpec 缺少对应凭据引用")]
    MissingCredential {
        /// 目标 id。
        target: String,
        /// 凭据档案名。
        name: String,
    },
}

impl JobSpec {
    /// 校验：目标非空、id 不重复、目标声明合法、凭据引用齐备。
    ///
    /// 顺序与 `plan` 阶段的快速失败语义一致（空目标 → 重复 → 非法声明 → 缺凭据），
    /// 引擎层与宿主流水线共用同一实现（T-M6-01）。
    pub fn validate(&self) -> Result<(), JobError> {
        if self.targets.is_empty() {
            return Err(JobError::EmptyTargets);
        }
        for (index, spec) in self.targets.iter().enumerate() {
            if self.targets[..index]
                .iter()
                .any(|other| other.id == spec.id)
            {
                return Err(JobError::DuplicateTarget(spec.id.clone()));
            }
            spec.validate().map_err(|reason| JobError::InvalidTarget {
                id: spec.id.clone(),
                reason,
            })?;
            if let Some(name) = &spec.signing
                && !self.secrets.iter().any(|secret| &secret.name == name)
            {
                return Err(JobError::MissingCredential {
                    target: spec.id.clone(),
                    name: name.clone(),
                });
            }
        }
        Ok(())
    }
}
