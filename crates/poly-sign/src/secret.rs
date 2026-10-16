//! 凭据解析层（T-M2-11）：`CredentialRef` → 后端链 → [`SigningMaterial`]。
//!
//! 后端载荷契约：**base64(JSON)**
//! `{"keystore":"<base64>","store_password":"…","key_alias"?,"key_password"?}`；
//! `key_alias` 省略时用密钥库内唯一别名（apksigner 行为），`key_password` 省略时回退
//! `store_password`。HarmonyOS（T-M3-04）在其上扩展两个可选字段：
//! `"app_cert"`（应用证书链，PEM 字节的 base64）与 `"profile"`（已签名 Profile，p7b 字节的
//! base64）——两者对 HAP 签名链是必需，由 `poly-sign::harmony` 在使用点校验。
//! 凭据值只存在于内存：绝不进日志 / 事件流 / 命令行 / `Debug` 输出。
//!
//! 后端注入是验收要求（keyring / Vault / CI 内存注入多实现可替换），
//! [`resolve`] 按 `CredentialRef.source` 从后端链里选实现。
//!
//! 缓行（lead 裁定，M2+ 签名线增强）：`CredentialSource::File { path, password_env, alias }`
//! 不在 M2 落——Env / Keyring / Vault 已覆盖 T-M2-09/11 验收，而 `poly-sign/src/ios.rs` 对
//! `CredentialSource` 是穷尽 match，中途加变体会同时打断 iOS 线。**冻结后**三方单批落：
//! poly-core 变体（含序列化往返测试）+ ios.rs 一行分支 + 此处一个分支。

use std::collections::HashMap;

use base64::Engine as _;
use poly_core::{CredentialRef, CredentialSource};
use serde::Deserialize;

use crate::error::SignError;

/// 系统密钥库后端的 id（`CredentialSource::Keyring`）。
pub const KEYRING_ID: &str = "keyring";
/// Vault 后端的 id（`CredentialSource::Vault`）。
pub const VAULT_ID: &str = "vault";
/// 环境变量后端的 id（`CredentialSource::Env`）。
pub const ENV_ID: &str = "env";
/// 内存后端的 id（CI 注入 / 测试桩；可用 [`MemoryBackend::with_id`] 冒充其它后端）。
pub const MEMORY_ID: &str = "memory";

/// 签名材料（内存态，用后即弃）。
///
/// `Debug` 手写脱敏（不派生）：任何 `format!("{:?}")` / 事件流序列化都不得带出口令。
pub struct SigningMaterial {
    /// PKCS12 或 JKS 的完整字节。
    pub keystore: Vec<u8>,
    /// 密钥库口令（绝不进日志 / 命令行 / 事件流）。
    pub store_password: String,
    /// 密钥别名；省略时 apksigner 用密钥库内唯一别名。
    pub key_alias: Option<String>,
    /// 密钥口令；省略时回退 `store_password`。
    pub key_password: Option<String>,
    /// HarmonyOS 应用证书链（载荷 `app_cert` 字段；Android / iOS 载荷为 `None`）。
    pub app_cert: Option<Vec<u8>>,
    /// HarmonyOS 已签名 Profile（载荷 `profile` 字段；Android / iOS 载荷为 `None`）。
    pub profile: Option<Vec<u8>>,
}

impl std::fmt::Debug for SigningMaterial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size = |blob: &Option<Vec<u8>>| blob.as_ref().map(|bytes| format!("<{} 字节>", bytes.len()));
        f.debug_struct("SigningMaterial")
            .field("keystore", &format_args!("<{} 字节>", self.keystore.len()))
            .field("store_password", &"<脱敏>")
            .field("key_alias", &self.key_alias)
            .field(
                "key_password",
                &self.key_password.as_ref().map(|_| "<脱敏>"),
            )
            .field("app_cert", &size(&self.app_cert))
            .field("profile", &size(&self.profile))
            .finish()
    }
}

/// 凭据后端：`id()` 对应 `CredentialSource` 的变体，`fetch` 返回 base64(JSON) 载荷原文。
pub trait SecretBackend: Send + Sync {
    /// 后端标识（[`KEYRING_ID`] / [`VAULT_ID`] / [`ENV_ID`] / [`MEMORY_ID`]）。
    fn id(&self) -> &'static str;
    /// 可用性探测：不可用必须返回带修复提示的错误，绝不 panic。
    fn available(&self) -> Result<(), SignError>;
    /// 拉取载荷原文（base64(JSON)）；条目缺失同样返回明确错误。
    fn fetch(&self, name: &str) -> Result<Vec<u8>, SignError>;
}

/// 按 `CredentialRef.source` 选取后端并解析出签名材料。
///
/// 每次调用都重新拉取（`plan()` / `sign()` 都不复用临时材料）：对齐
/// 「`poly retry --from sign` 独立重跑并重新拉取凭据」的验收路径。
pub fn resolve(
    reference: &CredentialRef,
    backends: &[Box<dyn SecretBackend>],
) -> Result<SigningMaterial, SignError> {
    // 本 match 是穷尽的（无 `_ =>` 兜底）：poly-core 加 `CredentialSource` 变体会在此处编译失败，
    // 强制本层同步——M2 缓行 `File { path, password_env, alias }` 的理由见文件头。
    match &reference.source {
        CredentialSource::Keyring => material_from(
            backend_by_id(backends, KEYRING_ID, &reference.name)?,
            reference,
        ),
        CredentialSource::Vault => material_from(
            backend_by_id(backends, VAULT_ID, &reference.name)?,
            reference,
        ),
        CredentialSource::Env { variable } => {
            material_from(&EnvBackend::new(variable.clone()), reference)
        }
    }
}

/// 默认后端链：系统密钥库。
///
/// Vault（需要 `VaultClient` 接线）与 Env（变量名来自 `CredentialSource::Env`）由调用方
/// 注入，见 [`VaultBackend`] / [`EnvBackend`]；CI 无密钥库时用 [`MemoryBackend`] 注入。
pub fn default_backends() -> Vec<Box<dyn SecretBackend>> {
    vec![Box::new(KeyringBackend::new())]
}

/// 按 id 从后端链里找实现；缺失即「凭据不可用」（稳定错误码 + 修复提示）。
fn backend_by_id<'a>(
    backends: &'a [Box<dyn SecretBackend>],
    id: &'static str,
    name: &str,
) -> Result<&'a dyn SecretBackend, SignError> {
    let found = backends
        .iter()
        .map(Box::as_ref)
        .find(|backend| backend.id() == id);
    found.ok_or_else(|| SignError::CredentialUnavailable {
        name: name.to_string(),
        backend: id,
        reason: format!("后端链中没有 {id} 后端"),
    })
}

/// 探测 → 拉取 → 解析载荷（顺序固定：先给出「不可用」而不是「载荷非法」）。
fn material_from(
    backend: &dyn SecretBackend,
    reference: &CredentialRef,
) -> Result<SigningMaterial, SignError> {
    backend.available()?;
    let payload = backend.fetch(&reference.name)?;
    material_from_payload(&reference.name, backend.id(), &payload)
}

/// 解析 base64(JSON) 载荷。
fn material_from_payload(
    name: &str,
    backend: &'static str,
    payload: &[u8],
) -> Result<SigningMaterial, SignError> {
    let invalid = |reason: String| SignError::CredentialInvalid {
        name: name.to_string(),
        backend,
        reason,
    };
    let json = b64()
        .decode(payload)
        .map_err(|err| invalid(format!("载荷不是合法 base64：{err}")))?;
    // 只回传 serde 的行列位置，不回传载荷内容（脱敏 + 避免把 JSON 片段带进事件流）。
    let parsed: Payload = serde_json::from_slice(&json).map_err(|err| {
        invalid(format!(
            "载荷不是合法 JSON 结构（第 {} 行第 {} 列）",
            err.line(),
            err.column()
        ))
    })?;
    let keystore = b64()
        .decode(&parsed.keystore)
        .map_err(|err| invalid(format!("keystore 字段不是合法 base64：{err}")))?;
    if keystore.is_empty() {
        return Err(invalid("keystore 字段为空".to_string()));
    }
    if parsed.store_password.is_empty() {
        return Err(invalid("store_password 字段为空".to_string()));
    }
    let decode_optional = |field: &str, value: &Option<String>| -> Result<Option<Vec<u8>>, SignError> {
        let Some(encoded) = value else {
            return Ok(None);
        };
        let bytes = b64()
            .decode(encoded)
            .map_err(|err| invalid(format!("{field} 字段不是合法 base64：{err}")))?;
        if bytes.is_empty() {
            return Err(invalid(format!("{field} 字段为空")));
        }
        Ok(Some(bytes))
    };
    Ok(SigningMaterial {
        keystore,
        store_password: parsed.store_password,
        key_alias: parsed.key_alias,
        key_password: parsed.key_password,
        app_cert: decode_optional("app_cert", &parsed.app_cert)?,
        profile: decode_optional("profile", &parsed.profile)?,
    })
}

/// 载荷 JSON 结构（只读契约；未知字段忽略，便于后续扩展）。
#[derive(Deserialize)]
struct Payload {
    /// keystore 字节的 base64（PKCS12 或 JKS）。
    keystore: String,
    /// 密钥库口令。
    store_password: String,
    /// 密钥别名（可省）。
    #[serde(default)]
    key_alias: Option<String>,
    /// 密钥口令（可省，回退 `store_password`）。
    #[serde(default)]
    key_password: Option<String>,
    /// HarmonyOS 应用证书链的 base64（可省；HAP 签名链必需，见 `crate::harmony`）。
    #[serde(default)]
    app_cert: Option<String>,
    /// HarmonyOS 已签名 Profile 的 base64（可省；HAP 签名链必需）。
    #[serde(default)]
    profile: Option<String>,
}

/// 标准字母表 base64。
fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

/// 环境变量后端（`CredentialSource::Env`）：变量值即 base64(JSON) 载荷（CI 注入）。
pub struct EnvBackend {
    /// 环境变量名（来自 `CredentialSource::Env.variable`）。
    variable: String,
}

impl EnvBackend {
    /// 绑定环境变量名。
    pub fn new(variable: impl Into<String>) -> Self {
        Self {
            variable: variable.into(),
        }
    }
}

impl SecretBackend for EnvBackend {
    fn id(&self) -> &'static str {
        ENV_ID
    }

    fn available(&self) -> Result<(), SignError> {
        if std::env::var_os(&self.variable).is_some() {
            return Ok(());
        }
        Err(SignError::CredentialUnavailable {
            name: self.variable.clone(),
            backend: ENV_ID,
            reason: format!(
                "环境变量 {} 不存在（CI 请注入该变量，值 = base64(JSON) 载荷）",
                self.variable
            ),
        })
    }

    fn fetch(&self, name: &str) -> Result<Vec<u8>, SignError> {
        match std::env::var(&self.variable) {
            Ok(value) if !value.is_empty() => Ok(value.into_bytes()),
            Ok(_) => Err(SignError::CredentialUnavailable {
                name: name.to_string(),
                backend: ENV_ID,
                reason: format!("环境变量 {} 为空", self.variable),
            }),
            Err(err) => Err(SignError::CredentialUnavailable {
                name: name.to_string(),
                backend: ENV_ID,
                reason: format!("环境变量 {} 不可读：{err}", self.variable),
            }),
        }
    }
}

/// 内存后端（CI 注入 / 测试桩）：构造时直接给载荷，不触碰任何持久化存储。
pub struct MemoryBackend {
    /// 对外声明的后端 id（默认 [`MEMORY_ID`]，可冒充 `keyring` / `vault`）。
    id: &'static str,
    /// 条目：档案名 → base64(JSON) 载荷原文。
    entries: HashMap<String, Vec<u8>>,
}

impl MemoryBackend {
    /// 空内存后端。
    pub fn new() -> Self {
        Self {
            id: MEMORY_ID,
            entries: HashMap::new(),
        }
    }

    /// 带一条载荷的内存后端（`payload` 为 base64(JSON) 原文）。
    pub fn with_entry(name: impl Into<String>, payload: impl Into<Vec<u8>>) -> Self {
        let mut backend = Self::new();
        backend.entries.insert(name.into(), payload.into());
        backend
    }

    /// 改写后端 id：同一载荷可分别作为 `keyring` / `vault` 后端注入。
    pub fn with_id(mut self, id: &'static str) -> Self {
        self.id = id;
        self
    }
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretBackend for MemoryBackend {
    fn id(&self) -> &'static str {
        self.id
    }

    fn available(&self) -> Result<(), SignError> {
        Ok(())
    }

    fn fetch(&self, name: &str) -> Result<Vec<u8>, SignError> {
        self.entries
            .get(name)
            .cloned()
            .ok_or_else(|| SignError::CredentialUnavailable {
                name: name.to_string(),
                backend: self.id,
                reason: "内存后端未注入该条目".to_string(),
            })
    }
}

/// 系统密钥库后端（`CredentialSource::Keyring`）。
///
/// 按平台走 keyring v3 的原生后端（见 `Cargo.toml`：Linux = Secret Service / zbus 纯 Rust，
/// macOS = Keychain，Windows = Credential Manager）。headless 环境探测失败只报错不 panic，
/// 提示改用 Env / Memory 注入。
pub struct KeyringBackend {
    /// 服务名（条目按「服务 + 用户名」定位，用户名 = `CredentialRef.name`）。
    service: String,
}

impl KeyringBackend {
    /// 默认服务名。
    pub const SERVICE: &'static str = "polypack";
    /// 可用性探测的条目名（只读；读不到恰好说明服务可达）。
    const PROBE: &'static str = "polypack-probe";

    /// 默认服务名（`polypack`）的后端。
    pub fn new() -> Self {
        Self {
            service: Self::SERVICE.to_string(),
        }
    }

    /// 读一个条目：`Ok(None)` = 服务可达但无该条目；`Err` = 后端不可用。
    fn read(&self, name: &str) -> Result<Option<String>, SignError> {
        let unavailable = |err: keyring::Error| SignError::CredentialUnavailable {
            name: name.to_string(),
            backend: KEYRING_ID,
            reason: err.to_string(),
        };
        let entry = keyring::Entry::new(&self.service, name).map_err(unavailable)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(unavailable(err)),
        }
    }
}

impl Default for KeyringBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretBackend for KeyringBackend {
    fn id(&self) -> &'static str {
        KEYRING_ID
    }

    fn available(&self) -> Result<(), SignError> {
        // 探测条目在不在都说明服务可达；不可达（无 Secret Service / 无凭证）由 read 报错。
        self.read(Self::PROBE).map(|_| ())
    }

    fn fetch(&self, name: &str) -> Result<Vec<u8>, SignError> {
        match self.read(name)? {
            Some(secret) => Ok(secret.into_bytes()),
            None => Err(SignError::CredentialUnavailable {
                name: name.to_string(),
                backend: KEYRING_ID,
                reason: "密钥库中无该条目（先写入 base64(JSON) 载荷）".to_string(),
            }),
        }
    }
}

/// 外部 Vault 客户端抽象（`CredentialSource::Vault`）。
///
/// 真实 HTTP 实现是 [`crate::vault::HttpVaultClient`]（KV v2，`VAULT_ADDR` / `VAULT_TOKEN`）；
/// 本 trait + [`VaultBackend`] 适配器是它的接缝：单元测试用内存假实现（**默认测试绝不联网**），
/// 真服务器的读写往返在 `tests/vault_real.rs`（`POLY_VAULT_REAL=1` 门控）。
pub trait VaultClient: Send + Sync {
    /// 按凭据档案名读取 base64(JSON) 载荷原文。
    fn read(&self, name: &str) -> Result<Vec<u8>, SignError>;
}

/// Vault 后端：把 [`VaultClient`] 适配成 [`SecretBackend`]。
pub struct VaultBackend<C: VaultClient> {
    /// 注入的客户端。
    client: C,
}

impl<C: VaultClient> VaultBackend<C> {
    /// 用给定客户端构造。
    pub fn new(client: C) -> Self {
        Self { client }
    }
}

impl<C: VaultClient> SecretBackend for VaultBackend<C> {
    fn id(&self) -> &'static str {
        VAULT_ID
    }

    fn available(&self) -> Result<(), SignError> {
        // 客户端能构造即视为可用；可达性由 read 的返回值表达。
        Ok(())
    }

    fn fetch(&self, name: &str) -> Result<Vec<u8>, SignError> {
        self.client.read(name)
    }
}
