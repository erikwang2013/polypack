//! `poly build`：装配真实流水线（带内容寻址缓存）并执行，随后做产物快照与 `[retention]` 淘汰。
//!
//! 阶段序见 ROADMAP §5.1；`compile` 与 `bundle(ui)` 为双支线并行，汇聚于 `package`。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use poly_cache::{Cache, LocalStore, RetentionPolicy, enforce_retention};
use poly_core::events::{HistoryRecord, HistoryStatus};
use poly_core::{BuildEvent, Diagnostic, EventSink, PluginGrant, Profile, pipeline};
use poly_targets::{TargetRequest, current_host};
use sha2::{Digest, Sha256};

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
    let started_wall = SystemTime::now();
    let project = project::load(&args.config)?;
    // T-M8-05：目标表来自统一 provider 注册表（官方目标 + `[[plugins]]` 插件同表）。
    let registry = crate::providers::target_registry(&project)?;
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
    // package / sign / verify 三个阶段由 CLI 注入：打包器矩阵在 poly-bundle、签名链与校验器
    // 在 poly-sign，poly-core 不得依赖它们（依赖方向约束）
    let plan = pipeline::plan_with_stages(
        cache.clone(),
        args.profile.name(),
        pipeline::InjectedStages {
            package: Some(|| Box::new(crate::package::PackageStage)),
            sign: Some(|| Box::new(crate::sign::SignStage)),
            verify: Some(|| Box::new(crate::verify::VerifyStage)),
        },
    )?;
    let mut events = project::sink(json);
    emit_plugin_grants(events.as_mut(), &job.plugins);
    // 事件透传的同时旁路记录告警，供历史状态判定（T-M9-16）；不改变 stdout 事件流。
    let (outcome, warnings, warned_targets) = {
        let mut recorder = RecordingSink {
            inner: events.as_mut(),
            warnings: Vec::new(),
            warned_targets: BTreeSet::new(),
        };
        let outcome = plan.run_with(Some(&project), &job, &mut recorder);
        (outcome, recorder.warnings, recorder.warned_targets)
    };

    if outcome.is_ok() {
        // 产物快照（每目标一份）与 [retention] 自动淘汰：失败只告警，不影响构建结果
        snapshot_artifacts(&project, &job.targets, events.as_mut());
        enforce_retention_policy(&project, events.as_mut());
    }

    // 成功与失败都落一条每目标记录；历史写失败只告警，绝不改变构建结果（§5.5）。
    record_history(
        HistoryContext {
            project: &project,
            job: &job,
            warnings: &warnings,
            warned_targets: &warned_targets,
            failure: outcome.as_ref().err().map(|err| &err.diagnostic),
            started_wall,
            elapsed: started.elapsed(),
        },
        events.as_mut(),
    );

    outcome.map_err(Diagnostic::from)?;

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

/// T-M8-01：授权清单进事件流（`poly build --json` 的 `plugin_granted` 事件）。
fn emit_plugin_grants(events: &mut dyn EventSink, plugins: &[PluginGrant]) {
    for grant in plugins {
        events.emit(&BuildEvent::PluginGranted {
            plugin: grant.id.clone(),
            fs_read: grant.fs_read.clone(),
            fs_write: grant.fs_write.clone(),
            net: grant.net.clone(),
        });
    }
}

/// 事件透传 + 告警旁路记录：历史状态用它判定「本次构建有告警 / 该目标被点名」。
struct RecordingSink<'a> {
    inner: &'a mut dyn EventSink,
    warnings: Vec<String>,
    warned_targets: BTreeSet<String>,
}

impl EventSink for RecordingSink<'_> {
    fn emit(&mut self, event: &BuildEvent) {
        if let BuildEvent::Warning { message, .. } = event {
            self.warnings.push(message.clone());
        }
        if let BuildEvent::Budgeted {
            target,
            code: Some(_),
            ..
        } = event
        {
            self.warned_targets.insert(target.clone());
        }
        self.inner.emit(event);
    }
}

/// 一次构建的历史落盘上下文（字段多于 7 个，收成结构体避免 `too_many_arguments`）。
struct HistoryContext<'a> {
    project: &'a poly_core::Project,
    job: &'a poly_core::JobSpec,
    warnings: &'a [String],
    warned_targets: &'a BTreeSet<String>,
    failure: Option<&'a Diagnostic>,
    started_wall: SystemTime,
    elapsed: Duration,
}

/// 追加每目标一条构建记录（成功 / 告警 / 失败）；写失败降级为告警。
fn record_history(context: HistoryContext<'_>, events: &mut dyn EventSink) {
    let ts = crate::history::now_millis();
    let build_id = format!("build-{ts}-{}", std::process::id());
    let duration_ms = u64::try_from(context.elapsed.as_millis()).unwrap_or(u64::MAX);
    let packages = project::packages_root(&context.project.root);

    let records: Vec<HistoryRecord> = context
        .job
        .targets
        .iter()
        .map(|target| {
            let artifact = newest_artifact(&packages.join(&target.id), context.started_wall);
            let warned = context.warned_targets.contains(&target.id)
                || context
                    .warnings
                    .iter()
                    .any(|message| message.contains(&target.id));
            let (status, error_code) = match context.failure {
                Some(diagnostic) => (HistoryStatus::Failed, Some(diagnostic.code.to_string())),
                None if artifact.is_none() || warned => (HistoryStatus::Warning, None),
                None => (HistoryStatus::Success, None),
            };
            let mut record =
                HistoryRecord::build(build_id.clone(), &target.id, status, ts, duration_ms);
            if let Some((path, digest)) = artifact {
                record = record.with_artifact(&context.project.root, &path, Some(digest));
            }
            if let Some(code) = error_code {
                record = record.with_error_code(code);
            }
            record
        })
        .collect();

    if let Err(err) = crate::history::append_history(&context.project.root, &records) {
        events.emit(&BuildEvent::Warning {
            stage: "history".to_string(),
            message: format!("构建历史写入失败（不阻断构建）: {err}"),
        });
    }
}

/// 目标本次构建产出的最新产物：先找本次写入的最新文件，再取它所在的顶层条目
/// （目录产物取整棵树的确定性摘要，避免把目录内的元数据文件误当成产物本身）。
fn newest_artifact(dir: &Path, started_wall: SystemTime) -> Option<(PathBuf, String)> {
    let mut stack = vec![dir.to_path_buf()];
    let mut newest: Option<(SystemTime, PathBuf)> = None;
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
                continue;
            };
            // 2 秒容差：部分文件系统 mtime 精度为秒，构建开始同秒内的写入仍算本次。
            if started_wall
                .duration_since(modified)
                .is_ok_and(|age| age > Duration::from_secs(2))
            {
                continue;
            }
            let replace = newest.as_ref().is_none_or(|(best, best_path)| {
                modified > *best || (modified == *best && path > *best_path)
            });
            if replace {
                newest = Some((modified, path));
            }
        }
    }
    let (_, path) = newest?;
    let relative = path.strip_prefix(dir).ok()?;
    let top = dir.join(relative.components().next()?);
    let digest = artifact_digest(&top)?;
    Some((top, digest))
}

/// 产物摘要：文件取内容 SHA-256；目录按相对路径排序后做确定性树摘要。
fn artifact_digest(path: &Path) -> Option<String> {
    let mut hasher = Sha256::new();
    if path.is_file() {
        let mut file = std::fs::File::open(path).ok()?;
        std::io::copy(&mut file, &mut hasher).ok()?;
        return Some(format!("sha256:{:x}", hasher.finalize()));
    }
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).ok()?.flatten() {
            let file_type = entry.file_type().ok()?;
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if file_type.is_file() {
                let entry_path = entry.path();
                entries.push((path_key(path, &entry_path), entry_path));
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (key, entry_path) in entries {
        hasher.update(key.as_bytes());
        hasher.update([0]);
        let mut file = std::fs::File::open(&entry_path).ok()?;
        std::io::copy(&mut file, &mut hasher).ok()?;
        hasher.update([0]);
    }
    Some(format!("sha256:{:x}", hasher.finalize()))
}

/// 树内相对路径（统一 `/` 分隔），用作树摘要的排序键。
fn path_key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Collect(Vec<BuildEvent>);

    impl EventSink for Collect {
        fn emit(&mut self, event: &BuildEvent) {
            self.0.push(event.clone());
        }
    }

    #[test]
    fn plugin_grants_are_emitted_to_event_stream() {
        let plugins = vec![PluginGrant {
            id: "my-plugin".to_string(),
            path: "plugins/my.wasm".to_string(),
            fs_read: vec!["assets".to_string()],
            fs_write: vec!["out".to_string()],
            net: vec!["api.example.com".to_string()],
            sri: None,
            signature: None,
            trust_root: None,
        }];
        let mut sink = Collect(Vec::new());
        emit_plugin_grants(&mut sink, &plugins);

        assert_eq!(
            sink.0,
            vec![BuildEvent::PluginGranted {
                plugin: "my-plugin".to_string(),
                fs_read: vec!["assets".to_string()],
                fs_write: vec!["out".to_string()],
                net: vec!["api.example.com".to_string()],
            }]
        );
        let json = serde_json::to_string(&sink.0[0]).expect("事件可序列化");
        assert!(json.contains("\"event\":\"plugin_granted\""), "{json}");
        assert!(json.contains("\"plugin\":\"my-plugin\""), "{json}");
    }
}
