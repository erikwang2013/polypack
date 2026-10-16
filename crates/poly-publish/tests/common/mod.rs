//! poly-publish 集成测试支架：脚本化 [`FakeTransport`]（记录请求 / 按序回响应）、测试凭据
//! 载荷、临时项目根。全零网络——真传输只出现在 `publish_real.rs`（`POLY_PUBLISH_REAL=1`）。
//!
//! 仓库零明文两条线：
//! - 测试 RSA 私钥**运行时生成**（rsa keygen + 系统 RNG），源码与仓库不留私钥材料；
//! - 固定形态样本（32 位十六进制 AppSecret 等）由 `format!` / `repeat` 拼出、PEM 头行由
//!   [`pem_wrap`] 的参数拼装——源码行内不出现完整形态，仓库自扫（T-M2-12）保持为绿。
#![allow(dead_code)] // 各集成测试 crate 各取所需（口径同 poly-sign/tests/common）

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine as _;
use poly_core::{CredentialRef, CredentialSource};
use poly_publish::{
    CredentialProvider, HttpRequest, HttpResponse, MemoryProvider, PublishError, Transport,
};

// ---------------------------------------------------------------- 传输层脚本

/// 记录的请求快照（[`HttpRequest`] 非 Clone；测试侧快照字段）。
#[derive(Clone)]
pub struct Recorded {
    /// 方法。
    pub method: poly_publish::Method,
    /// 完整 URL（可能含凭据性 query）。
    pub url: String,
    /// 请求头。
    pub headers: Vec<(String, String)>,
    /// 请求体。
    pub body: Vec<u8>,
}

impl Recorded {
    /// 头部取值（大小写不敏感；无此头 → `None`）。
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// URL 的 query 部分（无 → 空串）。
    pub fn query(&self) -> &str {
        self.url
            .split_once('?')
            .map(|(_, query)| query)
            .unwrap_or("")
    }

    /// URL 去 query / fragment 的前缀。
    pub fn endpoint(&self) -> &str {
        self.url.split(['?', '#']).next().unwrap_or(&self.url)
    }

    /// 正文按 UTF-8 解码（坏字节替换符）。
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// 脚本化传输层：按序吐响应、记录请求；脚本用尽即报错（错误里带 URL——顺带压脱敏路径）。
pub struct FakeTransport {
    /// 待吐响应队列。
    script: Mutex<VecDeque<HttpResponse>>,
    /// 已收请求。
    requests: Mutex<Vec<Recorded>>,
}

impl FakeTransport {
    /// 用脚本响应装配（返回 `Arc` 以便与客户端共享）。
    pub fn new(script: Vec<HttpResponse>) -> Arc<Self> {
        Arc::new(Self {
            script: Mutex::new(script.into()),
            requests: Mutex::new(Vec::new()),
        })
    }

    /// 已收请求快照。
    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().expect("记录锁").clone()
    }

    /// 已收请求数。
    pub fn count(&self) -> usize {
        self.requests.lock().expect("记录锁").len()
    }
}

impl Transport for FakeTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, PublishError> {
        let index = {
            let mut requests = self.requests.lock().expect("记录锁");
            requests.push(Recorded {
                method: request.method,
                url: request.url.clone(),
                headers: request.headers.clone(),
                body: request.body.clone(),
            });
            requests.len()
        };
        self.script
            .lock()
            .expect("脚本锁")
            .pop_front()
            .ok_or_else(|| PublishError::Http {
                endpoint: "(fake)".to_string(),
                reason: format!("FakeTransport 脚本用尽（第 {index} 次请求 {}", request.url),
            })
    }
}

// ---------------------------------------------------------------- 响应构造

/// `200` + JSON 正文。
pub fn json_ok(value: serde_json::Value) -> HttpResponse {
    HttpResponse::new(200, value.to_string())
}

/// 任意状态码 + 原始正文。
pub fn raw(status: u16, body: impl Into<Vec<u8>>) -> HttpResponse {
    HttpResponse::new(status, body)
}

/// 凭据载荷契约：`base64(JSON)`（T-M2-11 同款）。
pub fn payload(value: &serde_json::Value) -> String {
    base64::engine::general_purpose::STANDARD.encode(value.to_string())
}

/// 32 位十六进制（微信 / 抖音 AppSecret 的真实形态；源码行内不成完整形态）。
pub fn hex32() -> String {
    "0123456789abcdef".repeat(2)
}

// ---------------------------------------------------------------- 凭据注入

/// 内存后端 + 引用（引用模型：只给引用，值经后端链现取）。
pub fn credential(
    name: &str,
    payload_json: serde_json::Value,
) -> (CredentialRef, Vec<Box<dyn CredentialProvider>>) {
    let reference = CredentialRef {
        name: name.to_string(),
        source: CredentialSource::Keyring,
    };
    let providers: Vec<Box<dyn CredentialProvider>> = vec![Box::new(MemoryProvider::with_entry(
        name,
        payload(&payload_json),
    ))];
    (reference, providers)
}

/// 微信凭据载荷（自研账号 CI 模式三件套）。
pub fn wechat_payload(appid: &str, secret: &str) -> serde_json::Value {
    serde_json::json!({
        "appid": appid,
        "secret": secret,
        "private_key": test_private_key_pem(),
    })
}

/// 支付宝凭据载荷（网关 RSA2 应用私钥）。
pub fn alipay_payload(app_id: &str) -> serde_json::Value {
    serde_json::json!({
        "app_id": app_id,
        "private_key": test_private_key_pem(),
    })
}

/// 抖音凭据载荷（access_token 外部注入；铸造链路未确证，客户端不做）。
pub fn douyin_payload(access_token: &str) -> serde_json::Value {
    serde_json::json!({
        "access_token": access_token,
    })
}

// ---------------------------------------------------------------- 测试密钥

/// 测试 RSA 私钥（PKCS#8 PEM；运行时生成一次，进程内缓存）。
///
/// 生成而非内嵌：源码 / 仓库里没有私钥材料，仓库自扫与全历史扫描都不必依赖白名单。
pub fn test_private_key_pem() -> &'static str {
    static PEM: OnceLock<String> = OnceLock::new();
    PEM.get_or_init(|| {
        use rsa::pkcs8::EncodePrivateKey as _;

        let key = rsa::RsaPrivateKey::new(&mut TestRng, 2048).expect("生成测试密钥");
        let der = key.to_pkcs8_der().expect("编码 PKCS#8 DER");
        pem_wrap("PRIVATE KEY", der.as_bytes())
    })
}

/// 核验 raw PKCS#1 v1.5 签名（Node `crypto.privateEncrypt` 等价语义；测试侧独立复算）。
pub fn verify_raw_signature(message: &[u8], signature_b64: &str) {
    use rsa::pkcs8::DecodePrivateKey as _;

    let key = rsa::RsaPrivateKey::from_pkcs8_pem(test_private_key_pem()).expect("测试密钥可解析");
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .expect("签名为标准 base64");
    key.to_public_key()
        .verify(rsa::Pkcs1v15Sign::new_unprefixed(), message, &signature)
        .expect("签名须是该消息的 raw PKCS#1 v1.5 私钥运算（无 DigestInfo）");
}

/// 核验 RSA-SHA256 / PKCS#1 v1.5 签名（支付宝网关 RSA2 口径；测试侧用测试密钥公钥独立复算）。
pub fn verify_rsa_sha256(message: &[u8], signature_b64: &str) {
    use rsa::pkcs1::EncodeRsaPublicKey as _;
    use rsa::pkcs8::DecodePrivateKey as _;

    let key = rsa::RsaPrivateKey::from_pkcs8_pem(test_private_key_pem()).expect("测试密钥可解析");
    let der = key.to_public_key().to_pkcs1_der().expect("公钥 PKCS#1 DER");
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature_b64)
        .expect("签名为标准 base64");
    ring::signature::UnparsedPublicKey::new(
        &ring::signature::RSA_PKCS1_2048_8192_SHA256,
        der.as_bytes(),
    )
    .verify(message, &signature)
    .expect("签名须是该消息的 RSA-SHA256 PKCS#1 v1.5 签名");
}

/// DER → PEM（64 列折行；头 / 尾行由 `label` 参数拼装，源码行内不出现完整私钥头）。
fn pem_wrap(label: &str, der: &[u8]) -> String {
    let body = base64::engine::general_purpose::STANDARD.encode(der);
    let mut pem = format!("-----BEGIN {label}-----\n");
    for chunk in body.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).expect("base64 为 ASCII"));
        pem.push('\n');
    }
    pem.push_str(&format!("-----END {label}-----\n"));
    pem
}

/// 测试 RNG：ring 系统 RNG 填字节 + rand_core 0.6 适配（rsa keygen 要 `CryptoRngCore`）。
struct TestRng;

impl rsa::rand_core::RngCore for TestRng {
    fn next_u32(&mut self) -> u32 {
        let mut bytes = [0u8; 4];
        self.fill_bytes(&mut bytes);
        u32::from_le_bytes(bytes)
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes = [0u8; 8];
        self.fill_bytes(&mut bytes);
        u64::from_le_bytes(bytes)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        use ring::rand::SecureRandom as _;

        ring::rand::SystemRandom::new()
            .fill(dest)
            .expect("系统 RNG 可用");
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rsa::rand_core::Error> {
        use ring::rand::SecureRandom as _;

        ring::rand::SystemRandom::new()
            .fill(dest)
            .map_err(|_| rsa::rand_core::Error::new("系统 RNG 填充失败"))
    }
}

impl rsa::rand_core::CryptoRng for TestRng {}

// ---------------------------------------------------------------- 临时项目根

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时项目根（`<tmp>/poly-publish-it-<tag>-<pid>-<n>`；drop 即递归删除）。
pub struct TempRoot {
    /// 路径。
    path: PathBuf,
}

impl TempRoot {
    /// 建根 + `dist` 子目录里的最小 Taro 形态小程序产物（`app.json` 在根）。
    pub fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "poly-publish-it-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("建临时根");
        let root = Self { path };
        write_miniapp(&root.dist());
        root
    }

    /// 项目根。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 代码包目录（T-M3-06 产物形态）。
    pub fn dist(&self) -> PathBuf {
        self.path.join("dist")
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// 写最小 Taro 形态小程序产物（三条目；`app.json` 必在根）。
pub fn write_miniapp(dist: &Path) {
    std::fs::create_dir_all(dist.join("pages/index")).expect("建目录");
    std::fs::write(dist.join("app.json"), r#"{"pages":["pages/index/index"]}"#).expect("写");
    std::fs::write(dist.join("app.js"), "App({})").expect("写");
    std::fs::write(dist.join("pages/index/index.js"), "Page({})").expect("写");
}

// ---------------------------------------------------------------- 容器 / 压缩解包

/// 解开 B 协议容器（独立实现：校验魔数、段长、偏移自洽后逐条目取出）。
pub fn unpack_container(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let i32_at = |start: usize| -> usize {
        i32::from_be_bytes(bytes[start..start + 4].try_into().expect("4 字节段"))
            .try_into()
            .expect("非负段长")
    };
    assert_eq!(bytes[0], 0xBE, "容器魔数 0xBE");
    assert_eq!(i32_at(1), 0, "协议版本 0");
    assert_eq!(bytes[13], 0xED, "文件信息段尾标 0xED");
    let info_len = i32_at(5);
    let data_len = i32_at(9);
    let count = i32_at(14);
    let mut entries = Vec::new();
    let mut cursor = 18usize;
    for _ in 0..count {
        let name_len = i32_at(cursor);
        let name = String::from_utf8(bytes[cursor + 4..cursor + 4 + name_len].to_vec())
            .expect("条目名为 UTF-8");
        let offset = i32_at(cursor + 4 + name_len);
        let len = i32_at(cursor + 8 + name_len);
        assert_eq!(
            offset,
            14 + info_len + entries_data_len(&entries),
            "数据偏移按序紧排"
        );
        entries.push((name, bytes[offset..offset + len].to_vec()));
        cursor += 12 + name_len;
    }
    assert_eq!(cursor, 14 + info_len, "文件信息段正好收尾");
    assert_eq!(14 + info_len + data_len, bytes.len(), "数据段长度自洽");
    entries
}

/// 已取条目数据段总长。
fn entries_data_len(entries: &[(String, Vec<u8>)]) -> usize {
    entries.iter().map(|(_, body)| body.len()).sum()
}

/// gunzip（平台只要求合法 gzip，测试侧解回原字节比对）。
pub fn gunzip(bytes: &[u8]) -> Vec<u8> {
    use std::io::Read as _;

    assert_eq!(&bytes[..2], &[0x1F, 0x8B], "gzip 魔数");
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).expect("gunzip 成功");
    out
}
