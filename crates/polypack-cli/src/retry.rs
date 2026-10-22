//! `polypack retry`：从指定阶段独立重跑（T-M2-11 / T-M6-09 验收口径）。
//!
//! - **`--from sign`**：同一命令再跑一次即再拉一次凭据：凭据只以引用出现
//!   （`JobSpec.secrets` 或 `--credential` 覆盖），[`polypack_sign::AndroidSigner::plan`]
//!   先 fail-fast（缺工具 / 输入非法 / 引用不可用都在碰密钥之前报错），`sign` 每次
//!   重新解析后端载荷。事件流与日志只出现档案名 / 后端 id / 路径，绝不出现凭据值
//!   （脱敏由 polypack-sign 保证）。未签名 APK 优先取 `--input`，否则在上次构建的打包暂存
//!   目录（`<root>/.polypack/packages/<target>`）里找 `<名称>-<版本>-unsigned.apk`
//!   （T-M2-06 的产物命名约定），找不到即 `polypack::E_RETRY_0001`。
//! - **`--from verify`**：不经 DAG，直接复用 [`crate::verify`] 的声明校验器通路
//!   （同一套目标解析 / 腿分发 / SKIP 语义），供 QNX / Zephyr QEMU 冒烟失败后独立重跑。
//! - **`--from publish`**：复用 `polypack publish` 的完整通路（同一 `polypack.toml` 根下的回执
//!   与提审去重状态）——中断后重跑同键命令只从断点续走，不产生第二次提交。
//!
//! 与 `polypack build` 的 `sign` 阶段（[`crate::sign`]）共用派生与错误翻译：未签名 APK 查找、
//! 凭据引用、`SignError` → 诊断都在那边，本命令只保留「不经 DAG 的独立重跑」外壳。

use std::path::PathBuf;

use polypack_core::diagnostics::Diagnostic;
use polypack_core::events::BuildEvent;
use polypack_core::{CredentialSource, JobSpec, Profile, Project, codes};
use polypack_sign::{AndroidSigner, SignRequest};
use polypack_targets::{Registry, TargetRequest, TargetSpec, current_host};

use crate::project;
use crate::sign::sign_error;

/// `polypack retry` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 重跑起点。
    #[arg(long = "from", value_enum, default_value = "sign")]
    pub from: FromStage,
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// 重跑工作区根 `[workspace]` 声明的全部成员应用（按声明序）。
    #[arg(long, conflicts_with = "app")]
    pub workspace: bool,
    /// 只重跑工作区中的指定成员应用（id = 成员目录名）。
    #[arg(long, value_name = "ID", conflicts_with = "workspace")]
    pub app: Option<String>,
    /// 目标 id（`--from sign` 缺省 android；`--from verify` 缺省全部本机可达目标）。
    #[arg(long = "target", value_name = "ID")]
    pub target: Option<String>,
    /// 未签名 APK（缺省在打包暂存目录里按命名约定查找）。
    #[arg(long)]
    pub input: Option<PathBuf>,
    /// 凭据引用覆盖（`env:变量名` / `keyring[:条目名]` / `vault[:条目名]`；`--from publish` 必填）。
    #[arg(long, required_if_eq("from", "publish"))]
    pub credential: Option<String>,
    /// 签名产物落点（缺省 `<输入名>-signed.apk`）。
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// 平台应用 id（`--from publish` 必填）。
    #[arg(long, required_if_eq("from", "publish"))]
    pub appid: Option<String>,
    /// 版本号（`--from publish` 必填）。
    #[arg(long, required_if_eq("from", "publish"))]
    pub version: Option<String>,
    /// 渠道 id（`--from publish` 缺省取目标声明的第一个）。
    #[arg(long)]
    pub channel: Option<String>,
    /// 版本描述（`--from publish`）。
    #[arg(long, default_value = "")]
    pub description: String,
    /// 代码包路径（`--from publish` 缺省按目标产物落点定位）。
    #[arg(long)]
    pub package: Option<PathBuf>,
    /// 提审材料 JSON 文件（`--from publish`）。
    #[arg(long)]
    pub materials: Option<PathBuf>,
    /// 平台 API 端点（`--from publish` 快应用联盟商店必填）。
    #[arg(long, value_name = "URL")]
    pub api_base: Option<String>,
}

/// 重跑起点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FromStage {
    /// 签名阶段：重新拉取凭据、重签未签名 APK 并校验。
    Sign,
    /// 校验阶段：按 TargetSpec.verify 重跑声明校验器（含 QNX / Zephyr QEMU 冒烟）。
    Verify,
    /// 发布阶段：复用 `polypack publish` 通路（同一回执 + 提审去重状态续走，不重复提交）。
    Publish,
}

/// 执行 `polypack retry`；返回进程退出码。
///
/// 选择器：`--workspace` 按声明序重跑全部成员；`--app <id>` 只重跑一个成员；
/// 两者都不给时走单应用路径，行为不变。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    for project in selected_projects(args)? {
        run_project(&project, args, json)?;
    }
    Ok(0)
}

/// 选择重跑作用域：单应用 = 装载根配置；`--app` / `--workspace` = 工作区成员
/// （保留成员的 root / 状态根上下文，绝不退化成成员根上的单应用）。
fn selected_projects(args: &Args) -> Result<Vec<Project>, Diagnostic> {
    if !args.workspace && args.app.is_none() {
        return Ok(vec![project::load(&args.config)?]);
    }
    let workspace = polypack_core::workspace::discover(&args.config)?;
    // `select` 只给成员引用：按值复制四字段，保留 root / 工作区根 / app id 上下文。
    Ok(workspace
        .select(args.app.as_deref())?
        .into_iter()
        .map(|app| Project {
            root: app.project.root.clone(),
            config: app.project.config.clone(),
            workspace_root: app.project.workspace_root.clone(),
            app_id: app.project.app_id.clone(),
        })
        .collect())
}

/// 单个项目（单应用或工作区成员）的重跑；成员上下文（root / 状态根）原样下传。
fn run_project(project: &Project, args: &Args, json: bool) -> Result<i32, Diagnostic> {
    match args.from {
        FromStage::Sign => sign(project, args, json),
        FromStage::Verify => crate::verify::run_project(project, &verify_args(args), json),
        FromStage::Publish => crate::publish::run_project(project, &publish_args(args), json),
    }
}

/// `retry` 参数 → `polypack publish` 参数（clap 已保证 `--from publish` 时必填项在场）。
fn publish_args(args: &Args) -> crate::publish::Args {
    crate::publish::Args {
        config: args.config.clone(),
        target: args
            .target
            .clone()
            .unwrap_or_else(|| "quickapp".to_string()),
        channel: args.channel.clone(),
        appid: Some(
            args.appid
                .clone()
                .expect("clap 保证 --from publish 时 --appid 在场"),
        ),
        version: Some(
            args.version
                .clone()
                .expect("clap 保证 --from publish 时 --version 在场"),
        ),
        description: args.description.clone(),
        package: args.package.clone(),
        credential: Some(
            args.credential
                .clone()
                .expect("clap 保证 --from publish 时 --credential 在场"),
        ),
        materials: args.materials.clone(),
        // metadata 管线（M9-E5）不在 retry 的输入面：重跑沿用回执 + 原提审材料语义。
        metadata_dir: None,
        metadata_locale: polypack_publish::metadata::DEFAULT_LOCALE.to_string(),
        check_metadata: false,
        api_base: args.api_base.clone(),
        dry_run: false,
    }
}

/// `retry` 参数 → `polypack verify` 参数（单应用选择器由外层 `run` 负责）。
fn verify_args(args: &Args) -> crate::verify::Args {
    crate::verify::Args {
        config: args.config.clone(),
        reproducible: false,
        targets: args.target.iter().cloned().collect(),
        allow_skipped: false,
        sbom: false,
        workspace: false,
        app: None,
    }
}

/// `polypack retry --from sign`。
fn sign(project: &Project, args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let target = args.target.as_deref().unwrap_or("android");
    let (job, spec) = resolve(project, target)?;
    if spec.id != "android" {
        return Err(Diagnostic::error(
            codes::E_RETRY_0001,
            format!(
                "目标 {} 的签名重跑未接入（M2 只接入 android / apksigner）",
                spec.id
            ),
        )
        .with_hint("iOS 签名在 polypack-sign::ios（T-M2-10），CLI 接线未落；本命令当前只对 --target android 有效"));
    }

    let input = match &args.input {
        Some(path) => path.clone(),
        None => {
            crate::sign::unsigned_apk(project, &spec).ok_or_else(|| missing_apk(project, &spec))?
        }
    };
    let credential = crate::sign::credential_ref(&job, &spec, args.credential.as_deref())?;
    let request = SignRequest {
        input,
        out: args.out.clone(),
        credential,
    };

    let signer = AndroidSigner::with_default_backends();
    // fail-fast：工具 / 输入 / 凭据引用不可用都在碰密钥之前报错（E_SIG_0001/0002/0004 透传）
    let plan = signer.plan(&request).map_err(sign_error)?;
    project::report(
        "retry",
        json,
        serde_json::json!({
            "event": "retry-plan",
            "from": "sign",
            "target": spec.id,
            "rerun": true,
            "tool": plan.tool.to_string_lossy(),
            "input": plan.input.to_string_lossy(),
            "output": plan.output.to_string_lossy(),
            "key_alias": plan.key_alias,
            "credential": request.credential.name.as_str(),
        }),
    );

    // 每次调用重新解析凭据（独立重跑不需要额外开关）；临时密钥材料用后即清
    let artifact = signer.sign(&request).map_err(sign_error)?;
    let mut events = project::sink(json);
    events.emit(&BuildEvent::Signed {
        profile: request.credential.name.clone(),
    });
    project::report(
        "retry",
        json,
        serde_json::json!({
            "event": "retry-signed",
            "from": "sign",
            "target": spec.id,
            "rerun": true,
            "credential": request.credential.name.as_str(),
            "artifact": artifact.path.to_string_lossy(),
            "bytes": artifact.bytes,
            "evidence": artifact.verify_evidence,
        }),
    );
    Ok(0)
}

/// 解析目标（`JobSpec` + `TargetSpec`）：未知目标 / 不可达宿主由 `to_job` 给出稳定诊断。
fn resolve(project: &Project, target: &str) -> Result<(JobSpec, TargetSpec), Diagnostic> {
    let registry = Registry::builtin();
    let hosts: Vec<_> = current_host().into_iter().collect();
    let job = project
        .config
        .to_job(
            &registry,
            &TargetRequest::Explicit(vec![target.to_string()]),
            Profile::Local,
            &hosts,
        )
        .map_err(|err| err.diagnostic)?;
    let spec = job.targets.first().cloned().ok_or_else(|| {
        Diagnostic::error(codes::E_TGT_0001, format!("目标 {target} 没有解析结果"))
            .with_hint("用 polypack doctor 核对目标矩阵")
    })?;
    Ok((job, spec))
}

/// 未签名 APK 缺位（`--input` 未给且暂存目录里没有）时的稳定诊断。
fn missing_apk(project: &Project, spec: &TargetSpec) -> Diagnostic {
    let staged = project::app_packages_root(project).join(&spec.id);
    Diagnostic::error(
        codes::E_RETRY_0001,
        format!("{} 下没有未签名 APK（目标 {}）", staged.display(), spec.id),
    )
    .with_hint(
        "先跑 polypack build --target android 产出未签名 APK，或用 --input 指定未签名 APK 路径",
    )
}

/// 解析 `--credential` 引用（`env:变量名` / `keyring[:条目名]` / `vault[:条目名]`）。
///
/// 载荷形态（base64(JSON)）与脱敏由 polypack-sign 的后端链负责，这里只翻译成
/// [`CredentialSource`]；`:` 后的条目名覆盖档案名（`env:` 的变量名不覆盖档案名）。
pub(crate) fn parse_credential(
    value: &str,
) -> Result<(CredentialSource, Option<String>), Diagnostic> {
    let (kind, arg) = match value.split_once(':') {
        Some((kind, arg)) => (kind, Some(arg.trim()).filter(|arg| !arg.is_empty())),
        None => (value, None),
    };
    let unknown = || {
        Diagnostic::error(
            codes::E_PLAN_0002,
            format!("无法解析 --credential 引用: {value}"),
        )
        .with_hint("可写：env:变量名 / keyring[:条目名] / vault[:条目名]")
    };
    match kind {
        "env" => {
            let Some(variable) = arg else {
                return Err(unknown());
            };
            Ok((
                CredentialSource::Env {
                    variable: variable.to_string(),
                },
                None,
            ))
        }
        "keyring" => Ok((CredentialSource::Keyring, arg.map(str::to_string))),
        "vault" => Ok((CredentialSource::Vault, arg.map(str::to_string))),
        _ => Err(unknown()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    /// 临时树：Drop 时整树删除。
    struct TempTree(PathBuf);

    impl TempTree {
        fn new(tag: &str) -> Self {
            static SEQ: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "polypack-retry-selector-{tag}-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("创建临时目录");
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 两成员工作区（声明序 a、b）。
    fn workspace_fixture(tag: &str) -> TempTree {
        let tree = TempTree::new(tag);
        let root = tree.path();
        std::fs::write(
            root.join("polypack.toml"),
            "schema = 1\n\n[workspace]\nmembers = [\"apps/a\", \"apps/b\"]\n",
        )
        .expect("写工作区根配置");
        for app in ["a", "b"] {
            let dir = root.join("apps").join(app);
            std::fs::create_dir_all(&dir).expect("建成员目录");
            std::fs::write(
                dir.join("polypack.toml"),
                format!(
                    "schema = 1\n\n[package]\nname = \"retry-{app}\"\nversion = \"0.1.0\"\n\n\
                     [ui]\nframework = \"vanilla\"\nentry = \"index.html\"\n\n\
                     [core]\ncrate = \"../../core\"\n"
                ),
            )
            .expect("写成员配置");
        }
        tree
    }

    fn args(config: PathBuf) -> Args {
        Args {
            from: FromStage::Sign,
            config,
            workspace: false,
            app: None,
            target: None,
            input: None,
            credential: None,
            out: None,
            appid: None,
            version: None,
            channel: None,
            description: String::new(),
            package: None,
            materials: None,
            api_base: None,
        }
    }

    /// `--from publish` 的参数映射：缺省 target 与 `polypack publish` 同口径，凭据 / 材料直通。
    #[test]
    fn publish_rerun_maps_retry_args_to_publish_command() {
        let args = Args {
            from: FromStage::Publish,
            ..args(PathBuf::from("polypack.toml"))
        };
        let args = Args {
            credential: Some("env:POLYPACK_CRED".to_string()),
            appid: Some("wx1".to_string()),
            version: Some("1.0.0".to_string()),
            channel: Some("wechat-open-platform".to_string()),
            description: "重跑".to_string(),
            package: Some(PathBuf::from("dist")),
            ..args
        };
        let mapped = publish_args(&args);
        assert_eq!(mapped.target, "quickapp", "缺省 target 同 polypack publish");
        assert_eq!(mapped.appid.as_deref(), Some("wx1"));
        assert_eq!(mapped.version.as_deref(), Some("1.0.0"));
        assert_eq!(mapped.channel.as_deref(), Some("wechat-open-platform"));
        assert_eq!(mapped.credential.as_deref(), Some("env:POLYPACK_CRED"));
        assert!(!mapped.dry_run, "重跑走真腿");
    }

    /// `--app` 只选一个成员：root = 成员目录，状态根 = 工作区根 + app id。
    #[test]
    fn app_selector_resolves_member_root_and_state_scope() {
        let tree = workspace_fixture("app");
        let root = tree.path().canonicalize().expect("工作区根可规范化");
        let mut args = args(root.join("polypack.toml"));
        args.app = Some("b".to_string());
        let projects = selected_projects(&args).expect("成员 b 应可选");
        assert_eq!(projects.len(), 1, "--app 只选一个成员");
        assert_eq!(projects[0].root, root.join("apps/b"), "root = 成员目录");
        assert_eq!(projects[0].app_id.as_deref(), Some("b"));
        assert_eq!(
            projects[0].workspace_root.as_deref(),
            Some(root.as_path()),
            "保留工作区根（状态目录不落到成员根）"
        );
        assert_eq!(
            project::app_packages_root(&projects[0]),
            root.join(".polypack/packages/b"),
            "产物暂存根按工作区根 + app id 隔离"
        );
    }

    /// `--workspace` 按声明序作用于全部成员，各自保留成员根。
    #[test]
    fn workspace_selector_keeps_declaration_order_and_member_roots() {
        let tree = workspace_fixture("all");
        let root = tree.path().canonicalize().expect("工作区根可规范化");
        let mut args = args(root.join("polypack.toml"));
        args.workspace = true;
        let projects = selected_projects(&args).expect("工作区应可发现");
        let ids: Vec<_> = projects
            .iter()
            .map(|project| project.app_id.as_deref())
            .collect();
        assert_eq!(ids, vec![Some("a"), Some("b")], "声明顺序");
        assert_eq!(projects[0].root, root.join("apps/a"));
        assert_eq!(projects[1].root, root.join("apps/b"));
        assert!(
            projects
                .iter()
                .all(|project| project.workspace_root.as_deref() == Some(root.as_path()))
        );
    }

    /// 未知成员是 `E_WS_0004`，绝不静默退化成工作区根上的单应用。
    #[test]
    fn unknown_app_is_a_stable_workspace_error_not_a_root_fallback() {
        let tree = workspace_fixture("unknown");
        let root = tree.path().canonicalize().expect("工作区根可规范化");
        let mut args = args(root.join("polypack.toml"));
        args.app = Some("missing".to_string());
        let err = selected_projects(&args).err().expect("未知成员必须报错");
        assert_eq!(err.code, codes::E_WS_0004);
        assert!(err.message.contains("missing"), "{}", err.message);
        assert!(
            err.hint.unwrap_or_default().contains("a, b"),
            "提示应列出可用成员"
        );
    }

    /// 两个选择器都不给：仍是单应用通路（根配置所在目录，无 app id）。
    #[test]
    fn single_app_selection_is_unchanged() {
        let tree = TempTree::new("single");
        let root = tree.path();
        std::fs::write(
            root.join("polypack.toml"),
            "[package]\nname = \"single\"\nversion = \"0.1.0\"\n\
             [ui]\nframework = \"vanilla\"\nentry = \"index.html\"\n\
             [core]\ncrate = \"core\"\n",
        )
        .expect("写配置");
        let projects = selected_projects(&args(root.join("polypack.toml"))).expect("单应用可装载");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].root, root.canonicalize().expect("根可规范化"));
        assert_eq!(projects[0].app_id, None);
        assert_eq!(projects[0].workspace_root, None);
    }
}
