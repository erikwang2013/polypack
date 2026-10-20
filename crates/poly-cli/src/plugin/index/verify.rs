//! ADR 0004 验证链与本地回滚基线：签名 → schema → 新鲜度 → 序列。

use std::path::Path;

use poly_core::{Diagnostic, codes};
use poly_sign::{ArtifactVerifier, secret};
use serde::{Deserialize, Serialize};

use super::rules::{
    check_freshness, normalize_fingerprint, now_unix, validate_index, validate_revocations,
};
use super::schema::{
    IndexDocument, RevocationsDocument, canonical_bytes, index_error, parse_index,
    parse_revocations,
};
use super::{IndexArgs, credential, read_file, resolve_paths, signature_error, trust_error};

// ---------------------------------------------------------------- 验证链

/// 已验证的索引与吊销快照。
pub(super) struct Verified {
    pub(super) index: IndexDocument,
    pub(super) revocations: RevocationsDocument,
    index_digest: String,
    revocations_digest: String,
}

impl Verified {
    pub(super) fn save_state(&self, state_dir: &Path) -> Result<(), Diagnostic> {
        State {
            index_sequence: self.index.sequence,
            index_digest: self.index_digest.clone(),
            revocations_sequence: self.revocations.sequence,
            revocations_digest: self.revocations_digest.clone(),
        }
        .save(state_dir)
    }
}

pub(super) fn verify(args: &IndexArgs) -> Result<Verified, Diagnostic> {
    let paths = resolve_paths(args);

    // 1. 索引分离签名（签名在规范化字节上）。
    let index = parse_index(&read_file(&paths.index, "插件索引")?)?;
    let canonical_index = canonical_bytes(&index)?;
    let index_verifier = ArtifactVerifier::from_reference(
        &credential(&args.trust_root, "plugin-index")?,
        &secret::default_backends(),
    )
    .map_err(trust_error)?;
    let signer = index_verifier
        .verify_detached_signer(
            &canonical_index,
            &read_file(&paths.index_signature, "索引签名")?,
        )
        .map_err(|err| signature_error(err, codes::E_PLUGIN_0011))?;
    let declared = normalize_fingerprint(&index.signing_key_id).ok_or_else(|| {
        index_error(format!(
            "signing_key_id 不是 GPG 指纹：{:?}",
            index.signing_key_id
        ))
    })?;
    if signer != declared {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0011,
            format!(
                "索引签名指纹不符：声明 {}，实际 {signer}",
                index.signing_key_id
            ),
        )
        .with_hint("核对索引信任根；不要混用不同索引 / 镜像的签名密钥"));
    }

    // 2. schema / sequence / previous_digest / expires_at。
    validate_index(&index)?;
    let index_digest = poly_plugin::content_digest(&canonical_index);
    let state = State::load(&args.state)?;
    check_index_rollback(state.as_ref(), &index, &index_digest)?;

    // 3. 吊销快照：验签 → schema → 与索引 digest 绑定 → 序列回滚。
    let revocations = parse_revocations(&read_file(&paths.revocations, "吊销快照")?)?;
    let canonical_revocations = canonical_bytes(&revocations)?;
    let revocations_verifier = ArtifactVerifier::from_reference(
        &credential(&args.revocations_trust_root, "plugin-revocations")?,
        &secret::default_backends(),
    )
    .map_err(trust_error)?;
    let revocation_signer = revocations_verifier
        .verify_detached_signer(
            &canonical_revocations,
            &read_file(&paths.revocations_signature, "吊销快照签名")?,
        )
        .map_err(|err| signature_error(err, codes::E_PLUGIN_0011))?;
    if revocation_signer == signer {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0011,
            "索引与吊销快照必须由不同密钥签名（ADR 0004 §6）",
        )
        .with_hint("吊销密钥由安全维护者独立持有；核对两个 --*-trust-root"));
    }
    validate_revocations(&revocations)?;
    let revocations_digest = poly_plugin::content_digest(&canonical_revocations);
    if !revocations_digest.eq_ignore_ascii_case(&index.revocations_digest) {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0011,
            format!(
                "吊销快照与索引不匹配：索引绑定 {}，实际 {revocations_digest}",
                index.revocations_digest
            ),
        )
        .with_hint("使用与索引同一发布批次签发的 revocations.json 与 .asc"));
    }
    check_revocations_rollback(state.as_ref(), &revocations, &revocations_digest)?;
    check_freshness(&index, &revocations, now_unix())?;

    Ok(Verified {
        index,
        revocations,
        index_digest,
        revocations_digest,
    })
}

// ---------------------------------------------------------------- 回滚 / 状态

/// 本地最高已见 sequence 与摘要（add 安装成功后 / search 验证通过后推进）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    index_sequence: u64,
    index_digest: String,
    revocations_sequence: u64,
    revocations_digest: String,
}

impl State {
    fn load(dir: &Path) -> Result<Option<Self>, Diagnostic> {
        let path = dir.join("state.json");
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(crate::project::io_error("读取索引状态", &path, err)),
        };
        serde_json::from_slice(&bytes).map(Some).map_err(|_| {
            index_error(format!("本地索引状态损坏：{}", path.display()))
                .with_hint("确认索引可信后删除该 state.json 重建回滚基线")
        })
    }

    fn save(&self, dir: &Path) -> Result<(), Diagnostic> {
        std::fs::create_dir_all(dir)
            .map_err(|err| crate::project::io_error("创建索引状态目录", dir, err))?;
        let path = dir.join("state.json");
        let temp = dir.join("state.json.tmp");
        let bytes = serde_json::to_vec(self).map_err(|_| index_error("索引状态无法序列化"))?;
        std::fs::write(&temp, bytes)
            .map_err(|err| crate::project::io_error("写入索引状态", &temp, err))?;
        std::fs::rename(&temp, &path)
            .map_err(|err| crate::project::io_error("提交索引状态", &path, err))
    }
}

fn check_index_rollback(
    state: Option<&State>,
    index: &IndexDocument,
    digest: &str,
) -> Result<(), Diagnostic> {
    let Some(state) = state else {
        return Ok(());
    };
    if index.sequence < state.index_sequence {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0011,
            format!(
                "索引 sequence 回滚：本地已见 {}，收到 {}",
                state.index_sequence, index.sequence
            ),
        )
        .with_hint("拒绝降级；获取 sequence 不低于本地基线的签名索引"));
    }
    if index.sequence == state.index_sequence && !digest.eq_ignore_ascii_case(&state.index_digest) {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0012,
            format!("同一 sequence {} 出现不同内容（镜像分叉）", index.sequence),
        )
        .with_hint("拒绝该索引；核对多个镜像的摘要并切换可信镜像"));
    }
    // 只对紧邻的上一版验证链；跨版本跳号时只能依赖单调 sequence。
    if index.sequence == state.index_sequence + 1
        && !index
            .previous_digest
            .as_deref()
            .is_some_and(|previous| previous.eq_ignore_ascii_case(&state.index_digest))
    {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0011,
            format!(
                "索引 previous_digest 未链接到本地已见索引（sequence {}）",
                state.index_sequence
            ),
        )
        .with_hint("获取完整索引链；不要混用不同索引源"));
    }
    Ok(())
}

fn check_revocations_rollback(
    state: Option<&State>,
    revocations: &RevocationsDocument,
    digest: &str,
) -> Result<(), Diagnostic> {
    let Some(state) = state else {
        return Ok(());
    };
    if revocations.sequence < state.revocations_sequence {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0011,
            format!(
                "吊销快照 sequence 回滚：本地已见 {}，收到 {}",
                state.revocations_sequence, revocations.sequence
            ),
        )
        .with_hint("使用最新的签名吊销快照"));
    }
    if revocations.sequence == state.revocations_sequence
        && !digest.eq_ignore_ascii_case(&state.revocations_digest)
    {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0012,
            format!(
                "同一吊销 sequence {} 出现不同内容（镜像分叉）",
                revocations.sequence
            ),
        )
        .with_hint("拒绝该吊销快照并核对镜像摘要"));
    }
    Ok(())
}
