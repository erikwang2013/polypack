//! `poly release`：多目标仓库发布编排（T-M5-11；增量发布 T-M9-07）。
//!
//! 本层落 poly-cli 自有面：目标解析、产物定位与清单摘要、回执 schema/固定落点
//! （`<root>/.polypack/release/<target>/<version>-<digest16>.json`）、幂等与
//! `--changed` 判定、`E_RELEASE_*` 稳定错误码；仓库刷新/签名由 [`RepoBackend`] 承接
//! （缺宿主工具报 `E_RELEASE_0005`）。`--dry-run` 不触仓库；`--changed` 命中回执时
//! 输出 `release-skipped` 且不重写回执。shortcut: 回执按「目标 + 清单摘要」单键，
//! 同键并发按单写者假设（后写赢）。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use poly_cache::Digest;
use poly_core::Project;
use poly_core::codes;
use poly_core::diagnostics::Diagnostic;
use poly_targets::{Registry, TargetSpec};
use serde::{Deserialize, Serialize};

use crate::project;

mod artifacts;
mod repo;
mod report;
mod targets;

use artifacts::collect_artifacts;
pub(crate) use report::RepoKind;
use report::{
    is_noop, load_receipt, no_op_receipt, report_dry_run, report_published, report_skipped,
    save_receipt,
};
use targets::{apply_format_overrides, resolve_repo, resolve_targets};

/// 回执 schema 版本（格式变更即升；读侧拒绝陌生版本）。
const SCHEMA_VERSION: u32 = 1;

/// 清单摘要域前缀（与文件摘要隔离，避免不同用途的摘要碰撞）。
const MANIFEST_DOMAIN: &[u8] = b"polypack-release/v1";

/// `poly release` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// 发布目标（逗号分隔或重复：`--target fedora,freebsd`）。
    #[arg(long, required = true, value_delimiter = ',')]
    pub target: Vec<String>,
    /// 仓库根目录（相对路径按工程根解析；缺省 `<工程根>/.polypack/repo`）。
    #[arg(long, default_value = ".polypack/repo")]
    pub repo: PathBuf,
    /// 签名凭据引用（可重复）：裸 `REF` 为全局缺省，`TARGET=REF` 按目标覆盖
    /// （如 `fedora=keyring:fedora-gpg`）。形态 env:VAR / keyring[:条目] / vault[:条目]；
    /// dry-run 只报是否给出，值零落屏。
    #[arg(long, value_name = "REF", action = clap::ArgAction::Append)]
    pub credential: Vec<String>,
    /// 只解析产物与幂等锚点，不写仓库、不落回执。
    #[arg(long)]
    pub dry_run: bool,
    /// 只发布变更的目标：回执的 manifest_digest / cache_key、仓库根与仓库格式均与
    /// 当前产物一致时输出 `release-skipped` 并跳过（不写仓库、不重写回执、runs 不递增）。
    #[arg(long)]
    pub changed: bool,
    /// 变更基线 git 引用（配合 `--changed`）。当前发布模式不构建、回执也无按目标的
    /// 源码/包路径映射，无法稳健用 git diff 判定；给出时返回 E_PLAN_0002 而非静默忽略。
    #[arg(long, value_name = "REF")]
    pub since: Option<String>,
}

/// 执行 `poly release`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    if let Some(reference) = &args.since {
        return Err(since_unavailable(reference));
    }
    let project = project::load(&args.config)?;
    let registry = Registry::builtin();
    let mut specs = resolve_targets(&args.target, &registry)?;
    apply_format_overrides(&project, &mut specs)?;
    let credentials = parse_credentials(&args.credential, &specs)?;
    let repo = resolve_repo(&project.root, &args.repo);
    for spec in &specs {
        let prepared = prepare(&project, spec, &repo)?;
        let credential = credentials.resolve(&prepared.target);
        if args.dry_run {
            let cache_hit = no_op_receipt(&prepared)?.is_some();
            report_dry_run(&prepared, cache_hit, credential.is_some(), json);
            continue;
        }
        // deb / rpm / pkg / pkgsrc 都是本地自托管仓库后端（pkg(8) / pkgsrc 工具真跑需
        // FreeBSD / NetBSD 用户态）。
        let backend: Box<dyn RepoBackend> = Box::new(repo::LocalBackend);
        let now = unix_now();
        if args.changed {
            match release_changed(&prepared, credential, backend.as_ref(), now)? {
                ChangedOutcome::Skipped {
                    receipt,
                    key_source,
                } => report_skipped(&prepared, &receipt, key_source, json),
                ChangedOutcome::Published(outcome) => report_published(&prepared, &outcome, json),
            }
            continue;
        }
        let outcome = release_target(&prepared, credential, backend.as_ref(), now)?;
        report_published(&prepared, &outcome, json);
    }
    Ok(0)
}

/// `--since` 在本模式不可用：release 不构建，回执没有按目标的源码/包路径映射。
fn since_unavailable(reference: &str) -> Diagnostic {
    let message =
        format!("--since {reference} 在当前模式不可用：发布不构建，回执无按目标的源码/包路径映射");
    Diagnostic::error(codes::E_PLAN_0002, message).with_hint(
        "改用 --changed（按回执 manifest_digest / cache_key 判定），或先按 git diff 选 --target",
    )
}

/// `--credential` 解析结果：裸引用为全局缺省，`TARGET=REF` 按目标覆盖。
#[derive(Debug, Default)]
struct Credentials {
    default: Option<String>,
    per_target: std::collections::BTreeMap<String, String>,
}

impl Credentials {
    fn resolve(&self, target: &str) -> Option<&str> {
        let mapped = self.per_target.get(target).map(String::as_str);
        mapped.or(self.default.as_deref())
    }
}

/// 解析重复的 `--credential`：`=` 左侧无 `:` 且像目标（或右值像凭据引用）才按映射；
/// 其余整值按引用（env 变量名可含 `=`，不能见 `=` 就当映射）。
fn parse_credentials(values: &[String], specs: &[TargetSpec]) -> Result<Credentials, Diagnostic> {
    let ids: std::collections::BTreeSet<&str> = specs.iter().map(|s| s.id.as_str()).collect();
    let mut creds = Credentials::default();
    for value in values {
        let Some((left, right)) = value.split_once('=').filter(|(left, right)| {
            !left.contains(':')
                && (ids.contains(left) || crate::retry::parse_credential(right).is_ok())
        }) else {
            creds.default = Some(value.clone());
            continue;
        };
        if !ids.contains(left) {
            let msg = format!("--credential 映射 {left}= 的目标不在本次 --target 中");
            return Err(credential_error(msg));
        }
        if creds.per_target.insert(left.into(), right.into()).is_some() {
            let msg = format!("--credential 对目标 {left} 重复映射");
            return Err(credential_error(msg));
        }
    }
    Ok(creds)
}

/// 凭据参数错误统一口径（与凭据引用解析同码）。
fn credential_error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(codes::E_PLAN_0002, message.into())
        .with_hint("裸 REF 作全局缺省；TARGET=REF 按目标覆盖，左侧须是 --target 中的目标 id")
}

/// 产物清单的一项（回执里只留可移植字段；`path` 仅本次执行用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Artifact {
    /// 相对产物目录的路径（`/` 分隔，跨平台稳定）。
    name: String,
    bytes: u64,
    digest: String,
    /// 本次执行的绝对路径（不落回执）。
    #[serde(skip)]
    path: PathBuf,
}

/// 发布回执（目标 + 版本 + 清单摘要一份；永久保留）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Receipt {
    schema_version: u32,
    target: String,
    version: String,
    /// 仓库根（绝对路径；换仓库即不同发布）。
    repo: String,
    repo_kind: RepoKind,
    manifest_digest: String,
    /// 变更判定键（可选：旧回执缺省，读侧降级到 `manifest_digest`；本期值与清单摘要同源）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cache_key: Option<String>,
    /// 产物集合的 SBOM 摘要（可选：旧回执缺省；发布时由 `poly sbom` 同一实现计算）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sbom_digest: Option<String>,
    artifacts: Vec<Artifact>,
    /// 本键累计执行次数（首跑 1；no-op 也 +1——如实记录被跑过几次）。
    runs: u64,
    first_unix: u64,
    last_unix: u64,
}

/// 一次发布的解析结果（dry-run 与真腿共用）。
#[derive(Debug)]
struct Prepared {
    target: String,
    version: String,
    /// 仓库根（绝对路径）。
    repo: PathBuf,
    kind: RepoKind,
    signing: Option<String>,
    artifacts: Vec<Artifact>,
    manifest_digest: String,
    /// 变更判定键（本期与清单摘要同源；未来可替换为构建缓存键）。
    cache_key: String,
    receipt: PathBuf,
}

#[derive(Debug)]
struct Outcome {
    cache_hit: bool,
    runs: u64,
}

/// 仓库后端接线点：产物入库 → 元数据刷新 → 签名（T-M5-05/06 入 main 后实现）。
trait RepoBackend {
    /// 发布一组产物；成功即表示仓库已可被客户端消费。
    fn publish(&self, request: &RepoRequest<'_>) -> Result<(), Diagnostic>;
}

/// 一次仓库发布的输入（凭据只传引用，不含明文值）。
struct RepoRequest<'a> {
    target: &'a str,
    repo: &'a Path,
    kind: RepoKind,
    signing: Option<&'a str>,
    artifacts: &'a [Artifact],
    /// 签名凭据引用（未给 = 后端按目标声明解析）。
    credential: Option<&'a str>,
}

/// 解析一个目标：校验仓库格式、定位产物、算清单摘要与回执锚点。
fn prepare(project: &Project, spec: &TargetSpec, repo: &Path) -> Result<Prepared, Diagnostic> {
    let kind = RepoKind::from_format(&spec.package.format).ok_or_else(|| {
        Diagnostic::error(
            codes::E_RELEASE_0002,
            format!(
                "目标 {} 的产物格式 {} 不是仓库格式（deb / rpm / pkg / pkgsrc）",
                spec.id, spec.package.format
            ),
        )
        .with_hint(
            "poly release 只编排 deb / rpm / pkg / pkgsrc 仓库目标\
             （ubuntu / mint / fedora / freebsd / netbsd）；其它格式走 poly publish，\
             或先补该格式的仓库打包器",
        )
    })?;

    let artifact_root = project::packages_root(&project.root).join(&spec.id);
    let mut found = Vec::new();
    collect_artifacts(&artifact_root, kind.extension(), &artifact_root, &mut found).map_err(
        |err| {
            Diagnostic::error(
                codes::E_RELEASE_0001,
                format!("产物目录 {} 不可读：{err}", artifact_root.display()),
            )
            .with_hint(format!(
                "先跑 poly build --target {} 出包（产物落 <工程根>/.polypack/packages/{}）",
                spec.id, spec.id
            ))
        },
    )?;
    found.sort_by(|a, b| a.0.cmp(&b.0));
    if found.is_empty() {
        return Err(Diagnostic::error(
            codes::E_RELEASE_0001,
            format!(
                "目标 {} 在 {} 下没有 .{} 产物",
                spec.id,
                artifact_root.display(),
                kind.extension()
            ),
        )
        .with_hint(format!(
            "先跑 poly build --target {} 出包，或核对 --config 指向的工程",
            spec.id
        )));
    }

    let mut artifacts = Vec::with_capacity(found.len());
    for (name, path) in found {
        let bytes = std::fs::read(&path).map_err(|err| {
            Diagnostic::error(
                codes::E_RELEASE_0001,
                format!("产物 {} 不可读：{err}", path.display()),
            )
            .with_hint(format!("先跑 poly build --target {} 重新出包", spec.id))
        })?;
        artifacts.push(Artifact {
            name,
            bytes: bytes.len() as u64,
            digest: Digest::of_bytes(&bytes).to_string(),
            path,
        });
    }

    let manifest_digest = manifest_digest(&artifacts).to_string();
    let short = manifest_digest.get(..16).unwrap_or(&manifest_digest);
    let version = project.config.package.version.clone();
    let receipt = project
        .root
        .join(".polypack")
        .join("release")
        .join(&spec.id)
        .join(format!("{}-{short}.json", file_part(&version)));
    Ok(Prepared {
        target: spec.id.clone(),
        version,
        repo: repo.to_path_buf(),
        kind,
        signing: spec.signing.clone(),
        artifacts,
        cache_key: manifest_digest.clone(),
        manifest_digest,
        receipt,
    })
}

/// 产物集合清单摘要：按名排序后逐项混入名字 / 摘要 / 字节数（顺序无关、内容绑定）。
fn manifest_digest(artifacts: &[Artifact]) -> Digest {
    let mut sorted: Vec<&Artifact> = artifacts.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let mut input = Vec::new();
    input.extend_from_slice(MANIFEST_DOMAIN);
    for artifact in sorted {
        input.extend_from_slice(&(artifact.name.len() as u64).to_le_bytes());
        input.extend_from_slice(artifact.name.as_bytes());
        input.extend_from_slice(artifact.digest.as_bytes());
        input.extend_from_slice(&artifact.bytes.to_le_bytes());
    }
    Digest::of_bytes(&input)
}

/// 版本号 → 文件名安全子集（保留 `[A-Za-z0-9._-]`；空 → `empty`）。
fn file_part(part: &str) -> String {
    let cleaned: String = part
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "empty".to_string()
    } else {
        cleaned
    }
}

/// 回执命中使用的变更键来源（旧回执无 `cache_key` 时降级到清单摘要）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeySource {
    CacheKey,
    ManifestDigest,
}

impl KeySource {
    fn as_str(self) -> &'static str {
        match self {
            Self::CacheKey => "cache_key",
            Self::ManifestDigest => "manifest_digest",
        }
    }
}

/// `--changed` 的单目标结果：跳过时带回执（零写入）与键来源。
#[derive(Debug)]
enum ChangedOutcome {
    Skipped {
        receipt: Receipt,
        key_source: KeySource,
    },
    Published(Outcome),
}

/// `--changed` 判定：回执命中 → `Some`；无回执 / 键或仓库不符 → `None`（照常发布）。
fn changed_receipt(prepared: &Prepared) -> Result<Option<(Receipt, KeySource)>, Diagnostic> {
    let Some(receipt) = load_receipt(&prepared.receipt, prepared)? else {
        return Ok(None);
    };
    if !is_noop(&receipt, prepared) {
        return Ok(None);
    }
    let key_source = if receipt.cache_key.as_deref() == Some(prepared.cache_key.as_str()) {
        KeySource::CacheKey
    } else {
        KeySource::ManifestDigest
    };
    Ok(Some((receipt, key_source)))
}

/// `--changed` 单目标执行：命中即 Skipped（不触仓库、不重写回执、runs 不递增）；
/// 否则发布并写入 cache_key（供下次增量判定）。
fn release_changed(
    prepared: &Prepared,
    credential: Option<&str>,
    backend: &dyn RepoBackend,
    now: u64,
) -> Result<ChangedOutcome, Diagnostic> {
    if let Some((receipt, key_source)) = changed_receipt(prepared)? {
        return Ok(ChangedOutcome::Skipped {
            receipt,
            key_source,
        });
    }
    Ok(ChangedOutcome::Published(publish_target(
        prepared, credential, backend, now, true,
    )?))
}

/// 发布一个目标：同键命中即 no-op（runs+1）；否则经后端入库后落回执。
/// 不带 `--changed` 时回执字节与旧版本一致（不写 cache_key）。
fn release_target(
    prepared: &Prepared,
    credential: Option<&str>,
    backend: &dyn RepoBackend,
    now: u64,
) -> Result<Outcome, Diagnostic> {
    publish_target(prepared, credential, backend, now, false)
}

/// 发布实现；`record_cache_key` 仅 `--changed` 路径为 true。
fn publish_target(
    prepared: &Prepared,
    credential: Option<&str>,
    backend: &dyn RepoBackend,
    now: u64,
    record_cache_key: bool,
) -> Result<Outcome, Diagnostic> {
    let existing = load_receipt(&prepared.receipt, prepared)?;
    if let Some(hit) = existing
        .as_ref()
        .filter(|receipt| is_noop(receipt, prepared))
    {
        let mut receipt = hit.clone();
        receipt.runs += 1;
        receipt.last_unix = now;
        save_receipt(&prepared.receipt, &receipt)?;
        return Ok(Outcome {
            cache_hit: true,
            runs: receipt.runs,
        });
    }

    let request = RepoRequest {
        target: &prepared.target,
        repo: &prepared.repo,
        kind: prepared.kind,
        signing: prepared.signing.as_deref(),
        artifacts: &prepared.artifacts,
        credential,
    };
    backend.publish(&request)?;

    // T-M9-03：回执记录产物集合的 SBOM 摘要（与 `poly verify --sbom` 同一实现）。
    let artifact_paths: Vec<PathBuf> = prepared
        .artifacts
        .iter()
        .map(|artifact| artifact.path.clone())
        .collect();
    let sbom_digest = crate::sbom::digest_for_artifacts(&artifact_paths).map_err(|err| {
        Diagnostic::error(codes::E_RELEASE_0005, format!("SBOM 摘要计算失败：{err}"))
            .with_hint("核对待发布产物路径可读；SBOM 摘要用于发布回执审计")
    })?;

    let mut receipt = existing.unwrap_or_else(|| Receipt {
        schema_version: SCHEMA_VERSION,
        target: prepared.target.clone(),
        version: prepared.version.clone(),
        repo: prepared.repo.display().to_string(),
        repo_kind: prepared.kind,
        manifest_digest: prepared.manifest_digest.clone(),
        cache_key: None,
        sbom_digest: None,
        artifacts: Vec::new(),
        runs: 0,
        first_unix: now,
        last_unix: now,
    });
    receipt.repo = prepared.repo.display().to_string();
    receipt.repo_kind = prepared.kind;
    receipt.manifest_digest = prepared.manifest_digest.clone();
    receipt.cache_key = record_cache_key.then(|| prepared.cache_key.clone());
    receipt.sbom_digest = Some(sbom_digest);
    receipt.artifacts = prepared.artifacts.clone();
    receipt.runs += 1;
    receipt.last_unix = now;
    save_receipt(&prepared.receipt, &receipt)?;
    Ok(Outcome {
        cache_hit: false,
        runs: receipt.runs,
    })
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests;
