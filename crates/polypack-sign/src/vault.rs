//! 真实 Vault HTTP 客户端（KV v2）——`CredentialSource::Vault` 的生产接线（T-M2-11）。
//!
//! 约定：
//! - `VAULT_ADDR`（如 `http://127.0.0.1:8200`）与 `VAULT_TOKEN` 必填；`VAULT_CACERT`
//!   可选（私有 CA / 自签时给 PEM 路径，标准 Vault 客户端约定）；
//! - KV v2 挂载点 `secret`，API 路径 `{addr}/v1/secret/data/{凭据档案名}`（档案名含 `/`
//!   即嵌套路径，段不得为空 / `.` / `..`）；
//! - 载荷放字段 [`PAYLOAD_FIELD`]，值为 base64(JSON) 原文——与 keyring / Env 后端同一
//!   契约（见 [`crate::secret`] 文件头），本层只搬字节不解析。
//!
//! **脱敏**：token 只从环境进内存、只进 `X-Vault-Token` 请求头；[`HttpVaultClient`] 的
//! `Debug` 手写为 `<脱敏>`；服务端错误正文回显 token 时在进 reason 前被替换（错误会进
//! 日志 / 事件流，这条是硬要求）。
//!
//! TLS：关掉 ureq 自带的根证书（`webpki-roots` / `webpki-root-certs` 许可证不在仓库
//! `deny.toml` 白名单），改由本层显式给 [`RootCerts::Specific`]——`VAULT_CACERT` 优先，
//! 否则 `https://` **才**读系统信任库（`rustls-native-certs`，纯 Rust，无 *-sys /
//! openssl；企业内网 CA 生效），`http://` 不读（dev 服务器 / 无根证书的机器照样能用）。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ureq::tls::{Certificate, PemItem, RootCerts, TlsConfig, parse_pem};

use crate::error::SignError;
use crate::secret::{VAULT_ID, VaultClient};

/// Vault 服务地址环境变量（如 `http://127.0.0.1:8200`）。
pub const ADDR_ENV: &str = "VAULT_ADDR";
/// Vault token 环境变量（只进请求头与内存，绝不进输出）。
pub const TOKEN_ENV: &str = "VAULT_TOKEN";
/// 私有 CA / 自签证书 PEM 路径环境变量（可选）。
pub const CACERT_ENV: &str = "VAULT_CACERT";
/// KV v2 挂载点（API 路径 `{addr}/v1/{KV_MOUNT}/data/{名}`）。
pub const KV_MOUNT: &str = "secret";
/// 载荷字段名（值为 base64(JSON) 原文）。
pub const PAYLOAD_FIELD: &str = "payload";

/// 单次请求整体超时：构建链路不因后端挂起而吊死。
const TIMEOUT: Duration = Duration::from_secs(10);
/// 服务端错误正文进 reason 前的字符数（尾部；同 polypack-bundle 的 stderr 尾部口径）。
const BODY_TAIL: usize = 300;

/// 真 HTTP Vault 客户端（KV v2 读 / 写）。
pub struct HttpVaultClient {
    /// 规范化后的地址（去尾斜杠）。
    addr: String,
    /// token（脱敏边界见模块头）。
    token: String,
    /// 复用连接的 agent（含超时与 TLS 配置）。
    agent: ureq::Agent,
}

impl HttpVaultClient {
    /// 从 `VAULT_ADDR` / `VAULT_TOKEN`（可选 `VAULT_CACERT`）构造。
    pub fn from_env() -> Result<Self, SignError> {
        let addr = env_value(ADDR_ENV);
        let token = env_value(TOKEN_ENV);
        let cacert = env_value(CACERT_ENV);
        Self::new(
            &addr.ok_or_else(|| env_missing(ADDR_ENV, "如 http://127.0.0.1:8200"))?,
            &token.ok_or_else(|| {
                env_missing(
                    TOKEN_ENV,
                    "带 secret/data/<名> 读权限的 token（dev 服务器用其启动横幅里的 root token）",
                )
            })?,
            cacert.as_deref().map(Path::new),
        )
    }

    /// 显式构造（CLI 接线 / 真环境闭环用；`cacert_pem` 给私有 CA 的 PEM 路径）。
    pub fn new(addr: &str, token: &str, cacert_pem: Option<&Path>) -> Result<Self, SignError> {
        let addr = addr.trim().trim_end_matches('/').to_string();
        if addr.is_empty() {
            return Err(env_missing(ADDR_ENV, "如 http://127.0.0.1:8200"));
        }
        // 协议不静默降级：写错（https 写成 http）要明确报错，而不是把凭据请求发去明文端。
        if !(addr.starts_with("http://") || addr.starts_with("https://")) {
            return Err(config_err(format!(
                "Vault 地址须以 http:// 或 https:// 开头，当前是 {addr:?}"
            )));
        }
        // 根证书按需装配：显式 CA > （https 时）系统信任库 > 空（http 用不到 TLS，
        // 不在缺根证书的精简环境上拦下 dev 场景）。
        let roots = match cacert_pem {
            Some(path) => load_cacert(path)?,
            None if addr.starts_with("https://") => system_roots()?,
            None => Vec::new(),
        };
        let tls = TlsConfig::builder()
            .root_certs(RootCerts::Specific(Arc::new(roots)))
            .build();
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            // 非 2xx 也把响应交回来：Vault 的报错正文（errors[]）是修复依据。
            .http_status_as_error(false)
            .tls_config(tls)
            .build();
        Ok(Self {
            addr,
            token: token.trim().to_string(),
            agent: config.into(),
        })
    }

    /// KV v2 写入：base64(JSON) 载荷原文进字段 [`PAYLOAD_FIELD`]。
    ///
    /// 真环境闭环（写 → 经 [`crate::secret::VaultBackend`] 读回）与后续 CLI 播种用；
    /// 与 [`VaultClient::read`] 走同一条请求 / 脱敏 / 报错路径。
    pub fn put_payload(&self, name: &str, payload: &str) -> Result<(), SignError> {
        check_name(name)?;
        let mut data = serde_json::Map::new();
        data.insert(
            PAYLOAD_FIELD.to_string(),
            serde_json::Value::String(payload.to_string()),
        );
        let body = serde_json::json!({ "data": data }).to_string();
        self.call(name, Method::Post, &self.data_url(name), Some(body))
            .map(|_| ())
    }

    /// KV v2 删除（软删最新版本）：真环境负向路径用——删后读 404 → 后端不可用。
    pub fn delete_payload(&self, name: &str) -> Result<(), SignError> {
        check_name(name)?;
        self.call(name, Method::Delete, &self.data_url(name), None)
            .map(|_| ())
    }

    /// KV v2 数据面 URL（读 / 写 / 删同路径）。
    fn data_url(&self, name: &str) -> String {
        format!("{}/v1/{KV_MOUNT}/data/{}", self.addr, name)
    }

    /// 发一次请求并做状态 / 脱敏处理；2xx 返回正文。
    ///
    /// `get` / `post` 的构建器泛型不同（`WithoutBody` / `WithBody`），各自终结成
    /// 同一个 `Result<Response<Body>, Error>` 再合流。
    fn call(
        &self,
        name: &str,
        method: Method,
        url: &str,
        body: Option<String>,
    ) -> Result<String, SignError> {
        let result = match (method, body) {
            (Method::Post, Some(text)) => self
                .agent
                .post(url)
                .header("X-Vault-Token", &self.token)
                .header("Content-Type", "application/json")
                .send(text),
            (Method::Delete, _) => self
                .agent
                .delete(url)
                .header("X-Vault-Token", &self.token)
                .call(),
            _ => self
                .agent
                .get(url)
                .header("X-Vault-Token", &self.token)
                .call(),
        };
        self.finish(name, result)
    }

    /// 请求结果 → 正文 / 错误（含脱敏）。
    fn finish(
        &self,
        name: &str,
        result: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<String, SignError> {
        let mut response =
            result.map_err(|err| self.unavailable(name, format!("请求失败：{err}")))?;
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().unwrap_or_default();
        if (200..300).contains(&status) {
            Ok(text)
        } else {
            Err(self.unavailable(name, self.status_reason(status, &text)))
        }
    }

    /// 状态码 → 修复向的 reason（正文尾部经脱敏 / 截断）。
    fn status_reason(&self, status: u16, body: &str) -> String {
        let hint = match status {
            403 => "（token 无权读该路径：policy 需覆盖 secret/data/<名>）",
            404 => "（路径不存在：先播种条目，或核对 KV 挂载点是否 secret）",
            _ => "",
        };
        let tail = self.redact_tail(body);
        if tail.is_empty() {
            format!("Vault 返回 {status}{hint}")
        } else {
            format!("Vault 返回 {status}{hint}：{tail}")
        }
    }

    /// 服务端正文进日志前的脱敏 + 尾部截断。
    ///
    /// 防线理由：服务端（或中间人）若把请求头回显进错误正文，token 就会顺着 reason
    /// 进日志 / 事件流——这里先替换再截断。
    fn redact_tail(&self, text: &str) -> String {
        let redacted = if self.token.is_empty() {
            text.to_string()
        } else {
            text.replace(&self.token, "<脱敏>")
        };
        let count = redacted.chars().count();
        let tail: String = if count > BODY_TAIL {
            redacted.chars().skip(count - BODY_TAIL).collect()
        } else {
            redacted
        };
        tail.trim().to_string()
    }

    /// 组装本后端的不可用错误（name = 凭据档案名）。
    fn unavailable(&self, name: &str, reason: String) -> SignError {
        SignError::CredentialUnavailable {
            name: name.to_string(),
            backend: VAULT_ID,
            reason: format!("[{}] {reason}", self.addr),
        }
    }
}

impl VaultClient for HttpVaultClient {
    /// 读 `{addr}/v1/{KV_MOUNT}/data/{name}` 的 `data.data.payload`。
    fn read(&self, name: &str) -> Result<Vec<u8>, SignError> {
        check_name(name)?;
        let text = self.call(name, Method::Get, &self.data_url(name), None)?;
        let json: serde_json::Value = serde_json::from_str(&text).map_err(|err| {
            self.unavailable(name, format!("响应非 JSON（KV v2 期望 JSON 体）：{err}"))
        })?;
        let payload = json
            .pointer(&format!("/data/data/{PAYLOAD_FIELD}"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                self.unavailable(
                    name,
                    format!(
                        "KV 条目缺 `{PAYLOAD_FIELD}` 字段（应存 base64(JSON) 原文；\
                         路径 {}/{KV_MOUNT}/data/{name}）",
                        self.addr
                    ),
                )
            })?;
        Ok(payload.as_bytes().to_vec())
    }
}

impl std::fmt::Debug for HttpVaultClient {
    /// 手写脱敏（不派生）：`{:?}` 不得带 token。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpVaultClient")
            .field("addr", &self.addr)
            .field("token", &"<脱敏>")
            .finish_non_exhaustive()
    }
}

/// 请求方法（本模块只用到三种）。
enum Method {
    /// KV 读。
    Get,
    /// KV 写。
    Post,
    /// KV 软删最新版本。
    Delete,
}

/// 凭据档案名进 URL 路径前的校验（系统边界）：非空、段不得为空 / `.` / `..`。
fn check_name(name: &str) -> Result<(), SignError> {
    let bad = name.is_empty()
        || name
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..");
    if bad {
        return Err(SignError::CredentialInvalid {
            name: name.to_string(),
            backend: VAULT_ID,
            reason: "凭据档案名不能为空，且不能含空段 / `.` / `..`（它直接进 KV 路径）".to_string(),
        });
    }
    Ok(())
}

/// 读系统信任库根证书（`https://` 且未设 `VAULT_CACERT` 时）。
fn system_roots() -> Result<Vec<Certificate<'static>>, SignError> {
    let native = rustls_native_certs::load_native_certs();
    if native.certs.is_empty() {
        return Err(config_err(
            "系统信任库没有可用根证书（HTTPS 校验无从谈起）：用 VAULT_CACERT 指定 PEM，\
             或把 VAULT_ADDR 指向 http:// 的 dev 服务器"
                .to_string(),
        ));
    }
    Ok(native
        .certs
        .iter()
        .map(|der| Certificate::from_der(der.as_ref()).to_owned())
        .collect())
}

/// 读 PEM 证书文件（`VAULT_CACERT`）；取文件里**全部** CERTIFICATE 块（CA 链场景）。
fn load_cacert(path: &Path) -> Result<Vec<Certificate<'static>>, SignError> {
    let pem = std::fs::read(path).map_err(crate::error::io_err(path))?;
    let certs = parse_pem(&pem)
        .filter_map(|item| match item {
            Ok(PemItem::Certificate(cert)) => Some(Ok(cert)),
            Ok(_) => None, // 文件里混了私钥块等：跳过，只要证书
            Err(err) => Some(Err(err)),
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| config_err(format!("{} 解析失败：{err}", path.display())))?;
    if certs.is_empty() {
        return Err(config_err(format!(
            "{} 里没有 CERTIFICATE 块（VAULT_CACERT 应为 PEM 证书）",
            path.display()
        )));
    }
    Ok(certs)
}

/// 环境变量：非空才算有值（空串按未设置处理）。
fn env_value(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 「缺环境变量」错误（name 用变量名，日志里能直接看出缺哪枚）。
fn env_missing(var: &str, hint: &str) -> SignError {
    SignError::CredentialUnavailable {
        name: var.to_string(),
        backend: VAULT_ID,
        reason: format!("未设置（{hint}）"),
    }
}

/// 客户端配置错误。
fn config_err(reason: String) -> SignError {
    SignError::CredentialUnavailable {
        name: "<配置>".to_string(),
        backend: VAULT_ID,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> HttpVaultClient {
        HttpVaultClient::new("http://127.0.0.1:8200/", "s.token.value", None).expect("构造")
    }

    #[test]
    fn data_url_is_kv_v2_shape_without_double_slash() {
        let client = client();
        assert_eq!(
            client.data_url("pax-release"),
            "http://127.0.0.1:8200/v1/secret/data/pax-release"
        );
        // 嵌套路径（档案名含 `/`）原样进路径。
        assert_eq!(
            client.data_url("team/pax/release"),
            "http://127.0.0.1:8200/v1/secret/data/team/pax/release"
        );
    }

    #[test]
    fn bad_names_are_rejected_before_any_request() {
        for bad in ["", "/x", "x/", "a//b", ".", "..", "a/../b"] {
            let err = check_name(bad).expect_err("应拒绝");
            assert_eq!(err.code(), crate::codes::E_SIG_0002, "{bad:?}");
        }
        check_name("team/pax/release").expect("嵌套路径合法");
        check_name("pax-release").expect("平铺名合法");
    }

    #[test]
    fn bad_addr_and_missing_env_are_config_errors() {
        let err = HttpVaultClient::new("127.0.0.1:8200", "t", None).expect_err("无协议应拒绝");
        assert!(err.to_string().contains("http://"), "{err}");
        let err = HttpVaultClient::new("  ", "t", None).expect_err("空地址应拒绝");
        assert!(err.to_string().contains(ADDR_ENV), "{err}");
    }

    #[test]
    fn status_reasons_carry_fix_hints_and_redact_the_token() {
        let client = client();
        let reason = client.status_reason(403, "permission denied");
        assert!(
            reason.contains("403") && reason.contains("policy"),
            "{reason}"
        );
        let reason = client.status_reason(404, "");
        assert!(
            reason.contains("404") && reason.contains("挂载点"),
            "{reason}"
        );
        // 服务端回显 token 的形态：进 reason 前必须已被替换。
        let leaked = format!("bad request: header X-Vault-Token={} invalid", client.token);
        let reason = client.status_reason(400, &leaked);
        assert!(!reason.contains("s.token.value"), "{reason}");
        assert!(reason.contains("<脱敏>"), "{reason}");
    }

    #[test]
    fn debug_never_prints_the_token() {
        let debug = format!("{:?}", client());
        assert!(!debug.contains("s.token.value"), "{debug}");
        assert!(debug.contains("<脱敏>"), "{debug}");
    }

    #[test]
    fn long_bodies_are_tail_truncated() {
        let client = client();
        let long = "x".repeat(BODY_TAIL * 2);
        assert_eq!(client.redact_tail(&long).chars().count(), BODY_TAIL);
    }
}
