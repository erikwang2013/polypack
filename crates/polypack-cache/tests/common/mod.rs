//! 集成测试共用的临时目录助手（std 手写；Drop 时清理）。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use polypack_cache::Digest;

/// `std::env::temp_dir()` 下的唯一临时目录，Drop 时递归删除。
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// 以 `tag` + 进程号 + 时间戳构造唯一目录。
    pub fn new(tag: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("系统时钟应晚于 UNIX 纪元")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "polypack-cache-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("创建临时目录");
        Self { path }
    }

    /// 临时目录根路径。
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// 本地后端的对象文件路径：`<root>/objects/<hex 前2位>/<hex>`。
#[allow(dead_code)] // 缓存键测试不涉及对象文件布局
pub fn object_path(root: &Path, key: &Digest) -> PathBuf {
    root.join("objects")
        .join(&key.as_str()[..2])
        .join(key.as_str())
}
