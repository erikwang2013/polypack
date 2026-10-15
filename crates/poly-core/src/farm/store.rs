//! 跨机共享对象存储的参考实现（`ObjectStore` 的另一后端）。

use std::path::PathBuf;

use poly_cache::{CacheError, Digest, Entry, ObjectStore};

/// 共享对象文件头行（格式版本；改动格式时递增）。
const SHARED_HEADER: &str = "POLYPACK-FARM-OBJECT v0";

/// 跨机共享对象存储（参考实现）：单文件 + 头行格式，与 `LocalStore` 的分片格式不同，
/// 证明「同一 `ObjectStore` trait 换后端即跨机复用」；生产部署替换为厂商对象存储（M3+）。
///
/// 对象文件 = `POLYPACK-FARM-OBJECT v0` 行 ‖ blake3(载荷) 十六进制行 ‖ 载荷。
pub struct DirSharedStore {
    root: PathBuf,
}

impl DirSharedStore {
    /// 以共享根目录构造（目录按需创建）。
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
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
        let object = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(io(&path, err)),
        };
        // 头行 / 摘要不符（跨机传输损坏）视为未命中，不返回可疑内容
        Ok(parse_shared(&object).map(<[u8]>::to_vec))
    }

    fn put(&self, key: &Digest, bytes: &[u8]) -> Result<(), CacheError> {
        let path = self.object_path(key);
        let dir = path.parent().expect("对象路径必有父目录");
        std::fs::create_dir_all(dir).map_err(|err| io(dir, err))?;
        let mut object = format!("{SHARED_HEADER}\n{}\n", Digest::of_bytes(bytes)).into_bytes();
        object.extend_from_slice(bytes);
        let tmp = dir.join(format!(".{}.{}.tmp", key.as_str(), std::process::id()));
        std::fs::write(&tmp, &object).map_err(|err| io(&tmp, err))?;
        std::fs::rename(&tmp, &path).map_err(|err| io(&path, err))
    }

    fn remove(&self, key: &Digest) -> Result<u64, CacheError> {
        let path = self.object_path(key);
        let bytes = match std::fs::metadata(&path) {
            Ok(meta) => meta.len(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(err) => return Err(io(&path, err)),
        };
        std::fs::remove_file(&path).map_err(|err| io(&path, err))?;
        Ok(bytes)
    }

    fn entries(&self) -> Result<Vec<Entry>, CacheError> {
        let shards = match std::fs::read_dir(&self.root) {
            Ok(iter) => iter,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(io(&self.root, err)),
        };
        let mut out = Vec::new();
        for shard in shards.flatten().filter(|entry| entry.path().is_dir()) {
            let Ok(items) = std::fs::read_dir(shard.path()) else {
                continue;
            };
            for item in items.flatten() {
                let Some(name) = item.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                let (Ok(key), Ok(meta)) = (Digest::parse(&name), item.metadata()) else {
                    continue;
                };
                if let Ok(last_used) = meta.modified()
                    && meta.is_file()
                {
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

/// IO 失败（`CacheError::Io` 的字段公开，`io()` 构造函数则是 crate 私有）。
fn io(path: impl AsRef<std::path::Path>, source: std::io::Error) -> CacheError {
    CacheError::Io {
        path: path.as_ref().to_path_buf(),
        source,
    }
}

/// 解析共享对象文件：头行不符 / 摘要不符 → `None`（视为未命中）。
fn parse_shared(object: &[u8]) -> Option<&[u8]> {
    let first = object.iter().position(|&byte| byte == b'\n')?;
    if object[..first] != *SHARED_HEADER.as_bytes() {
        return None;
    }
    let rest = &object[first + 1..];
    let second = rest.iter().position(|&byte| byte == b'\n')?;
    let digest = std::str::from_utf8(&rest[..second]).ok()?;
    let payload = &rest[second + 1..];
    (Digest::of_bytes(payload) == Digest::parse(digest).ok()?).then_some(payload)
}
