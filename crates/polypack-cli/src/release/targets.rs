//! 发布目标解析：逗号 / 重复展开后的注册表查询与配置覆盖。

use std::path::{Path, PathBuf};

use polypack_core::Project;
use polypack_core::codes;
use polypack_core::diagnostics::Diagnostic;
use polypack_targets::{Registry, TargetSpec};

/// 解析目标列表：逗号 / 重复由 clap 展开；重复 id 去重保序，未知 id 报 `E_TGT_0001`。
pub(super) fn resolve_targets(
    ids: &[String],
    registry: &Registry,
) -> Result<Vec<TargetSpec>, Diagnostic> {
    let mut seen = std::collections::BTreeSet::new();
    let mut specs = Vec::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let spec = registry.get(id).ok_or_else(|| {
            Diagnostic::error(codes::E_TGT_0001, format!("目标 {id} 不在内建注册表"))
                .with_hint("用 polypack doctor 核对目标矩阵")
        })?;
        specs.push(spec.clone());
    }
    Ok(specs)
}

/// `[targets.<id>].format` 覆盖（与 polypack-core 的 JobSpec 解析同口径：配置是产品真值源）。
pub(super) fn apply_format_overrides(
    project: &Project,
    specs: &mut [TargetSpec],
) -> Result<(), Diagnostic> {
    for spec in specs {
        let Some(value) = project
            .config
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
            .with_hint("写成 format = \"rpm\" 这样的字面量；可选值见 polypack doctor 的目标矩阵"));
        };
        spec.package.format = format.to_string();
    }
    Ok(())
}

/// 仓库根：相对路径按工程根解析（缺省 `.polypack/repo`）。
pub(super) fn resolve_repo(root: &Path, repo: &Path) -> PathBuf {
    if repo.is_absolute() {
        repo.to_path_buf()
    } else {
        root.join(repo)
    }
}
