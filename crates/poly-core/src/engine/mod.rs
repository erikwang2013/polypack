//! 引擎层（T-M6-01/02）：`no_std + alloc` 可编译的核心面 + 稳定 C ABI。
//!
//! 收口三件事：JobSpec 校验（[`JobSpec::validate`]）、TargetSpec 解析
//! （[`parse_target_spec`] / [`parse_job_spec`]）与缓存键计算（[`cache_key`]）。
//! 宿主依赖（poly-toolchain / toml / zip / 桥与流水线）只在 `std` feature 后。
//!
//! 缓存键 std 与 no_std 一致：两侧编译的是 `poly-cache` 的同一个 [`cache_key`]
//! 函数，键构造里没有任何随 target 或 feature 变化的分支。

pub mod ffi;
#[cfg(feature = "std")]
pub mod header;

use alloc::format;
use alloc::string::String;

use serde::de::DeserializeOwned;

pub use poly_cache::{CacheInputs, Digest, cache_key};
pub use poly_targets::{SpecError, TargetSpec};

use crate::job::JobSpec;

/// 引擎层 JSON 解析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// JSON 语法或字段类型不匹配。
    Json(String),
}

impl core::fmt::Display for EngineError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Json(text) => write!(f, "JSON 解析失败: {text}"),
        }
    }
}

fn parse_json<T: DeserializeOwned>(json: &str) -> Result<T, EngineError> {
    serde_json::from_str(json).map_err(|err| EngineError::Json(format!("{err}")))
}

/// 解析单个 [`TargetSpec`]（JSON；引擎层唯一解析入口）。
pub fn parse_target_spec(json: &str) -> Result<TargetSpec, EngineError> {
    parse_json(json)
}

/// 解析 [`JobSpec`]（JSON；引擎层唯一解析入口）。
pub fn parse_job_spec(json: &str) -> Result<JobSpec, EngineError> {
    parse_json(json)
}
