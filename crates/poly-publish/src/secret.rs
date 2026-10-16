//! 发布凭据解析层（T-M3-09 / T-M3-10）：`CredentialRef` → 后端链 → 平台凭据。
//!
//! 契约与 T-M2-11（`poly-sign::secret`）同源：后端载荷 = **base64(JSON)**，字段按平台约定：
//! - 微信：`{"appid":"…","secret":"…","private_key":"<PEM 代码上传密钥>"}`；
//! - 支付宝：`{"app_id":"…","private_key":"<PEM RSA2 应用私钥>","app_auth_token"?:"…"}`；
//! - 抖音：`{"access_token":"…"}`（token 由外部链路获取后注入——铸造链路口径互斥、未实测，
//!   见 [`DouyinCredentials`]）。
//!
//! 依赖方向约束（`crates/poly-cli/tests/dependency_direction.rs`）只允许本 crate 依赖
//! `poly-core`，故不复用 poly-sign 的后端链：这里定义同形的 [`CredentialProvider`] 接缝 +
//! 自带 [`MemoryProvider`]（CI 注入 / 测试桩）与 [`EnvProvider`]（环境变量注入）；
//! keyring / Vault 由 CLI 侧以桥接实现注入（语义同 T-M2-11：按引用现取、用后即弃）。
//!
//! **脱敏**：凭据值只存在于内存；`Debug` 一律手写脱敏；取自传输层 / 平台响应的任何文本
//! 在进错误前须经 [`Redactor`]。

use std::collections::HashMap;

use base64::Engine as _;
use poly_core::{CredentialRef, CredentialSource};
use serde::Deserialize;

use crate::error::PublishError;

/// 内存后端的 id（CI 注入 / 测试桩）。
pub const MEMORY_ID: &str = "memory";
/// 环境变量后端的 id（`CredentialSource::Env`）。
pub const ENV_ID: &str = "env";

/// 凭据后端：按 [`CredentialRef`] 拉取 base64(JSON) 载荷原文。
///
/// 不宜服务的引用（如 Env 后端遇到 Keyring 引用）返回
/// [`PublishError::CredentialUnavailable`]，让调用方继续走链上的下一个后端。
pub trait CredentialProvider: Send + Sync {
    /// 后端标识（[`MEMORY_ID`] / [`ENV_ID`] / 桥接实现自报）。
    fn id(&self) -> &'static str;
    /// 拉取载荷原文（base64(JSON)）；条目缺失 / 变量未设同样返回明确错误。
    fn payload(&self, reference: &CredentialRef) -> Result<Vec<u8>, PublishError>;
}

/// 按后端链顺序解析载荷；返回（载荷原文, 命中的后端 id）。
pub fn resolve_payload(
    reference: &CredentialRef,
    providers: &[Box<dyn CredentialProvider>],
) -> Result<(Vec<u8>, &'static str), PublishError> {
    if providers.is_empty() {
        return Err(PublishError::CredentialUnavailable {
            name: reference.name.clone(),
            providers: "（空）".to_string(),
            reason: "凭据链为空：请注入 memory / env 或桥接后端（keyring / Vault）".to_string(),
        });
    }
    let mut ids = Vec::new();
    let mut reasons = Vec::new();
    for provider in providers {
        ids.push(provider.id());
        match provider.payload(reference) {
            Ok(payload) => return Ok((payload, provider.id())),
            // 「本后端不适用 / 条目缺失」→ 继续走链；其余错误（如载荷非法）原样上抛
            Err(PublishError::CredentialUnavailable { reason, .. }) => {
                reasons.push(format!("{}: {reason}", provider.id()));
            }
            Err(other) => return Err(other),
        }
    }
    Err(PublishError::CredentialUnavailable {
        name: reference.name.clone(),
        providers: ids.join(" / "),
        reason: reasons.join("；"),
    })
}

/// 内存后端：构造时直接给载荷，不触碰任何持久化存储。
pub struct MemoryProvider {
    /// 条目：档案名 → base64(JSON) 载荷原文。
    entries: HashMap<String, Vec<u8>>,
}

impl MemoryProvider {
    /// 空内存后端。
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// 带一条载荷的内存后端（`payload` 为 base64(JSON) 原文）。
    pub fn with_entry(name: impl Into<String>, payload: impl Into<Vec<u8>>) -> Self {
        let mut provider = Self::new();
        provider.entries.insert(name.into(), payload.into());
        provider
    }
}

impl Default for MemoryProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialProvider for MemoryProvider {
    fn id(&self) -> &'static str {
        MEMORY_ID
    }

    fn payload(&self, reference: &CredentialRef) -> Result<Vec<u8>, PublishError> {
        self.entries.get(&reference.name).cloned().ok_or_else(|| {
            PublishError::CredentialUnavailable {
                name: reference.name.clone(),
                providers: MEMORY_ID.to_string(),
                reason: "内存后端未注入该条目".to_string(),
            }
        })
    }
}

/// 环境变量后端（`CredentialSource::Env`）：变量值即 base64(JSON) 载荷（CI 注入）。
pub struct EnvProvider;

impl CredentialProvider for EnvProvider {
    fn id(&self) -> &'static str {
        ENV_ID
    }

    fn payload(&self, reference: &CredentialRef) -> Result<Vec<u8>, PublishError> {
        let CredentialSource::Env { variable } = &reference.source else {
            return Err(PublishError::CredentialUnavailable {
                name: reference.name.clone(),
                providers: ENV_ID.to_string(),
                reason: "凭据来源不是 env".to_string(),
            });
        };
        match std::env::var(variable) {
            Ok(value) if !value.is_empty() => Ok(value.into_bytes()),
            Ok(_) => Err(PublishError::CredentialUnavailable {
                name: reference.name.clone(),
                providers: ENV_ID.to_string(),
                reason: format!("环境变量 {variable} 为空"),
            }),
            Err(err) => Err(PublishError::CredentialUnavailable {
                name: reference.name.clone(),
                providers: ENV_ID.to_string(),
                reason: format!("环境变量 {variable} 不可读：{err}"),
            }),
        }
    }
}

/// 凭据值脱敏器：把已知凭据值从任意文本里替换掉（传输层 / 平台响应文本进错误前的关口）。
pub struct Redactor {
    /// 需抹除的值（空值与过短值不收录——过短值的替换会误伤正常文本）。
    secrets: Vec<String>,
}

impl Redactor {
    /// 从候选凭据值构造（过滤空值与 < 6 字节的值）。
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(secrets: I) -> Self {
        Self {
            secrets: secrets
                .into_iter()
                .map(Into::into)
                .filter(|value| value.len() >= 6)
                .collect(),
        }
    }

    /// 追加一个待脱敏值（如运行中取得的 access_token；同样过滤空值与 < 6 字节的值）。
    pub fn add(&mut self, secret: impl Into<String>) {
        let secret = secret.into();
        if secret.len() >= 6 {
            self.secrets.push(secret);
        }
    }

    /// 抹除文本中出现的全部凭据值。
    pub fn redact(&self, text: &str) -> String {
        let mut out = text.to_string();
        for secret in &self.secrets {
            out = out.replace(secret.as_str(), "<脱敏>");
        }
        out
    }

    /// 递归抹除 JSON 里的凭据值（平台响应快照进回执落盘前的关口）。
    pub fn redact_value(&self, value: &serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match value {
            Value::String(text) => Value::String(self.redact(text)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| self.redact_value(item)).collect())
            }
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, item)| (key.clone(), self.redact_value(item)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

// ---------------------------------------------------------------- 平台凭据

/// 微信凭据（自研账号 CI 模式：AppSecret 换 access_token + 代码上传密钥签名上传）。
///
/// `Debug` 手写脱敏（不派生）：任何 `format!("{:?}")` 都不得带出口令 / 私钥。
pub struct WechatCredentials {
    /// 小程序 AppID。
    pub appid: String,
    /// 小程序 AppSecret（换 access_token 用；绝不进日志 / 错误 / 回执）。
    pub secret: String,
    /// 代码上传密钥（PEM；`--pkp` 同物，签名上传请求用）。
    pub private_key: String,
    /// 命中的后端 id（诊断用；载荷来源的如实记录）。
    pub source: &'static str,
}

impl std::fmt::Debug for WechatCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WechatCredentials")
            .field("appid", &self.appid)
            .field("secret", &"<脱敏>")
            .field("private_key", &"<脱敏>")
            .field("source", &self.source)
            .finish()
    }
}

impl WechatCredentials {
    /// 本凭据的脱敏器（覆盖 secret 与 private_key）。
    pub fn redactor(&self) -> Redactor {
        Redactor::new([self.secret.clone(), self.private_key.clone()])
    }
}

/// 支付宝凭据（开放平台网关：RSA2 应用私钥签名）。
pub struct AlipayCredentials {
    /// 应用 app_id。
    pub app_id: String,
    /// 应用私钥（PEM，RSA2 签名用；绝不进日志 / 错误 / 回执）。
    pub private_key: String,
    /// 第三方应用授权 token（`app_auth_token`，ISV 场景可省）。
    pub app_auth_token: Option<String>,
    /// 命中的后端 id（诊断用）。
    pub source: &'static str,
}

impl std::fmt::Debug for AlipayCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlipayCredentials")
            .field("app_id", &self.app_id)
            .field("private_key", &"<脱敏>")
            .field(
                "app_auth_token",
                &self.app_auth_token.as_ref().map(|_| "<脱敏>"),
            )
            .field("source", &self.source)
            .finish()
    }
}

impl AlipayCredentials {
    /// 本凭据的脱敏器（覆盖 private_key 与 app_auth_token）。
    pub fn redactor(&self) -> Redactor {
        Redactor::new([
            self.private_key.clone(),
            self.app_auth_token.clone().unwrap_or_default(),
        ])
    }
}

/// 抖音凭据（**access_token 外部注入**）。
///
/// 铸造链路未确证：官方文档在「自研小程序 `developer.toutiao.com/api/apps/v2/token`」与
/// 「服务商 tpapp（两个候选 URL）」间口径互斥，且未实测——故客户端绝不自造 token，
/// 只消费注入值（never fake）；token 铸造 / 刷新由外部链路（含 28001008 过期后重取）负责。
pub struct DouyinCredentials {
    /// 开放平台 access_token（`access-token` 头；绝不进日志 / 错误 / 回执）。
    pub access_token: String,
    /// 命中的后端 id（诊断用）。
    pub source: &'static str,
}

impl std::fmt::Debug for DouyinCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DouyinCredentials")
            .field("access_token", &"<脱敏>")
            .field("source", &self.source)
            .finish()
    }
}

impl DouyinCredentials {
    /// 本凭据的脱敏器（覆盖 access_token）。
    pub fn redactor(&self) -> Redactor {
        Redactor::new([self.access_token.clone()])
    }
}

/// 解析微信凭据（载荷字段约定见模块头）。
pub fn wechat_credentials(
    reference: &CredentialRef,
    providers: &[Box<dyn CredentialProvider>],
) -> Result<WechatCredentials, PublishError> {
    #[derive(Deserialize)]
    struct Payload {
        appid: String,
        secret: String,
        private_key: String,
    }
    let (payload, provider) = parse::<Payload>(reference, providers)?;
    require_all(
        reference,
        provider,
        [
            ("appid", &payload.appid),
            ("secret", &payload.secret),
            ("private_key", &payload.private_key),
        ],
    )?;
    Ok(WechatCredentials {
        appid: payload.appid,
        secret: payload.secret,
        private_key: payload.private_key,
        source: provider,
    })
}

/// 解析支付宝凭据（载荷字段约定见模块头）。
pub fn alipay_credentials(
    reference: &CredentialRef,
    providers: &[Box<dyn CredentialProvider>],
) -> Result<AlipayCredentials, PublishError> {
    #[derive(Deserialize)]
    struct Payload {
        app_id: String,
        private_key: String,
        #[serde(default)]
        app_auth_token: Option<String>,
    }
    let (payload, provider) = parse::<Payload>(reference, providers)?;
    require_all(
        reference,
        provider,
        [
            ("app_id", &payload.app_id),
            ("private_key", &payload.private_key),
        ],
    )?;
    Ok(AlipayCredentials {
        app_id: payload.app_id,
        private_key: payload.private_key,
        app_auth_token: payload.app_auth_token,
        source: provider,
    })
}

/// 解析抖音凭据（载荷字段约定见模块头：`{"access_token":"…"}`）。
pub fn douyin_credentials(
    reference: &CredentialRef,
    providers: &[Box<dyn CredentialProvider>],
) -> Result<DouyinCredentials, PublishError> {
    #[derive(Deserialize)]
    struct Payload {
        access_token: String,
    }
    let (payload, provider) = parse::<Payload>(reference, providers)?;
    require_all(
        reference,
        provider,
        [("access_token", &payload.access_token)],
    )?;
    Ok(DouyinCredentials {
        access_token: payload.access_token,
        source: provider,
    })
}

/// 解析 base64(JSON) 载荷（只回传形态信息，绝不回传载荷内容）。
///
/// 两段解析：先 `Value`（语法错误的消息只有解析位置、不带文档值，安全直出），再进平台结构
/// （`from_value` 的类型错误消息**会回显字段值**——如 `invalid type: string "…"`——故只有
/// `missing field` 形态原样透传，其余归为一句不带值的类型提示）。
///
/// 返回（载荷, 命中的后端 id）——后端 id 继续传给 [`require_all`]，错误里点名准确来源。
fn parse<T: serde::de::DeserializeOwned>(
    reference: &CredentialRef,
    providers: &[Box<dyn CredentialProvider>],
) -> Result<(T, &'static str), PublishError> {
    let (payload, provider) = resolve_payload(reference, providers)?;
    let invalid = |reason: String| PublishError::CredentialInvalid {
        name: reference.name.clone(),
        provider,
        reason,
    };
    let json = b64()
        .decode(&payload)
        .map_err(|err| invalid(format!("载荷不是合法 base64：{err}")))?;
    let value: serde_json::Value = serde_json::from_slice(&json)
        .map_err(|err| invalid(format!("载荷不是合法 JSON：{err}")))?;
    let parsed = serde_json::from_value::<T>(value).map_err(|err| {
        let text = err.to_string();
        if text.starts_with("missing field") {
            invalid(format!("载荷缺字段（{text}）"))
        } else {
            // 类型错误原文带字段值（`invalid type: string "…"`），只能给不带值的提示。
            invalid(
                "载荷不是可识别的 JSON 结构（字段须按模块头的载荷契约给非空字符串）".to_string(),
            )
        }
    })?;
    Ok((parsed, provider))
}

/// 必填字段非空校验（点名缺哪个字段，不回显其它内容）。
fn require_all<'a, I: IntoIterator<Item = (&'a str, &'a String)>>(
    reference: &CredentialRef,
    provider: &'static str,
    fields: I,
) -> Result<(), PublishError> {
    for (name, value) in fields {
        if value.trim().is_empty() {
            return Err(PublishError::CredentialInvalid {
                name: reference.name.clone(),
                provider,
                reason: format!("字段 {name} 为空"),
            });
        }
    }
    Ok(())
}

/// 标准字母表 base64。
fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}
