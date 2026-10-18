//! worker 侧任务隔离（T-M8-07）：进程边界之内的**一次性私有工作目录**。
//!
//! 隔离层级：容器边界（可选，接入时由部署层提供）之外，worker 进程本身就是主端与任务
//! 之间的进程边界；每任务再叠加一个 0700、名字不复用的工作目录，作为该任务的临时根
//! （签名材料等 `std::env::temp_dir()` 消费者由 CLI 经 [`super::TaskScope`] 重定向到此）。
//! 任务结束整目录删除——`remove_dir_all` 不跟随符号链接，指向目录外的链接只删链接本身。
//!
//! 目录名只由 pid + 进程内序号生成，**绝不包含**来自网络的任务标识 / 路径（防路径逃逸）；
//! 创建用 `create_dir`（已存在即失败重试），因此预置同名符号链接无法把目录引到别处。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::diagnostics::{Diagnostic, codes};

/// 进程内目录序号（跨 worker 线程不重名）。
static SEQ: AtomicU64 = AtomicU64::new(0);

/// worker 私有一次根：worker 启动时建（0700），退出时整目录删除。
pub(super) struct WorkRoot {
    /// `None` = 已显式删除（Drop 不再重复删）。
    path: Option<PathBuf>,
}

impl WorkRoot {
    /// 在 `base`（缺省系统临时目录）下建 `<prefix><pid>-<序号>`；`base` 不存在则创建。
    pub(super) fn create(base: Option<&Path>) -> Result<Self, Diagnostic> {
        let base = base
            .map(Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        std::fs::create_dir_all(&base)
            .map_err(|err| isolation_error("创建任务工作目录的父目录", &base, err))?;
        for _ in 0..16 {
            let candidate = base.join(format!(
                "polypack-farm-work-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            match create_private_dir(&candidate) {
                Ok(()) => {
                    return Ok(Self {
                        path: Some(candidate),
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(isolation_error("创建 worker 工作根", &candidate, err)),
            }
        }
        Err(Diagnostic::error(
            codes::E_FARM_0006,
            format!("worker 工作根连续 16 次重名（{}）", base.display()),
        )
        .with_hint("清理系统临时目录下的 polypack-farm-work-* 残留后重试"))
    }

    /// 工作根路径（每任务在其下建子目录）。
    pub(super) fn path(&self) -> &Path {
        self.path.as_deref().expect("工作根未移除")
    }

    /// 显式删除并上报失败（`Drop` 只兜底，静默失败不可接受）。
    pub(super) fn remove(mut self) -> Result<(), Diagnostic> {
        let path = self.path.take().expect("工作根只移除一次");
        remove_private_dir(&path)
    }
}

impl Drop for WorkRoot {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

/// 单个任务的一次性私有目录（0700）：任务结束删除，跨任务不复用。
pub(super) struct TaskWorkspace {
    /// `None` = 已显式删除（Drop 不再重复删）。
    path: Option<PathBuf>,
}

impl TaskWorkspace {
    /// 在 worker 工作根下创建；名字只含 pid + 序号（不含任务标识）。
    pub(super) fn create(root: &Path) -> Result<Self, Diagnostic> {
        for _ in 0..16 {
            let candidate = root.join(format!(
                "task-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            match create_private_dir(&candidate) {
                Ok(()) => {
                    return Ok(Self {
                        path: Some(candidate),
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(isolation_error("创建任务工作目录", &candidate, err)),
            }
        }
        Err(Diagnostic::error(
            codes::E_FARM_0006,
            format!("任务工作目录连续 16 次重名（{}）", root.display()),
        )
        .with_hint("清理 worker 工作根下的 task-* 残留后重试"))
    }

    /// 本任务独享目录。
    pub(super) fn path(&self) -> &Path {
        self.path.as_deref().expect("任务工作目录未移除")
    }

    /// 显式删除并上报失败（`Drop` 只兜底）。
    pub(super) fn remove(mut self) -> Result<(), Diagnostic> {
        let path = self.path.take().expect("任务工作目录只移除一次");
        remove_private_dir(&path)
    }
}

impl Drop for TaskWorkspace {
    fn drop(&mut self) {
        if let Some(path) = &self.path {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

/// 建 0700 目录（已存在 / 已存在同名符号链接都返回 `AlreadyExists`，绝不跟随）。
fn create_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// 整目录删除：`remove_dir_all` 不会跟随目录内的符号链接（只删链接本身）。
fn remove_private_dir(path: &Path) -> Result<(), Diagnostic> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(isolation_error("删除任务工作目录", path, err)),
    }
}

/// 隔离失败 → `poly::E_FARM_0006`（带「任务结束即销毁」的修复方向）。
fn isolation_error(action: &str, path: &Path, err: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        codes::E_FARM_0006,
        format!("{action}失败（{}）：{err}", path.display()),
    )
    .with_hint("检查临时目录权限，或用 --work-dir 指定 worker 私有可写目录；残留目录须人工清理")
}
