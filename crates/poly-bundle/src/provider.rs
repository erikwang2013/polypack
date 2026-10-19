//! 内建目标即官方插件（T-M8-05，ROADMAP 原则 6）。
//!
//! [`BuiltinProvider`] 把既有 [`Packager`] 包成统一 [`TargetProvider`]：`describe` 返回
//! `TargetSpec`，`package` 委托打包器矩阵——**不复制任何打包逻辑**。注册表由
//! [`official_registry`] 一次建成，调用侧（poly-cli）与 wasm 插件走同一查找 / 调用路径。
//!
//! 输入两种形态：宿主侧已组装的完整上下文（`PackageInput.host` 下转为 [`BundleInputs`]，
//! 生产路径）或通用字节输入（契约测试 / 插件等价路径，现场物化到临时目录）。

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use poly_targets::TargetSpec;
use poly_targets::provider::{
    PackageArtifact, PackageInput, ProviderError, ProviderRegistry, TargetProvider,
};

use crate::error::codes;
use crate::inputs::{BundleInputs, ProductInfo};
use crate::packager::{ArtifactKind, Packager};

/// 官方 provider：一个内建目标 + 服务它的既有打包器。
pub struct BuiltinProvider {
    spec: TargetSpec,
    packager: Box<dyn Packager>,
}

impl BuiltinProvider {
    /// 绑定目标声明与打包器；`serves` / `format` 必须与声明一致（注册期即拦漂移）。
    pub fn new(spec: TargetSpec, packager: Box<dyn Packager>) -> Result<Self, ProviderError> {
        if !packager.serves(&spec.id) || packager.format() != spec.package.format {
            return Err(ProviderError::Package {
                code: codes::E_BND_0003,
                message: format!(
                    "打包器 {}（{}）不服务目标 {} 的格式 {}",
                    packager.id(),
                    packager.format(),
                    spec.id,
                    spec.package.format
                ),
                hint: Some("核对 TargetSpec.package 与 Packager::serves / format".to_string()),
            });
        }
        Ok(Self { spec, packager })
    }

    /// 被委托的打包器 id（契约测试断言「仍是既有矩阵」）。
    pub fn packager_id(&self) -> &'static str {
        self.packager.id()
    }
}

impl TargetProvider for BuiltinProvider {
    fn describe(&self) -> Result<TargetSpec, ProviderError> {
        Ok(self.spec.clone())
    }

    fn package(&self, input: &PackageInput<'_>) -> Result<PackageArtifact, ProviderError> {
        if input.target != self.spec.id {
            return Err(ProviderError::Package {
                code: codes::E_BND_0003,
                message: format!("provider {} 收到目标 {} 的输入", self.spec.id, input.target),
                hint: None,
            });
        }
        // 生产路径用宿主侧完整上下文；无上下文时才按通用字节输入物化。
        let materialized = input.host.is_none();
        let scratch = if materialized {
            Some(materialize(input)?)
        } else {
            None
        };
        let bundle = match (&scratch, input.host) {
            (Some(scratch), _) => &scratch.bundle,
            (None, Some(host)) => {
                host.downcast_ref::<BundleInputs>()
                    .ok_or_else(|| ProviderError::Package {
                        code: codes::E_BND_0003,
                        message: "宿主上下文不是 BundleInputs".to_string(),
                        hint: None,
                    })?
            }
            (None, None) => unreachable!("materialize 覆盖了无上下文分支"),
        };
        let artifact = self
            .packager
            .package(bundle, &self.spec)
            .map_err(provider_error)?;
        // 目录产物无法内联：物化树随 Scratch 删除，必须走宿主 BundleInputs 路径。
        if materialized && artifact.kind == ArtifactKind::Directory {
            return Err(ProviderError::Package {
                code: codes::E_BND_0003,
                message: format!(
                    "目标 {} 的产物是目录树，不能经通用字节输入物化",
                    self.spec.id
                ),
                hint: Some(
                    "目录产物须走宿主上下文（poly build / verify 的 BundleInputs）".to_string(),
                ),
            });
        }
        // 物化树随 Scratch 删除：单文件产物改为内联字节返回（与 wasm provider 同形态），
        // 宿主路径则保留落盘路径。
        let content = if materialized && artifact.kind == ArtifactKind::File {
            std::fs::read(&artifact.path).map_err(io_error)?
        } else {
            Vec::new()
        };
        let path = if materialized {
            None
        } else {
            Some(artifact.path.to_string_lossy().into_owned())
        };
        Ok(PackageArtifact {
            target: artifact.target,
            format: artifact.format,
            path,
            bytes: if materialized && !content.is_empty() {
                content.len() as u64
            } else {
                artifact.bytes
            },
            content,
            directory: artifact.kind == ArtifactKind::Directory,
            notes: artifact.notes,
        })
    }
}

/// 全部官方 provider：内建目标声明的默认格式 + 配置可覆盖的其它格式
/// （与 [`crate::packager::find`] 的（目标，格式）解析面一一对应）。
pub fn official_providers() -> Vec<BuiltinProvider> {
    poly_targets::builtin::builtin()
        .iter()
        .flat_map(providers_for)
        .collect()
}

/// 单个目标的官方 provider：默认格式先、其它可覆盖格式随后（保持声明顺序）。
fn providers_for(spec: &TargetSpec) -> Vec<BuiltinProvider> {
    let mut out = Vec::new();
    for packager in crate::packager::builtin() {
        if packager.serves(&spec.id) && packager.format() == spec.package.format {
            out.push(BuiltinProvider {
                spec: spec.clone(),
                packager,
            });
        }
    }
    for packager in crate::packager::builtin() {
        if packager.serves(&spec.id) && packager.format() != spec.package.format {
            let mut alt = spec.clone();
            alt.package.format = packager.format().to_string();
            alt.package.builder = packager.id().to_string();
            out.push(BuiltinProvider {
                spec: alt,
                packager,
            });
        }
    }
    out
}

/// 官方 provider 注册表（注册期校验声明与重复项；内建数据不一致即 panic）。
///
/// 尚无内建打包器的实验目标也以 provider 形态登记：目标表与
/// `Registry::builtin()` 保持逐字段一致，打包调用如实返回 [`ProviderError::Unavailable`]，
/// 调用侧按环境性缺失跳过——不在调用侧留「有没有打包器」的旁路判断（后续新缺口沿用此形态）。
pub fn official_registry() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    for spec in poly_targets::builtin::builtin() {
        let providers = providers_for(&spec);
        if providers.is_empty() {
            registry
                .register(Box::new(MissingPackager { spec }))
                .expect("缺口目标登记必须通过校验且不与既有 provider 重复");
            continue;
        }
        for provider in providers {
            registry
                .register(Box::new(provider))
                .expect("官方 provider 注册表必须通过校验且无重复（目标 id，格式）");
        }
    }
    registry
}

/// 已声明但尚无内建打包器的目标：`describe` 原样返回声明，`package` 如实不可用。
struct MissingPackager {
    spec: TargetSpec,
}

impl TargetProvider for MissingPackager {
    fn describe(&self) -> Result<TargetSpec, ProviderError> {
        Ok(self.spec.clone())
    }

    fn package(&self, _input: &PackageInput<'_>) -> Result<PackageArtifact, ProviderError> {
        Err(ProviderError::Unavailable(format!(
            "目标 {}（format = {}）尚无内建打包器",
            self.spec.id, self.spec.package.format
        )))
    }
}

// ---------------------------------------------------------------- 通用输入物化

/// 物化后的临时上下文；Drop 时整树删除（不污染工程与系统临时目录）。
struct Scratch {
    root: PathBuf,
    bundle: BundleInputs,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// 把通用字节输入物化为打包器可消费的 [`BundleInputs`]（UI 文件树 + 核心库）。
fn materialize(input: &PackageInput<'_>) -> Result<Scratch, ProviderError> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "polypack-provider-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    let ui = root.join("dist");
    let out = root.join("out");
    std::fs::create_dir_all(&ui)
        .and_then(|_| std::fs::create_dir_all(&out))
        .map_err(|err| ProviderError::Package {
            code: codes::E_BND_0005,
            message: format!("创建 provider 临时目录失败：{err}"),
            hint: None,
        })?;

    for (name, contents) in input.files {
        let rel = safe_rel(name)?;
        let path = ui.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_error)?;
        }
        std::fs::write(&path, contents).map_err(io_error)?;
    }

    let mut product = ProductInfo {
        name: input.name.to_string(),
        version: input.version.to_string(),
        identifier: String::new(),
        manufacturer: "PolyPack".to_string(),
        description: input.name.to_string(),
        homepage: None,
    };
    product.identifier = format!("com.polypack.{}", product.kebab_name());
    let mut bundle = BundleInputs::new(product, out);
    if !input.files.is_empty() {
        bundle.ui_dist = Some(ui);
    }
    if !input.lib.is_empty() {
        let core = root.join("core.bin");
        std::fs::write(&core, input.lib).map_err(io_error)?;
        bundle.core_binary = Some(core);
    }
    Ok(Scratch { root, bundle })
}

/// 相对路径边界：拒绝绝对路径、盘符前缀与 `..`（插件输入不得逃出物化目录）。
fn safe_rel(name: &str) -> Result<&Path, ProviderError> {
    let path = Path::new(name);
    let ok = !name.is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if !ok {
        return Err(ProviderError::Package {
            code: codes::E_BND_0003,
            message: format!("UI 文件路径非法：{name:?}"),
            hint: Some("插件 / provider 输入只允许相对路径（不得含 `..` 或绝对前缀）".to_string()),
        });
    }
    Ok(path)
}

fn io_error(err: std::io::Error) -> ProviderError {
    ProviderError::Package {
        code: codes::E_BND_0005,
        message: format!("provider 物化输入失败：{err}"),
        hint: None,
    }
}

/// `BundleError` → 统一 provider 错误：环境性缺失走可跳过分支，其余保留错误码与提示。
fn provider_error(err: crate::error::BundleError) -> ProviderError {
    match err {
        err @ (crate::error::BundleError::ToolMissing { .. }
        | crate::error::BundleError::MissingInput { .. }) => {
            ProviderError::Unavailable(err.to_string())
        }
        other => ProviderError::Package {
            code: other.code(),
            message: other.to_string(),
            hint: other.hint().map(str::to_string),
        },
    }
}
