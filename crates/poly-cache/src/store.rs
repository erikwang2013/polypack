//! 对象存储 trait 与本地文件系统后端。
//!
//! 分片布局：`<root>/objects/<hex 前2位>/<hex>`；对象文件 = 32 字节
//! `blake3(载荷)` 前缀 ‖ 载荷（校验方式见 crate 文档）。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use crate::digest::Digest;
use crate::error::CacheError;

/// 对象文件头部校验摘要的长度（blake3 = 32 字节）。
const CHECKSUM_LEN: usize = 32;

/// 缓存条目元信息；`last_used` 取自对象文件 mtime。
#[derive(Debug, Clone)]
pub struct Entry {
    /// 对象键。
    pub key: Digest,
    /// 对象文件字节数（含校验前缀）。
    pub bytes: u64,
    /// 最近使用时间（命中读取时会 touch）。
    pub last_used: SystemTime,
}

/// 对象存储后端抽象：本地文件系统为默认实现，跨机共享可换远端实现。
pub trait ObjectStore: Send + Sync {
    /// 读对象；内容与写时摘要不符时视为未命中并删除损坏条目。
    fn get(&self, key: &Digest) -> Result<Option<Vec<u8>>, CacheError>;

    /// 写对象（同键覆盖，原子改名）。
    fn put(&self, key: &Digest, bytes: &[u8]) -> Result<(), CacheError>;

    /// 删除对象，返回释放的字节数（不存在时为 0）。
    fn remove(&self, key: &Digest) -> Result<u64, CacheError>;

    /// 列出全部条目。
    fn entries(&self) -> Result<Vec<Entry>, CacheError>;
}

/// 本地文件系统 CAS 后端。
pub struct LocalStore {
    root: PathBuf,
}

impl LocalStore {
    /// 以 `root` 为缓存根构造；目录按需创建。
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 缓存根目录。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 对象文件路径：`<root>/objects/<前2位>/<hex>`。
    fn object_path(&self, key: &Digest) -> PathBuf {
        let hex = key.as_str();
        self.root.join("objects").join(&hex[..2]).join(hex)
    }

    /// 命中时 touch mtime 以驱动 LRU；失败不致命（LRU 计时退化，内容正确性不受影响）。
    fn touch(&self, path: &Path) {
        let _ = fs::OpenOptions::new()
            .write(true)
            .open(path)
            .and_then(|file| file.set_modified(SystemTime::now()));
    }
}

impl ObjectStore for LocalStore {
    fn get(&self, key: &Digest) -> Result<Option<Vec<u8>>, CacheError> {
        let path = self.object_path(key);
        let object = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(CacheError::io(&path, e)),
        };
        // 读后重算 blake3，与写时前缀不符 → 视为未命中并删除损坏条目
        if object.len() < CHECKSUM_LEN
            || object[..CHECKSUM_LEN] != *blake3::hash(&object[CHECKSUM_LEN..]).as_bytes()
        {
            self.remove(key)?;
            return Ok(None);
        }
        self.touch(&path);
        Ok(Some(object[CHECKSUM_LEN..].to_vec()))
    }

    fn put(&self, key: &Digest, bytes: &[u8]) -> Result<(), CacheError> {
        let path = self.object_path(key);
        let dir = path.parent().expect("对象路径必有父目录");
        fs::create_dir_all(dir).map_err(|e| CacheError::io(dir, e))?;
        let mut object = Vec::with_capacity(CHECKSUM_LEN + bytes.len());
        object.extend_from_slice(blake3::hash(bytes).as_bytes());
        object.extend_from_slice(bytes);
        // 先写临时文件再改名：中断不会留下半截对象
        let tmp = dir.join(tmp_name(key.as_str()));
        fs::write(&tmp, &object).map_err(|e| CacheError::io(&tmp, e))?;
        fs::rename(&tmp, &path).map_err(|e| CacheError::io(&path, e))
    }

    fn remove(&self, key: &Digest) -> Result<u64, CacheError> {
        let path = self.object_path(key);
        let bytes = match fs::metadata(&path) {
            Ok(meta) => meta.len(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(CacheError::io(&path, e)),
        };
        fs::remove_file(&path).map_err(|e| CacheError::io(&path, e))?;
        if let Some(shard) = path.parent() {
            // 空分片目录顺手删除；失败（并发写入等）不影响结果
            let _ = fs::remove_dir(shard);
        }
        Ok(bytes)
    }

    fn entries(&self) -> Result<Vec<Entry>, CacheError> {
        let objects = self.root.join("objects");
        let shards = match fs::read_dir(&objects) {
            Ok(iter) => iter,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(CacheError::io(&objects, e)),
        };
        let mut out = Vec::new();
        for shard in shards {
            let shard = shard.map_err(|e| CacheError::io(&objects, e))?;
            let shard_path = shard.path();
            if !shard
                .file_type()
                .map_err(|e| CacheError::io(&shard_path, e))?
                .is_dir()
            {
                continue;
            }
            for item in fs::read_dir(&shard_path).map_err(|e| CacheError::io(&shard_path, e))? {
                let item = item.map_err(|e| CacheError::io(&shard_path, e))?;
                let name = item.file_name();
                // 非 64 位十六进制文件名的条目（临时文件等）不是本实现写的对象
                let Some(name) = name.to_str() else { continue };
                let Ok(key) = Digest::parse(name) else {
                    continue;
                };
                let meta = item
                    .metadata()
                    .map_err(|e| CacheError::io(item.path(), e))?;
                if !meta.is_file() {
                    continue;
                }
                // 无 mtime 的条目无法参与 LRU，跳过
                let Ok(last_used) = meta.modified() else {
                    continue;
                };
                out.push(Entry {
                    key,
                    bytes: meta.len(),
                    last_used,
                });
            }
        }
        Ok(out)
    }
}

/// 临时文件名（`.` 开头且非 64 位十六进制，不会被 `entries` 当作对象）。
fn tmp_name(hex: &str) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    format!(
        ".{hex}.tmp-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}
