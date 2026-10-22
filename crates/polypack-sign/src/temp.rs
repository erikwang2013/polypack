//! 一次性私有临时目录 / 文件（T-M2-09 起共用；T-M5-05 的 GNUPGHOME 同源）。
//!
//! 目录 0700、文件 0600；`Drop` 兜底整目录删除——签名成功与失败都不留密钥材料。
//! Windows 无等价权限 API，靠目录随 [`TempDir`] 整体删除兜底。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{SignError, io_err};

/// 临时目录序号（同一进程内多次签名不重名）。
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 一次性临时目录。
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    /// 在系统临时目录下建 `<prefix><pid>-<序号>`（0700）。
    pub(crate) fn create(prefix: &str) -> Result<Self, SignError> {
        // farm worker 的任务作用域把临时根指向一次性私有目录（T-M8-07）；本地构建走系统临时目录。
        let base = crate::inject::temp_root().unwrap_or_else(std::env::temp_dir);
        for _ in 0..16 {
            let candidate = base.join(format!(
                "{prefix}{}-{}",
                std::process::id(),
                TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            match create_private_dir(&candidate) {
                Ok(()) => return Ok(Self(candidate)),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(io_err(&candidate)(err)),
            }
        }
        Err(SignError::Io {
            path: base,
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("临时目录名连续 16 次冲突（{prefix}<pid>-<序号>）"),
            ),
        })
    }

    /// 临时目录路径。
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 写 0600 私有文件（已存在即报错，不覆盖）。
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<(), SignError> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(io_err(path))?;
    file.write_all(bytes).map_err(io_err(path))
}

/// 建 0700 目录（Windows 无等价 API，忽略权限）。
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
