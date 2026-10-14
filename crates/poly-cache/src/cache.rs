//! 缓存门面：命中统计与目录级（compile 产物 / UI dist）存取。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::digest::Digest;
use crate::error::CacheError;
use crate::store::{LocalStore, ObjectStore};

/// 命中统计（`lookup` 与 `restore_tree` 各计一次）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    /// 命中次数。
    pub hits: u64,
    /// 未命中次数。
    pub misses: u64,
}

impl CacheStats {
    /// 命中率；无请求时为 0.0。
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// 缓存门面：对象后端 + 命中统计。
pub struct Cache {
    store: Box<dyn ObjectStore>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl Cache {
    /// 使用给定后端构造。
    pub fn new(store: impl ObjectStore + 'static) -> Self {
        Self {
            store: Box::new(store),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    /// 本地文件系统后端的便捷构造。
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self::new(LocalStore::new(root))
    }

    /// 读对象；未命中与损坏条目均返回 `None`，并计入命中统计。
    pub fn lookup(&self, key: &Digest) -> Result<Option<Vec<u8>>, CacheError> {
        let hit = self.store.get(key)?;
        self.count(hit.is_some());
        Ok(hit)
    }

    /// 写对象。
    pub fn store(&self, key: &Digest, bytes: &[u8]) -> Result<(), CacheError> {
        self.store.put(key, bytes)
    }

    /// 目录入库：清单（JSON `[{path, digest}]`，相对路径升序）存于 `key`，
    /// 各文件内容按内容摘要存入 CAS；空目录不入清单。
    pub fn store_tree(&self, key: &Digest, dir: &Path) -> Result<(), CacheError> {
        let mut files = Vec::new();
        collect_files(dir, String::new(), &mut files)?;
        files.sort(); // 相对路径升序 → 清单文本确定
        let mut manifest = Vec::with_capacity(files.len());
        for (rel, path) in files {
            let bytes = fs::read(&path).map_err(|e| CacheError::io(&path, e))?;
            let digest = Digest::of_bytes(&bytes);
            self.store.put(&digest, &bytes)?;
            manifest.push(ManifestEntry { path: rel, digest });
        }
        let json = serde_json::to_vec(&manifest).map_err(|e| CacheError::corrupt(key, e))?;
        self.store.put(key, &json)
    }

    /// 目录还原到 `dest`（覆盖写，不清理 `dest` 中多余文件）。
    ///
    /// `false` = 未命中（清单或其任一 blob 缺失）；每个 blob 都重新校验内容摘要。
    /// 未命中返回时 `dest` 可能已被部分写入（blob 逐个落盘，不整体缓冲）。
    pub fn restore_tree(&self, key: &Digest, dest: &Path) -> Result<bool, CacheError> {
        let Some(json) = self.store.get(key)? else {
            self.count(false);
            return Ok(false);
        };
        let manifest: Vec<ManifestEntry> =
            serde_json::from_slice(&json).map_err(|e| CacheError::corrupt(key, e))?;
        fs::create_dir_all(dest).map_err(|e| CacheError::io(dest, e))?;
        for entry in &manifest {
            // 远端对象存储可能在清单里塞路径穿越，属信任边界
            if !is_safe_rel(&entry.path) {
                return Err(CacheError::corrupt(
                    key,
                    format!("清单路径非法: {}", entry.path),
                ));
            }
            let Some(bytes) = self.store.get(&entry.digest)? else {
                self.count(false);
                return Ok(false);
            };
            if Digest::of_bytes(&bytes) != entry.digest {
                return Err(CacheError::corrupt(
                    &entry.digest,
                    "blob 内容与清单摘要不符",
                ));
            }
            let path = dest.join(&entry.path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| CacheError::io(parent, e))?;
            }
            fs::write(&path, &bytes).map_err(|e| CacheError::io(&path, e))?;
        }
        self.count(true);
        Ok(true)
    }

    /// 命中统计快照。
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
        }
    }

    fn count(&self, hit: bool) {
        let counter = if hit { &self.hits } else { &self.misses };
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

/// 清单条目：`path` 为 `/` 分隔的相对路径（非 UTF-8 文件名按 lossy 转换）。
#[derive(Debug, Serialize, Deserialize)]
struct ManifestEntry {
    path: String,
    digest: Digest,
}

/// 递归收集目录下的普通文件（相对路径, 绝对路径）；符号链接等其它类型不入清单。
fn collect_files(
    dir: &Path,
    prefix: String,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), CacheError> {
    let entries = fs::read_dir(dir).map_err(|e| CacheError::io(dir, e))?;
    for item in entries {
        let item = item.map_err(|e| CacheError::io(dir, e))?;
        let path = item.path();
        let file_type = item.file_type().map_err(|e| CacheError::io(&path, e))?;
        let name = item.file_name();
        let name = name.to_string_lossy();
        let rel = if prefix.is_empty() {
            name.into_owned()
        } else {
            format!("{prefix}/{name}")
        };
        if file_type.is_dir() {
            collect_files(&path, rel, out)?;
        } else if file_type.is_file() {
            out.push((rel, path));
        }
    }
    Ok(())
}

/// 清单路径必须非空、相对且不含 `.` / `..` 段。
fn is_safe_rel(rel: &str) -> bool {
    !rel.is_empty()
        && !Path::new(rel).is_absolute()
        && !rel
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..")
}
