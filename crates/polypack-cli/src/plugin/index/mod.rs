//! `polypack plugin add` / `polypack plugin search`（T-M9-41）：ADR 0004 签名插件索引。
//!
//! 验证顺序固定：索引分离签名（`ArtifactVerifier` + `CredentialRef` 信任根）→
//! schema / sequence / previous_digest / expires_at → 吊销快照验签、序列与命中 →
//! 制品 SRI / 字节数 → 发布者分离签名与指纹 → `wit_version`。任一步失败即拒绝。
//! 默认离线：只接受 `file:` 制品 / 签名 URL，不发起网络请求。

mod rules;
mod schema;
mod select;
mod verify;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use polypack_core::{CredentialRef, Diagnostic, codes};
use polypack_sign::{ArtifactVerifier, SignError, secret};

use rules::{matching_revocation, normalize_fingerprint, valid_plugin_id};
use schema::{PluginEntry, SUPPORTED_WIT, index_error};
use select::{entry_version, host_supports, host_version, matches_query, parse_spec, select_entry};
use verify::verify;

/// `polypack plugin add` 的参数。
#[derive(Debug, clap::Args)]
pub struct AddArgs {
    /// 插件 id 或 id@version（省略版本时选最新可用 SemVer）。
    pub spec: String,
    /// 签名索引路径；目录时读取 index.json。
    #[command(flatten)]
    pub index: IndexArgs,
    /// 发布者公钥的凭据引用（env:VAR / keyring[:条目] / vault[:条目]）。
    #[arg(long)]
    pub publisher_key: String,
}

/// `polypack plugin search` 的参数。
#[derive(Debug, clap::Args)]
pub struct SearchArgs {
    /// 检索词；空字符串列出全部可用插件。
    pub query: String,
    /// 签名索引路径；目录时读取 index.json。
    #[command(flatten)]
    pub index: IndexArgs,
}

/// 索引 / 吊销 / 信任根的公共参数。
#[derive(Debug, clap::Args)]
pub struct IndexArgs {
    /// 签名索引路径（index.json 或包含 index.json 的目录）。
    #[arg(long)]
    pub index: PathBuf,
    /// 索引分离签名（缺省 `<index>.asc`）。
    #[arg(long)]
    pub index_signature: Option<PathBuf>,
    /// 吊销快照（缺省索引同目录 revocations.json）。
    #[arg(long)]
    pub revocations: Option<PathBuf>,
    /// 吊销快照分离签名（缺省 `<revocations>.asc`）。
    #[arg(long)]
    pub revocations_signature: Option<PathBuf>,
    /// 索引签名信任根公钥的凭据引用。
    #[arg(long)]
    pub trust_root: String,
    /// 吊销签名信任根公钥的凭据引用（必须独立密钥）。
    #[arg(long)]
    pub revocations_trust_root: String,
    /// 本地状态目录（回滚基线 + 已安装插件）。
    #[arg(long, default_value = ".polypack/plugin-index")]
    pub state: PathBuf,
}

/// 执行 `polypack plugin add`。
pub fn add(args: &AddArgs, json: bool) -> Result<i32, Diagnostic> {
    let verified = verify(&args.index)?;
    let (id, wanted) = parse_spec(&args.spec)?;
    if !valid_plugin_id(id) {
        return Err(index_error(format!("插件 id 非法：{id:?}")));
    }
    let host = host_version();
    let entry = select_entry(&verified.index, &verified.revocations, id, wanted, &host)?;
    let base = index_base(&args.index);

    // 4. 制品 SRI 与字节数（ADR 0004 验证顺序）。
    let artifact_path = local_url(&entry.artifact.url, &base)?;
    let signature_path = local_url(&entry.signature.url, &base)?;
    let artifact = read_file(&artifact_path, "插件制品")?;
    if artifact.len() as u64 != entry.artifact.size {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0006,
            format!(
                "插件 {}@{} 制品大小不符：索引 {} 字节，实际 {} 字节",
                entry.id,
                entry.version,
                entry.artifact.size,
                artifact.len()
            ),
        )
        .with_hint("重新下载制品；仍不符说明索引 / 制品被篡改或版本错配"));
    }
    let artifact_digest = polypack_plugin::content_digest(&artifact);
    if !artifact_digest.eq_ignore_ascii_case(&entry.artifact.sri) {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0006,
            format!(
                "插件 {}@{} SRI 不符：索引 {}，实际 {artifact_digest}",
                entry.id, entry.version, entry.artifact.sri
            ),
        )
        .with_hint("制品字节与索引声明的 blake3 摘要不一致，拒绝安装"));
    }

    // 5. 发布者分离签名 + 指纹绑定。
    let publisher_reference = credential(&args.publisher_key, "plugin-publisher")?;
    let publisher_signature = read_file(&signature_path, "发布者签名")?;
    let publisher_verifier =
        ArtifactVerifier::from_reference(&publisher_reference, &secret::default_backends())
            .map_err(trust_error)?;
    let signer = publisher_verifier
        .verify_detached_signer(&artifact, &publisher_signature)
        .map_err(|err| signature_error(err, codes::E_PLUGIN_0009))?;
    let expected = normalize_fingerprint(&entry.publisher.key_id)
        .expect("索引校验已保证 publisher.key_id 是指纹");
    if signer != expected {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0009,
            format!(
                "发布者指纹不符：索引 {}，签名公钥 {signer}",
                entry.publisher.key_id
            ),
        )
        .with_hint("核对插件的发布者指纹；不要仅凭 display_name 判断"));
    }

    // 索引安装不执行插件；只落盘制品与已验证条目。
    let install_dir = args
        .index
        .state
        .join("plugins")
        .join(&entry.id)
        .join(&entry.version);
    std::fs::create_dir_all(&install_dir)
        .map_err(|err| crate::project::io_error("创建插件安装目录", &install_dir, err))?;
    let wasm_path = install_dir.join("plugin.wasm");
    std::fs::write(&wasm_path, &artifact)
        .map_err(|err| crate::project::io_error("写入插件制品", &wasm_path, err))?;
    let entry_path = install_dir.join("entry.json");
    let entry_json =
        serde_json::to_vec_pretty(entry).map_err(|_| index_error("已验证条目无法序列化"))?;
    std::fs::write(&entry_path, entry_json)
        .map_err(|err| crate::project::io_error("写入插件条目", &entry_path, err))?;
    verified.save_state(&args.index.state)?;

    if json {
        println!(
            "{}",
            serde_json::json!({
                "event": "plugin_add",
                "id": entry.id,
                "version": entry.version,
                "path": display_path(&wasm_path),
                "entry": display_path(&entry_path),
                "sri": artifact_digest,
                "publisher": entry.publisher.key_id,
                "index_sequence": verified.index.sequence,
            })
        );
    } else {
        println!(
            "已安装插件 {} {}（仅落盘，未执行）",
            entry.id, entry.version
        );
        println!("  制品：{}", wasm_path.display());
        println!("  条目：{}", entry_path.display());
        println!("  SRI：{artifact_digest}");
        println!(
            "  发布者：{} <{}>",
            entry.publisher.display_name, entry.publisher.key_id
        );
        println!("  索引 sequence：{}", verified.index.sequence);
    }
    Ok(0)
}

/// 执行 `polypack plugin search`：只读签名索引，不触网；推进本地回滚基线。
pub fn search(args: &SearchArgs, json: bool) -> Result<i32, Diagnostic> {
    let verified = verify(&args.index)?;
    verified.save_state(&args.index.state)?;
    let host = host_version();
    let query = args.query.to_lowercase();
    let mut latest: BTreeMap<&str, &PluginEntry> = BTreeMap::new();
    for entry in &verified.index.plugins {
        if entry.status != "active"
            || matching_revocation(entry, &verified.revocations).is_some()
            || entry.wit_version != SUPPORTED_WIT
            || !host_supports(entry, &host)
            || !matches_query(entry, &query)
        {
            continue;
        }
        match latest.get(entry.id.as_str()) {
            Some(current) if entry_version(entry) <= entry_version(current) => {}
            _ => {
                latest.insert(entry.id.as_str(), entry);
            }
        }
    }
    let results: Vec<&PluginEntry> = latest.into_values().collect();

    if json {
        let results: Vec<serde_json::Value> = results
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "id": entry.id,
                    "name": entry.name,
                    "summary": entry.summary,
                    "version": entry.version,
                    "sri": entry.artifact.sri,
                    "publisher": entry.publisher.key_id,
                    "publisher_name": entry.publisher.display_name,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "event": "plugin_search",
                "query": args.query,
                "index_sequence": verified.index.sequence,
                "results": results,
            })
        );
    } else if results.is_empty() {
        println!("无匹配插件（查询 {:?}）", args.query);
    } else {
        println!("插件索引（sequence {}）：", verified.index.sequence);
        for entry in &results {
            println!(
                "  {} {} — {}：{}",
                entry.id, entry.version, entry.name, entry.summary
            );
            println!(
                "      发布者：{} <{}>",
                entry.publisher.display_name, entry.publisher.key_id
            );
        }
    }
    Ok(0)
}

// ---------------------------------------------------------------- 选择 / 路径

struct Paths {
    index: PathBuf,
    index_signature: PathBuf,
    revocations: PathBuf,
    revocations_signature: PathBuf,
}

fn resolve_paths(args: &IndexArgs) -> Paths {
    let index = if args.index.is_dir() {
        args.index.join("index.json")
    } else {
        args.index.clone()
    };
    let base = index
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let index_signature = args
        .index_signature
        .clone()
        .unwrap_or_else(|| with_asc(&index));
    let revocations = args
        .revocations
        .clone()
        .unwrap_or_else(|| base.join("revocations.json"));
    let revocations_signature = args
        .revocations_signature
        .clone()
        .unwrap_or_else(|| with_asc(&revocations));
    Paths {
        index,
        index_signature,
        revocations,
        revocations_signature,
    }
}

fn index_base(args: &IndexArgs) -> PathBuf {
    let index = if args.index.is_dir() {
        args.index.join("index.json")
    } else {
        args.index.clone()
    };
    index
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn with_asc(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".asc");
    path.with_file_name(name)
}

/// `file:` URL → 本地路径；远程 URL 在默认离线模式下拒绝。
fn local_url(url: &str, base: &Path) -> Result<PathBuf, Diagnostic> {
    let Some(rest) = url.strip_prefix("file:") else {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0011,
            format!("默认离线：不自动下载远程制品 / 签名（{url}）"),
        )
        .with_hint("先把制品与签名下载到本地，再把索引条目的 URL 改为 file:"));
    };
    let path = if let Some(after) = rest.strip_prefix("//") {
        if after.starts_with('/') {
            after
        } else {
            return Err(Diagnostic::error(
                codes::E_PLUGIN_0011,
                format!("不支持带主机段的 file:// URL：{url}"),
            )
            .with_hint("使用 file:///绝对路径 或 file:相对路径"));
        }
    } else {
        rest
    };
    let path = PathBuf::from(path);
    Ok(if path.is_absolute() {
        path
    } else {
        base.join(path)
    })
}

fn read_file(path: &Path, what: &str) -> Result<Vec<u8>, Diagnostic> {
    std::fs::read(path).map_err(|err| crate::project::io_error(&format!("读取{what}"), path, err))
}

fn credential(value: &str, name: &str) -> Result<CredentialRef, Diagnostic> {
    let (source, alias) = crate::retry::parse_credential(value).map_err(|err| {
        Diagnostic::error(
            codes::E_PLUGIN_0010,
            format!("信任根引用非法：{}", err.message),
        )
        .with_hint("可写：env:变量名 / keyring[:条目名] / vault[:条目名]")
    })?;
    Ok(CredentialRef {
        name: alias.unwrap_or_else(|| name.to_string()),
        source,
    })
}

fn trust_error(err: SignError) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(codes::E_PLUGIN_0010, err.to_string());
    if let Some(hint) = err.hint() {
        diagnostic = diagnostic.with_hint(hint);
    }
    diagnostic
}

/// 验签失败映射到调用方的稳定码（索引 0011 / 发布者 0009）；信任根不可用仍归 0010。
fn signature_error(err: SignError, code: polypack_core::Code) -> Diagnostic {
    if matches!(
        err,
        SignError::CredentialUnavailable { .. }
            | SignError::CredentialInvalid { .. }
            | SignError::ToolMissing { .. }
            | SignError::Io { .. }
    ) {
        return trust_error(err);
    }
    let mut diagnostic = Diagnostic::error(code, err.to_string());
    if let Some(hint) = err.hint() {
        diagnostic = diagnostic.with_hint(hint);
    }
    diagnostic
}

fn display_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}
