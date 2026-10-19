//! `poly retry`：从指定阶段独立重跑（T-M2-11 / T-M6-09 验收口径）。
//!
//! - **`--from sign`**：同一命令再跑一次即再拉一次凭据：凭据只以引用出现
//!   （`JobSpec.secrets` 或 `--credential` 覆盖），[`poly_sign::AndroidSigner::plan`]
//!   先 fail-fast（缺工具 / 输入非法 / 引用不可用都在碰密钥之前报错），`sign` 每次
//!   重新解析后端载荷。事件流与日志只出现档案名 / 后端 id / 路径，绝不出现凭据值
//!   （脱敏由 poly-sign 保证）。未签名 APK 优先取 `--input`，否则在上次构建的打包暂存
//!   目录（`<root>/.polypack/packages/<target>`）里找 `<名称>-<版本>-unsigned.apk`
//!   （T-M2-06 的产物命名约定），找不到即 `poly::E_RETRY_0001`。
//! - **`--from verify`**：不经 DAG，直接复用 [`crate::verify`] 的声明校验器通路
//!   （同一套目标解析 / 腿分发 / SKIP 语义），供 QNX / Zephyr QEMU 冒烟失败后独立重跑。
//! - **`--from publish`**：复用 `poly publish` 的完整通路（同一 `polypack.toml` 根下的回执
//!   与提审去重状态）——中断后重跑同键命令只从断点续走，不产生第二次提交。
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
    /// 重跑起点。
    #[arg(long = "from", value_enum, default_value = "sign")]
    pub from: FromStage,
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
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
    /// 发布阶段：复用 `poly publish` 通路（同一回执 + 提审去重状态续走，不重复提交）。
    Publish,
}

/// 执行 `poly retry`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    match args.from {
        FromStage::Sign => sign(args, json),
        FromStage::Verify => verify(args, json),
        FromStage::Publish => publish(args, json),
    }
}

/// `poly retry --from publish`：复用 `poly publish` 的完整通路（同一回执 + 提审幂等状态）。
///
/// 中断后重跑同键命令：已受理（journal `Accepted`）→ 只续轮询，不产生第二次提交。
fn publish(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    crate::publish::run(&publish_args(args), json)
}

/// `retry` 参数 → `poly publish` 参数（clap 已保证 `--from publish` 时必填项在场）。
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
        metadata_locale: poly_publish::metadata::DEFAULT_LOCALE.to_string(),
        check_metadata: false,
        api_base: args.api_base.clone(),
        dry_run: false,
    }
}

/// `poly retry --from verify`：复用 `poly verify` 的声明校验器通路，独立重跑。
fn verify(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let verify = crate::verify::Args {
        config: args.config.clone(),
        reproducible: false,
        targets: args.target.iter().cloned().collect(),
        allow_skipped: false,
        sbom: false,
    };
    crate::verify::run(&verify, json)
}

/// `poly retry --from sign`。
fn sign(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let project = project::load(&args.config)?;
    let target = args.target.as_deref().unwrap_or("android");
    let (job, spec) = resolve(&project, target)?;
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
    let staged = project::app_packages_root(project).join(&spec.id);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `--from publish` 的参数映射：缺省 target 与 `poly publish` 同口径，凭据 / 材料直通。
    #[test]
    fn publish_rerun_maps_retry_args_to_publish_command() {
        let args = Args {
            from: FromStage::Publish,
            config: PathBuf::from("polypack.toml"),
            target: None,
            input: None,
            credential: Some("env:POLY_CRED".to_string()),
            out: None,
            appid: Some("wx1".to_string()),
            version: Some("1.0.0".to_string()),
            channel: Some("wechat-open-platform".to_string()),
            description: "重跑".to_string(),
            package: Some(PathBuf::from("dist")),
            materials: None,
            api_base: None,
        };
        let mapped = publish_args(&args);
        assert_eq!(mapped.target, "quickapp", "缺省 target 同 poly publish");
        assert_eq!(mapped.appid.as_deref(), Some("wx1"));
        assert_eq!(mapped.version.as_deref(), Some("1.0.0"));
        assert_eq!(mapped.channel.as_deref(), Some("wechat-open-platform"));
        assert_eq!(mapped.credential.as_deref(), Some("env:POLY_CRED"));
        assert!(!mapped.dry_run, "重跑走真腿");
    }
}
