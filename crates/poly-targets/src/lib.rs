//! PolyPack 目标注册表：`TargetSpec` 声明、校验与内建目标。
//!
//! 本 crate 处于 workspace 依赖图的叶层，不依赖任何其他 poly-* crate。

pub mod builtin;
pub mod registry;
pub mod spec;

pub use registry::{
    ExcludedTarget, Registry, ResolvedTargets, TargetRegistry, TargetRequest, current_host,
};
pub use spec::{Bridge, Host, PackageSpec, Runtime, SpecError, TargetSpec};
