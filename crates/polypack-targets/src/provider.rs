//! 统一目标 provider（T-M8-05，ROADMAP 原则 6「插件优先」）。
//!
//! 内建目标只是「官方插件」：内建 provider（polypack-bundle）与第三方 wasm 插件
//! （polypack-plugin）实现同一 [`TargetProvider`] 契约，注册表按（目标 id，产物格式）
//! 统一登记与查找，调用侧不再区分来源。
//!
//! 本模块保持 `no_std + alloc`：`describe` / `package` 的通用输入输出只含
//! `String` / `Vec`；宿主侧富上下文以 `&dyn Any` 透传（仅内建 provider 下转）。

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::any::Any;

use crate::spec::{SpecError, TargetSpec};

/// 通用打包输入（与 `target-spec/v1` 的 `ui-bundle` + `core-lib` 对齐）。
///
/// wasm provider 直接消费这些字段；内建 provider 在 `host` 在场时优先使用宿主侧
/// 已组装的完整上下文（图标 / 裸产物 / 平台输入等），否则由通用字段现场物化。
#[derive(Clone, Copy)]
pub struct PackageInput<'a> {
    /// 应用名。
    pub name: &'a str,
    /// 应用版本。
    pub version: &'a str,
    /// 目标 id。
    pub target: &'a str,
    /// UI 入口（相对路径）。
    pub entry: &'a str,
    /// UI 文件（相对路径 → 内容）。
    pub files: &'a [(String, Vec<u8>)],
    /// 核心库三元组。
    pub triple: &'a str,
    /// 核心库字节。
    pub lib: &'a [u8],
    /// 宿主侧完整上下文（内建 provider 下转为 `BundleInputs`；wasm provider 忽略）。
    pub host: Option<&'a dyn Any>,
}

/// 统一打包产物。
///
/// 内建 provider 返回落盘路径与体积；wasm 插件返回内联字节，由调用侧落盘后回填路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageArtifact {
    /// 目标 id。
    pub target: String,
    /// 产物格式。
    pub format: String,
    /// 落盘路径（内联产物落盘后回填）。
    pub path: Option<String>,
    /// 产物体积（文件 / 目录树，或内联字节数）。
    pub bytes: u64,
    /// 内联产物字节（wasm 插件；内建 provider 为空）。
    pub content: Vec<u8>,
    /// 是否为目录产物。
    pub directory: bool,
    /// 降级 / 环境注记（转发为告警）。
    pub notes: Vec<String>,
}

impl PackageArtifact {
    /// 内联产物（wasm 插件）。
    pub fn inline(target: impl Into<String>, format: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            target: target.into(),
            format: format.into(),
            path: None,
            bytes: bytes.len() as u64,
            content: bytes,
            directory: false,
            notes: Vec::new(),
        }
    }
}

/// provider 失败原因。
///
/// `Package` / `Describe` 携带稳定错误码与修复提示，调用侧原样进诊断表
/// （内建 provider 透传 `polypack::E_BND_*`，wasm provider 透传 `polypack::E_PLUGIN_*`）。
/// `Unavailable` = 环境性缺失（打包工具缺席 / 输入未就绪）：调用侧按跳过处理。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    /// 目标声明非法。
    #[error("目标声明非法: {0}")]
    Spec(#[from] SpecError),
    /// provider 描述失败（版本协商 / 插件执行）。
    #[error("[{code}] {message}")]
    Describe {
        /// 稳定错误码。
        code: &'static str,
        /// 人类可读描述。
        message: String,
        /// 修复提示。
        hint: Option<String>,
    },
    /// 打包失败。
    #[error("[{code}] {message}")]
    Package {
        /// 稳定错误码。
        code: &'static str,
        /// 人类可读描述。
        message: String,
        /// 修复提示。
        hint: Option<String>,
    },
    /// 环境性缺失：调用侧跳过本目标，不硬失败。
    #[error("环境性缺失: {0}")]
    Unavailable(String),
    /// 同一（目标 id，产物格式）重复注册。
    #[error("provider 重复注册: {id}（{format}）")]
    Duplicate {
        /// 目标 id。
        id: String,
        /// 产物格式。
        format: String,
    },
}

impl ProviderError {
    /// 稳定错误码（`Unavailable` / `Duplicate` 无错误码返回 `None`）。
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::Spec(_) | Self::Unavailable(_) | Self::Duplicate { .. } => None,
            Self::Describe { code, .. } | Self::Package { code, .. } => Some(code),
        }
    }

    /// 修复提示。
    pub fn hint(&self) -> Option<&str> {
        match self {
            Self::Describe { hint, .. } | Self::Package { hint, .. } => hint.as_deref(),
            _ => None,
        }
    }
}

/// 统一目标 provider 契约：`describe` 声明目标，`package` 承接打包。
pub trait TargetProvider {
    /// 声明的目标规格（`target-spec/v1` 的宿主侧形态）。
    fn describe(&self) -> Result<TargetSpec, ProviderError>;
    /// 按通用输入打包。
    fn package(&self, input: &PackageInput<'_>) -> Result<PackageArtifact, ProviderError>;
}

/// 按（目标 id，产物格式）注册的 provider 注册表。
///
/// 注册顺序保留：同一 id 的多个格式中，先注册者作为 `specs()` 的默认声明
/// （官方注册表先登记目标声明的默认格式，再登记配置可覆盖的其它格式）。
#[derive(Default)]
pub struct ProviderRegistry {
    providers: BTreeMap<(String, String), Box<dyn TargetProvider>>,
    order: Vec<(String, String)>,
}

impl ProviderRegistry {
    /// 空注册表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册 provider：先 `describe` 并校验声明，再按（id，格式）登记。
    ///
    /// 返回被登记的目标声明（即 `describe` 的结果）。
    pub fn register(
        &mut self,
        provider: Box<dyn TargetProvider>,
    ) -> Result<TargetSpec, ProviderError> {
        let spec = provider.describe()?;
        spec.validate()?;
        let key = (spec.id.clone(), spec.package.format.clone());
        if self.providers.contains_key(&key) {
            return Err(ProviderError::Duplicate {
                id: spec.id,
                format: spec.package.format,
            });
        }
        self.order.push(key.clone());
        self.providers.insert(key, provider);
        Ok(spec)
    }

    /// 按（目标 id，产物格式）查找 provider。
    pub fn get(&self, target: &str, format: &str) -> Option<&dyn TargetProvider> {
        self.providers
            .get(&(target.to_string(), format.to_string()))
            .map(Box::as_ref)
    }

    /// 已注册的（目标 id，产物格式）对，按注册顺序。
    pub fn keys(&self) -> Vec<(String, String)> {
        self.order.clone()
    }

    /// 全部已注册目标声明（按 id 去重，保留注册顺序中的首个格式）。
    pub fn specs(&self) -> Vec<TargetSpec> {
        let mut out: Vec<TargetSpec> = Vec::new();
        for key in &self.order {
            let Some(provider) = self.providers.get(key) else {
                continue;
            };
            let Ok(spec) = provider.describe() else {
                continue;
            };
            if out.iter().all(|have| have.id != spec.id) {
                out.push(spec);
            }
        }
        out
    }

    /// 注册项数量。
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}
