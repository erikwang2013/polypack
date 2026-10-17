//! 缓存层错误类型。

#[cfg(feature = "std")]
use core::fmt;

#[cfg(feature = "std")]
use std::path::{Path, PathBuf};

use alloc::string::String;
#[cfg(feature = "std")]
use alloc::string::ToString;

/// 缓存操作错误。
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    /// 文件系统 IO 失败（带路径上下文）。
    #[cfg(feature = "std")]
    #[error("缓存 IO 失败（{path}）: {source}")]
    Io {
        /// 出错路径。
        path: PathBuf,
        /// 底层 IO 错误。
        #[source]
        source: std::io::Error,
    },
    /// 摘要文本非法（应为 64 位十六进制 blake3 摘要）。
    #[error("摘要非法：{0}")]
    Digest(String),
    /// 对象内容与写时摘要不符，或清单不可解析。
    #[error("缓存条目损坏（键 {key}）: {detail}")]
    Corrupt {
        /// 对象键。
        key: String,
        /// 具体原因。
        detail: String,
    },
}

impl CacheError {
    /// IO 错误（统一补路径上下文）。
    #[cfg(feature = "std")]
    pub(crate) fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// 损坏错误（带键与原因）。
    #[cfg(feature = "std")]
    pub(crate) fn corrupt(key: impl fmt::Display, detail: impl fmt::Display) -> Self {
        Self::Corrupt {
            key: key.to_string(),
            detail: detail.to_string(),
        }
    }
}
