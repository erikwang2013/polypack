//! 目标注册表与 `--target all` 语义（ROADMAP §4.3）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::spec::{Host, SpecError, TargetSpec};

/// 目标解析请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetRequest {
    /// `--target all`：全部可构建目标（排除后给出原因清单）。
    All,
    /// 显式列出的目标：不可达即报错（除非 `allow_unreachable`）。
    Explicit(Vec<String>),
}

/// 被排除的目标及其原因。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExcludedTarget {
    /// 目标 id。
    pub id: String,
    /// 排除原因（写入 plan 阶段输出）。
    pub reason: String,
}

/// 解析结果：入选目标 + 排除清单。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResolvedTargets {
    /// 本次构建将执行的目标。
    pub selected: Vec<TargetSpec>,
    /// 被排除的目标与原因。
    pub excluded: Vec<ExcludedTarget>,
}

/// 目标注册表的读取接口（供 polypack-core / polypack-cli 使用）。
pub trait TargetRegistry {
    /// 按 id 解析单个目标。
    fn resolve(&self, id: &str) -> Result<TargetSpec, SpecError>;
    /// 全部已注册目标 id。
    fn ids(&self) -> Vec<String>;
}

/// 内建 + 注入的目标注册表。
#[derive(Debug, Clone, Default)]
pub struct Registry {
    specs: BTreeMap<String, TargetSpec>,
}

/// 当前宿主平台（探测失败返回 `None`）。
pub fn current_host() -> Option<Host> {
    if cfg!(target_os = "windows") {
        Some(Host::Win)
    } else if cfg!(target_os = "macos") {
        Some(Host::Macos)
    } else if cfg!(any(
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )) {
        Some(Host::Bsd)
    } else if cfg!(target_os = "linux") {
        Some(Host::Linux)
    } else {
        None
    }
}

impl Registry {
    /// 从目标列表构建注册表：逐一校验并拒绝重复 id。
    pub fn from_specs(specs: Vec<TargetSpec>) -> Result<Self, SpecError> {
        let mut map = BTreeMap::new();
        for spec in specs {
            spec.validate()?;
            if map.contains_key(&spec.id) {
                return Err(SpecError::Duplicate(spec.id));
            }
            map.insert(spec.id.clone(), spec);
        }
        Ok(Self { specs: map })
    }

    /// 内建目标注册表（L1 七目标 + L2 小程序族四目标见 `builtin`）。
    pub fn builtin() -> Self {
        Self::from_specs(crate::builtin::builtin()).expect("内建目标必须通过校验")
    }

    /// 按 id 查询。
    pub fn get(&self, id: &str) -> Option<&TargetSpec> {
        self.specs.get(id)
    }

    /// 全部目标（按 id 排序）。
    pub fn all(&self) -> impl Iterator<Item = &TargetSpec> {
        self.specs.values()
    }

    /// 解析目标请求（`--target all` 语义：可构建性检查 + 排除原因）。
    ///
    /// - `All`：遍历全部目标，宿主 / 授权不可达者进 `excluded` 并附原因；
    /// - `Explicit`：显式目标不可达直接报错（`allow_unreachable` 时强制入选）。
    pub fn resolve_all(
        &self,
        request: &TargetRequest,
        hosts_available: &[Host],
        allow_unreachable: bool,
    ) -> Result<ResolvedTargets, SpecError> {
        let mut selected = Vec::new();
        let mut excluded = Vec::new();
        match request {
            TargetRequest::All => {
                for spec in self.all() {
                    match unreachable_reason(spec, hosts_available) {
                        Some(reason) if !allow_unreachable => {
                            excluded.push(ExcludedTarget {
                                id: spec.id.clone(),
                                reason,
                            });
                        }
                        _ => selected.push(spec.clone()),
                    }
                }
            }
            TargetRequest::Explicit(ids) => {
                for id in ids {
                    let spec = self
                        .get(id)
                        .ok_or_else(|| SpecError::UnknownTarget(id.clone()))?;
                    match unreachable_reason(spec, hosts_available) {
                        Some(reason) if !allow_unreachable => {
                            return Err(SpecError::Unreachable(id.clone(), reason));
                        }
                        _ => selected.push(spec.clone()),
                    }
                }
            }
        }
        Ok(ResolvedTargets { selected, excluded })
    }
}

impl TargetRegistry for Registry {
    fn resolve(&self, id: &str) -> Result<TargetSpec, SpecError> {
        self.get(id)
            .cloned()
            .ok_or_else(|| SpecError::UnknownTarget(id.to_string()))
    }

    fn ids(&self) -> Vec<String> {
        self.specs.keys().cloned().collect()
    }
}

/// 目标在给定宿主集合下不可达的原因（可达返回 `None`）。
///
/// 规则：L7 需厂商授权默认不可达；宿主集合不含目标声明的任何宿主亦不可达。
fn unreachable_reason(spec: &TargetSpec, hosts_available: &[Host]) -> Option<String> {
    if spec.tier == 7 {
        return Some("需厂商授权（L7，仅以官方 SDK 插件形式提供）".to_string());
    }
    let host_ok = spec.hosts.iter().any(|h| hosts_available.contains(h));
    if !host_ok {
        return Some(format!("宿主不可达：目标声明宿主 {:?}", spec.hosts));
    }
    None
}
