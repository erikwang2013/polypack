//! 传输层接缝（T-M3-09）：平台客户端只组装请求 / 解读响应，真实网络细节收敛在
//! [`UreqTransport`]；mock HTTP 契约测试（tests/）注入 FakeTransport，零网络。
//!
//! 两条硬要求：
//! - **非 2xx 也把响应交回来**（`http_status_as_error(false)`）：平台错误正文是修复依据，
//!   由客户端决定把它转成 [`PublishError::Platform`] 还是 [`PublishError::Http`]；
//! - **脱敏**：错误文本可能带完整 URL（微信 access_token / secret 走 query，支付宝 sign
//!   / 抖音 access-token 走头或正文）——[`endpoint_of`] 先去掉 query，进错误的正文再过
//!   [`Redactor`] + 尾部截断（口径同 poly-sign::vault 的 BODY_TAIL）。
//!
//! TLS：关掉 ureq 自带的根证书（`webpki-roots` / `webpki-root-certs` 许可证不在
//! `deny.toml` 白名单，同 poly-sign/Cargo.toml 注释），显式给 [`RootCerts::Specific`]：
//! 系统信任库（`rustls-native-certs`，纯 Rust，无 *-sys / openssl）。

use std::sync::Arc;
use std::time::Duration;

use ureq::tls::{Certificate, RootCerts, TlsConfig};

use crate::error::PublishError;
use crate::secret::Redactor;

/// 单次请求整体超时：上传请求带整包正文（几 MB），给宽些；重试从回执断点续走。
const TIMEOUT: Duration = Duration::from_secs(120);
/// 错误 reason 里保留的正文尾字符数（同 poly-sign::vault）。
const BODY_TAIL: usize = 300;
/// 幂等键请求头（T-M8-10）：提审重发带同一键；支持该头的网关按原响应去重，
/// 不支持的平台会忽略它，仍由平台原生去重码（85009 / 11303 / …）兜底。
pub const IDEMPOTENCY_HEADER: &str = "Idempotency-Key";

/// 请求方法（本 crate 只用到两种）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// GET（微信取 token；抖音轮询 / 列表查询）。
    Get,
    /// POST（上传 / 提审；支付宝网关全部走 POST 表单）。
    Post,
}

/// 一次 HTTP 请求（平台客户端组装，传输层只搬字节）。
pub struct HttpRequest {
    /// 方法。
    pub method: Method,
    /// 完整 URL（可能含凭据性 query——故不实现 `Debug`）。
    pub url: String,
    /// 显式请求头（按数组顺序发送）。
    pub headers: Vec<(String, String)>,
    /// 请求体（[`Method::Get`] 恒空）。
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// GET 请求。
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::Get,
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// POST 请求（`content_type` 非空时带上 Content-Type 头）。
    pub fn post(url: impl Into<String>, content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        let mut request = Self {
            method: Method::Post,
            url: url.into(),
            headers: Vec::new(),
            body: body.into(),
        };
        if !content_type.is_empty() {
            request
                .headers
                .push(("Content-Type".to_string(), content_type.to_string()));
        }
        request
    }

    /// 追加一个请求头。
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// 一次 HTTP 响应（含非 2xx，状态判断留给调用方）。
pub struct HttpResponse {
    /// 状态码。
    pub status: u16,
    /// 正文原始字节。
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// 构造（mock 契约测试给脚本化响应）。
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }

    /// 是否 2xx。
    pub fn is_ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// 正文按 UTF-8 解码（坏字节用替换符；仅用于错误文本 / JSON 解析）。
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// 传输层：发送一次请求，拿到响应或传输失败。
pub trait Transport: Send + Sync {
    /// 发请求；网络层失败（连接 / TLS / 读写）→ [`PublishError::Http`]。
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, PublishError>;
}

/// 真网络传输（ureq agent：全局超时 + 系统信任库）。
pub struct UreqTransport {
    /// 复用连接的 agent。
    agent: ureq::Agent,
}

impl UreqTransport {
    /// 装配 agent（真实模式用；mock 契约测试不经过本类型）。
    pub fn new() -> Result<Self, PublishError> {
        let tls = TlsConfig::builder()
            .root_certs(RootCerts::Specific(Arc::new(system_roots()?)))
            .build();
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            // 非 2xx 也把响应交回来：平台报错正文是修复依据（同 poly-sign::vault）。
            .http_status_as_error(false)
            .tls_config(tls)
            .build();
        Ok(Self {
            agent: config.into(),
        })
    }
}

impl Transport for UreqTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, PublishError> {
        // get / post 的构建器泛型不同（WithoutBody / WithBody），各自终结成同一结果再合流。
        let result = match request.method {
            Method::Get => {
                let mut builder = self.agent.get(&request.url);
                for (name, value) in &request.headers {
                    builder = builder.header(name.as_str(), value.as_str());
                }
                builder.call()
            }
            Method::Post => {
                let mut builder = self.agent.post(&request.url);
                for (name, value) in &request.headers {
                    builder = builder.header(name.as_str(), value.as_str());
                }
                builder.send(request.body.as_slice())
            }
        };
        let mut response = result.map_err(|err| PublishError::Http {
            endpoint: endpoint_of(&request.url),
            status: None,
            reason: format!("请求失败：{err}"),
        })?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_vec()
            .map_err(|err| PublishError::Http {
                endpoint: endpoint_of(&request.url),
                status: None,
                reason: format!("读取响应正文失败：{err}"),
            })?;
        Ok(HttpResponse { status, body })
    }
}

/// 发请求 + 错误脱敏（平台客户端统一入口）。
///
/// 传输层错误文本可能带完整 URL（其中有凭据参数），这里统一过 [`Redactor`] 后才进错误。
pub(crate) fn send(
    transport: &dyn Transport,
    request: &HttpRequest,
    redactor: &Redactor,
) -> Result<HttpResponse, PublishError> {
    transport
        .send(request)
        .map_err(|error| redact_error(error, redactor))
}

/// 把 [`PublishError::Http`] 的文本再过一道脱敏（其余变体原样透传）。
pub(crate) fn redact_error(error: PublishError, redactor: &Redactor) -> PublishError {
    match error {
        PublishError::Http {
            endpoint,
            status,
            reason,
        } => PublishError::Http {
            endpoint: redactor.redact(&endpoint),
            status,
            reason: redactor.redact(&reason),
        },
        other => other,
    }
}

/// 响应 → JSON：非 2xx / 非 JSON 即 `PublishError::Http`（文本脱敏 + 尾部截断）。
pub(crate) fn ok_json(
    response: &HttpResponse,
    endpoint: &str,
    redactor: &Redactor,
) -> Result<serde_json::Value, PublishError> {
    if !response.is_ok() {
        return Err(PublishError::Http {
            endpoint: endpoint.to_string(),
            status: Some(response.status),
            reason: format!(
                "HTTP {}：{}",
                response.status,
                redact_tail(redactor, &response.text())
            ),
        });
    }
    serde_json::from_slice(&response.body).map_err(|err| PublishError::Http {
        endpoint: endpoint.to_string(),
        status: Some(response.status),
        reason: format!(
            "响应非 JSON：{err}（正文尾部：{}）",
            redact_tail(redactor, &response.text())
        ),
    })
}

/// 响应 / 平台文本进错误前的脱敏 + 尾部截断。
pub(crate) fn redact_tail(redactor: &Redactor, text: &str) -> String {
    let redacted = redactor.redact(text);
    let count = redacted.chars().count();
    let tail: String = if count > BODY_TAIL {
        redacted.chars().skip(count - BODY_TAIL).collect()
    } else {
        redacted
    };
    tail.trim().to_string()
}

/// 端点（去 query / fragment）：错误 / 日志只出现不含凭据参数的 URL 前缀。
pub(crate) fn endpoint_of(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).to_string()
}

/// 查询串 / 表单值的百分号编码（RFC 3986 未保留集 + UTF-8；口径同 Node
/// `querystring.stringify` / `encodeURIComponent`——非 ASCII 逐字节转义）。
pub(crate) fn form_encode(value: &str) -> String {
    /// 保留 `-._~`（其余非字母数字全转义）。
    const SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~');

    percent_encoding::utf8_percent_encode(value, SET).to_string()
}

/// 读系统信任库根证书。
fn system_roots() -> Result<Vec<Certificate<'static>>, PublishError> {
    let native = rustls_native_certs::load_native_certs();
    if native.certs.is_empty() {
        return Err(PublishError::Http {
            endpoint: "(TLS)".to_string(),
            status: None,
            reason: "系统信任库没有可用根证书（HTTPS 校验无从谈起）：装 ca-certificates \
                     或换有信任链的环境后重跑"
                .to_string(),
        });
    }
    Ok(native
        .certs
        .iter()
        .map(|der| Certificate::from_der(der.as_ref()).to_owned())
        .collect())
}
