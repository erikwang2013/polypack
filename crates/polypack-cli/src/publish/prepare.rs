//! `polypack publish` 的输入解析：渠道 / 代码包 / 凭据形态 / 端点 / 材料。
//!
//! dry-run 与真腿共用同一份口径；凭据值零落屏、零落盘。

use std::path::{Path, PathBuf};

use base64::Engine;
use polypack_core::CredentialRef;
use polypack_core::codes;
use polypack_core::diagnostics::Diagnostic;
use polypack_publish::Package;
use polypack_publish::metadata::{self, DEFAULT_LOCALE, Platform};
use polypack_publish::secret::{CredentialProvider, resolve_payload};
use polypack_targets::TargetSpec;

use super::args::Args;
use super::{DIST_DIR, MINIAPP_DIR, UNION_STORE_CHANNEL, publish_error};
use crate::project;

/// 一次提审的解析结果（dry-run 与真腿共用；值零落屏）。
#[derive(Debug)]
pub(super) struct Prepared {
    /// 渠道 id。
    pub(super) channel: String,
    /// 代码包路径（文件 = `.rpk`；目录 = 各平台代码包目录）。
    pub(super) package: PathBuf,
    /// 代码包字节数（文件载体才给）。
    pub(super) package_bytes: Option<u64>,
    /// 载具形态（`single-file` = `.rpk` 等单文件；`dir` = T-M3-06 产物目录）。
    pub(super) carrier: &'static str,
    /// 载具摘要（blake3 hex；与四步驱动 `Package::load` 同源同口径）。
    pub(super) digest: String,
    /// 回执键（appid-版本-摘要前 16；幂等重跑 = 同键续走 / 终态 no-op）。
    pub(super) receipt_key: String,
    /// 凭据引用（引用模型；值经后端链现取）。
    pub(super) reference: CredentialRef,
    /// 命中的凭据后端 id（`env` 等）。
    pub(super) provider: &'static str,
    /// 凭据载荷字段名（有序；**只报形态，不报值**）。
    pub(super) payload_fields: Vec<String>,
    /// 显式注入的端点（快应用联盟必填）。
    pub(super) api_base: Option<String>,
    /// 提审材料（文件解析 / metadata 打包后）。
    pub(super) materials: Option<serde_json::Value>,
    /// 材料实际使用的 locale（`--metadata-dir`；旧 `--materials` 为 None）。
    pub(super) materials_locale: Option<String>,
    /// 材料摘要（canonical JSON blake3；无材料 None）。
    pub(super) materials_digest: Option<String>,
    /// metadata 加载告警（如 locale 回退）。
    pub(super) metadata_warnings: Vec<String>,
}

/// 解析输入（渠道 / 代码包 / 凭据形态 / 端点 / 材料）；dry-run 与真腿共用同一份口径。
pub(super) fn prepare(
    args: &Args,
    project: &polypack_core::Project,
    spec: &TargetSpec,
    providers: &[Box<dyn CredentialProvider>],
) -> Result<Prepared, Diagnostic> {
    // 渠道：显式优先，否则取目标声明的第一个（快应用 = quickapp-union-store）。
    let channel = resolve_channel(args, spec)?;

    // 端点：快应用联盟商店无公开 CI 端点 → 必填、无缺省域名（不许猜一个域名去撞）。
    if channel == UNION_STORE_CHANNEL && args.api_base.is_none() {
        return Err(Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("渠道 {channel} 未注入 API 端点（--api-base 必填）"),
        )
        .with_hint(
            "快应用联盟商店无公开 CI 端点（web 控制台 / IDE 上传），端点必须显式注入；\
             协议面为最小模型，待真账号联调（R4 记录在案）",
        ));
    }

    // 代码包：显式路径原样透传；缺省定位目标产物落点里的 `.rpk`。
    let (package, package_bytes) = match &args.package {
        Some(path) => (path.clone(), file_bytes(path)?),
        None => locate_rpk(project, spec)?,
    };

    // 载具 pre-flight（rpk 腿接线）：经 polypack-publish 公开入口 `Package::load` 自动判别载具，
    // 拿到与四步驱动**同源**的摘要，据此算回执键——dry-run 即完整预演幂等重跑的落点锚。
    let carrier = Package::load(&package).map_err(|err| {
        Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("代码包 {} 不可用作载具：{err}", package.display()),
        )
        .with_hint(
            "载具 = 单文件（快应用 .rpk）或 T-M3-06 产物目录（<…>/miniapp/dist，根须有 app.json）；\
             先 polypack build --target quickapp 重新出包",
        )
    })?;
    let digest = carrier.digest().to_string();
    let carrier_kind = if carrier.single_file().is_some() {
        "single-file"
    } else {
        "dir"
    };
    let appid = required_flag(args.appid.as_deref(), "--appid")?;
    let version = required_flag(args.version.as_deref(), "--version")?;
    let receipt_key = polypack_publish::receipt_key(appid, version, &digest);

    // 凭据引用（引用模型）：`env:VAR` → CredentialRef + EnvProvider 现取；只留形态不留值。
    let credential = required_flag(args.credential.as_deref(), "--credential")?;
    let (source, alias) = crate::retry::parse_credential(credential)?;
    let reference = CredentialRef {
        name: alias.unwrap_or_else(|| format!("{channel}.{appid}")),
        source,
    };
    let (payload, provider) = resolve_payload(&reference, providers).map_err(publish_error)?;
    let payload_fields = payload_fields(&payload)?;

    // 提审材料（平台各自解释）：`--materials` 与 `--metadata-dir` 互斥（一处事实源）。
    let (materials, materials_locale, materials_digest, metadata_warnings) =
        match (&args.materials, &args.metadata_dir) {
            (Some(_), Some(_)) => {
                return Err(Diagnostic::error(
                    codes::E_PUBLISH_0001,
                    "--materials 与 --metadata-dir 互斥：只能选择一个材料来源",
                )
                .with_hint(
                    "--metadata-dir 走版本化目录（store/metadata/<platform>/<locale>.toml）；\
                     --materials 保留旧单文件 JSON",
                ));
            }
            (Some(path), None) => {
                let value = read_materials(path)?;
                let digest = metadata::materials_digest(&value);
                (Some(value), None, Some(digest), Vec::new())
            }
            (None, Some(root)) => load_metadata_materials(args, &channel, root)?,
            (None, None) => (None, None, None, Vec::new()),
        };

    Ok(Prepared {
        channel,
        package,
        package_bytes,
        carrier: carrier_kind,
        digest,
        receipt_key,
        reference,
        provider,
        payload_fields,
        api_base: args.api_base.clone(),
        materials,
        materials_locale,
        materials_digest,
        metadata_warnings,
    })
}

/// 渠道解析：显式 `--channel` 优先，否则取目标声明的第一个。
pub(super) fn resolve_channel(args: &Args, spec: &TargetSpec) -> Result<String, Diagnostic> {
    args.channel
        .clone()
        .or_else(|| spec.publish.first().cloned())
        .ok_or_else(|| {
            Diagnostic::error(
                codes::E_PUBLISH_0001,
                format!("目标 {} 未声明发布渠道（TargetSpec.publish 为空）", spec.id),
            )
            .with_hint("用 --channel 显式指定，或核对目标注册表的 publish 声明")
        })
}

/// 必填参数（clap 已把 `--check-metadata` 之外的必填项把关；直达调用时给稳定诊断）。
pub(super) fn required_flag<'a>(value: Option<&'a str>, flag: &str) -> Result<&'a str, Diagnostic> {
    value.ok_or_else(|| {
        Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("缺少 {flag}（--check-metadata 之外必填）"),
        )
        .with_hint("补齐 --appid / --version / --credential；只校验 metadata 用 --check-metadata")
    })
}

/// 旧 `--materials <file>`：读 JSON（形态错误 → 稳定输入诊断）。
fn read_materials(path: &Path) -> Result<serde_json::Value, Diagnostic> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("提审材料 {} 不可读：{err}", path.display()),
        )
    })?;
    serde_json::from_str(&text).map_err(|err| {
        Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("提审材料 {} 不是合法 JSON：{err}", path.display()),
        )
    })
}

/// `--metadata-dir` 结果：材料值、实际 locale、材料摘要、加载告警。
type MetadataMaterials = (
    Option<serde_json::Value>,
    Option<String>,
    Option<String>,
    Vec<String>,
);

/// `--metadata-dir`：校验整个 platform 目录，构建确定性材料（key 排序）。
///
/// 请求 locale 缺失 → 回退 `default` 并显式告警（INITIATIVES M9-E5 风险缓解口径）。
fn load_metadata_materials(
    args: &Args,
    channel: &str,
    root: &Path,
) -> Result<MetadataMaterials, Diagnostic> {
    let platform = Platform::from_channel(channel).ok_or_else(|| {
        Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("渠道 {channel} 没有 metadata 规则（当前支持 wechat / alipay）"),
        )
        .with_hint("--metadata-dir 仅支持 store/metadata/{wechat,alipay}/<locale>.toml")
    })?;
    let dir = root.join(platform.id());
    let set = metadata::load(platform, &dir).map_err(|err| {
        Diagnostic::error(codes::E_PUBLISH_0001, format!("metadata 不可读：{err}")).with_hint(
            "--metadata-dir 应指向 store/metadata 根目录（内含 <platform>/<locale>.toml）",
        )
    })?;
    if !set.is_ok() {
        return Err(metadata_diagnostic(&set));
    }
    let requested = args.metadata_locale.as_str();
    let fallback = set.falls_back(requested);
    let built = set.materials(requested).ok_or_else(|| {
        Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("metadata 缺少 locale {requested} 且无 {DEFAULT_LOCALE} 可回退"),
        )
        .with_hint("补 <locale>.toml / default.toml 后重跑")
    })?;
    let warnings = if fallback {
        vec![format!(
            "metadata locale {requested} 缺失，已回退 {DEFAULT_LOCALE}（显式告警，不静默）"
        )]
    } else {
        Vec::new()
    };
    Ok((
        Some(built.value),
        Some(built.locale),
        Some(built.digest),
        warnings,
    ))
}

/// 校验失败诊断：一次带出全部违规（稳定码 + 可读消息）。
pub(super) fn metadata_diagnostic(set: &metadata::MetadataSet) -> Diagnostic {
    let violations = set.violations();
    let rendered = violations
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    Diagnostic::error(
        codes::E_PUBLISH_0001,
        format!("metadata 校验失败（{} 项）：{rendered}", violations.len()),
    )
    .with_hint(
        "按 polypack-publish/src/metadata.rs 的规则表修正后重跑 polypack publish --check-metadata\
         （只校验，不会发布）",
    )
}

/// 缺省定位：`<packages>/<target>/miniapp/dist/*.rpk`（hap-toolkit 产物；名序取尾，多个告警）。
pub(super) fn locate_rpk(
    project: &polypack_core::Project,
    spec: &TargetSpec,
) -> Result<(PathBuf, Option<u64>), Diagnostic> {
    let dist = project::app_packages_root(project)
        .join(&spec.id)
        .join(MINIAPP_DIR)
        .join(DIST_DIR);
    let mut rpks: Vec<PathBuf> = std::fs::read_dir(&dist)
        .map_err(|err| {
            Diagnostic::error(
                codes::E_PUBLISH_0001,
                format!("目标 {} 无产物目录 {}（{err}）", spec.id, dist.display()),
            )
            .with_hint(format!(
                "先跑 polypack build --target {} 出包（hap-toolkit 在场时直出 .rpk），或用 --package 指定",
                spec.id
            ))
        })?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "rpk"))
        .collect();
    rpks.sort();
    let Some(package) = rpks.pop() else {
        return Err(Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("{} 下没有 .rpk（目标 {}）", dist.display(), spec.id),
        )
        .with_hint(
            "hap-toolkit 不在场时打包腿如实跳过（无 .rpk）；装齐工具链重跑 polypack build，\
             或用 --package 显式指定代码包",
        ));
    };
    if !rpks.is_empty() {
        // 口径同 verify 的快应用腿：名序取尾，多份如实告警（不静默挑一件）
        eprintln!(
            "publish 告警：{} 下有 {} 个 .rpk，按名序取尾 {}（其余：{}）",
            dist.display(),
            rpks.len() + 1,
            package.display(),
            rpks.iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" / ")
        );
    }
    let bytes = file_bytes(&package)?;
    Ok((package, bytes))
}

/// 文件载体给字节数；目录载体给 `None`（代码包形态由 polypack-publish 的 `Package` 判别）。
fn file_bytes(path: &Path) -> Result<Option<u64>, Diagnostic> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => Ok(Some(meta.len())),
        Ok(_) => Ok(None),
        Err(err) => Err(Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("代码包 {} 不可读：{err}", path.display()),
        )),
    }
}

/// 凭据载荷形态：base64(JSON) → 顶层字段名（有序）。值零落屏——只回字段名。
pub(super) fn payload_fields(payload: &[u8]) -> Result<Vec<String>, Diagnostic> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|err| {
            Diagnostic::error(
                codes::E_PUBLISH_0001,
                format!("凭据载荷不是合法 base64({err})"),
            )
            .with_hint("env 后端的变量值须为 base64(JSON) 载荷（同 T-M2-11 模型）")
        })?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).map_err(|err| {
        Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("凭据载荷不是合法 JSON({err})"),
        )
        .with_hint("env 后端的变量值须为 base64(JSON) 载荷（同 T-M2-11 模型）")
    })?;
    let Some(object) = value.as_object() else {
        return Err(Diagnostic::error(
            codes::E_PUBLISH_0001,
            "凭据载荷 JSON 顶层应为对象（字段名可选报，值从不落屏）",
        ));
    };
    let mut fields: Vec<String> = object.keys().cloned().collect();
    fields.sort();
    Ok(fields)
}
