//! T-M1-02：宿主 triple 探测与核心 Rust crate 的 release 编译阶段。
//!
//! M1 只编译「宿主原生桌面目标」（`runtime == Native && bridge == TauriIpc`，见 [`CompileStage`]）；
//! wasm 核心与跨语言桥（uniffi / napi / cffi）核心编译归 M2+。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use poly_targets::{Bridge, Runtime, TargetSpec};

use crate::diagnostics::{Diagnostic, codes};
use crate::events::BuildEvent;
use crate::plan::{Project, Stage, StageContext, StageError, StageOutcome};

/// 探测宿主 triple：执行 `rustc -vV` 并解析 `host: <triple>` 行。
///
/// rustc 缺失 / 退出码非零 / 输出缺 `host:` 行 → `poly::E_PROBE_0001` + 修复提示。
pub fn host_triple() -> Result<String, StageError> {
    let output = Command::new("rustc").arg("-vV").output().map_err(|err| {
        Diagnostic::error(
            codes::E_PROBE_0001,
            format!("无法执行 rustc 探测宿主 triple: {err}"),
        )
        .with_hint("安装 Rust 工具链（https://rustup.rs）并确保 rustc 在 PATH 中")
    })?;
    if !output.status.success() {
        return Err(Diagnostic::error(
            codes::E_PROBE_0001,
            format!("rustc -vV 退出码非零: {}", output.status),
        )
        .with_hint("修复 Rust 工具链安装（rustup check / rustup update）后重试")
        .into());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let triple = stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(|triple| triple.trim().to_string())
        .filter(|triple| !triple.is_empty())
        .ok_or_else(|| {
            Diagnostic::error(codes::E_PROBE_0001, "rustc -vV 输出缺少 host: 行")
                .with_hint("确认使用官方 rustup 发行版（rustc -vV 应输出 host 行）")
        })?;
    Ok(triple)
}

/// 一次核心 crate 编译的产物与度量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileResult {
    /// crate 名（取自 crate 目录的 `Cargo.toml` 的 `[package].name`）。
    pub crate_name: String,
    /// 编译所用目标三元组（宿主 triple）。
    pub triple: String,
    /// 产物路径：`<crate_dir>/target/<triple>/release/<crate_name>`。
    pub artifact: PathBuf,
    /// 编译耗时。
    pub duration: Duration,
}

/// 编译核心 crate 的 release 产物（真实执行 `cargo build --release --target <host_triple>`）。
///
/// - 工作目录 `<project.root>/<config.core.crate_path>`；crate 名读该目录的 `Cargo.toml`；
/// - 显式把 `CARGO_TARGET_DIR` 指到 crate 目录下的 `target/`：产物路径因此可预期，
///   且不受调用方环境变量影响（也避免与宿主的构建争 target 目录锁）；
/// - 失败 → `poly::E_COMPILE_0001`，修复提示携带 stderr 尾部关键行（定位线索）。
pub fn compile_core(project: &Project, target: &TargetSpec) -> Result<CompileResult, StageError> {
    let triple = host_triple()?;
    let crate_dir = project.root.join(&project.config.core.crate_path);
    let crate_name = read_crate_name(&crate_dir)?;

    let started = Instant::now();
    let output = Command::new("cargo")
        .args(["build", "--release", "--target", &triple])
        .current_dir(&crate_dir)
        .env("CARGO_TARGET_DIR", crate_dir.join("target"))
        .output()
        .map_err(|err| {
            Diagnostic::error(
                codes::E_COMPILE_0001,
                format!("无法执行 cargo build（目标 {}）: {err}", target.id),
            )
            .with_hint("安装 Rust 工具链（https://rustup.rs）并确保 cargo 在 PATH 中")
        })?;
    let duration = started.elapsed();

    if !output.status.success() {
        return Err(Diagnostic::error(
            codes::E_COMPILE_0001,
            format!(
                "核心 crate {crate_name} 编译失败（目标 {}，triple {triple}）",
                target.id
            ),
        )
        .with_hint(format!(
            "修复编译错误（缺 linker / SDK 时先装齐）后重试；stderr 尾部: {}",
            stderr_tail(&output.stderr)
        ))
        .into());
    }

    let artifact = artifact_path(&crate_dir, &crate_name, &triple);
    if !artifact.is_file() {
        return Err(Diagnostic::error(
            codes::E_COMPILE_0001,
            format!("编译成功但未找到产物 {}", artifact.display()),
        )
        .with_hint("确认核心 crate 为 bin 形态（[package] 且有 src/main.rs）")
        .into());
    }

    Ok(CompileResult {
        crate_name,
        triple,
        artifact,
        duration,
    })
}

/// compile 阶段：宿主原生桌面目标的核心 release 编译。
///
/// 只对「宿主原生桌面目标」（`runtime == Native && bridge == TauriIpc`）真编译；多个此类
/// 目标共享同一核心 crate 与同一宿主 triple，故**只编译一次并复用同一产物**（策略键 =
/// (crate, triple)；M2 引入 wasm / 跨语言桥后每个 triple 各编一次）。其余目标 `Skipped`
/// （wasm 核心编译归 M2；`runtime == None` 无核心）；无项目上下文整体 `Skipped`，绝不失败。
pub struct CompileStage;

impl Stage for CompileStage {
    fn name(&self) -> &'static str {
        "compile"
    }

    fn plan(&self, _job: &crate::job::JobSpec) -> Result<(), StageError> {
        Ok(())
    }

    fn run(&self, ctx: &mut StageContext<'_>) -> Result<StageOutcome, StageError> {
        let Some(project) = ctx.project else {
            return Ok(StageOutcome::Skipped {
                reason: "缺少项目上下文".to_string(),
            });
        };
        let Some(representative) =
            ctx.job.targets.iter().find(|target| {
                target.runtime == Runtime::Native && target.bridge == Bridge::TauriIpc
            })
        else {
            return Ok(StageOutcome::Skipped {
                reason: "无宿主原生桌面目标（wasm 核心编译归 M2；runtime=none 无核心）".to_string(),
            });
        };

        let result = compile_core(project, representative)?;
        ctx.events.emit(&BuildEvent::Compiled {
            crate_name: result.crate_name,
            triple: result.triple,
        });
        Ok(StageOutcome::Done)
    }
}

/// 确保 crate 目录存在 `Cargo.lock`（缓存键的输入之一）。
///
/// 缺失时 `cargo build` 会在构建过程中自行生成锁文件，使同一份源码在首构建前后算出两个
/// 不同的缓存键（首构建后必有一次假失效）；故在算键前先显式生成。失败由调用方决定如何
/// 降级（键退回「无锁文件」形态），本函数只报错。
pub(crate) fn ensure_lockfile(crate_dir: &Path) -> Result<(), StageError> {
    if crate_dir.join("Cargo.lock").is_file() {
        return Ok(());
    }
    let output = Command::new("cargo")
        .arg("generate-lockfile")
        .current_dir(crate_dir)
        .output()
        .map_err(|err| {
            Diagnostic::error(
                codes::E_COMPILE_0001,
                format!("无法执行 cargo generate-lockfile: {err}"),
            )
            .with_hint("安装 Rust 工具链（https://rustup.rs）并确保 cargo 在 PATH 中")
        })?;
    if !output.status.success() {
        return Err(Diagnostic::error(
            codes::E_COMPILE_0001,
            format!("生成锁文件失败（{}）", crate_dir.display()),
        )
        .with_hint(format!(
            "离线且依赖未缓存时可能失败；联网后重试。stderr 尾部: {}",
            stderr_tail(&output.stderr)
        ))
        .into());
    }
    Ok(())
}

/// 读 crate 目录的 `Cargo.toml`，取 `[package].name`。
pub(crate) fn read_crate_name(crate_dir: &Path) -> Result<String, StageError> {
    let manifest = crate_dir.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|err| {
        Diagnostic::error(
            codes::E_COMPILE_0001,
            format!("无法读取核心 crate 清单 {}: {err}", manifest.display()),
        )
        .with_hint("核对 polypack.toml 的 [core].crate 指向含 Cargo.toml 的 crate 目录")
    })?;
    let value: toml::Value = toml::from_str(&text).map_err(|err| {
        Diagnostic::error(
            codes::E_COMPILE_0001,
            format!("核心 crate 清单解析失败（{}）: {err}", manifest.display()),
        )
        .with_hint("修正 Cargo.toml 语法")
    })?;
    let name = value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            Diagnostic::error(
                codes::E_COMPILE_0001,
                format!("{} 缺少 [package].name", manifest.display()),
            )
            .with_hint("在 Cargo.toml 中补上 [package] 段与 name 字段")
        })?;
    Ok(name.to_string())
}

/// 产物路径：`<crate_dir>/target/<triple>/release/<bin>`（与 [`compile_core`] 同一推导）。
pub(crate) fn artifact_path(crate_dir: &Path, crate_name: &str, triple: &str) -> PathBuf {
    crate_dir
        .join("target")
        .join(triple)
        .join("release")
        .join(binary_name(crate_name, triple))
}

/// 产物文件名：Windows triple 的 bin 带 `.exe` 后缀，其余为 crate 名。
fn binary_name(crate_name: &str, triple: &str) -> String {
    if triple.contains("windows") {
        format!("{crate_name}.exe")
    } else {
        crate_name.to_string()
    }
}

/// stderr 尾部关键行（≤3 行非空行）——塞进修复提示给出定位线索。
fn stderr_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let mut lines: Vec<&str> = text
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(3)
        .collect();
    lines.reverse();
    lines.join(" | ")
}
