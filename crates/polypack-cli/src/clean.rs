//! `polypack clean`：清理构建缓存与产物快照（T-M1-15，§5.5）。
//!
//! 默认 dry-run：只列出可回收条目与空间，不产生任何写入；`--yes` 才执行。
//! 自动淘汰（按 `[retention]` 的天数 / 容量 / 次数）由 `polypack build` 在构建后完成，
//! 本命令是显式清空：`--cache` / `--artifacts` / `--all`（缺省二者都清）。

use std::path::{Path, PathBuf};

use polypack_cache::{CacheError, LocalStore, ObjectStore};
use polypack_core::{Diagnostic, codes};

use crate::project;

/// `polypack clean` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 清空构建缓存（`.polypack/cache`）。
    #[arg(long)]
    pub cache: bool,
    /// 清空产物快照（`.polypack/artifacts`）。
    #[arg(long)]
    pub artifacts: bool,
    /// 清空缓存与产物快照。
    #[arg(long)]
    pub all: bool,
    /// 实际执行（缺省为 dry-run）。
    #[arg(long)]
    pub yes: bool,
}

/// 一个待回收条目。
struct Reclaimable {
    /// 分类（缓存 / 产物快照）。
    kind: &'static str,
    /// 条目路径（缓存为对象键）。
    path: String,
    /// 占用字节数。
    bytes: u64,
}

/// 列出 / 执行清理；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let root = std::env::current_dir()
        .map_err(|err| project::io_error("读取当前目录", Path::new("."), err))?;
    let both = !args.cache && !args.artifacts && !args.all;
    let scope_cache = args.cache || args.all || both;
    let scope_artifacts = args.artifacts || args.all || both;

    let store = LocalStore::new(project::cache_root(&root));
    let entries = if scope_cache {
        store.entries().map_err(cache_error)?
    } else {
        Vec::new()
    };
    let snapshots = if scope_artifacts {
        list_snapshots(&project::artifacts_root(&root))?
    } else {
        Vec::new()
    };
    let packages = if scope_artifacts {
        list_packages(&project::packages_root(&root))?
    } else {
        Vec::new()
    };

    let reclaimable: Vec<Reclaimable> = entries
        .iter()
        .map(|entry| Reclaimable {
            kind: "cache",
            path: entry.key.as_str().to_string(),
            bytes: entry.bytes,
        })
        .chain(snapshots.iter().map(|snapshot| Reclaimable {
            kind: "artifacts",
            path: snapshot.path.display().to_string(),
            bytes: snapshot.bytes,
        }))
        .chain(packages.iter().map(|packages| Reclaimable {
            kind: "packages",
            path: packages.path.display().to_string(),
            bytes: packages.bytes,
        }))
        .collect();
    let total_bytes: u64 = reclaimable.iter().map(|item| item.bytes).sum();

    if args.yes {
        for entry in &entries {
            store.remove(&entry.key).map_err(cache_error)?;
        }
        for snapshot in &snapshots {
            std::fs::remove_dir_all(&snapshot.path)
                .map_err(|err| project::io_error("删除产物快照", &snapshot.path, err))?;
        }
        for packages in &packages {
            std::fs::remove_dir_all(&packages.path)
                .map_err(|err| project::io_error("删除打包产物", &packages.path, err))?;
        }
    }

    if json {
        let report = serde_json::json!({
            "event": "clean",
            "dry_run": !args.yes,
            "entries": reclaimable.len(),
            "bytes": total_bytes,
            "cache_entries": entries.len(),
            "artifact_snapshots": snapshots.len(),
            "package_dirs": packages.len(),
        });
        println!(
            "{}",
            serde_json::to_string(&report).expect("清理报告可序列化")
        );
    } else {
        println!(
            "polypack clean — {}",
            if args.yes {
                "执行清理"
            } else {
                "dry-run（加 --yes 执行）"
            }
        );
        for item in &reclaimable {
            println!(
                "  [{}] {}（{}）",
                item.kind,
                item.path,
                project::human_bytes(item.bytes)
            );
        }
        println!(
            "共 {} 项，{}；{}{}",
            reclaimable.len(),
            project::human_bytes(total_bytes),
            if args.yes {
                "已回收"
            } else {
                "可回收（未写入磁盘）"
            },
            if args.yes { "" } else { "，加 --yes 执行" }
        );
    }
    Ok(0)
}

/// 一份产物快照。
struct Snapshot {
    path: PathBuf,
    bytes: u64,
}

/// 列出 `.polypack/artifacts/<target>/build-<millis>/` 快照（不存在即空）。
fn list_snapshots(artifacts: &Path) -> Result<Vec<Snapshot>, Diagnostic> {
    let mut snapshots = Vec::new();
    let targets = match std::fs::read_dir(artifacts) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(snapshots),
        Err(err) => return Err(project::io_error("读取产物目录", artifacts, err)),
    };
    for target in targets {
        let target = target.map_err(|err| project::io_error("读取产物目录项", artifacts, err))?;
        if !target.path().is_dir() {
            continue;
        }
        let builds = std::fs::read_dir(target.path())
            .map_err(|err| project::io_error("读取目标产物目录", &target.path(), err))?;
        for build in builds {
            let build =
                build.map_err(|err| project::io_error("读取产物快照", &target.path(), err))?;
            let path = build.path();
            if !path.is_dir() {
                continue;
            }
            let bytes = project::dir_size(&path)
                .map_err(|err| project::io_error("统计产物快照", &path, err))?
                .0;
            snapshots.push(Snapshot { path, bytes });
        }
    }
    snapshots.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(snapshots)
}

/// 列出 `.polypack/packages/<target>/`（打包产物：每目标只留最新一份，不存在即空）。
fn list_packages(packages: &Path) -> Result<Vec<Snapshot>, Diagnostic> {
    let mut found = Vec::new();
    let targets = match std::fs::read_dir(packages) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(err) => return Err(project::io_error("读取打包产物目录", packages, err)),
    };
    for target in targets {
        let target =
            target.map_err(|err| project::io_error("读取打包产物目录项", packages, err))?;
        let path = target.path();
        if !path.is_dir() {
            continue;
        }
        let bytes = project::dir_size(&path)
            .map_err(|err| project::io_error("统计打包产物", &path, err))?
            .0;
        found.push(Snapshot { path, bytes });
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(found)
}

/// 缓存错误 → 稳定诊断。
fn cache_error(err: CacheError) -> Diagnostic {
    Diagnostic::error(codes::E_CACHE_0001, format!("构建缓存操作失败: {err}"))
        .with_hint("检查 .polypack/cache 的权限与磁盘空间")
}
