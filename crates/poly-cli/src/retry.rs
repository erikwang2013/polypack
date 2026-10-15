//! `poly retry --from sign`：从签名阶段独立重跑（T-M2-11 验收口径）。
//!
//! 「独立重跑」= 同一命令再跑一次即再拉一次凭据：凭据只以引用出现（`JobSpec.secrets`
//! 或 `--credential` 覆盖），[`poly_sign::AndroidSigner::plan`] 先 fail-fast（缺工具 /
//! 输入非法 / 引用不可用都在碰密钥之前报错），`sign` 每次重新解析后端载荷。
//! 事件流与日志只出现档案名 / 后端 id / 路径，绝不出现凭据值（脱敏由 poly-sign 保证）。
//!
//! M2 只接入 Android 签名（apksigner）：`poly::E_SIG_*` 原样透传，绝不重新编号。
//! 未签名 APK 优先取 `--input`，否则在上次构建的打包暂存目录
//! （`<root>/.polypack/packages/<target>`）里找 `<名称>-<版本>-unsigned.apk`
//! （T-M2-06 的产物命名约定），找不到即 `poly::E_RETRY_0001` 并提示先跑
//! `poly build --target android`。
//!
//! 与 `poly build` 的 `sign` 阶段（[`crate::sign`]）共用派生与错误翻译：未签名 APK 查找、
//! 凭据引用、`SignError` → 诊断都在那边，本命令只保留「不经 DAG 的独立重跑」外壳。

use std::path::PathBuf;

use poly_core::diagnostics::Diagnostic;
use poly_core::events::BuildEvent;
use poly_core::{CredentialSource, JobSpec, Profile, Project, codes};
use poly_sign::{AndroidSigner, SignRequest};
use poly_targets::{Registry, TargetRequest, TargetSpec, current_host};

use crate::project;
use crate::sign::sign_error;

/// `poly retry` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 重跑起点（M2 只接入 `sign`）。
    #[arg(long = "from", value_enum, default_value = "sign")]
    pub from: FromStage,
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// 目标 id（M2 只接入 android）。
    #[arg(long = "target", value_name = "ID", default_value = "android")]
    pub target: String,
    /// 未签名 APK（缺省在打包暂存目录里按命名约定查找）。
    #[arg(long)]
    pub input: Option<PathBuf>,
    /// 凭据引用覆盖（`env:变量名` / `keyring[:条目名]` / `vault[:条目名]`）。
    #[arg(long)]
    pub credential: Option<String>,
    /// 签名产物落点（缺省 `<输入名>-signed.apk`）。
    #[arg(long)]
    pub out: Option<PathBuf>,
}

/// 重跑起点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum FromStage {
    /// 签名阶段：重新拉取凭据、重签未签名 APK 并校验。
    Sign,
}

/// 执行 `poly retry`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    match args.from {
        FromStage::Sign => sign(args, json),
    }
}

/// `poly retry --from sign`。
fn sign(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let project = project::load(&args.config)?;
    let (job, spec) = resolve(&project, &args.target)?;
    if spec.id != "android" {
        return Err(Diagnostic::error(
            codes::E_RETRY_0001,
            format!(
                "目标 {} 的签名重跑未接入（M2 只接入 android / apksigner）",
                spec.id
            ),
        )
        .with_hint("iOS 签名在 poly-sign::ios（T-M2-10），CLI 接线未落；本命令当前只对 --target android 有效"));
    }

    let input = match &args.input {
        Some(path) => path.clone(),
        None => crate::sign::unsigned_apk(&project, &spec)
            .ok_or_else(|| missing_apk(&project, &spec))?,
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
            .with_hint("用 poly doctor 核对目标矩阵")
    })?;
    Ok((job, spec))
}

/// 未签名 APK 缺位（`--input` 未给且暂存目录里没有）时的稳定诊断。
fn missing_apk(project: &Project, spec: &TargetSpec) -> Diagnostic {
    let staged = project::packages_root(&project.root).join(&spec.id);
    Diagnostic::error(
        codes::E_RETRY_0001,
        format!("{} 下没有未签名 APK（目标 {}）", staged.display(), spec.id),
    )
    .with_hint("先跑 poly build --target android 产出未签名 APK，或用 --input 指定未签名 APK 路径")
}

/// 解析 `--credential` 引用（`env:变量名` / `keyring[:条目名]` / `vault[:条目名]`）。
///
/// 载荷形态（base64(JSON)）与脱敏由 poly-sign 的后端链负责，这里只翻译成
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
