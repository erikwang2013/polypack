//! PolyPack 目标注册表：`TargetSpec` 声明、校验与内建目标。
//!
//! 本 crate 处于 workspace 依赖图的叶层，不依赖任何其他 poly-* crate。
//!
//! T-M6-01：默认 `std`；`--no-default-features` 时以 `no_std + alloc` 编译，
//! 供 `poly-core` 引擎层在裸机目标上做 `TargetSpec` 解析与校验。

#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

pub mod builtin;
pub mod registry;
pub mod spec;

pub use registry::{
    ExcludedTarget, Registry, ResolvedTargets, TargetRegistry, TargetRequest, current_host,
};
pub use spec::{Bridge, Host, PackageSpec, Runtime, SpecError, TargetSpec};
