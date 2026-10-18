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
    /// 对象存储配置非法（URI / 环境变量）；消息绝不包含密钥值。
    #[cfg(feature = "std")]
    #[error("对象存储配置非法: {0}")]
    Config(String),
    /// 远端对象存储返回非 2xx：只保留端点、HTTP 状态与 S3 错误码，
    /// 响应正文与请求头（含 Authorization）绝不进入错误文本。
    #[cfg(feature = "std")]
    #[error("对象存储请求失败（{endpoint}，HTTP {status}）: {code}")]
    Remote {
        /// 端点（`scheme://host`，不含 query）。
        endpoint: String,
        /// HTTP 状态码。
        status: u16,
        /// S3 错误码（响应 XML 的 `<Code>`；缺失时为 `unknown`）。
        code: String,
    },
    /// 远端连接 / 传输失败（只保留端点与简短原因）。
    #[cfg(feature = "std")]
    #[error("对象存储连接失败（{endpoint}）: {detail}")]
    RemoteTransport {
        /// 端点（`scheme://host`，不含 query）。
        endpoint: String,
        /// 简短原因（不含请求 URL、query 与请求头）。
        detail: String,
    },
    /// 远端响应不符合预期（XML 非法 / 正文或分页超过上限）。
    #[cfg(feature = "std")]
    #[error("对象存储响应非法: {0}")]
    Protocol(String),
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
