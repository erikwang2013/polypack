//! `poly release`：多目标仓库发布编排（T-M5-11 准备面）。
//!
//! 本层只落 **poly-cli 自有面**：目标列表解析（逗号 / 重复）、产物集合定位与清单摘要、
//! 回执 schema 与固定落点（`<root>/.polypack/release/<target>/<version>-<digest16>.json`）、
//! 幂等判定（同清单摘要 + 同仓库 = no-op，`runs+1`）与 `E_RELEASE_*` 稳定错误码。
//!
//! 仓库元数据刷新与签名由 [`RepoBackend`] 承接——真实后端等 T-M5-05/06 入 main 后接线；
//! 当前 [`PendingBackend`] 如实报 `E_RELEASE_0004`，绝不假装发布过。`--dry-run` 不受
//! 后端缺席影响：完整解析产物、清单摘要、回执锚点与命中预测。
//!
//! shortcut: 回执按「目标 + 清单摘要」单键，同键并发按单写者假设（后写赢）——发布按
//! 单脚本串行跑（同 poly-publish 归档口径）。

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
mod report;
mod targets;

use artifacts::collect_artifacts;
use report::{report_dry_run, report_published};
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
    /// 签名凭据引用（形态由仓库后端解释；dry-run 只报是否给出，值零落屏）。
    #[arg(long, value_name = "REF")]
    pub credential: Option<String>,
    /// 只解析产物与幂等锚点，不写仓库、不落回执。
    #[arg(long)]
    pub dry_run: bool,
}

/// 执行 `poly release`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let project = project::load(&args.config)?;
    let registry = Registry::builtin();
    let mut specs = resolve_targets(&args.target, &registry)?;
    apply_format_overrides(&project, &mut specs)?;
    let repo = resolve_repo(&project.root, &args.repo);
    for spec in &specs {
        let prepared = prepare(&project, spec, &repo)?;
        if args.dry_run {
            let cache_hit = no_op_receipt(&prepared)?.is_some();
            report_dry_run(&prepared, cache_hit, args.credential.is_some(), json);
            continue;
        }
        let outcome = release_target(
            &prepared,
            args.credential.as_deref(),
            &PendingBackend,
            unix_now(),
        )?;
        report_published(&prepared, &outcome, json);
    }
    Ok(0)
}

/// 仓库产物格式（本命令只编排有仓库语义的格式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RepoKind {
    /// Fedora / RHEL 系（`.rpm` + repodata）。
    Rpm,
    /// FreeBSD pkg 仓库（`.pkg` + `packagesite`）。
    Pkg,
}

impl RepoKind {
    /// 目标声明的产物格式 → 仓库格式；其它格式不归本命令。
    fn from_format(format: &str) -> Option<Self> {
        match format {
            "rpm" => Some(Self::Rpm),
            "pkg" => Some(Self::Pkg),
            _ => None,
        }
    }

    /// 产物文件扩展名（也是回执里的稳定文本）。
    fn extension(self) -> &'static str {
        match self {
            Self::Rpm => "rpm",
            Self::Pkg => "pkg",
        }
    }
}

/// 产物清单的一项（回执里只留可移植字段；`path` 仅本次执行用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Artifact {
    /// 相对产物目录的路径（`/` 分隔，跨平台稳定）。
    name: String,
    /// 字节数。
    bytes: u64,
    /// blake3 摘要（hex）。
    digest: String,
    /// 本次执行的绝对路径（不落回执）。
    #[serde(skip)]
    path: PathBuf,
}

/// 发布回执（目标 + 版本 + 清单摘要一份；永久保留）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Receipt {
    /// schema 版本（= [`SCHEMA_VERSION`]）。
    schema_version: u32,
    /// 目标 id。
    target: String,
    /// 工程版本。
    version: String,
    /// 仓库根（绝对路径；换仓库即不同发布）。
    repo: String,
    /// 仓库格式。
    repo_kind: RepoKind,
    /// 产物集合清单摘要（blake3 hex；与文件系统顺序无关）。
    manifest_digest: String,
    /// 本次入库的产物集合。
    artifacts: Vec<Artifact>,
    /// 本键累计执行次数（首跑 1；no-op 也 +1——如实记录被跑过几次）。
    runs: u64,
    /// 首次执行时刻（unix 秒）。
    first_unix: u64,
    /// 最近一次执行时刻（unix 秒）。
    last_unix: u64,
}

/// 一次发布的解析结果（dry-run 与真腿共用）。
#[derive(Debug)]
struct Prepared {
    /// 目标 id。
    target: String,
    /// 工程版本。
    version: String,
    /// 仓库根（绝对路径）。
    repo: PathBuf,
    /// 仓库格式。
    kind: RepoKind,
    /// 产物集合（按名排序）。
    artifacts: Vec<Artifact>,
    /// 清单摘要（blake3 hex）。
    manifest_digest: String,
    /// 回执落点。
    receipt: PathBuf,
}

/// 一次真腿的结果。
#[derive(Debug)]
struct Outcome {
    /// 是否命中 no-op（同清单摘要 + 同仓库）。
    cache_hit: bool,
    /// 本键累计执行次数（含本次）。
    runs: u64,
}

/// 仓库后端接线点：产物入库 → 元数据刷新 → 签名（T-M5-05/06 入 main 后实现）。
trait RepoBackend {
    /// 发布一组产物；成功即表示仓库已可被客户端消费。
    fn publish(&self, request: &RepoRequest<'_>) -> Result<(), Diagnostic>;
}

/// 一次仓库发布的输入（凭据只传引用，不含明文值）。
struct RepoRequest<'a> {
    /// 目标 id。
    target: &'a str,
    /// 工程版本。
    version: &'a str,
    /// 仓库根。
    repo: &'a Path,
    /// 仓库格式。
    kind: RepoKind,
    /// 产物集合。
    artifacts: &'a [Artifact],
    /// 签名凭据引用（未给 = 后端按目标声明解析）。
    credential: Option<&'a str>,
}

/// 接线前占位：如实报缺口，绝不假装发布过（dry-run 不经过这里）。
struct PendingBackend;

impl RepoBackend for PendingBackend {
    fn publish(&self, request: &RepoRequest<'_>) -> Result<(), Diagnostic> {
        let sample = request
            .artifacts
            .first()
            .map(|artifact| artifact.path.display().to_string())
            .unwrap_or_else(|| "无".to_string());
        Err(Diagnostic::error(
            codes::E_RELEASE_0004,
            format!(
                "{} 仓库后端未接线（目标 {} 版本 {}，仓库 {}，产物 {} 件如 {}）：元数据刷新与签名待 T-M5-05/06 入 main",
                request.kind.extension(),
                request.target,
                request.version,
                request.repo.display(),
                request.artifacts.len(),
                sample,
            ),
        )
        .with_hint(format!(
            "凭据引用{}；先用 --dry-run 校验产物与回执锚点，后端落位后同命令零改动激活",
            if request.credential.is_some() {
                "已给出（值不落屏）"
            } else {
                "未给出"
            }
        )))
    }
}

/// 解析一个目标：校验仓库格式、定位产物、算清单摘要与回执锚点。
fn prepare(project: &Project, spec: &TargetSpec, repo: &Path) -> Result<Prepared, Diagnostic> {
    let kind = RepoKind::from_format(&spec.package.format).ok_or_else(|| {
        Diagnostic::error(
            codes::E_RELEASE_0002,
            format!(
                "目标 {} 的产物格式 {} 不是仓库格式（rpm / pkg）",
                spec.id, spec.package.format
            ),
        )
        .with_hint(
            "poly release 只编排 rpm / pkg 仓库目标（fedora / freebsd）；\
             其它格式走 poly publish，或先补该格式的仓库打包器",
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
        artifacts,
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

/// 回执命中判定：清单摘要、仓库根、仓库格式三者一致才 no-op。
fn is_noop(receipt: &Receipt, prepared: &Prepared) -> bool {
    receipt.manifest_digest == prepared.manifest_digest
        && receipt.repo == prepared.repo.display().to_string()
        && receipt.repo_kind == prepared.kind
}

/// 读回执并判命中（dry-run 的命中预测；读失败照常报错）。
fn no_op_receipt(prepared: &Prepared) -> Result<Option<Receipt>, Diagnostic> {
    Ok(load_receipt(&prepared.receipt, prepared)?.filter(|receipt| is_noop(receipt, prepared)))
}

/// 读回执：不存在 → `Ok(None)`；形态非法 / 版本陌生 / 身份不符 → `E_RELEASE_0003`。
fn load_receipt(path: &Path, prepared: &Prepared) -> Result<Option<Receipt>, Diagnostic> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(receipt_error(path, format!("回执不可读：{err}"))),
    };
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .map_err(|err| receipt_error(path, format!("回执 JSON 非法：{err}")))?;
    if receipt.schema_version != SCHEMA_VERSION {
        return Err(receipt_error(
            path,
            format!(
                "回执 schema 版本 {} 与本期 {SCHEMA_VERSION} 不符（格式固定；迁移后重跑）",
                receipt.schema_version
            ),
        ));
    }
    if receipt.target != prepared.target || receipt.version != prepared.version {
        return Err(receipt_error(
            path,
            format!(
                "回执身份（{} {}）与路径不符",
                receipt.target, receipt.version
            ),
        ));
    }
    Ok(Some(receipt))
}

/// 回执错误统一口径（永久保留，不静默重建）。
fn receipt_error(path: &Path, message: String) -> Diagnostic {
    Diagnostic::error(
        codes::E_RELEASE_0003,
        format!("{}：{message}", path.display()),
    )
    .with_hint("回执永久保留：修复或迁移后重跑；勿手改 schema 版本")
}

/// 原子落盘（同目录临时文件 + rename）。
fn save_receipt(path: &Path, receipt: &Receipt) -> Result<(), Diagnostic> {
    let dir = path.parent().expect("回执路径必有父目录");
    std::fs::create_dir_all(dir)
        .map_err(|err| receipt_error(dir, format!("回执目录不可建：{err}")))?;
    let json = serde_json::to_vec_pretty(receipt).expect("回执序列化不会失败");
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|err| receipt_error(&tmp, format!("回执不可写：{err}")))?;
    std::fs::rename(&tmp, path).map_err(|err| receipt_error(path, format!("回执落盘失败：{err}")))
}

/// 发布一个目标：同键命中即 no-op（runs+1）；否则经后端入库后落回执。
fn release_target(
    prepared: &Prepared,
    credential: Option<&str>,
    backend: &dyn RepoBackend,
    now: u64,
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
        version: &prepared.version,
        repo: &prepared.repo,
        kind: prepared.kind,
        artifacts: &prepared.artifacts,
        credential,
    };
    backend.publish(&request)?;

    let mut receipt = existing.unwrap_or_else(|| Receipt {
        schema_version: SCHEMA_VERSION,
        target: prepared.target.clone(),
        version: prepared.version.clone(),
        repo: prepared.repo.display().to_string(),
        repo_kind: prepared.kind,
        manifest_digest: prepared.manifest_digest.clone(),
        artifacts: Vec::new(),
        runs: 0,
        first_unix: now,
        last_unix: now,
    });
    receipt.repo = prepared.repo.display().to_string();
    receipt.repo_kind = prepared.kind;
    receipt.manifest_digest = prepared.manifest_digest.clone();
    receipt.artifacts = prepared.artifacts.clone();
    receipt.runs += 1;
    receipt.last_unix = now;
    save_receipt(&prepared.receipt, &receipt)?;
    Ok(Outcome {
        cache_hit: false,
        runs: receipt.runs,
    })
}

/// 当前 unix 秒。
fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests;
