//! PolyPack 流水线核心：`JobSpec`、`Stage`/`Plan`、诊断与事件。
//!
//! 依赖方向（ROADMAP §4.1）：`poly-core` → `poly-targets` / `poly-toolchain` / `poly-cache`。

pub mod config;
pub mod diagnostics;
pub mod events;
pub mod job;
pub mod plan;

pub use config::{CONFIG_SCHEMA, ConfigError, ConfigJobError, PolypackConfig};
pub use diagnostics::{Code, Diagnostic, Severity, codes};
pub use events::{BuildEvent, EventSink, HumanSink, JsonSink};
pub use job::{Budget, CredentialRef, CredentialSource, JobSpec, Profile, TimeBudgets};
pub use plan::{Plan, Stage, StageContext, StageError, StageOutcome};
