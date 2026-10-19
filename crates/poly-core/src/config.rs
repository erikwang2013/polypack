//! `polypack.toml` 配置模型（ROADMAP §3.4 / §4.6 schema 版本策略）。
//!
//! 配置装载 → `JobSpec` 的路径：`PolypackConfig::parse` 校验 schema 版本，
//! [`PolypackConfig::to_job`] 结合目标注册表解析出本次构建的目标与凭据引用。

use std::collections::BTreeMap;
use std::path::Path;

use poly_targets::{Host, Registry, TargetRequest};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::diagnostics::{Diagnostic, codes};
use crate::job::{
    Budget, CredentialRef, CredentialSource, JobSpec, PluginGrant, Profile, SizePolicy,
};

/// 当前支持的配置 schema 版本。
pub const CONFIG_SCHEMA: u32 = 1;

/// 配置解析与校验失败。
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// 读取文件失败。
    #[error("无法读取配置文件 {path}: {source}")]
    Io {
        /// 文件路径。
        path: String,
        /// 底层错误。
        source: std::io::Error,
    },
    /// TOML 语法或字段错误。
    #[error("polypack.toml 解析失败: {0}")]
    Toml(#[from] toml::de::Error),
    /// schema 版本不受支持。
    #[error("不支持的配置 schema 版本 {0}（当前 poly 支持 {CONFIG_SCHEMA}），请升级 poly")]
    UnsupportedSchema(u32),
}

impl ConfigError {
    /// 转为稳定诊断：`poly::E_CFG_0001` 版本不受支持 / `poly::E_CFG_0002` 解析失败 / `poly::E_IO_0001` 读取失败。
    pub fn diagnostic(&self) -> Diagnostic {
        match self {
            Self::Io { path, source } => Diagnostic::error(
                codes::E_IO_0001,
                format!("无法读取配置文件 {path}: {source}"),
            )
            .with_hint("确认文件存在且当前用户可读"),
            Self::Toml(err) => {
                Diagnostic::error(codes::E_CFG_0002, format!("polypack.toml 解析失败: {err}"))
                    .with_hint("核对字段名与类型；完整字段清单见 PolypackConfig::json_schema()")
            }
            Self::UnsupportedSchema(version) => Diagnostic::error(
                codes::E_CFG_0001,
                format!("不支持的配置 schema 版本 {version}（当前 poly 支持 {CONFIG_SCHEMA}）"),
            )
            .with_hint("升级 poly，或把 polypack.toml 的 schema 改为受支持版本"),
        }
    }
}

/// `to_job` 的目标解析失败：一律携带可展示的稳定诊断。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{diagnostic}")]
pub struct ConfigJobError {
    /// 根因诊断（错误码 + 修复提示）。
    pub diagnostic: Diagnostic,
}

impl From<Diagnostic> for ConfigJobError {
    fn from(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

fn default_schema() -> u32 {
    CONFIG_SCHEMA
}

/// `[package]` 段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageSection {
    /// 应用名。
    pub name: String,
    /// 应用版本。
    pub version: String,
}

/// `[ui]` 段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiSection {
    /// 界面框架（当前仅 `react`）。
    pub framework: String,
    /// 入口文件路径。
    pub entry: String,
    /// PWA `theme_color`（缺省用 Pax 品牌橙，见 [`DEFAULT_THEME_COLOR`]）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_color: Option<String>,
}

/// PWA manifest 的缺省主题色：Pax 品牌橙（`docs/assets/pax-mark.svg` 底色）。
pub const DEFAULT_THEME_COLOR: &str = "#ea580c";

/// `[core]` 段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoreSection {
    /// Rust 业务核心 crate 路径（TOML 键 `crate`，见 README 示例配置）。
    #[serde(rename = "crate")]
    pub crate_path: String,
}

/// `[publish]` 段。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishSection {
    /// 发布渠道列表（如 `play` / `appstore`）。
    #[serde(default)]
    pub stores: Vec<String>,
}

/// `[retention]` 段（ROADMAP §5.5 清理与保留策略）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetentionSection {
    /// 构建缓存保留天数。
    #[serde(default = "default_cache_days")]
    pub cache_days: u32,
    /// 构建缓存容量上限（GiB）。
    #[serde(default = "default_cache_gib")]
    pub cache_gib: u32,
    /// 每目标保留的构建产物次数。
    #[serde(default = "default_artifact_builds")]
    pub artifact_builds: u32,
}

fn default_cache_days() -> u32 {
    30
}
fn default_cache_gib() -> u32 {
    20
}
fn default_artifact_builds() -> u32 {
    10
}

impl Default for RetentionSection {
    fn default() -> Self {
        Self {
            cache_days: default_cache_days(),
            cache_gib: default_cache_gib(),
            artifact_builds: default_artifact_builds(),
        }
    }
}

/// `[[plugins]]` 段（T-M8-01）：插件能力授权清单，缺省空 = 沙箱默认（无文件系统、无网络）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginSection {
    /// 插件 id。
    pub id: String,
    /// 组件路径（相对工程根）。
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
    /// 制品内容摘要（`blake3-<hex>`，`poly plugin digest` 产出）；设置后加载前校验（T-M8-03）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sri: Option<String>,
    /// 分离签名文件路径（相对工程根）；设置后加载前必须验签通过（T-M8-03）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// 签名信任根（公钥）的凭据引用；`signature` 设置时必填（T-M8-03）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_root: Option<CredentialRef>,
}

/// `[budget]` 段（ROADMAP §2 原则 4「预算式构建」；T-M3-08 体积门禁）。
///
/// `size` 是每个打包产物的体积上限（字节）：出产物后按 [`Budget::size`] 判定，
/// 超预算按 `policy` 告警（`warn`）或失败（`fail`，`poly::E_BUDGET_0001`）。
/// 缺省不设门禁（体积报告照常进事件流）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BudgetSection {
    /// 产物体积上限（字节）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// 超预算策略（`warn` 告警 / `fail` 失败）。
    #[serde(default)]
    pub policy: SizePolicy,
}

/// `polypack.toml` 的完整模型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolypackConfig {
    /// 配置 schema 版本（缺省为 1）。
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// `[package]`。
    pub package: PackageSection,
    /// `[ui]`。
    pub ui: UiSection,
    /// `[core]`。
    pub core: CoreSection,
    /// `[targets.*]`：目标 id → 平台参数（结构随目标而异）。
    ///
    /// 值是 `toml::Value`，无法派生 schema：导出时以宽松对象 + 说明描述嵌入。
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, serde_json::Value>")]
    pub targets: BTreeMap<String, toml::Value>,
    /// `[publish]`。
    #[serde(default)]
    pub publish: Option<PublishSection>,
    /// `[retention]`。
    #[serde(default)]
    pub retention: RetentionSection,
    /// `[budget]`。
    #[serde(default)]
    pub budget: BudgetSection,
    /// `[[plugins]]`：插件能力授权清单（T-M8-01）。
    #[serde(default)]
    pub plugins: Vec<PluginSection>,
}

impl PolypackConfig {
    /// 从 TOML 文本解析并校验 schema 版本。
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text)?;
        if config.schema != CONFIG_SCHEMA {
            return Err(ConfigError::UnsupportedSchema(config.schema));
        }
        Ok(config)
    }

    /// 从文件装载。
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::parse(&text)
    }

    /// 导出 `polypack.toml` 的 JSON Schema（Draft 7）。
    ///
    /// `[targets.*]` 的值是 `toml::Value`（结构由 poly-targets 的目标声明决定），
    /// 无法派生，故以宽松对象嵌入并补一段说明性描述。
    pub fn json_schema() -> serde_json::Value {
        let mut schema =
            serde_json::to_value(schemars::schema_for!(PolypackConfig)).expect("schema 可序列化");
        if let Some(targets) = schema.pointer_mut("/properties/targets") {
            targets["description"] = serde_json::json!(
                "目标 id → 平台参数；结构随目标而异（见 poly-targets 的目标声明）"
            );
        }
        schema
    }

    /// 从配置 + 目标注册表解析出本次构建的 `JobSpec`。
    ///
    /// - 目标选择交由 `Registry::resolve_all`（`--target all` 语义见 ROADMAP §4.3）：
    ///   `All` 时不可达目标进 `excluded` 并附原因，显式目标不可达即失败；
    /// - 配置 `[targets.*]` 出现注册表未知的目标即失败（`poly::E_TGT_0001`）；
    /// - `[targets.<id>].format` 覆盖注册表默认格式（M1 唯一可覆盖字段，见函数内 shortcut）；
    /// - 入选目标的 `TargetSpec.signing` 映射为 `job.secrets` 的 `CredentialRef`（按档案名去重）。
    ///   M0 占位来源固定为 `CredentialSource::Keyring`；M1 按 `[targets.*]` 与 profile 解析
    ///   真实来源（Vault / CI 环境变量注入），届时 `CredentialSource` 的非 Keyring 分支才会出现。
    /// - `id` 为 M0 的确定性占位（`包名-版本-profile`），M1 起由缓存键 / 请求序号取代。
    pub fn to_job(
        &self,
        registry: &Registry,
        request: &TargetRequest,
        profile: Profile,
        hosts_available: &[Host],
    ) -> Result<JobSpec, ConfigJobError> {
        for id in self.targets.keys() {
            if registry.get(id).is_none() {
                return Err(Diagnostic::error(
                    codes::E_TGT_0001,
                    format!("polypack.toml 声明了未知目标: {id}"),
                )
                .with_hint("核对 [targets.*] 段的目标 id，或升级 poly 以支持该目标")
                .into());
            }
        }

        let mut resolved = registry
            .resolve_all(request, hosts_available, false)
            .map_err(target_error)?;

        // 配置覆盖格式：`[targets.<id>].format` 是产品真值源（§3.4「目标即配置」），覆盖
        // 注册表默认（如 linux 默认 appimage → `format = "deb"`）。值合法性由打包器
        // `check_format` 把关（错误码更清楚），这里只管把它接上。
        // shortcut: M1 只开放 `format` 一个覆盖字段，其余仍以注册表为准；M2 再扩字段。
        for spec in &mut resolved.selected {
            let Some(value) = self
                .targets
                .get(&spec.id)
                .and_then(|section| section.get("format"))
            else {
                continue;
            };
            let Some(format) = value.as_str() else {
                return Err(Diagnostic::error(
                    codes::E_TGT_0003,
                    format!(
                        "[targets.{}].format 必须是字符串（当前是 {}）",
                        spec.id,
                        value.type_str()
                    ),
                )
                .with_hint("写成 format = \"deb\" 这样的字面量；可选值见 poly doctor 的目标矩阵")
                .into());
            };
            spec.package.format = format.to_string();
        }

        let mut secrets: Vec<CredentialRef> = Vec::new();
        for spec in &resolved.selected {
            let Some(name) = &spec.signing else { continue };
            if secrets.iter().any(|secret| &secret.name == name) {
                continue; // 多个目标可共用同一档案（如 iOS / macOS 的 apple-dev）
            }
            secrets.push(CredentialRef {
                name: name.clone(),
                source: CredentialSource::Keyring,
            });
        }

        // T-M8-03：签名与信任根必须成对配置；信任根只引用公钥（明文私钥永不进配置）。
        for plugin in &self.plugins {
            if plugin.signature.is_some() != plugin.trust_root.is_some() {
                return Err(Diagnostic::error(
                    codes::E_PLUGIN_0008,
                    format!("插件 {} 的 signature / trust_root 必须成对配置", plugin.id),
                )
                .with_hint(
                    "signature 指向分离签名文件（相对工程根），trust_root 是公钥的 CredentialRef\
                     （keyring / vault / env）",
                )
                .into());
            }
        }

        Ok(JobSpec {
            id: format!(
                "{}-{}-{}",
                self.package.name,
                self.package.version,
                match profile {
                    Profile::Ci => "ci",
                    Profile::Local => "local",
                }
            ),
            // 单应用配置没有工作区上下文；CLI 在成员构建时装填成员 id。
            app_id: None,
            profile,
            targets: resolved.selected,
            excluded: resolved.excluded,
            secrets,
            // `[budget]`（T-M3-08）：体积上限 + 超预算策略；`startup_ms` 尚无采集来源（M3+）
            budget: Budget {
                size: self.budget.size,
                startup_ms: None,
                policy: self.budget.policy,
            },
            // T-M8-01：授权清单随 JobSpec 下传（宿主按项放行，空列表 = 无能力）。
            plugins: self
                .plugins
                .iter()
                .map(|plugin| PluginGrant {
                    id: plugin.id.clone(),
                    path: plugin.path.clone(),
                    fs_read: plugin.fs_read.clone(),
                    fs_write: plugin.fs_write.clone(),
                    net: plugin.net.clone(),
                    sri: plugin.sri.clone(),
                    signature: plugin.signature.clone(),
                    trust_root: plugin.trust_root.clone(),
                })
                .collect(),
        })
    }
}

/// 目标解析失败 → 稳定诊断（未知目标 / 不可达 / 声明非法）。
fn target_error(err: poly_targets::SpecError) -> ConfigJobError {
    use poly_targets::SpecError;
    let (code, hint) = match &err {
        SpecError::UnknownTarget(_) => (
            codes::E_TGT_0001,
            "核对 polypack.toml 的 [targets.*] 段或 --target 参数",
        ),
        SpecError::Unreachable(..) => (
            codes::E_TGT_0002,
            "该目标需特定宿主 / 授权 / 凭据：换用其他目标，或在可达宿主机上构建",
        ),
        _ => (codes::E_TGT_0003, "目标声明非法，用 poly doctor 核对"),
    };
    Diagnostic::error(code, err.to_string())
        .with_hint(hint)
        .into()
}
