//! 统一目标 provider 注册与调用（T-M8-05，ROADMAP 原则 6）。
//!
//! 官方目标与 `[[plugins]]` 的 wasm 插件登记进同一个 [`ProviderRegistry`]：
//! `poly build` 的目标表来自 `specs()`，package / verify 的打包调用一律经
//! [`package`]——调用侧不再判断产物来自内建还是插件。

use std::path::{Path, PathBuf};

use poly_bundle::error::codes as bundle_codes;
use poly_core::codes;
use poly_core::{Code, Diagnostic, PluginGrant, Project};
use poly_plugin::{PluginProvider, ResourceLimits, Sandbox};
use poly_targets::provider::{PackageArtifact, PackageInput, ProviderError, ProviderRegistry};
use poly_targets::{Runtime, TargetSpec};

/// 官方 provider + `[[plugins]]` 插件统一注册。
pub(crate) fn registry(project: &Project) -> Result<ProviderRegistry, Diagnostic> {
    let mut registry = poly_bundle::official_registry();
    for section in &project.config.plugins {
        let grant = PluginGrant {
            id: section.id.clone(),
            path: section.path.clone(),
            fs_read: section.fs_read.clone(),
            fs_write: section.fs_write.clone(),
            net: section.net.clone(),
        };
        let path = project.root.join(&grant.path);
        let bytes = std::fs::read(&path).map_err(|err| {
            plugin_diag(format!(
                "读取插件 {}（{}）失败：{err}",
                grant.id,
                path.display()
            ))
        })?;
        let sandbox = Sandbox::new(ResourceLimits::default()).map_err(|err| err.diagnostic())?;
        let plugin = sandbox
            .load(&bytes, &grant.id, None, &project.root, &grant)
            .map_err(|err| err.diagnostic())?;
        // describe 的输入快照：插件可据 name / version / target 声明目标。
        let probe = poly_plugin::PackageInput {
            name: project.config.package.name.clone(),
            version: project.config.package.version.clone(),
            target: grant.id.clone(),
            entry: String::new(),
            files: Vec::new(),
            triple: String::new(),
            lib: Vec::new(),
        };
        let provider = PluginProvider::load(sandbox, plugin, &probe).map_err(diag)?;
        registry.register(Box::new(provider)).map_err(diag)?;
    }
    Ok(registry)
}

/// 统一目标表（官方目标 + `[[plugins]]`）→ 目标注册表。
///
/// `poly build` 与 `poly verify` 的目标选择同源于此，插件目标与内建目标同表可用。
pub(crate) fn target_registry(project: &Project) -> Result<poly_targets::Registry, Diagnostic> {
    let specs = registry(project)?.specs();
    poly_targets::Registry::from_specs(specs).map_err(|err| {
        Diagnostic::error(
            Code::new(codes::E_TGT_0003.as_str()),
            format!("目标声明非法：{err}"),
        )
    })
}

/// 经统一注册表打包一个目标；内联产物（wasm 插件）落盘后回填路径。
pub(crate) fn package(
    registry: &ProviderRegistry,
    spec: &TargetSpec,
    inputs: &poly_bundle::BundleInputs,
) -> Result<PackageArtifact, ProviderError> {
    let provider = registry
        .get(&spec.id, &spec.package.format)
        .ok_or_else(|| ProviderError::Package {
            code: codes::E_TGT_0003.as_str(),
            message: format!(
                "无 provider（目标 {}，格式 {}）",
                spec.id, spec.package.format
            ),
            hint: None,
        })?;
    let files = collect_files(inputs.ui_dist.as_deref())?;
    let lib = match &inputs.core_binary {
        Some(path) => std::fs::read(path).map_err(|err| ProviderError::Package {
            code: bundle_codes::E_BND_0005,
            message: format!("读取核心产物 {} 失败：{err}", path.display()),
            hint: None,
        })?,
        None => Vec::new(),
    };
    let entry = files
        .iter()
        .find(|(name, _)| name == "index.html")
        .map(|(name, _)| name.clone())
        .unwrap_or_default();
    let triple = match spec.runtime {
        Runtime::Wasm => "wasm32-unknown-unknown",
        _ => "native",
    };
    let portable = PackageInput {
        name: &inputs.product.name,
        version: &inputs.product.version,
        target: &spec.id,
        entry: &entry,
        files: &files,
        triple,
        lib: &lib,
        host: Some(inputs as &dyn std::any::Any),
    };
    let mut artifact = provider.package(&portable)?;
    if artifact.path.is_none() {
        if artifact.content.is_empty() {
            return Err(ProviderError::Package {
                code: codes::E_TGT_0003.as_str(),
                message: format!("目标 {} 的 provider 未返回产物", spec.id),
                hint: None,
            });
        }
        let path = write_inline(inputs, &artifact)?;
        artifact.path = Some(path.to_string_lossy().into_owned());
    }
    Ok(artifact)
}

/// UI 产物树 → 通用输入（相对路径、字典序；只取常规文件）。
fn collect_files(root: Option<&Path>) -> Result<Vec<(String, Vec<u8>)>, ProviderError> {
    let Some(root) = root else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    walk(root, Path::new(""), &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn walk(root: &Path, rel: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), ProviderError> {
    let dir = root.join(rel);
    let entries = std::fs::read_dir(&dir).map_err(|err| io_error(&dir, &err))?;
    for entry in entries {
        let entry = entry.map_err(|err| io_error(&dir, &err))?;
        let path = entry.path();
        let rel_path = rel.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&path).map_err(|err| io_error(&path, &err))?;
        if metadata.is_dir() {
            walk(root, &rel_path, out)?;
        } else if metadata.is_file() {
            let bytes = std::fs::read(&path).map_err(|err| io_error(&path, &err))?;
            let name = rel_path
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push((name, bytes));
        }
    }
    Ok(())
}

fn io_error(path: &Path, err: &std::io::Error) -> ProviderError {
    ProviderError::Package {
        code: bundle_codes::E_BND_0005,
        message: format!("读取 UI 产物 {} 失败：{err}", path.display()),
        hint: None,
    }
}

/// 插件内联产物落盘：`<out_dir>/<kebab>-<version>.<format>`（文件名安全校验）。
fn write_inline(
    inputs: &poly_bundle::BundleInputs,
    artifact: &PackageArtifact,
) -> Result<PathBuf, ProviderError> {
    let format = &artifact.format;
    if format.is_empty()
        || !format
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(ProviderError::Package {
            code: codes::E_TGT_0003.as_str(),
            message: format!("产物格式 {format:?} 不能用作文件名"),
            hint: Some("插件 package_format 仅允许字母数字与 `-` / `_`".to_string()),
        });
    }
    let path = inputs.out_dir.join(format!(
        "{}-{}.{}",
        inputs.product.kebab_name(),
        inputs.product.version,
        format.to_ascii_lowercase()
    ));
    std::fs::create_dir_all(&inputs.out_dir)
        .and_then(|_| std::fs::write(&path, &artifact.content))
        .map_err(|err| ProviderError::Package {
            code: bundle_codes::E_BND_0005,
            message: format!("写入插件产物 {} 失败：{err}", path.display()),
            hint: None,
        })?;
    Ok(path)
}

fn diag(err: ProviderError) -> Diagnostic {
    let code = err.code().unwrap_or(codes::E_TGT_0003.as_str());
    let mut diagnostic = Diagnostic::error(Code::new(code), err.to_string());
    if let Some(hint) = err.hint() {
        diagnostic = diagnostic.with_hint(hint);
    }
    diagnostic
}

fn plugin_diag(message: String) -> Diagnostic {
    Diagnostic::error(Code::new(codes::E_PLUGIN_0007.as_str()), message)
        .with_hint("核对 polypack.toml `[[plugins]].path` 与工程根")
}
