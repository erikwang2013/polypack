//! 嵌入式 / 车机目标的校验腿（T-M6-03/04）：QNX 镜像结构、ARXML 清单、C ABI 符号。
//!
//! 口径与其它校验腿一致：
//! - 产物不在场（QNX 无 SDP 时 package 按环境槽 SKIP）→ 告警点名，返回 `false`；
//! - 结构缺陷 / 符号漂移 → 硬失败（`bundle_diagnostic`，不降级成跳过）；
//! - 外部工具（xmllint / nm）缺席 → 结构断言照跑，工具腿打印 SKIP-with-print，不假装跑过。
//!
//! C ABI 的期望清单直接读 `poly_core::engine::ffi::FFI_FUNCTIONS`（唯一来源），
//! 不从打包产物反推——否则产物写错也会"自洽"。

use std::path::{Path, PathBuf};

use poly_bundle::error::BundleError;
use poly_core::Project;
use poly_core::events::{BuildEvent, EventSink};
use poly_core::plan::StageError;
use poly_targets::TargetSpec;

use crate::project;
use crate::verify::{bundle_diagnostic, warning};

/// QNX 镜像载荷结构（`qnx-image-structure`，T-M6-03）。
pub(crate) fn verify_qnx_image(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = staged_dir(project, spec);
    if !staged.join("qnx").is_dir() {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 QNX 镜像载荷（无 SDP 时 package 按环境槽 \
                 SKIP-with-print，不伪造产物）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    }
    let structure = match poly_bundle::check_qnx_structure(&staged) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    let Some(image) = qnx_image(&staged) else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *-image.tar.gz（结构文件在但镜像载荷缺席）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    warning(
        events,
        &format!(
            "目标 {} 的 QNX 镜像结构检查：triple {}，{} 个 C ABI 符号，buildfile {}，镜像 {}",
            spec.id,
            structure.triple,
            structure.symbols.len(),
            structure.buildfile.display(),
            image.file_name().unwrap_or_default().to_string_lossy()
        ),
    );
    emit_verified(events, ids);
    Ok(true)
}

/// ARXML 清单（`arxml-schema`，T-M6-04）：结构断言 + 服务与 C ABI 一一对应 + xmllint。
pub(crate) fn verify_arxml_schema(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = staged_dir(project, spec);
    let structure = match poly_bundle::check_arxml(&staged) {
        Ok(structure) => structure,
        Err(err @ BundleError::Io { .. }) => {
            warning(
                events,
                &format!(
                    "目标 {} 未校验：{} 下没有可读的 ARXML（先跑 poly build）——{err}",
                    spec.id,
                    staged.display()
                ),
            );
            return Ok(false);
        }
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    let expected = ffi_symbols();
    if structure.services != expected {
        return Err(bundle_diagnostic(
            spec,
            &BundleError::Archive {
                path: structure.path.clone(),
                reason: format!(
                    "ARXML 服务 {:?} 与 C ABI 符号 {expected:?} 不一一对应",
                    structure.services
                ),
            },
        )
        .into());
    }
    warning(
        events,
        &format!(
            "目标 {} 的 ARXML 结构检查：{} 个服务 / {} 个端口 / {} 个方法与 C ABI 清单一一对应",
            spec.id,
            structure.services.len(),
            structure.ports.len(),
            structure.methods.len()
        ),
    );
    match poly_bundle::xmllint_tool() {
        Some(tool) => {
            let mut command = poly_bundle::xmllint_command(&structure.path, &structure.schema);
            command.program = tool.display().to_string();
            match poly_bundle::exec::run(&command) {
                Ok(_) => warning(
                    events,
                    &format!("目标 {} 的 xmllint 真跑通过：{}", spec.id, command.line()),
                ),
                Err(err @ BundleError::ToolMissing { .. }) => {
                    warning(events, &format!("目标 {} 的 xmllint 未跑：{err}", spec.id));
                }
                Err(err) => {
                    return Err(bundle_diagnostic(spec, &err).into());
                }
            }
        }
        None => warning(
            events,
            &format!(
                "目标 {} 的 xmllint 未跑：PATH 无 xmllint（结构断言已过；schema 校验 \
                 SKIP-with-print，装 libxml2-utils 后重跑）",
                spec.id
            ),
        ),
    }
    emit_verified(events, ids);
    Ok(true)
}

/// 静态库导出符号（`c-abi-symbols`，T-M6-03/04）：nm 真跑，缺席退化声明侧结构断言。
pub(crate) fn verify_c_abi_symbols(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = staged_dir(project, spec);
    let Some(library) = engine_library(&staged, &spec.id) else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有引擎静态库（无 SDP / 缺输入时 package 按环境槽 \
                 SKIP-with-print）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    let expected = ffi_symbols();
    match poly_bundle::embedded::nm_engine_symbols(&library) {
        Some(found) => {
            if found != expected {
                return Err(bundle_diagnostic(
                    spec,
                    &BundleError::Archive {
                        path: library.clone(),
                        reason: format!("归档导出符号 {found:?} 与 C ABI 清单 {expected:?} 不一致"),
                    },
                )
                .into());
            }
            warning(
                events,
                &format!(
                    "目标 {} 的 nm 真跑：{} 个 poly_engine_* 导出符号与 C ABI 清单一致（{}）",
                    spec.id,
                    found.len(),
                    library.display()
                ),
            );
        }
        None => {
            let declared = declared_symbols(&staged, &spec.id, spec)?;
            if declared != expected {
                return Err(bundle_diagnostic(
                    spec,
                    &BundleError::Archive {
                        path: library.clone(),
                        reason: format!(
                            "声明侧符号 {declared:?} 与 C ABI 清单 {expected:?} 不一致"
                        ),
                    },
                )
                .into());
            }
            warning(
                events,
                &format!(
                    "目标 {} 的 nm 未跑：PATH 无 nm（声明侧 {} 个符号与 C ABI 清单一致；\
                     归档符号对照 SKIP-with-print）",
                    spec.id,
                    declared.len()
                ),
            );
        }
    }
    emit_verified(events, ids);
    Ok(true)
}

/// 打包暂存目录（与 package 阶段同源：`<packages>/<target>/`）。
fn staged_dir(project: &Project, spec: &TargetSpec) -> PathBuf {
    project::app_packages_root(project).join(&spec.id)
}

/// C ABI 期望符号（唯一来源；排序去重后与 nm / 声明侧同口径）。
fn ffi_symbols() -> Vec<String> {
    let mut symbols: Vec<String> = poly_core::engine::ffi::FFI_FUNCTIONS
        .iter()
        .map(|function| function.name.to_string())
        .collect();
    symbols.sort();
    symbols.dedup();
    symbols
}

/// QNX 镜像载荷（`*-image.tar.gz`；多份取字典序最后一份）。
fn qnx_image(staged: &Path) -> Option<PathBuf> {
    let mut images: Vec<PathBuf> = std::fs::read_dir(staged)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with("-image.tar.gz"))
        })
        .collect();
    images.sort();
    images.pop()
}

/// 引擎静态库落点（QNX / AUTOSAR 打包器的产物名不同）。
fn engine_library(staged: &Path, target_id: &str) -> Option<PathBuf> {
    let name = match target_id {
        "qnx" => "libpolypack_engine_qnx.a",
        "autosar" => "libpolypack_engine_autosar.a",
        _ => return None,
    };
    let path = staged.join("lib").join(name);
    path.is_file().then_some(path)
}

/// 声明侧符号：QNX 读 manifest，AUTOSAR 读 ARXML（nm 缺席时的真实结构断言）。
fn declared_symbols(
    staged: &Path,
    target_id: &str,
    spec: &TargetSpec,
) -> Result<Vec<String>, StageError> {
    let mut symbols = match target_id {
        "qnx" => qnx_manifest_symbols(staged).map_err(|err| bundle_diagnostic(spec, &err))?,
        "autosar" => {
            poly_bundle::check_arxml(staged)
                .map_err(|err| bundle_diagnostic(spec, &err))?
                .services
        }
        _ => Vec::new(),
    };
    symbols.sort();
    symbols.dedup();
    Ok(symbols)
}

/// QNX `qnx/manifest.json` 的 symbols 数组。
fn qnx_manifest_symbols(staged: &Path) -> Result<Vec<String>, BundleError> {
    let manifest = staged.join("qnx/manifest.json");
    let bytes = std::fs::read(&manifest).map_err(|source| BundleError::Io {
        path: manifest.clone(),
        source,
    })?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|err| BundleError::Archive {
            path: manifest.clone(),
            reason: format!("manifest 不是合法 JSON：{err}"),
        })?;
    let symbols = value["symbols"]
        .as_array()
        .ok_or_else(|| BundleError::Archive {
            path: manifest.clone(),
            reason: "manifest 缺 symbols".to_string(),
        })?
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect();
    Ok(symbols)
}

fn emit_verified(events: &mut dyn EventSink, ids: &[String]) {
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
}
