//! M1 真实流水线：带内容寻址缓存的 `compile` / `bundle(ui)` 双支线（T-M1-01/02/04/09）。
//!
//! [`Plan::standard`] 是 M0 的骨架阶段序（无项目上下文即可跑通）；本模块的 [`plan`] 是
//! CLI 使用的真实阶段序：在骨架拓扑上把 `compile` / `bundle` 换成带缓存的实现——
//! 命中即还原产物并跳过重编译 / 重打包，未命中则执行后写入 CAS（§5.2 分级缓存）。
//!
//! 缓存键输入（§5.2）：源文件树摘要 ‖ 依赖锁文件 ‖ TargetSpec 版本 ‖ 工具链版本 ‖ profile，
//! 由 [`poly_cache::cache_key`] 组合；任一项变化即键变化（级联失效）。

use std::path::Path;
use std::sync::Arc;

use poly_cache::{Cache, CacheError, CacheInputs, Digest};
use poly_targets::{Bridge, Runtime, TargetSpec};

use crate::compile::{self, CompileResult};
use crate::diagnostics::{Diagnostic, codes};
use crate::events::BuildEvent;
use crate::job::JobSpec;
use crate::plan::{
    NotYetStage, Plan, PlanStage, ProbeStage, Project, Stage, StageContext, StageError, StageNode,
    StageOutcome,
};
use crate::web::{self, UiEngine};

/// 缓存键输入中排除的目录名（构建产物与依赖目录不参与源树摘要）。
const SOURCE_EXCLUDE: &[&str] = &["target", "node_modules", "dist", ".polypack", ".git"];

/// 组装 CLI 使用的真实流水线：`compile` / `bundle(ui)` 双支线带缓存。
///
/// `profile` 为 `ci` / `local`，是缓存键的「构建参数」输入（§5.2）。
pub fn plan(cache: Arc<Cache>, profile: &str) -> Result<Plan, StageError> {
    plan_with_stages(cache, profile, InjectedStages::default())
}

/// 由上层注入的真实阶段实现：`None` = 保留骨架 `NotYetStage` 占位。
///
/// 打包器矩阵在 poly-bundle、签名链与校验器在 poly-sign，而依赖方向约束
/// （`tests/dependency_direction.rs`）禁止 poly-core 依赖它们，故三者都由 poly-cli 注入。
/// 工厂形态（`fn` 指针）与农场 worker 的 [`crate::farm::StageFactory`] 同型：`Copy`，
/// 便于跨 worker 参数与主端两侧传递。
#[derive(Clone, Copy, Default)]
pub struct InjectedStages {
    /// `package` 阶段（打包器矩阵 T-M1-05..08）。
    pub package: Option<crate::farm::StageFactory>,
    /// `sign` 阶段（签名链 T-M2-10）。
    pub sign: Option<crate::farm::StageFactory>,
    /// `verify` 阶段（校验器注册表 T-M2-10）。
    pub verify: Option<crate::farm::StageFactory>,
}

/// 同 [`plan`]，三个可注入阶段（`package` / `sign` / `verify`）分别以真实实现替换。
pub fn plan_with_stages(
    cache: Arc<Cache>,
    profile: &str,
    stages: InjectedStages,
) -> Result<Plan, StageError> {
    Plan::from_nodes(vec![
        StageNode::new("plan", PlanStage),
        StageNode::new("probe", ProbeStage).after(&["plan"]),
        StageNode::new("compile", CompileStage::new(cache.clone(), profile)).after(&["probe"]),
        StageNode::new("bundle", BundleStage::new(cache, profile)).after(&["probe"]),
        injected(
            "package",
            stages.package,
            "M1（打包器矩阵 T-M1-05..08）",
            &["compile", "bundle"],
        ),
        injected("sign", stages.sign, "M2（T-M2-10 签名链）", &["package"]),
        injected("verify", stages.verify, "M2（校验器注册表）", &["sign"]),
        StageNode::new(
            "publish",
            NotYetStage {
                name: "publish",
                since: "M2",
            },
        )
        .after(&["verify"]),
    ])
}

/// 注入实现 / 骨架占位的二选一节点构造（`since` 说明占位归属的里程碑与任务）。
fn injected(
    name: &'static str,
    factory: Option<crate::farm::StageFactory>,
    since: &'static str,
    deps: &[&'static str],
) -> StageNode {
    match factory {
        Some(factory) => StageNode {
            name,
            deps: deps.to_vec(),
            stage: factory(),
        },
        None => StageNode::new(name, NotYetStage { name, since }).after(deps),
    }
}

/// `compile` 支线：核心 crate 的宿主 release 编译，带库级缓存。
pub struct CompileStage {
    cache: Arc<Cache>,
    profile: String,
}

impl CompileStage {
    /// 以给定缓存与 profile 构造。
    pub fn new(cache: Arc<Cache>, profile: &str) -> Self {
        Self {
            cache,
            profile: profile.to_string(),
        }
    }
}

impl Stage for CompileStage {
    fn name(&self) -> &'static str {
        "compile"
    }

    fn plan(&self, _job: &JobSpec) -> Result<(), StageError> {
        Ok(())
    }

    fn run(&self, ctx: &mut StageContext<'_>) -> Result<StageOutcome, StageError> {
        let Some(project) = ctx.project else {
            return Ok(StageOutcome::Skipped {
                reason: "缺少项目上下文".to_string(),
            });
        };
        let Some(target) = native_desktop_target(ctx.job) else {
            return Ok(StageOutcome::Skipped {
                reason: "无宿主原生桌面目标（wasm 核心编译归 M2；runtime=none 无核心）".to_string(),
            });
        };

        let crate_dir = project.root.join(&project.config.core.crate_path);
        // 先补锁文件再算键：否则首构建自己生成的锁文件会让下一次的键变掉（假失效）
        if let Err(err) = compile::ensure_lockfile(&crate_dir) {
            ctx.events.emit(&BuildEvent::Warning {
                stage: "compile".to_string(),
                message: format!(
                    "无法预生成锁文件，缓存键退化为「无锁文件」形态: {}",
                    err.diagnostic.message
                ),
            });
        }
        let key = compile_key(project, target, &self.profile)?;
        let crate_name = compile::read_crate_name(&crate_dir)?;
        let triple = compile::host_triple()?;
        let artifact = compile::artifact_path(&crate_dir, &crate_name, &triple);

        let hit = self.cache.lookup(&key).map_err(cache_error)?;
        ctx.events.emit(&BuildEvent::Cached {
            key: key.as_str().to_string(),
            hit: hit.is_some(),
        });
        if let Some(bytes) = hit {
            write_artifact(&artifact, &bytes)?;
            ctx.events
                .emit(&BuildEvent::Compiled { crate_name, triple });
            return Ok(StageOutcome::Done);
        }

        let result = compile::compile_core(project, target)?;
        store_result(&self.cache, ctx, &key, &result)?;
        ctx.events.emit(&BuildEvent::Compiled {
            crate_name: result.crate_name,
            triple: result.triple,
        });
        Ok(StageOutcome::Done)
    }
}

/// `bundle(ui)` 支线：UI 构建 + PWA manifest，带包级缓存（目录级 `store_tree`）。
pub struct BundleStage {
    cache: Arc<Cache>,
    profile: String,
}

impl BundleStage {
    /// 以给定缓存与 profile 构造。
    pub fn new(cache: Arc<Cache>, profile: &str) -> Self {
        Self {
            cache,
            profile: profile.to_string(),
        }
    }
}

impl Stage for BundleStage {
    fn name(&self) -> &'static str {
        "bundle"
    }

    fn plan(&self, _job: &JobSpec) -> Result<(), StageError> {
        Ok(())
    }

    fn run(&self, ctx: &mut StageContext<'_>) -> Result<StageOutcome, StageError> {
        let Some(project) = ctx.project else {
            return Ok(StageOutcome::Skipped {
                reason: "缺少项目上下文".to_string(),
            });
        };
        // UI 支线产出的是 Web 产物：只有 `react-dom` 系适配器消费它（react-native 系归 M2
        // 的端上适配器）；按目标声明序取首个 DOM 适配器，避免受目标 BTreeMap 序影响。
        let Some(adapter) = ctx
            .job
            .targets
            .iter()
            .find_map(|target| target.ui.iter().find(|ui| *ui == "react-dom"))
        else {
            return Ok(StageOutcome::Skipped {
                reason: "本次构建的入选目标无 react-dom 适配器（react-native 系 UI 打包归 M2）"
                    .to_string(),
            });
        };
        let dist = project.root.join("dist");
        let ui = web::ui_dir(project)?;
        let engine = web::detect_engine(&ui);
        let key = bundle_key(project, &engine, &self.profile)?;

        // 命中还原是合并写入：先清空 dist，免得上一版产物（如旧 hash 资产）残留
        if dist.exists() {
            std::fs::remove_dir_all(&dist).map_err(|err| {
                Diagnostic::error(
                    codes::E_WEB_0001,
                    format!("清空 UI 产物目录失败（{}）: {err}", dist.display()),
                )
                .with_hint("确认 dist 未被其它进程占用后重试")
            })?;
        }
        let hit = self.cache.restore_tree(&key, &dist).map_err(cache_error)?;
        ctx.events.emit(&BuildEvent::Cached {
            key: key.as_str().to_string(),
            hit,
        });

        if !hit {
            let build = web::build_ui(project, &dist)?;
            if let Err(err) = self.cache.store_tree(&key, &dist) {
                ctx.events.emit(&BuildEvent::Warning {
                    stage: "bundle".to_string(),
                    message: format!("UI 缓存写入失败（不阻断构建）: {err}"),
                });
            }
            if build.engine == UiEngine::Static {
                // 降级语义要说全：只是拷贝。模板的 React / JSX 入口这样拷出来的产物跑不起来
                let caveat = if web::needs_bundler(&ui) {
                    "产物仅拷贝、未转译，JSX / TS 入口在浏览器中无法运行"
                } else {
                    "产物仅拷贝，未做打包 / 转译"
                };
                ctx.events.emit(&BuildEvent::Warning {
                    stage: "bundle".to_string(),
                    message: format!(
                        "未找到 vite，已用内建静态打包完成 UI 产物（{}）——{caveat}；在 UI 目录 npm install 后自动切换 vite",
                        build.dist.display()
                    ),
                });
            }
        }

        ctx.events.emit(&BuildEvent::UiBundled {
            adapter: adapter.clone(),
        });
        Ok(StageOutcome::Done)
    }
}

/// 首个「宿主原生桌面目标」：`runtime == Native && bridge == TauriIpc`（与 `CompileStage` 同判据）。
fn native_desktop_target(job: &JobSpec) -> Option<&TargetSpec> {
    job.targets
        .iter()
        .find(|target| target.runtime == Runtime::Native && target.bridge == Bridge::TauriIpc)
}

/// compile 支线的缓存键输入：核心 crate 源树 + 其锁文件 + 目标声明 + 工具链 + profile。
pub fn compile_key(
    project: &Project,
    target: &TargetSpec,
    profile: &str,
) -> Result<Digest, StageError> {
    let crate_dir = project.root.join(&project.config.core.crate_path);
    Ok(poly_cache::cache_key(&CacheInputs {
        sources: Digest::of_tree(&crate_dir, SOURCE_EXCLUDE).map_err(cache_error)?,
        lockfile: lockfile_digest(&crate_dir)?,
        target_spec: serde_json::to_string(target).expect("TargetSpec 可序列化"),
        toolchain: toolchain_version()?,
        profile: profile.to_string(),
    }))
}

/// bundle 支线的缓存键输入：UI 源树 + UI 锁文件 + 影响 manifest 的配置 + 引擎 + profile。
pub fn bundle_key(
    project: &Project,
    engine: &UiEngine,
    profile: &str,
) -> Result<Digest, StageError> {
    let ui = web::ui_dir(project)?;
    let spec = serde_json::json!({
        "ui": project.config.ui,
        "package": project.config.package,
    });
    Ok(poly_cache::cache_key(&CacheInputs {
        sources: Digest::of_tree(&ui, SOURCE_EXCLUDE).map_err(cache_error)?,
        lockfile: lockfile_digest(&ui)?,
        target_spec: spec.to_string(),
        toolchain: format!("{engine:?}"),
        profile: profile.to_string(),
    }))
}

/// 锁文件摘要：`Cargo.lock` / `package-lock.json` / `pnpm-lock.yaml` / `yarn.lock` 中存在的第一个；
/// 都没有时用空内容摘要（键保持稳定，且对「补上锁文件」敏感）。
fn lockfile_digest(dir: &Path) -> Result<Digest, StageError> {
    for name in [
        "Cargo.lock",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
    ] {
        let path = dir.join(name);
        if path.is_file() {
            let bytes = std::fs::read(&path).map_err(|err| {
                cache_error_from(format!("读取锁文件失败（{}）: {err}", path.display()))
            })?;
            return Ok(Digest::of_bytes(&bytes));
        }
    }
    Ok(Digest::of_bytes(b""))
}

/// 工具链版本（缓存键输入）：rustc / cargo 版本 + 宿主 triple。
fn toolchain_version() -> Result<String, StageError> {
    let triple = compile::host_triple()?;
    let version = poly_toolchain::default_probes()
        .iter()
        .find(|probe| probe.id() == "rust")
        .and_then(|probe| probe.locate().found)
        .and_then(|toolchain| toolchain.version)
        .unwrap_or_else(|| "unknown".to_string());
    Ok(format!("{version}|{triple}"))
}

/// 编译产物写入 CAS（失败只告警不阻断：缓存不可用不该让构建失败）。
fn store_result(
    cache: &Cache,
    ctx: &mut StageContext<'_>,
    key: &Digest,
    result: &CompileResult,
) -> Result<(), StageError> {
    let bytes = std::fs::read(&result.artifact).map_err(|err| {
        Diagnostic::error(
            codes::E_CACHE_0001,
            format!("读取编译产物失败（{}）: {err}", result.artifact.display()),
        )
        .with_hint("确认核心 crate 为 bin 形态且产物已落盘")
    })?;
    if let Err(err) = cache.store(key, &bytes) {
        ctx.events.emit(&BuildEvent::Warning {
            stage: "compile".to_string(),
            message: format!("编译缓存写入失败（不阻断构建）: {err}"),
        });
    }
    Ok(())
}

/// 还原的产物写回磁盘（父目录可能已被清理）。
fn write_artifact(artifact: &Path, bytes: &[u8]) -> Result<(), StageError> {
    if let Some(parent) = artifact.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            cache_error_from(format!("创建产物目录失败（{}）: {err}", parent.display()))
        })?;
    }
    std::fs::write(artifact, bytes).map_err(|err| {
        cache_error_from(format!("还原缓存产物失败（{}）: {err}", artifact.display()))
    })
}

/// 核心 crate 产物路径（报告 / 产物快照用）。
pub fn artifact_path(project: &Project) -> Result<std::path::PathBuf, StageError> {
    let crate_dir = project.root.join(&project.config.core.crate_path);
    let crate_name = compile::read_crate_name(&crate_dir)?;
    let triple = compile::host_triple()?;
    Ok(compile::artifact_path(&crate_dir, &crate_name, &triple))
}

/// 缓存错误 → 稳定诊断（`poly::E_CACHE_0001`）。
fn cache_error(err: CacheError) -> StageError {
    cache_error_from(format!("构建缓存操作失败: {err}"))
}

fn cache_error_from(message: String) -> StageError {
    Diagnostic::error(codes::E_CACHE_0001, message)
        .with_hint("检查 .polypack/cache 的权限与磁盘空间，或用 poly clean --cache 重建缓存")
        .into()
}
