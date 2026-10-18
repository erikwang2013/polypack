//! S3 / OSS 兼容对象存储后端（std）：AWS SigV4 直连，零 AWS SDK。
//!
//! - 布局与共享目录后端完全一致：对象键 `<prefix>/<hex 前 2 位>/<hex>`，对象体 =
//!   `POLYPACK-FARM-OBJECT v0` 头行 ‖ blake3 摘要行 ‖ 载荷（见 [`crate::store`]）；
//!   因此 `aws s3 sync` 在共享目录与桶之间迁移后仍可直接命中；
//! - 认证：AWS Signature Version 4（HMAC-SHA256），签名 host、全部 `x-amz-*` 与
//!   可选的 `x-oss-s3-compat`；不做 profile / IMDS / STS；
//! - 错误只保留端点、HTTP 状态与 S3 `<Code>`：Authorization、密钥、会话令牌与
//!   响应正文绝不进入错误文本或日志；
//! - OSS 只覆盖 S3 兼容模式（`POLYPACK_S3_OSS_COMPAT=1` 时加 `x-oss-s3-compat`），
//!   原生 OSS4 签名不在范围内。

mod sigv4;

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ureq::tls::{Certificate, RootCerts, TlsConfig};

use crate::digest::Digest;
use crate::error::CacheError;
use crate::store::{Entry, ObjectStore, encode_shared, parse_shared};

use self::sigv4::{
    EMPTY_SHA256, amz_date, canonical_query, civil_from_days, days_from_civil, encode_path, sign,
};

/// 单次请求整体超时。
const TIMEOUT: Duration = Duration::from_secs(120);
/// GET 正文的理智上限（覆盖 ureq `read_to_vec` 默认 10 MiB；对象是构建产物）。
const MAX_OBJECT_BYTES: u64 = 1 << 30;
/// ListObjectsV2 单页正文上限（S3 一页最多 1000 条）。
const MAX_LIST_BYTES: u64 = 4 << 20;
/// 错误响应正文读取上限（只为解析 `<Code>`）。
const MAX_ERROR_BYTES: u64 = 64 << 10;
/// 分页硬上限：防止异常服务端无限续页。
const MAX_LIST_PAGES: usize = 1_000;
/// ListObjectsV2 每页最大条数（S3 协议上限）。
const LIST_PAGE_SIZE: &str = "1000";

/// S3 访问密钥（仅内存；不实现 `Debug`，避免误打印密钥）。
#[derive(Clone)]
pub struct S3Credentials {
    /// Access Key ID。
    pub access_key_id: String,
    /// Secret Access Key。
    pub secret_access_key: String,
    /// 会话令牌（临时凭证；`None` = 长期密钥）。
    pub session_token: Option<String>,
}

/// 由调用方解析好的环境值；[`S3Config::from_uri`] 负责默认值与校验。
///
/// `POLYPACK_*` 优先，`AWS_*` 回退（见 [`S3Env::from_env`]）。不实现 `Debug`：
/// 字段可能含密钥。
#[derive(Clone, Default)]
pub struct S3Env {
    /// `POLYPACK_S3_ENDPOINT`（`http(s)://host[:port]`；缺省 = AWS 端点）。
    pub endpoint: Option<String>,
    /// `POLYPACK_S3_REGION` → `AWS_REGION` → `AWS_DEFAULT_REGION`；缺省 `us-east-1`。
    pub region: Option<String>,
    /// `POLYPACK_S3_ACCESS_KEY_ID` → `AWS_ACCESS_KEY_ID`。
    pub access_key_id: Option<String>,
    /// `POLYPACK_S3_SECRET_ACCESS_KEY` → `AWS_SECRET_ACCESS_KEY`。
    pub secret_access_key: Option<String>,
    /// `POLYPACK_S3_SESSION_TOKEN` → `AWS_SESSION_TOKEN`。
    pub session_token: Option<String>,
    /// `POLYPACK_S3_PATH_STYLE`（`1`/`true`、`0`/`false`；缺省 = 设置了 endpoint 时为 `1`）。
    pub path_style: Option<String>,
    /// `POLYPACK_S3_OSS_COMPAT=1` 时给请求加 `x-oss-s3-compat: true`。
    pub oss_compat: Option<String>,
}

impl S3Env {
    /// 从进程环境按契约读取（空值视为未设置）。
    pub fn from_env() -> Self {
        fn var(name: &str) -> Option<String> {
            std::env::var(name).ok().filter(|value| !value.is_empty())
        }
        fn pick(primary: &str, fallback: &str) -> Option<String> {
            var(primary).or_else(|| var(fallback))
        }
        Self {
            endpoint: var("POLYPACK_S3_ENDPOINT"),
            region: pick("POLYPACK_S3_REGION", "AWS_REGION").or_else(|| var("AWS_DEFAULT_REGION")),
            access_key_id: pick("POLYPACK_S3_ACCESS_KEY_ID", "AWS_ACCESS_KEY_ID"),
            secret_access_key: pick("POLYPACK_S3_SECRET_ACCESS_KEY", "AWS_SECRET_ACCESS_KEY"),
            session_token: pick("POLYPACK_S3_SESSION_TOKEN", "AWS_SESSION_TOKEN"),
            path_style: var("POLYPACK_S3_PATH_STYLE"),
            oss_compat: var("POLYPACK_S3_OSS_COMPAT"),
        }
    }
}

/// S3 / OSS 兼容后端配置。
///
/// 不实现 `Debug`：`credentials` 含密钥。`Display` 只输出
/// `s3://<bucket>/<prefix> @ <endpoint>`（无凭据），可直接用于告警。
#[derive(Clone)]
pub struct S3Config {
    /// 桶名。
    pub bucket: String,
    /// 对象键前缀（不含首尾 `/`；空 = 桶根）。
    pub prefix: String,
    /// 签名 region。
    pub region: String,
    /// 自定义端点（`http(s)://host[:port]`）；`None` = AWS。
    pub endpoint: Option<String>,
    /// path-style（`true`）或 virtual-host（`false`）。
    pub path_style: bool,
    /// OSS S3 兼容模式（加 `x-oss-s3-compat: true`）。
    pub oss_compat: bool,
    /// 访问密钥。
    pub credentials: S3Credentials,
}

impl S3Config {
    /// 解析 `s3://<bucket>[/<prefix>]` 并套用环境默认值。
    ///
    /// 配置错误（URI 非法 / 凭据缺失 / 开关取值非法）→ [`CacheError::Config`]；
    /// 消息只含变量名，不含任何密钥值。
    pub fn from_uri(uri: &str, env: S3Env) -> Result<Self, CacheError> {
        let rest = uri
            .strip_prefix("s3://")
            .ok_or_else(|| CacheError::Config(format!("缓存 URI 不是 s3:// 形式: {uri}")))?;
        if rest.is_empty() {
            return Err(CacheError::Config(format!("缓存 URI 缺少桶名: {uri}")));
        }
        if rest.contains(['?', '#']) {
            return Err(CacheError::Config(format!(
                "缓存 URI 不接受 query / fragment: {uri}"
            )));
        }
        let (bucket, prefix) = match rest.split_once('/') {
            Some((bucket, prefix)) => (bucket, prefix.trim_matches('/')),
            None => (rest, ""),
        };
        if bucket.is_empty() {
            return Err(CacheError::Config(format!("缓存 URI 缺少桶名: {uri}")));
        }
        if bucket
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || ch == '@')
        {
            return Err(CacheError::Config(format!(
                "桶名含非法字符（空白 / 控制符 / @）: {bucket}"
            )));
        }
        let endpoint = env
            .endpoint
            .map(|value| value.trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty());
        if let Some(endpoint) = &endpoint {
            split_endpoint(endpoint)?;
        }
        let path_style = match env.path_style.as_deref() {
            Some(value) => parse_switch("POLYPACK_S3_PATH_STYLE", value)?,
            None => endpoint.is_some(),
        };
        let oss_compat = match env.oss_compat.as_deref() {
            Some(value) => parse_switch("POLYPACK_S3_OSS_COMPAT", value)?,
            None => false,
        };
        let access_key_id = env.access_key_id.filter(|value| !value.is_empty());
        let secret_access_key = env.secret_access_key.filter(|value| !value.is_empty());
        let (Some(access_key_id), Some(secret_access_key)) = (access_key_id, secret_access_key)
        else {
            return Err(CacheError::Config(
                "缺少访问密钥：设置 POLYPACK_S3_ACCESS_KEY_ID / POLYPACK_S3_SECRET_ACCESS_KEY（或 AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY）"
                    .to_string(),
            ));
        };
        Ok(Self {
            bucket: bucket.to_string(),
            prefix: prefix.to_string(),
            region: env
                .region
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "us-east-1".to_string()),
            endpoint,
            path_style,
            oss_compat,
            credentials: S3Credentials {
                access_key_id,
                secret_access_key,
                session_token: env.session_token.filter(|value| !value.is_empty()),
            },
        })
    }
}

impl fmt::Display for S3Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "s3://{}", self.bucket)?;
        if !self.prefix.is_empty() {
            write!(f, "/{}", self.prefix)?;
        }
        if let Some(endpoint) = &self.endpoint {
            write!(f, " @ {endpoint}")?;
        }
        Ok(())
    }
}

/// S3 / OSS 兼容对象存储后端。
pub struct S3Store {
    config: S3Config,
    endpoint: Endpoint,
    agent: ureq::Agent,
}

impl S3Store {
    /// 装配 HTTP agent（HTTPS 用系统信任库；HTTP 端点跳过 TLS 根证书检查）。
    pub fn new(config: &S3Config) -> Result<Self, CacheError> {
        let endpoint = endpoint_of(config)?;
        let agent = build_agent(&endpoint)?;
        Ok(Self {
            config: config.clone(),
            endpoint,
            agent,
        })
    }

    /// 可用性探针：ListObjectsV2 `max-keys=1`（有前缀时限定在该前缀）。
    /// 任何非 2xx → 错误，调用方据此降级为本地缓存。
    pub fn ensure_ready(&self) -> Result<(), CacheError> {
        let list_prefix = self.list_prefix();
        let mut query: Vec<(&str, &str)> = vec![("list-type", "2"), ("max-keys", "1")];
        if !list_prefix.is_empty() {
            query.push(("prefix", list_prefix.as_str()));
        }
        let mut response = self.execute("GET", None, &query, None)?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let body = self.read_body(&mut response, MAX_ERROR_BYTES)?;
        Err(self.remote_error(status, &body))
    }

    /// 对象键：`<prefix>/<hex 前 2 位>/<hex>`。
    fn object_key(&self, key: &Digest) -> String {
        let hex = key.as_str();
        if self.config.prefix.is_empty() {
            format!("{}/{hex}", &hex[..2])
        } else {
            format!("{}/{}/{hex}", self.config.prefix, &hex[..2])
        }
    }

    /// 列目录前缀（带结尾 `/`，保证只匹配本后端的分片布局）。
    fn list_prefix(&self) -> String {
        if self.config.prefix.is_empty() {
            String::new()
        } else {
            format!("{}/", self.config.prefix)
        }
    }

    /// 规范 URI（已百分号编码；path-style 时桶名在路径第一段）。
    fn canonical_uri(&self, object: Option<&str>) -> String {
        let raw = match (object, self.config.path_style) {
            (Some(key), true) => format!("/{}/{}", self.config.bucket, key),
            (Some(key), false) => format!("/{key}"),
            (None, true) => format!("/{}", self.config.bucket),
            (None, false) => "/".to_string(),
        };
        encode_path(&raw)
    }

    /// 发一次已签名请求；返回响应（非 2xx 也交回，由调用方判定）。
    fn execute(
        &self,
        method: &str,
        object: Option<&str>,
        query: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<ureq::http::Response<ureq::Body>, CacheError> {
        let uri = self.canonical_uri(object);
        let query_string = canonical_query(query);
        let url = if query_string.is_empty() {
            format!("{}{uri}", self.endpoint.base_url())
        } else {
            format!("{}{uri}?{query_string}", self.endpoint.base_url())
        };
        let payload_hash = match body {
            Some(bytes) => sha256_hex(bytes),
            None => EMPTY_SHA256.to_string(),
        };
        let now = amz_date(SystemTime::now());
        let mut headers: Vec<(String, String)> = vec![
            ("host".to_string(), self.endpoint.host.clone()),
            ("x-amz-content-sha256".to_string(), payload_hash.clone()),
            ("x-amz-date".to_string(), now.clone()),
        ];
        if let Some(token) = &self.config.credentials.session_token {
            headers.push(("x-amz-security-token".to_string(), token.clone()));
        }
        if self.config.oss_compat {
            headers.push(("x-oss-s3-compat".to_string(), "true".to_string()));
        }
        let authorization = sign(
            method,
            &uri,
            &query_string,
            &headers,
            &payload_hash,
            &now,
            &self.config.region,
            &self.config.credentials.access_key_id,
            &self.config.credentials.secret_access_key,
        );
        let mut builder = ureq::http::Request::builder()
            .method(method)
            .uri(url.as_str());
        for (name, value) in &headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        builder = builder.header("authorization", authorization.as_str());
        let response = match body {
            Some(bytes) => {
                let request = builder
                    .body(bytes.to_vec())
                    .map_err(|err| CacheError::Protocol(format!("构造对象存储请求失败: {err}")))?;
                self.agent.run(request)
            }
            None => {
                let request = builder
                    .body(())
                    .map_err(|err| CacheError::Protocol(format!("构造对象存储请求失败: {err}")))?;
                self.agent.run(request)
            }
        };
        response.map_err(|err| self.transport_error(err))
    }

    /// 读响应正文（带上限）；超限 / 网络错误映射为不含 URL 的错误。
    fn read_body(
        &self,
        response: &mut ureq::http::Response<ureq::Body>,
        limit: u64,
    ) -> Result<Vec<u8>, CacheError> {
        response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|err| match err {
                ureq::Error::BodyExceedsLimit(limit) => {
                    CacheError::Protocol(format!("对象存储响应正文超过 {limit} 字节上限"))
                }
                other => self.transport_error(other),
            })
    }

    /// 非 2xx → 端点 + 状态 + S3 `<Code>`（正文不进入错误）。
    fn remote_error(&self, status: u16, body: &[u8]) -> CacheError {
        let text = String::from_utf8_lossy(body);
        CacheError::Remote {
            endpoint: self.endpoint.base_url(),
            status,
            code: xml_value(&text, "Code").unwrap_or_else(|| "unknown".to_string()),
        }
    }

    /// 传输错误只保留简短分类：ureq 的错误文本可能带完整 URL / query。
    fn transport_error(&self, err: ureq::Error) -> CacheError {
        let detail = match &err {
            ureq::Error::Timeout(_) => "请求超时".to_string(),
            ureq::Error::HostNotFound => "主机名解析失败".to_string(),
            ureq::Error::ConnectionFailed => "连接失败".to_string(),
            ureq::Error::Io(io) => format!("网络 IO 失败: {}", io.kind()),
            _ => "请求失败".to_string(),
        };
        CacheError::RemoteTransport {
            endpoint: self.endpoint.base_url(),
            detail,
        }
    }

    /// 对象键 → 摘要（只接受本后端的 `<2hex>/<64hex>` 分片布局）。
    fn digest_of(&self, object: &str) -> Option<Digest> {
        let prefix = self.list_prefix();
        let relative = object.strip_prefix(prefix.as_str())?;
        let mut parts = relative.split('/');
        let shard = parts.next()?;
        let name = parts.next()?;
        if parts.next().is_some()
            || shard.len() != 2
            || name.len() != 64
            || !name.starts_with(shard)
        {
            return None;
        }
        Digest::parse(name).ok()
    }
}

impl ObjectStore for S3Store {
    fn get(&self, key: &Digest) -> Result<Option<Vec<u8>>, CacheError> {
        let object = self.object_key(key);
        let mut response = self.execute("GET", Some(&object), &[], None)?;
        let status = response.status().as_u16();
        match status {
            200 => {}
            404 => return Ok(None),
            _ => {
                let body = self.read_body(&mut response, MAX_ERROR_BYTES)?;
                return Err(self.remote_error(status, &body));
            }
        }
        let body = self.read_body(&mut response, MAX_OBJECT_BYTES)?;
        match parse_shared(&body) {
            Some(payload) => Ok(Some(payload.to_vec())),
            None => {
                // 损坏条目不再参与后续命中；删除失败（只读凭据等）不影响本次未命中
                let _ = self.remove(key);
                Ok(None)
            }
        }
    }

    fn put(&self, key: &Digest, bytes: &[u8]) -> Result<(), CacheError> {
        let object = self.object_key(key);
        let encoded = encode_shared(bytes);
        let mut response = self.execute("PUT", Some(&object), &[], Some(&encoded))?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let body = self.read_body(&mut response, MAX_ERROR_BYTES)?;
        Err(self.remote_error(status, &body))
    }

    fn remove(&self, key: &Digest) -> Result<u64, CacheError> {
        let object = self.object_key(key);
        // HEAD 取对象大小：S3 DELETE 不返回长度，缓存统计需要释放字节数
        let mut head = self.execute("HEAD", Some(&object), &[], None)?;
        let status = head.status().as_u16();
        let size = match status {
            200 => head
                .headers()
                .get("content-length")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0),
            404 => return Ok(0),
            _ => {
                let body = self.read_body(&mut head, MAX_ERROR_BYTES)?;
                return Err(self.remote_error(status, &body));
            }
        };
        let mut response = self.execute("DELETE", Some(&object), &[], None)?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) || status == 404 {
            return Ok(size);
        }
        let body = self.read_body(&mut response, MAX_ERROR_BYTES)?;
        Err(self.remote_error(status, &body))
    }

    fn entries(&self) -> Result<Vec<Entry>, CacheError> {
        let list_prefix = self.list_prefix();
        let mut entries = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..MAX_LIST_PAGES {
            let mut query: Vec<(&str, &str)> =
                vec![("list-type", "2"), ("max-keys", LIST_PAGE_SIZE)];
            if !list_prefix.is_empty() {
                query.push(("prefix", list_prefix.as_str()));
            }
            if let Some(token) = &token {
                query.push(("continuation-token", token.as_str()));
            }
            let mut response = self.execute("GET", None, &query, None)?;
            let status = response.status().as_u16();
            if !(200..300).contains(&status) {
                let body = self.read_body(&mut response, MAX_ERROR_BYTES)?;
                return Err(self.remote_error(status, &body));
            }
            let body = self.read_body(&mut response, MAX_LIST_BYTES)?;
            let text = String::from_utf8_lossy(&body);
            for block in xml_blocks(&text, "Contents") {
                let Some(object) = xml_value(block, "Key") else {
                    continue;
                };
                let Some(key) = self.digest_of(&object) else {
                    continue;
                };
                let Some(bytes) = xml_value(block, "Size").and_then(|size| size.parse().ok())
                else {
                    continue;
                };
                // 无 LastModified 的条目无法参与 LRU，跳过（与本地后端一致）
                let Some(last_used) =
                    xml_value(block, "LastModified").and_then(|value| parse_last_modified(&value))
                else {
                    continue;
                };
                entries.push(Entry {
                    key,
                    bytes,
                    last_used,
                });
            }
            if xml_value(&text, "IsTruncated").as_deref() != Some("true") {
                return Ok(entries);
            }
            token = xml_value(&text, "NextContinuationToken");
            if token.is_none() {
                return Ok(entries);
            }
        }
        Err(CacheError::Protocol(format!(
            "ListObjectsV2 分页超过 {MAX_LIST_PAGES} 页"
        )))
    }
}

/// 解析端点的 scheme 与 host（拒绝路径 / userinfo / 非 http(s)）。
fn split_endpoint(endpoint: &str) -> Result<(&str, &str), CacheError> {
    let (scheme, host) = endpoint.split_once("://").ok_or_else(|| {
        CacheError::Config(format!(
            "POLYPACK_S3_ENDPOINT 需要 http(s)://host[:port] 形式: {endpoint}"
        ))
    })?;
    if scheme != "http" && scheme != "https" {
        return Err(CacheError::Config(format!(
            "POLYPACK_S3_ENDPOINT 只接受 http / https: {endpoint}"
        )));
    }
    if host.is_empty() || host.contains(['/', '@']) || host.chars().any(char::is_whitespace) {
        return Err(CacheError::Config(format!(
            "POLYPACK_S3_ENDPOINT 只接受 host[:port]（不含路径 / 凭据）: {endpoint}"
        )));
    }
    Ok((scheme, host))
}

/// 解析后的端点（virtual-host 时 bucket 已并入 host）。
struct Endpoint {
    scheme: String,
    host: String,
}

impl Endpoint {
    fn base_url(&self) -> String {
        format!("{}://{}", self.scheme, self.host)
    }
}

fn endpoint_of(config: &S3Config) -> Result<Endpoint, CacheError> {
    match &config.endpoint {
        Some(raw) => {
            let (scheme, host) = split_endpoint(raw)?;
            let host = if config.path_style {
                host.to_string()
            } else {
                format!("{}.{host}", config.bucket)
            };
            Ok(Endpoint {
                scheme: scheme.to_string(),
                host,
            })
        }
        None => Ok(Endpoint {
            scheme: "https".to_string(),
            host: if config.path_style {
                format!("s3.{}.amazonaws.com", config.region)
            } else {
                format!("{}.s3.{}.amazonaws.com", config.bucket, config.region)
            },
        }),
    }
}

/// 装配 agent：系统信任库 + 全局超时 + 非 2xx 交回 + 不跟随重定向。
fn build_agent(endpoint: &Endpoint) -> Result<ureq::Agent, CacheError> {
    let mut builder = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .max_redirects(0);
    if endpoint.scheme == "https" {
        let native = rustls_native_certs::load_native_certs();
        if native.certs.is_empty() {
            return Err(CacheError::Config(
                "系统信任库没有可用根证书（HTTPS 无从校验）：装 ca-certificates 后重跑".to_string(),
            ));
        }
        let roots: Vec<Certificate<'static>> = native
            .certs
            .iter()
            .map(|der| Certificate::from_der(der.as_ref()).to_owned())
            .collect();
        builder = builder.tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::Specific(Arc::new(roots)))
                .build(),
        );
    }
    Ok(builder.build().into())
}

/// `1`/`true`、`0`/`false`（大小写不敏感）；其他值 → 配置错误。
fn parse_switch(name: &str, value: &str) -> Result<bool, CacheError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(CacheError::Config(format!(
            "{name} 只接受 1/true 或 0/false: {value}"
        ))),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 取第一个 `<tag>…</tag>` 的文本（去空白并反转义）。
fn xml_value(xml: &str, tag: &str) -> Option<String> {
    xml_blocks(xml, tag)
        .first()
        .map(|value| xml_unescape(value.trim()))
}

/// 取全部 `<tag>…</tag>` 块（S3 的 `<Contents>` 列表）。
fn xml_blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut blocks = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let content = &rest[start + open.len()..];
        let Some(end) = content.find(&close) else {
            break;
        };
        blocks.push(&content[..end]);
        rest = &content[end + close.len()..];
    }
    blocks
}

/// 反转义 S3 / OSS 列表响应里出现的 XML 实体（`&amp;` 必须最后处理）。
fn xml_unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// 解析 S3 的 `LastModified`（`2026-10-19T12:34:56.000Z`）；非法日期返回 `None`。
fn parse_last_modified(text: &str) -> Option<SystemTime> {
    let bytes = text.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || bytes.last() != Some(&b'Z')
    {
        return None;
    }
    // 可选小数秒：`...:56Z` 或 `...:56.000Z`
    if bytes.len() > 20
        && (bytes[19] != b'.'
            || !bytes[20..bytes.len() - 1]
                .iter()
                .all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let year = number(text, 0, 4)?;
    let month = number(text, 5, 7)? as u32;
    let day = number(text, 8, 10)? as u32;
    let hour = number(text, 11, 13)?;
    let minute = number(text, 14, 16)?;
    let second = number(text, 17, 19)?;
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=59).contains(&second) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    // 分量必须真实存在（拒绝 2 月 30 日经由 civil 转换回绕）
    if civil_from_days(days) != (year, month, day) {
        return None;
    }
    let seconds = days.checked_mul(86_400)? + hour * 3_600 + minute * 60 + second;
    u64::try_from(seconds)
        .ok()
        .and_then(|seconds| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
}

/// 定长十进制字段（只接受 ASCII 数字）。
fn number(text: &str, start: usize, end: usize) -> Option<i64> {
    let part = text.get(start..end)?;
    if !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> S3Env {
        S3Env {
            access_key_id: Some("AK".to_string()),
            secret_access_key: Some("SK".to_string()),
            ..S3Env::default()
        }
    }

    #[test]
    fn from_uri_parses_bucket_prefix_and_defaults() {
        let config = S3Config::from_uri(
            "s3://build-cache/team/polypack/",
            S3Env {
                endpoint: Some("http://127.0.0.1:9000/".to_string()),
                ..env()
            },
        )
        .expect("URI 合法");
        assert_eq!(config.bucket, "build-cache");
        assert_eq!(config.prefix, "team/polypack");
        assert_eq!(config.region, "us-east-1");
        assert_eq!(config.endpoint.as_deref(), Some("http://127.0.0.1:9000"));
        assert!(config.path_style, "自定义端点缺省 path-style");
        assert!(!config.oss_compat);

        let aws = S3Config::from_uri("s3://bucket", env()).expect("URI 合法");
        assert_eq!(aws.prefix, "");
        assert!(!aws.path_style, "AWS 缺省 virtual-host");
    }

    #[test]
    fn from_uri_rejects_bad_config_without_secrets() {
        let error = S3Config::from_uri("https://bucket", env())
            .err()
            .expect("scheme 非法");
        assert!(matches!(error, CacheError::Config(_)), "{error}");

        let error = S3Config::from_uri("s3://bucket?x=1", env())
            .err()
            .expect("query 非法");
        assert!(matches!(error, CacheError::Config(_)), "{error}");

        let error = S3Config::from_uri("s3://bucket", S3Env::default())
            .err()
            .expect("缺凭据");
        let text = error.to_string();
        assert!(text.contains("POLYPACK_S3_ACCESS_KEY_ID"), "{text}");

        let error = S3Config::from_uri(
            "s3://bucket",
            S3Env {
                path_style: Some("maybe".to_string()),
                ..env()
            },
        )
        .err()
        .expect("开关非法");
        assert!(matches!(error, CacheError::Config(_)), "{error}");
    }

    #[test]
    fn object_keys_follow_shared_layout() {
        let key = Digest::of_bytes(b"layout");
        let store = S3Store {
            config: S3Config::from_uri(
                "s3://bucket/pfx",
                S3Env {
                    endpoint: Some("http://127.0.0.1:1".to_string()),
                    ..env()
                },
            )
            .unwrap(),
            endpoint: Endpoint {
                scheme: "http".to_string(),
                host: "127.0.0.1:1".to_string(),
            },
            agent: ureq::Agent::new_with_defaults(),
        };
        let object = store.object_key(&key);
        assert_eq!(
            object,
            format!("pfx/{}/{}", &key.as_str()[..2], key.as_str())
        );
        assert_eq!(store.digest_of(&object), Some(key));
        assert_eq!(store.digest_of("pfx/zz/not-a-key"), None);
        assert_eq!(
            store.canonical_uri(Some(&object)),
            format!("/bucket/{object}")
        );
    }

    #[test]
    fn last_modified_parses_utc_and_rejects_bad_dates() {
        let time = parse_last_modified("2024-02-29T00:00:00.000Z").expect("闰日合法");
        assert_eq!(
            time.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            1_709_164_800
        );
        assert!(parse_last_modified("2024-02-29T00:00:00Z").is_some());
        assert!(parse_last_modified("2023-02-29T00:00:00.000Z").is_none());
        assert!(parse_last_modified("2024-02-29 00:00:00.000Z").is_none());
        assert!(parse_last_modified("2024-02-29T24:00:00.000Z").is_none());
    }

    #[test]
    fn listing_xml_extracts_contents_and_entities() {
        let xml = "<ListBucketResult><IsTruncated>true</IsTruncated>\
<NextContinuationToken>a&amp;b</NextContinuationToken>\
<Contents><Key>pfx/ab/abcd</Key><Size>42</Size>\
<LastModified>2024-02-29T00:00:00.000Z</LastModified></Contents>\
<Contents><Key>pfx/cd/cdef</Key><Size>7</Size>\
<LastModified>2024-03-01T00:00:00.000Z</LastModified></Contents></ListBucketResult>";
        let blocks = xml_blocks(xml, "Contents");
        assert_eq!(blocks.len(), 2);
        assert_eq!(xml_value(blocks[1], "Key").as_deref(), Some("pfx/cd/cdef"));
        assert_eq!(
            xml_value(xml, "NextContinuationToken").as_deref(),
            Some("a&b")
        );
        assert_eq!(xml_value(xml, "IsTruncated").as_deref(), Some("true"));
    }
}
