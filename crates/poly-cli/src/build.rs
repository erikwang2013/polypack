//! `poly build`：装配真实流水线（带内容寻址缓存）并执行，随后做产物快照与 `[retention]` 淘汰。
//!
//! 阶段序见 ROADMAP §5.1；`compile` 与 `bundle(ui)` 为双支线并行，汇聚于 `package`。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use poly_cache::{Cache, LocalStore, RetentionPolicy, enforce_retention};
use poly_core::{BuildEvent, Diagnostic, EventSink, Profile, pipeline};
use poly_targets::{Registry, TargetRequest, current_host};

use crate::project;

/// 构建 profile。
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ProfileArg {
    /// 本地开发。
    Local,
    /// CI（更严格、非交互）。
    Ci,
}

impl ProfileArg {
    /// profile 名（缓存键的「构建参数」输入）。
    fn name(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Ci => "ci",
        }
    }

    fn profile(self) -> Profile {
        match self {
            Self::Ci => Profile::Ci,
            Self::Local => Profile::Local,
        }
    }
}

/// `poly build` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// 目标 id（可重复；缺省 `all` = 当前宿主可达的全部已声明目标）。
    #[arg(long = "target", value_name = "ID")]
    pub targets: Vec<String>,
    /// 构建 profile。
    #[arg(long, value_enum, default_value_t = ProfileArg::Local)]
    pub profile: ProfileArg,
}

/// 执行一次构建；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let started = Instant::now();
    let project = project::load(&args.config)?;
    let registry = Registry::builtin();
    let hosts: Vec<_> = current_host().into_iter().collect();
    // 缺省与 `--target all` 同为「当前宿主可达的全部已声明目标」（§4.3）
    let request = if args.targets.is_empty() || args.targets.iter().any(|id| id == "all") {
        TargetRequest::All
    } else {
        TargetRequest::Explicit(args.targets.clone())
    };
    let job = project
        .config
        .to_job(&registry, &request, args.profile.profile(), &hosts)
        .map_err(|err| err.diagnostic)?;

    let cache = Arc::new(Cache::from_root(project::cache_root(&project.root)));
    // package 阶段由 CLI 注入：打包器矩阵在 poly-bundle，poly-core 不得依赖它（依赖方向约束）
    let plan = pipeline::plan_with_package(
        cache.clone(),
        args.profile.name(),
        Some(Box::new(crate::package::PackageStage)),
    )?;
    let mut events = project::sink(json);
    plan.run_with(Some(&project), &job, events.as_mut())?;

    // 产物快照（每目标一份）与 [retention] 自动淘汰：失败只告警，不影响构建结果
    snapshot_artifacts(&project, &job.targets, events.as_mut());
    enforce_retention_policy(&project, events.as_mut());

    if !json {
        let stats = cache.stats();
        println!(
            "构建完成：{} 个目标；缓存命中 {}/{}（{:.0}%）；耗时 {:.2}s",
            job.targets.len(),
            stats.hits,
            stats.hits + stats.misses,
            stats.hit_rate() * 100.0,
            started.elapsed().as_secs_f64()
        );
    }
    Ok(0)
}

/// 产物快照：`<root>/.polypack/artifacts/<target>/build-<millis>/{core,dist}`，每目标保留
/// `[retention].artifact_builds` 次（LRU = 快照名序，时间戳宽度固定）。
fn snapshot_artifacts(
    project: &poly_core::Project,
    targets: &[poly_targets::TargetSpec],
    events: &mut dyn EventSink,
) {
    let artifacts = project::artifacts_root(&project.root);
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
        let staged = project::packages_root(&project.root).join(&target.id);
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
fn enforce_retention_policy(project: &poly_core::Project, events: &mut dyn EventSink) {
    let policy = RetentionPolicy {
        cache_days: project.config.retention.cache_days,
        cache_gib: project.config.retention.cache_gib,
    };
    match enforce_retention(
        &LocalStore::new(project::cache_root(&project.root)),
        &policy,
    ) {
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
