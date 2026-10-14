//! PolyPack 内容寻址构建缓存（M1：T-M1-09 实现）。
//!
//! 缓存键公式与分级缓存见 ROADMAP §5.2；清理与保留策略见 §5.5（T-M1-15）。
//!
//! 本 crate 位于依赖图叶层：不依赖任何其他 poly-* crate，缓存键输入一律用
//! 字符串 / 摘要表达（依赖方向约束见 `crates/poly-cli/tests/dependency_direction.rs`）。
//!
//! 对象文件格式：`blake3(载荷)` 32 字节前缀 ‖ 载荷。键既可能是内容摘要（blob），
//! 也可能是由构建输入推导的缓存键（如目录清单），二者都不是「键 == 载荷摘要」的
//! 关系，故校验用文件内自带的前缀而非键本身。

mod cache;
mod digest;
mod error;
mod retention;
mod store;

pub use cache::{Cache, CacheStats};
pub use digest::{CacheInputs, Digest, cache_key};
pub use error::CacheError;
pub use retention::{Eviction, RetentionPolicy, enforce_retention};
pub use store::{Entry, LocalStore, ObjectStore};
