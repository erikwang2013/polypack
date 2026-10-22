//! 对象存储 trait 与两个生产后端：本地文件系统（默认）与跨机共享目录。
//!
//! 本地分片布局：`<root>/objects/<hex 前2位>/<hex>`；对象文件 = 32 字节
//! `blake3(载荷)` 前缀 ‖ 载荷（校验方式见 crate 文档）。
//!
//! 共享目录布局：`<root>/<hex 前2位>/<hex>`；对象文件 = 版本头行 ‖ 摘要行 ‖ 载荷
//! （见 [`DirSharedStore`]）。两个布局各自自描述，后端只经 [`ObjectStore`] trait
//! 被上层使用；换厂商对象存储（S3 / OSS 等）只需再实现该 trait。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// 共享对象文件头行（格式版本；改动格式时递增）。
pub(crate) const SHARED_HEADER: &str = "POLYPACK-FARM-OBJECT v0";

/// 编码共享对象信封（头行 ‖ 摘要行 ‖ 载荷）：共享目录与 S3 / OSS 后端共用同一布局。
pub(crate) fn encode_shared(bytes: &[u8]) -> Vec<u8> {
    let mut object = format!("{SHARED_HEADER}\n{}\n", Digest::of_bytes(bytes)).into_bytes();
    object.extend_from_slice(bytes);
    object
}

/// 共享目录 CAS 后端（生产形态）：多构建地挂载同一共享盘即可跨 worker 复用。
///
/// 对象文件 = 头行 ‖ blake3(载荷) 十六进制行 ‖ 载荷；写入经同目录临时文件原子改名，
/// 读取校验摘要（头行 / 摘要不符视为未命中并删除，绝不返回可疑内容）。临时名含进程号
/// 与序号，同名键的多进程 / 多线程并发写不会互相截断。
///
/// 真实云对象存储腿见 [`crate::s3::S3Store`]（S3 兼容 / OSS `x-oss-s3-compat` 模式）；
/// 共享目录与 S3 后端共用同一对象信封与分片布局，`aws s3 sync` 迁移可直接复用。
pub struct DirSharedStore {
    root: PathBuf,
}

impl DirSharedStore {
    /// 以共享根目录构造（目录按需创建）。
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 确保共享根目录可用（存在或可创建）。失败 = 共享盘未挂载 / 路径被占用 / 权限不足，
    /// 调用方据此降级为本地缓存而不是让构建失败。
    pub fn ensure_ready(&self) -> Result<(), CacheError> {
        fs::create_dir_all(&self.root).map_err(|e| CacheError::io(&self.root, e))
    }

    /// 对象路径：`<root>/<hex 前 2 位>/<hex>`。
    fn object_path(&self, key: &Digest) -> PathBuf {
        let hex = key.as_str();
        self.root.join(&hex[..2]).join(hex)
    }
}

impl ObjectStore for DirSharedStore {
    fn get(&self, key: &Digest) -> Result<Option<Vec<u8>>, CacheError> {
        let path = self.object_path(key);
        let object = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(CacheError::io(&path, err)),
        };
        let Some(payload) = parse_shared(&object) else {
            // 损坏条目不再参与后续命中；删除失败（只读挂载等）不影响本次未命中
            let _ = self.remove(key);
            return Ok(None);
        };
        Ok(Some(payload.to_vec()))
    }

    fn put(&self, key: &Digest, bytes: &[u8]) -> Result<(), CacheError> {
        let path = self.object_path(key);
        let dir = path.parent().expect("对象路径必有父目录");
        fs::create_dir_all(dir).map_err(|e| CacheError::io(dir, e))?;
        let tmp = dir.join(tmp_name(key.as_str()));
        fs::write(&tmp, encode_shared(bytes)).map_err(|e| CacheError::io(&tmp, e))?;
        fs::rename(&tmp, &path).map_err(|e| CacheError::io(&path, e))
    }

    fn remove(&self, key: &Digest) -> Result<u64, CacheError> {
        let path = self.object_path(key);
        let bytes = match fs::metadata(&path) {
            Ok(meta) => meta.len(),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(err) => return Err(CacheError::io(&path, err)),
        };
        fs::remove_file(&path).map_err(|e| CacheError::io(&path, e))?;
        if let Some(shard) = path.parent() {
            // 空分片目录顺手删除；失败（并发写入等）不影响结果
            let _ = fs::remove_dir(shard);
        }
        Ok(bytes)
    }

    fn entries(&self) -> Result<Vec<Entry>, CacheError> {
        let shards = match fs::read_dir(&self.root) {
            Ok(iter) => iter,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(CacheError::io(&self.root, err)),
        };
        let mut out = Vec::new();
        for shard in shards {
            let shard = shard.map_err(|e| CacheError::io(&self.root, e))?;
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
                // 非 64 位十六进制文件名（临时文件等）不是本实现写的对象
                let Some(name) = name.to_str() else { continue };
                let Ok(key) = Digest::parse(name) else {
                    continue;
                };
                let meta = item
                    .metadata()
                    .map_err(|e| CacheError::io(item.path(), e))?;
                // 无 mtime 的条目无法参与 LRU，跳过
                let Ok(last_used) = meta.modified() else {
                    continue;
                };
                if meta.is_file() {
                    out.push(Entry {
                        key,
                        bytes: meta.len(),
                        last_used,
                    });
                }
            }
        }
        Ok(out)
    }
}

/// 解析共享对象文件：头行 / 摘要行不符或载荷摘要不符 → `None`（视为未命中）。
pub(crate) fn parse_shared(object: &[u8]) -> Option<&[u8]> {
    let first = object.iter().position(|&byte| byte == b'\n')?;
    if object[..first] != *SHARED_HEADER.as_bytes() {
        return None;
    }
    let rest = &object[first + 1..];
    let second = rest.iter().position(|&byte| byte == b'\n')?;
    let digest = core::str::from_utf8(&rest[..second]).ok()?;
    let payload = &rest[second + 1..];
    (Digest::of_bytes(payload) == Digest::parse(digest).ok()?).then_some(payload)
}

/// 主后端 + 降级后备的组合后端：主后端任何 IO 错误都告警一次（`warn`）并落到后备后端。
///
/// 缓存是优化面：共享对象存储缺席 / 掉线时构建应继续（后备 = 工程本地 CAS），
/// 而不是把缓存错误升级为构建失败。命中统计仍由 [`crate::Cache`] 如实累计。
pub struct FallbackStore<P, F> {
    primary: P,
    fallback: F,
    warn: Box<dyn Fn(&str) + Send + Sync>,
    reported: AtomicBool,
}

impl<P: ObjectStore, F: ObjectStore> FallbackStore<P, F> {
    /// 以主后端、后备后端与告警回调构造；同一实例的降级只告警一次。
    pub fn new(primary: P, fallback: F, warn: impl Fn(&str) + Send + Sync + 'static) -> Self {
        Self {
            primary,
            fallback,
            warn: Box::new(warn),
            reported: AtomicBool::new(false),
        }
    }

    /// 首次降级时报告（后续错误不再重复刷屏）。
    fn report(&self, err: &CacheError) {
        if !self.reported.swap(true, Ordering::Relaxed) {
            (self.warn)(&format!("对象存储后端不可用，已降级到后备缓存: {err}"));
        }
    }
}

impl<P: ObjectStore, F: ObjectStore> ObjectStore for FallbackStore<P, F> {
    fn get(&self, key: &Digest) -> Result<Option<Vec<u8>>, CacheError> {
        match self.primary.get(key) {
            Ok(Some(bytes)) => Ok(Some(bytes)),
            Ok(None) => self.fallback.get(key),
            Err(err) => {
                self.report(&err);
                self.fallback.get(key)
            }
        }
    }

    fn put(&self, key: &Digest, bytes: &[u8]) -> Result<(), CacheError> {
        // 主后端失败不阻断：继续写后备，保证本机下次仍可命中
        if let Err(err) = self.primary.put(key, bytes) {
            self.report(&err);
        }
        self.fallback.put(key, bytes)
    }

    fn remove(&self, key: &Digest) -> Result<u64, CacheError> {
        let primary = match self.primary.remove(key) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.report(&err);
                0
            }
        };
        Ok(primary + self.fallback.remove(key)?)
    }

    fn entries(&self) -> Result<Vec<Entry>, CacheError> {
        match self.primary.entries() {
            Ok(entries) => Ok(entries),
            Err(err) => {
                self.report(&err);
                self.fallback.entries()
            }
        }
    }
}
