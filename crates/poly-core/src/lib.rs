//! PolyPack 流水线核心：`JobSpec`、`Stage`/`Plan`、诊断与事件。
//!
//! 依赖方向（ROADMAP §4.1）：`poly-core` → `poly-targets` / `poly-toolchain` / `poly-cache`。
//!
//! T-M6-01 分层：默认 `std` 是完整宿主面；`--no-default-features` 时本 crate 以
//! `no_std + alloc` 编译，只保留引擎层（[`engine`]）与 [`job`]——JobSpec 校验、
//! TargetSpec 解析与缓存键计算。宿主模块（桥、农场、流水线等）全部在 std feature 后。

#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

pub mod engine;
pub mod job;

#[cfg(feature = "std")]
pub mod bridge;
#[cfg(feature = "std")]
pub mod budget;
#[cfg(feature = "std")]
pub mod compile;
#[cfg(feature = "std")]
pub mod config;
#[cfg(feature = "std")]
pub mod diagnostics;
#[cfg(feature = "std")]
pub mod events;
#[cfg(feature = "std")]
pub mod farm;
#[cfg(feature = "std")]
pub mod pipeline;
#[cfg(feature = "std")]
pub mod plan;
#[cfg(feature = "std")]
pub mod scan;
#[cfg(feature = "std")]
pub mod web;

#[cfg(feature = "std")]
pub use budget::{SizeVerdict, size_verdict};
#[cfg(feature = "std")]
pub use compile::{CompileResult, CompileStage, compile_core, host_triple};
#[cfg(feature = "std")]
pub use config::{CONFIG_SCHEMA, ConfigError, ConfigJobError, DEFAULT_THEME_COLOR, PolypackConfig};
#[cfg(feature = "std")]
pub use diagnostics::{Code, Diagnostic, Severity, codes};
#[cfg(feature = "std")]
pub use events::{BuildEvent, EventSink, HumanSink, JsonSink};
pub use job::{
    Budget, CredentialRef, CredentialSource, JobError, JobSpec, PluginGrant, Profile, SizePolicy,
    TimeBudgets,
};
#[cfg(feature = "std")]
pub use plan::{Plan, Project, Stage, StageContext, StageError, StageNode, StageOutcome};
#[cfg(feature = "std")]
pub use web::{UiBuild, UiEngine, build_ui, detect_engine, ui_dir, write_manifest};
