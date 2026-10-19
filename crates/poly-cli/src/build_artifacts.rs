//! `poly build` 的产物快照与 `[retention]` 淘汰（从 `build.rs` 拆出，保持单文件可读）。
//!
//! 失败只告警，不影响构建结果。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use poly_cache::{LocalStore, RetentionPolicy, enforce_retention};
use poly_core::{BuildEvent, EventSink, pipeline};

use crate::project;

/// 产物快照：`<root>/.polypack/artifacts/<target>/build-<millis>/{core,dist}`，每目标保留
/// `[retention].artifact_builds` 次（LRU = 快照名序，时间戳宽度固定）。
pub fn snapshot_artifacts(
    project: &poly_core::Project,
    targets: &[poly_targets::TargetSpec],
    events: &mut dyn EventSink,
) {
    let artifacts = project::app_artifacts_root(project);
    let build_id = format!(
        "build-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_millis())
            .unwrap_or(0)
    );
    let dist = project.root.join("dist");
    let core_artifact = pipeline::artifact_path(project).ok();

    for target in targets {
        let dir = artifacts.join(&target.id).join(&build_id);
        let mut copied = Vec::new();
        if let Some(artifact) = &core_artifact
            && artifact.is_file()
        {
            let dest = dir.join("core").join(file_name(artifact));
            if copy_file(artifact, &dest).is_ok() {
                copied.push("core");
            }
        }
        if dist.is_dir() && copy_dir(&dist, &dir.join("dist")).is_ok() {
            copied.push("dist");
        }
        // 打包产物（`.polypack/packages/<target>` = 最新一份）复制进快照留档；原件保留，
        // 打包器告警里给用户的路径才指向真实存在的产物
        let staged = project::app_packages_root(project).join(&target.id);
        if staged.is_dir() && copy_dir(&staged, &dir.join("packages")).is_ok() {
            copied.push("packages");
        }
        if copied.is_empty() {
            let _ = std::fs::remove_dir_all(&dir);
            continue;
        }
        prune_snapshots(
            &artifacts.join(&target.id),
            project.config.retention.artifact_builds,
            events,
        );
    }
}

/// 每目标保留最近 `keep` 次快照（按名称序 = 时间序），其余删除。
fn prune_snapshots(target_dir: &Path, keep: u32, events: &mut dyn EventSink) {
    let Ok(entries) = std::fs::read_dir(target_dir) else {
        return;
    };
    let mut snapshots: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    snapshots.sort();
    let keep = keep as usize;
    while snapshots.len() > keep {
        let oldest = snapshots.remove(0);
        let freed = project::dir_size(&oldest)
            .map(|(bytes, _)| bytes)
            .unwrap_or(0);
        if std::fs::remove_dir_all(&oldest).is_ok() {
            events.emit(&BuildEvent::Warning {
                stage: "retention".to_string(),
                message: format!(
                    "产物快照超限（每目标保留 {keep} 次），已淘汰 {}（释放 {}）",
                    oldest.display(),
                    project::human_bytes(freed)
                ),
            });
        }
    }
}

/// 构建缓存 `[retention]`：先按天数过期、再按容量 LRU 淘汰（§5.5）。
pub fn enforce_retention_policy(project: &poly_core::Project, events: &mut dyn EventSink) {
    let policy = RetentionPolicy {
        cache_days: project.config.retention.cache_days,
        cache_gib: project.config.retention.cache_gib,
    };
    match enforce_retention(&LocalStore::new(project::app_cache_root(project)), &policy) {
        Ok(eviction) if !eviction.removed.is_empty() => events.emit(&BuildEvent::Warning {
            stage: "retention".to_string(),
            message: format!(
                "构建缓存超限（{} 天 / {} GiB），LRU 淘汰 {} 项（释放 {}）",
                policy.cache_days,
                policy.cache_gib,
                eviction.removed.len(),
                project::human_bytes(eviction.freed_bytes)
            ),
        }),
        Ok(_) => {}
        Err(err) => events.emit(&BuildEvent::Warning {
            stage: "retention".to_string(),
            message: format!("缓存保留策略执行失败（不阻断构建）: {err}"),
        }),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "artifact".to_string())
}

fn copy_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to).map(|_| ())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}
