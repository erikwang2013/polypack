//! `sign` 阶段（§5.1 第二汇聚点）：把 `package` 的产物按目标交给签名链（poly-sign）。
//!
//! 语义（内联进 `poly build`，凭据门控；`poly retry --from sign` 仍是不经 DAG 的显式重跑）：
//! - **未声明签名档案**（`TargetSpec.signing = None`，如 web）→ 静默跳过（无签名可言）；
//! - **宿主缺签名工具**（`E_SIG_0001`）与**凭据不可用**（后端无材料 / 非机密配置项缺失，
//!   `E_SIG_0002`）→ 告警跳过（skip-if-missing 语义：本机没有密钥材料，与缺 aapt2 同类）；
//! - **凭据引用非法**（`E_SIG_0002`）与**外部命令失败**（`E_SIG_0003` / IO `E_SIG_0004`）
//!   → 硬失败——没签成却报绿是假绿。
//!
//! 目标覆盖（M2/M3）：**android**（apksigner 重签打包暂存目录里的未签名 APK）、**ios**
//! （`codesign` 硬化运行时 + 安全时间戳签 `export/Payload/<产品>.app`）与 **harmonyos**
//! （hap-sign-tool `sign-app` + `verify-app` 签未签名 HAP，T-M3-04）。三腿都是 poly-sign 的
//! 「计划 → 执行」两段式：计划先 fail-fast（缺工具 / 输入非法 / 引用不可用都在碰密钥之前报错），
//! 凭据只以引用出现（值零入库；apksigner 口令经环境变量注入、hap-sign-tool 只能经 argv 且展示
//! 全量脱敏；临时材料用后即清）。
//!
//! iOS 的签名**单元是 `.app`**（未签名 IPA 只是它的 zip 快照）：签在 `.app` 上，公证链
//! （`ditto` 重新压 zip → notarytool → stapler）见 [`poly_sign::ios`] 的计划出口，M2 不在 DAG 里
//! 串（要 Apple 账号 + `--wait` 阻塞分钟级；且 identity / 团队 ID 之外的认证材料需先
//! `notarytool store-credentials` 注入钥匙串）。`identity` / `team_id` 是非机密配置项，
//! poly-core 无此配置面，故这里读环境变量（[`IOS_IDENTITY_ENV`] / [`IOS_TEAM_ID_ENV`]）。
//!
//! shortcut: `POLYPACK_IOS_*` 环境变量面，能用即够（M2 无 Apple 账号可验证真跑）；
//! 真机跑通后若需要在 `polypack.toml` 里声明，再升配置面（M3）。

use std::path::PathBuf;

use poly_core::diagnostics::{Code, Diagnostic};
use poly_core::events::{BuildEvent, EventSink};
use poly_core::job::JobSpec;
use poly_core::plan::{Stage, StageContext, StageError, StageOutcome};
use poly_core::{CredentialRef, codes};
use poly_sign::{AndroidSigner, HarmonySignRequest, HarmonySigner, SignError, SignRequest, ios};
use poly_targets::{TargetSpec, current_host};

use crate::project;

/// iOS 签名标识的来源（非机密；`codesign -s` 的值，如 `Apple Distribution: Pax (TEAM123)`）。
pub const IOS_IDENTITY_ENV: &str = "POLYPACK_IOS_IDENTITY";

/// iOS 团队 ID 的来源（非机密；notarytool `--team-id` 的值）。
pub const IOS_TEAM_ID_ENV: &str = "POLYPACK_IOS_TEAM_ID";

/// `sign` 阶段。
pub struct SignStage;

impl Stage for SignStage {
    fn name(&self) -> &'static str {
        "sign"
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
        let Some(host) = current_host() else {
            return Ok(StageOutcome::Skipped {
                reason: "无法识别宿主（交叉签名走远程农场 macOS worker，T-M2-13）".to_string(),
            });
        };
        let native: Vec<TargetSpec> = ctx
            .job
            .targets
            .iter()
            .filter(|spec| spec.hosts.contains(&host))
            .cloned()
            .collect();
        if native.is_empty() {
            return Ok(StageOutcome::Skipped {
                reason: format!("本次构建无 {host:?} 宿主目标（交叉签名归 M2 之后）"),
            });
        }

        let mut signed = 0;
        for spec in &native {
            if sign_target(project, spec, ctx)? {
                signed += 1;
            }
        }
        if signed == 0 {
            return Ok(StageOutcome::Skipped {
                reason: format!("{} 个宿主目标均未签名（见 sign 告警）", native.len()),
            });
        }
        Ok(StageOutcome::Done)
    }
}

/// 签名单个目标；返回是否真的签了（跳过 / 未接入为 `false`）。
fn sign_target(
    project: &poly_core::Project,
    spec: &TargetSpec,
    ctx: &mut StageContext<'_>,
) -> Result<bool, StageError> {
    // 未声明签名档案：web 这一类本就没有签名环节，静默跳过（告警只留给「该签却没签成」）
    let Some(profile) = spec.signing.clone() else {
        return Ok(false);
    };
    match spec.package.format.as_str() {
        "aab" | "apk" => sign_android(project, spec, &profile, ctx),
        "ipa" => sign_ios(project, spec, &profile, ctx),
        "hap" => sign_harmony(project, spec, &profile, ctx),
        other => {
            // 该签却没签成必须发声：`Skipped` 不是事件，逐目标告警是唯一可见通道
            warning(
                ctx.events,
                &format!(
                    "目标 {} 未签名：{}（format = {other}）",
                    spec.id,
                    signing_owner(other).unwrap_or("签名链未接入")
                ),
            );
            Ok(false)
        }
    }
}

/// 未接入签名链的目标格式 → 归属与替代路径；`None` = 注册表里出现了没人认领的格式
/// （如新增目标忘了接线，由单测保证 M2 的注册表上这里是死路）。
fn signing_owner(format: &str) -> Option<&'static str> {
    match format {
        "dmg" | "msi" => Some("桌面签名链（codesign / signtool）归 M2 之后"),
        "deb" | "appimage" => Some("Linux 仓库 GPG 签名归 M3"),
        "static-site" => Some("静态站点无内建签名（SRI 清单已由打包器产出）"),
        // Tizen 作者证书签名（凭据引用 `tizen-author`）归三星签名工具链；webOS 明文声明
        // 无签名档案（发布签名在平台侧，见 TargetSpec.notes），不落到这里
        "wgt" => Some(
            "Tizen 作者证书签名（三星签名工具）归平台侧（本机无 tizen CLI；凭据引用 tizen-author \
             已在 TargetSpec.signing 声明）",
        ),
        // T-M5-07：signify 包签名在 OpenBSD 打包腿内（pkg_sign，钥经
        // POLYPACK_OPENBSD_SIGNIFY_KEY）；真跑需 OpenBSD VM（T-M5-09）。
        "ports" => Some(
            "OpenBSD signify 包签名归打包腿（pkg_sign；POLYPACK_OPENBSD_SIGNIFY_KEY），\
             真跑需 OpenBSD VM（T-M5-09）",
        ),
        _ => None,
    }
}

/// Android 腿：打包暂存目录里的未签名 APK → apksigner 重签 + 就地校验（T-M2-09）。
fn sign_android(
    project: &poly_core::Project,
    spec: &TargetSpec,
    profile: &str,
    ctx: &mut StageContext<'_>,
) -> Result<bool, StageError> {
    let Some(input) = unsigned_apk(project, spec) else {
        warning(
            ctx.events,
            &format!(
                "目标 {} 未签名：{} 下没有未签名 APK（package 阶段未产出或已跳过）",
                spec.id,
                project::packages_root(&project.root)
                    .join(&spec.id)
                    .display()
            ),
        );
        return Ok(false);
    };
    let credential = match credential_ref(ctx.job, spec, None) {
        Ok(credential) => credential,
        // 档案缺失 / 引用不可用：JobSpec 由 to_job 从注册表派生，走到这里说明配置面不自洽
        Err(err) => {
            warning(
                ctx.events,
                &format!("目标 {} 未签名：{}", spec.id, err.message),
            );
            return Ok(false);
        }
    };
    let request = SignRequest {
        input,
        out: None,
        credential,
    };

    let signer = AndroidSigner::with_default_backends();
    // fail-fast：工具 / 输入 / 凭据引用不可用都在碰密钥之前报错（E_SIG_0001/0002/0004 透传）
    let plan = match signer.plan(&request) {
        Ok(plan) => plan,
        Err(err) => return skip_or_fail(ctx.events, spec, err),
    };
    // 每次调用重新解析凭据（`poly retry --from sign` 的独立重跑语义）；临时密钥材料用后即清
    let artifact = match signer.sign(&request) {
        Ok(artifact) => artifact,
        Err(err) => return skip_or_fail(ctx.events, spec, err),
    };
    ctx.events.emit(&BuildEvent::Signed {
        profile: profile.to_string(),
    });
    // 已签产物落点只在这里出现（Signed 事件只有档案名），不静默
    warning(
        ctx.events,
        &format!(
            "目标 {} 已签名：{} → {}（{}，{} 字节）",
            spec.id,
            plan.tool.display(),
            artifact.path.display(),
            artifact.verify_evidence,
            artifact.bytes
        ),
    );
    Ok(true)
}

/// HarmonyOS 腿：打包暂存目录里的未签名 HAP → hap-sign-tool `sign-app` + `verify-app`（T-M3-04）。
fn sign_harmony(
    project: &poly_core::Project,
    spec: &TargetSpec,
    profile: &str,
    ctx: &mut StageContext<'_>,
) -> Result<bool, StageError> {
    let Some(input) = unsigned_hap(project, spec) else {
        warning(
            ctx.events,
            &format!(
                "目标 {} 未签名：{} 下没有未签名 HAP（package 阶段未产出或已跳过）",
                spec.id,
                project::packages_root(&project.root)
                    .join(&spec.id)
                    .display()
            ),
        );
        return Ok(false);
    };
    let credential = match credential_ref(ctx.job, spec, None) {
        Ok(credential) => credential,
        Err(err) => {
            warning(
                ctx.events,
                &format!("目标 {} 未签名：{}", spec.id, err.message),
            );
            return Ok(false);
        }
    };
    let request = HarmonySignRequest {
        input,
        out: None,
        credential,
    };

    let signer = HarmonySigner::with_default_backends();
    // fail-fast：工具 / 输入 / 凭据引用与载荷字段都在碰密钥之前报错（E_SIG_0001/0002/0004 透传）
    let plan = match signer.plan(&request) {
        Ok(plan) => plan,
        Err(err) => return skip_or_fail(ctx.events, spec, err),
    };
    // 每次调用重新解析凭据（`poly retry --from sign` 的独立重跑语义）；临时密钥材料用后即清
    let artifact = match signer.sign(&request) {
        Ok(artifact) => artifact,
        Err(err) => return skip_or_fail(ctx.events, spec, err),
    };
    ctx.events.emit(&BuildEvent::Signed {
        profile: profile.to_string(),
    });
    warning(
        ctx.events,
        &format!(
            "目标 {} 已签名：hap-sign-tool（{}）→ {}（{}，{} 字节）",
            spec.id,
            plan.tool.display(),
            artifact.path.display(),
            artifact.verify_evidence,
            artifact.bytes
        ),
    );
    Ok(true)
}

/// iOS 腿：`export/Payload/<产品>.app` → codesign 硬化运行时 + 安全时间戳（T-M2-10）。
fn sign_ios(
    project: &poly_core::Project,
    spec: &TargetSpec,
    profile: &str,
    ctx: &mut StageContext<'_>,
) -> Result<bool, StageError> {
    let app = ios_app(project, spec);
    if !app.is_dir() {
        warning(
            ctx.events,
            &format!(
                "目标 {} 未签名：{} 不存在（package 阶段未产出或已跳过）",
                spec.id,
                app.display()
            ),
        );
        return Ok(false);
    }
    let credentials = match ios_credentials(ctx.job) {
        Ok(credentials) => credentials,
        Err(err) => return skip_or_fail(ctx.events, spec, err),
    };
    match ios::sign(&app, &credentials) {
        Ok(path) => {
            ctx.events.emit(&BuildEvent::Signed {
                profile: profile.to_string(),
            });
            warning(
                ctx.events,
                &format!(
                    "目标 {} 已签名：codesign（硬化运行时 + 安全时间戳）→ {}；\
                     公证链（notarytool / stapler）不在 DAG 内（M2 边界）",
                    spec.id,
                    path.display()
                ),
            );
            Ok(true)
        }
        Err(err) => skip_or_fail(ctx.events, spec, err),
    }
}

/// iOS 签名凭据：引用链给出公证认证与描述文件，非机密的 `identity` / `team_id` 取环境变量。
fn ios_credentials(job: &JobSpec) -> Result<ios::IosCredentials, SignError> {
    let mut credentials = ios::IosCredentials::from_credential_ref(&job.secrets)?;
    credentials.identity = env_value(IOS_IDENTITY_ENV);
    credentials.team_id = env_value(IOS_TEAM_ID_ENV);
    Ok(credentials)
}

/// 环境变量的非空取值（未设 / 空白 → `None`，由签名计划的 fail-fast 点名缺哪一项）。
fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 环境性缺失（工具 / 凭据不可用）→ 告警跳过；其余（引用非法、命令失败）→ 硬失败。
fn skip_or_fail(
    events: &mut dyn EventSink,
    spec: &TargetSpec,
    err: SignError,
) -> Result<bool, StageError> {
    if matches!(
        err,
        SignError::ToolMissing { .. } | SignError::CredentialUnavailable { .. }
    ) {
        warning(events, &format!("目标 {} 未签名：{err}", spec.id));
        return Ok(false);
    }
    Err(sign_error(err).into())
}

/// 告警（跳过原因如实进事件流）。
fn warning(events: &mut dyn EventSink, message: &str) {
    events.emit(&BuildEvent::Warning {
        stage: "sign".to_string(),
        message: message.to_string(),
    });
}

// ---------------------------------------------------------------- 与 package / retry 共用的派生

/// iOS 打包器落 `.app` 的位置（打包器只认产品名，不派生 kebab：`export/Payload/<产品名>.app`）。
pub(crate) fn ios_app(project: &poly_core::Project, spec: &TargetSpec) -> PathBuf {
    project::packages_root(&project.root)
        .join(&spec.id)
        .join("export")
        .join("Payload")
        .join(format!(
            "{}.app",
            crate::package::product_info(&project.config.package).name
        ))
}

/// 未签名 APK：打包暂存目录里按命名约定找（`<kebab 名称>-<版本>-unsigned.apk`，否则退化为
/// `*-unsigned.apk` 扫描取名字序第一份）。
pub(crate) fn unsigned_apk(project: &poly_core::Project, spec: &TargetSpec) -> Option<PathBuf> {
    find_unsigned(project, spec, "apk")
}

/// 未签名 HAP：与 APK 同一约定（`<kebab 名称>-<版本>-unsigned.hap`，见 poly-bundle HAP 打包器）。
pub(crate) fn unsigned_hap(project: &poly_core::Project, spec: &TargetSpec) -> Option<PathBuf> {
    find_unsigned(project, spec, "hap")
}

/// 打包暂存目录里的未签名产物：命名约定优先，缺失时退化为 `*-unsigned.<ext>` 扫描。
fn find_unsigned(project: &poly_core::Project, spec: &TargetSpec, ext: &str) -> Option<PathBuf> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let preferred = staged.join(unsigned_name_for(project, ext));
    if preferred.is_file() {
        return Some(preferred);
    }
    // 命名约定之外的落点（如手工放置）退化为扫描；`-signed.<ext>` 不匹配 `-unsigned.<ext>`
    let suffix = format!("-unsigned.{ext}");
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&staged)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(&suffix))
        })
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}

/// `<kebab 名称>-<版本>-unsigned.<ext>`（各打包器的产物命名约定同源）。
pub(crate) fn unsigned_name_for(project: &poly_core::Project, ext: &str) -> String {
    // 借打包器的派生出口拿 kebab 形态，避免在 CLI 里复制一份命名规则
    let product = crate::package::product_info(&project.config.package);
    format!(
        "{}-{}-unsigned.{ext}",
        product.kebab_name(),
        product.version
    )
}

/// 凭据引用：取 `JobSpec.secrets` 里本目标档案（= `TargetSpec.signing`）的那条；
/// `override_ref` 只覆盖来源（`env:` 的变量名 / `keyring:` 的条目名），不改凭据值。
pub(crate) fn credential_ref(
    job: &JobSpec,
    spec: &TargetSpec,
    override_ref: Option<&str>,
) -> Result<CredentialRef, Diagnostic> {
    let name = spec.signing.clone().ok_or_else(|| {
        Diagnostic::error(
            codes::E_PLAN_0002,
            format!("目标 {} 未声明签名凭据（TargetSpec.signing 为空）", spec.id),
        )
        .with_hint("在 polypack.toml 的 [targets.<id>] 声明签名档案后再重跑")
    })?;
    let mut reference = job
        .secrets
        .iter()
        .find(|secret| secret.name == name)
        .cloned()
        .ok_or_else(|| {
            Diagnostic::error(
                codes::E_PLAN_0002,
                format!("JobSpec 缺少 {name} 的凭据引用"),
            )
            .with_hint("用 poly doctor 核对目标注册表的 signing 档案名")
        })?;
    if let Some(value) = override_ref {
        let (source, alias) = crate::retry::parse_credential(value)?;
        reference.source = source;
        if let Some(alias) = alias {
            reference.name = alias;
        }
    }
    Ok(reference)
}

/// `SignError` → 诊断：错误码原样透传（`poly::E_SIG_*`，绝不重新编号），提示取 `hint()`。
pub(crate) fn sign_error(err: SignError) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(Code::new(err.code()), err.to_string());
    if let Some(hint) = err.hint() {
        diagnostic = diagnostic.with_hint(hint);
    }
    diagnostic
}

#[cfg(test)]
mod tests {
    use super::*;
    use poly_core::events::HumanSink;
    use poly_core::job::{Budget, Profile};

    /// 最小工程配置（与 `package.rs` 单测同源文本，保持单一口径）。
    const FIXTURE_TOML: &str = "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n";

    /// 建最小工程骨架；返回临时根目录。
    fn fixture(tag: &str) -> (PathBuf, poly_core::Project) {
        let root = std::env::temp_dir().join(format!("polypack-sign-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("core")).expect("工程目录可建");
        std::fs::write(
            root.join("core/Cargo.toml"),
            "[package]\nname = \"fixture-app\"\n",
        )
        .expect("写核心 crate 清单");
        let config =
            poly_core::config::PolypackConfig::parse(FIXTURE_TOML).expect("fixture 配置应合法");
        let project = poly_core::Project {
            root: root.clone(),
            config,
        };
        (root, project)
    }

    /// 内建 android 目标（format 可覆写成未接入的形态做边界）。
    fn spec(format: &str) -> TargetSpec {
        let mut spec = poly_targets::Registry::builtin()
            .get("android")
            .expect("内建 android 目标")
            .clone();
        spec.package.format = format.to_string();
        spec
    }

    fn job() -> JobSpec {
        JobSpec {
            id: "t".to_string(),
            profile: Profile::Local,
            targets: Vec::new(),
            excluded: Vec::new(),
            secrets: Vec::new(),
            budget: Budget::default(),
        }
    }

    /// `.app` 落点与 iOS 打包器同源：`export/Payload/<产品名>.app`（不派生 kebab）。
    #[test]
    fn ios_app_path_matches_the_packager_layout() {
        let (root, project) = fixture("ios");
        let mut ios = spec("ipa");
        ios.id = "ios".to_string();
        assert_eq!(
            ios_app(&project, &ios),
            root.join(".polypack/packages/ios/export/Payload/fixture-app.app"),
            "产品名是 `[package].name` 原样（打包器的 `payload.join(format!(\"{{name}}.app\"))`）"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 未签名 APK 的两级查找：命名约定优先，缺失时退化为 `*-unsigned.apk` 扫描。
    #[test]
    fn unsigned_apk_prefers_the_naming_convention() {
        let (root, project) = fixture("apk");
        let spec = spec("aab");
        let staged = root.join(".polypack/packages/android");
        std::fs::create_dir_all(&staged).expect("暂存目录可建");
        assert_eq!(unsigned_apk(&project, &spec), None, "空目录没有可签的输入");
        std::fs::write(staged.join("other-0.0.1-unsigned.apk"), b"x").expect("写扫描候选");
        assert_eq!(
            unsigned_apk(&project, &spec),
            Some(staged.join("other-0.0.1-unsigned.apk")),
            "命名约定之外退化为扫描"
        );
        assert_eq!(
            unsigned_name_for(&project, "apk"),
            "fixture-app-0.0.1-unsigned.apk"
        );
        let named = staged.join(unsigned_name_for(&project, "apk"));
        std::fs::write(&named, b"y").expect("写约定名");
        assert_eq!(unsigned_apk(&project, &spec), Some(named), "约定名优先");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 未声明签名档案静默跳过；未接入的 format 如实点名（该签却没签成 ≠ 不存在签名环节）。
    #[test]
    fn undeclared_signing_is_silent_and_unknown_format_is_named() {
        let (root, project) = fixture("skip");
        let job = job();
        let mut sink = HumanSink {
            out: Vec::<u8>::new(),
        };
        let mut plain = spec("aab");
        plain.signing = None;
        {
            let mut ctx = StageContext {
                job: &job,
                events: &mut sink,
                project: Some(&project),
            };
            assert!(
                !sign_target(&project, &plain, &mut ctx).expect("未声明签名不该失败"),
                "无签名档案 → 不签"
            );
        }
        assert!(
            sink.out.is_empty(),
            "无签名档案应静默，实得 {}",
            String::from_utf8_lossy(&sink.out)
        );

        let unknown = spec("msi");
        {
            let mut ctx = StageContext {
                job: &job,
                events: &mut sink,
                project: Some(&project),
            };
            assert!(
                !sign_target(&project, &unknown, &mut ctx).expect("未接入不该失败"),
                "未接入 format → 不签"
            );
        }
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("未签名"), "{text}");
        assert!(text.contains("msi"), "{text}");
        assert!(text.contains("M2 之后"), "归属要具体：{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 内建注册表里每个声明了签名档案的目标，其格式都要么已接入（android / ios / harmonyos）
    /// 要么有归属：新增目标 / 新格式漏接线即红（否则该签却没签成会静默）。
    #[test]
    fn every_signed_target_format_is_served_or_owned() {
        let registry = poly_targets::Registry::builtin();
        let mut declared = 0;
        for spec in registry.all() {
            if spec.signing.is_none() {
                continue;
            }
            declared += 1;
            let format = spec.package.format.as_str();
            assert!(
                matches!(format, "aab" | "apk" | "ipa" | "hap") || signing_owner(format).is_some(),
                "目标 {} 声明了签名档案但格式 {format} 既未接入也没有归属登记",
                spec.id
            );
        }
        assert!(declared >= 5, "内建注册表的签名目标数不该缩水：{declared}");
        assert!(signing_owner("no-such-format").is_none());
    }

    /// 未签名 HAP 的两级查找：命名约定优先；`-signed.hap` 不误入扫描。
    #[test]
    fn unsigned_hap_prefers_the_naming_convention() {
        let (root, project) = fixture("hap");
        let mut spec = spec("aab");
        spec.id = "harmonyos".to_string();
        let staged = root.join(".polypack/packages/harmonyos");
        std::fs::create_dir_all(&staged).expect("暂存目录可建");
        assert_eq!(unsigned_hap(&project, &spec), None, "空目录没有可签的输入");
        std::fs::write(staged.join("fixture-app-0.0.1-unsigned-signed.hap"), b"s").expect("写已签");
        assert_eq!(unsigned_hap(&project, &spec), None, "已签产物不是可签输入");
        std::fs::write(staged.join("other-0.0.1-unsigned.hap"), b"x").expect("写扫描候选");
        assert_eq!(
            unsigned_hap(&project, &spec),
            Some(staged.join("other-0.0.1-unsigned.hap")),
            "命名约定之外退化为扫描"
        );
        let named = staged.join("fixture-app-0.0.1-unsigned.hap");
        std::fs::write(&named, b"y").expect("写约定名");
        assert_eq!(unsigned_hap(&project, &spec), Some(named), "约定名优先");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 环境性缺失（缺工具 / 凭据不可用）跳过并告警；引用非法 / 命令失败硬失败。
    #[test]
    fn environmental_errors_skip_and_the_rest_fail() {
        let (root, _project) = fixture("errs");
        let spec = spec("aab");
        let mut sink = HumanSink {
            out: Vec::<u8>::new(),
        };
        assert!(
            !skip_or_fail(
                &mut sink,
                &spec,
                SignError::ToolMissing {
                    tool: "apksigner".to_string(),
                    hint: "装 Android SDK build-tools".to_string(),
                },
            )
            .expect("缺工具应跳过"),
            "缺工具 → 跳过"
        );
        assert!(
            !skip_or_fail(
                &mut sink,
                &spec,
                SignError::CredentialUnavailable {
                    name: "android-keystore".to_string(),
                    backend: "keyring",
                    reason: "条目缺失".to_string(),
                },
            )
            .expect("凭据不可用应跳过"),
            "凭据不可用 → 跳过"
        );
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("apksigner"), "{text}");
        assert!(text.contains("android-keystore"), "{text}");

        let err = skip_or_fail(
            &mut sink,
            &spec,
            SignError::CredentialInvalid {
                name: "android-keystore".to_string(),
                backend: "env",
                reason: "载荷不是 base64(JSON)".to_string(),
            },
        )
        .expect_err("引用非法应硬失败");
        assert_eq!(err.diagnostic.code.to_string(), "poly::E_SIG_0002");
        let err = skip_or_fail(
            &mut sink,
            &spec,
            SignError::ToolFailed {
                command: "apksigner sign".to_string(),
                status: "1".to_string(),
                stderr: "error".to_string(),
            },
        )
        .expect_err("命令失败应硬失败");
        assert_eq!(err.diagnostic.code.to_string(), "poly::E_SIG_0003");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// iOS 凭据装配：引用链给公证认证，`identity` / `team_id` 由环境变量补（非机密配置项）。
    #[test]
    fn ios_credentials_take_identity_and_team_from_env() {
        let mut job = job();
        job.secrets = vec![CredentialRef {
            name: ios::REF_SIGNING_PROFILE.to_string(),
            source: poly_core::CredentialSource::Keyring,
        }];
        let credentials =
            ios::IosCredentials::from_credential_ref(&job.secrets).expect("引用可装配");
        assert!(
            credentials.identity.is_none(),
            "引用链不给 identity（非机密配置项由调用方填）"
        );
        assert!(
            matches!(
                credentials.notary,
                Some(ios::NotaryAuth::KeychainProfile(_))
            ),
            "Keyring 来源 → notarytool 钥匙串档案"
        );
        // 环境读取的边界：未设 / 空白都算「未配置」（由签名计划的 fail-fast 点名缺哪一项）
        assert_eq!(
            env_value("POLYPACK_IOS_IDENTITY_UNSET_TEST_ONLY"),
            None,
            "未设的变量不是配置项"
        );
        assert_eq!(env_value("PATH").as_deref().filter(|v| v.is_empty()), None);
        // 装配出的 identity 只可能来自环境（此处未设则 None，设了才是值）
        assert_eq!(
            ios_credentials(&job)
                .expect("装配不因缺 identity 失败")
                .identity,
            env_value(IOS_IDENTITY_ENV)
        );
    }

    /// 凭据引用：档案名取 `TargetSpec.signing`，`--credential` 形态只覆盖来源；缺档案硬错误。
    #[test]
    fn credential_ref_takes_the_target_profile_and_honours_the_override() {
        let spec = spec("aab");
        let mut job = job();
        let err = credential_ref(&job, &spec, None).expect_err("JobSpec 里没有档案 → 硬错误");
        assert_eq!(err.code.to_string(), "poly::E_PLAN_0002");

        job.secrets = vec![CredentialRef {
            name: "android-keystore".to_string(),
            source: poly_core::CredentialSource::Keyring,
        }];
        assert_eq!(
            credential_ref(&job, &spec, None)
                .expect("按注册表档案取")
                .source,
            poly_core::CredentialSource::Keyring
        );
        let overridden =
            credential_ref(&job, &spec, Some("env:POLYPACK_ANDROID_KS")).expect("覆盖来源");
        assert_eq!(
            overridden.source,
            poly_core::CredentialSource::Env {
                variable: "POLYPACK_ANDROID_KS".to_string()
            },
            "env: 的变量名是来源，不是档案名"
        );
        assert_eq!(overridden.name, "android-keystore", "档案名不被 env: 覆盖");
        assert!(
            credential_ref(&job, &spec, Some("bogus:x")).is_err(),
            "非法引用形态硬错误"
        );
        let mut unsigned = spec.clone();
        unsigned.signing = None;
        assert!(credential_ref(&job, &unsigned, None).is_err());
    }
}
