//! 插件签名策略（T-M8-03）：分离签名 + [`CredentialRef`] 信任根，验签失败拒绝实例化。
//!
//! 密码学与 GPG 调用复用 `polypack-sign`：签名侧 [`polypack_sign::GpgSigner`]
//! （[`polypack_sign::RepoSigner::detach_sign`]），验签侧 [`polypack_sign::ArtifactVerifier`]。
//! 本模块只做策略映射与 ROADMAP §7 脱敏——错误 / 日志不含宿主绝对路径或凭据值。

use polypack_core::CredentialRef;
use polypack_core::codes;
use polypack_sign::secret::SecretBackend;
use polypack_sign::{ArtifactVerifier, SignError};

use crate::error::PluginError;

/// 签名策略：`polypack.toml` 配置要求签名时，由宿主构造并传给
/// [`crate::Sandbox::load_signed`]。
pub struct SignaturePolicy<'a> {
    /// 分离签名（`.wasm.sig` 内容）；`None` = 缺失，直接拒绝。
    pub detached: Option<&'a [u8]>,
    /// 信任根（公钥）的凭据引用（keyring / Vault / Env）。
    pub trust_root: &'a CredentialRef,
    /// 凭据后端链（与 `polypack-sign` 同一套；测试可注入内存后端）。
    pub backends: &'a [Box<dyn SecretBackend>],
}

/// 验签：签名缺失 / 不符 → `E_PLUGIN_0009`；信任根不可用 → `E_PLUGIN_0010`。
pub(crate) fn verify(
    id: &str,
    bytes: &[u8],
    policy: &SignaturePolicy<'_>,
) -> Result<(), PluginError> {
    let Some(detached) = policy.detached else {
        return Err(PluginError::new(
            codes::E_PLUGIN_0009,
            format!("插件 {id} 配置要求签名，但未提供分离签名"),
        )
        .with_hint(
            "随制品分发发布方对 .wasm 的 detached 签名（如 <plugin>.wasm.sig）；\
             签名与 SRI 必须同时更新",
        ));
    };
    let verifier = ArtifactVerifier::from_reference(policy.trust_root, policy.backends)
        .map_err(|err| trust_root_error(id, policy.trust_root, &err))?;
    verifier.verify_detached(bytes, detached).map_err(|_| {
        PluginError::new(
            codes::E_PLUGIN_0009,
            format!("插件 {id} 的分离签名校验失败（签名与制品或信任根不匹配）"),
        )
        .with_hint("用发布方信任根对应的私钥重新签名；篡改后的制品即使同步更新 SRI 也必须被拒绝")
    })
}

/// 信任根错误 → 固定分类文案（不拼接底层 stderr / 临时路径，§7 脱敏）。
fn trust_root_error(id: &str, root: &CredentialRef, err: &SignError) -> PluginError {
    let reason = match err {
        SignError::ToolMissing { .. } => "宿主缺少 gpg（验签工具）",
        SignError::CredentialUnavailable { .. } => "凭据后端不可用或条目缺失",
        SignError::CredentialInvalid { .. } => "载荷不是合法的 public_key 公钥载荷",
        SignError::ToolFailed { .. } => "公钥导入失败（载荷可能不是有效公钥）",
        SignError::Io { .. } => "一次性验签目录读写失败",
    };
    PluginError::new(
        codes::E_PLUGIN_0010,
        format!("插件 {id} 的签名信任根 {} 不可用：{reason}", root.name),
    )
    .with_hint(
        "信任根只存公钥：把 ASCII-armor 公钥按 base64(JSON) \
         {\"public_key\":\"<base64>\"} 写入 keyring / Vault / CI 环境变量后重试",
    )
}
