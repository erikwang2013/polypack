//! PolyPack 流水线核心：`JobSpec`、`Stage`/`Plan`、诊断与事件。
//!
//! 依赖方向（ROADMAP §4.1）：`poly-core` → `poly-targets` / `poly-toolchain` / `poly-cache`。

pub mod bridge;
pub mod budget;
pub mod compile;
pub mod config;
pub mod diagnostics;
pub mod events;
pub mod farm;
pub mod job;
pub mod pipeline;
pub mod plan;
pub mod scan;
pub mod web;

pub use budget::{SizeVerdict, size_verdict};
pub use compile::{CompileResult, CompileStage, compile_core, host_triple};
pub use config::{CONFIG_SCHEMA, ConfigError, ConfigJobError, DEFAULT_THEME_COLOR, PolypackConfig};
pub use diagnostics::{Code, Diagnostic, Severity, codes};
pub use events::{BuildEvent, EventSink, HumanSink, JsonSink};
pub use job::{Budget, CredentialRef, CredentialSource, JobSpec, Profile, SizePolicy, TimeBudgets};
pub use plan::{Plan, Project, Stage, StageContext, StageError, StageNode, StageOutcome};
pub use web::{UiBuild, UiEngine, build_ui, detect_engine, ui_dir, write_manifest};
