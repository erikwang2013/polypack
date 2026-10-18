//! 微信开放平台 publish 客户端（T-M3-09）：代码包上传 → 提审 → 审核状态轮询。
//!
//! 协议口径（官方文档 + miniprogram-ci 2.1.31 源码交叉印证；上传通道无公开文档，
//! 口径直接取自其 `src/ci/upload.ts` 的编译产物 `dist/ci/upload.js` / `dist/utils/sign.js`）：
//!
//! 1. **上传**（servicewechat.com CI 通道）：
//!    - `POST https://servicewechat.com/wxa/ci/getrandstr`（JSON `{"appid":…,"clientRand":<int>}`）
//!      → `data.randomString`；
//!    - 签名 = base64（RSA **PKCS#1 v1.5 原语**私钥运算，Node `crypto.privateEncrypt`
//!      语义——不含 DigestInfo）over `{"appid":"…","rand_str":"<randomString>"}`，
//!      密钥 = 代码上传密钥（PEM）；
//!    - 容器 = B 协议 pack（头 `0xBE | i32be(版本=0) | i32be(文件信息段长) |
//!      i32be(数据段长) | 0xED`；见 [`pack_container`]），额外含 `ci.signature` 条目
//!      `{"signature":"…","version":"2.1.31"}`；
//!    - `POST https://servicewechat.com/wxa/ci/upload?codeprotect=0&type=miniProgram&appid=…&version=…&desc=…&robot=1&debugLaunchInfo={"scene":1011}`，
//!      body = gzip(容器)；响应 `errCode == 0` 即受理。
//! 2. **提审** `POST https://api.weixin.qq.com/wxa/submit_audit?access_token=…`，
//!    body `{"item_list":[…]}`——类目 1–5 项、必填且须已在账号后台配置（materials 原样给，
//!    本客户端不猜类目）；`errcode == 0` 即受理。85009 = 已有审核中版本（幂等重发的
//!    正常形态：按已受理续走，不重复提交）。请求带 `Idempotency-Key`（同键重发由网关去重）。
//! 3. **轮询** `GET https://api.weixin.qq.com/wxa/get_latest_auditstatus?access_token=…`：
//!    status 0 通过 / 1 拒绝 / 2 审核中 / 3 已撤回 / 4 审核延后。取「最新一次提审单」，
//!    无需 auditid——断点续走时没有历史状态也能续。
//!
//! 凭据经引用模型注入（[`poly_core::CredentialRef`] + 后端链）；access_token 运行内缓存
//! （提前 5 分钟过期），密钥 / 口令 / token 绝不出现在错误、日志与回执里。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::Value;

use crate::archive::AuditState;
use crate::error::PublishError;
use crate::secret::{Redactor, WechatCredentials, wechat_credentials};
use crate::transport::{self, HttpRequest, Transport};
use crate::{Channel, Job, PollOutcome, Publisher, Submission, Uploaded};

/// access_token 端点（`grant_type=client_credential`）。
const TOKEN_URL: &str = "https://api.weixin.qq.com/cgi-bin/token";
/// 取签名随机串端点。
const RAND_URL: &str = "https://servicewechat.com/wxa/ci/getrandstr";
/// 代码包上传端点。
const UPLOAD_URL: &str = "https://servicewechat.com/wxa/ci/upload";
/// 提审端点。
const SUBMIT_URL: &str = "https://api.weixin.qq.com/wxa/submit_audit";
/// 最新一次提审单状态端点。
const POLL_URL: &str = "https://api.weixin.qq.com/wxa/get_latest_auditstatus";

/// `ci.signature` 里的协议代际字段（对齐 miniprogram-ci 2.1.31）。
pub const CI_PROTOCOL_VERSION: &str = "2.1.31";
/// 容器里签名条目的文件名（pack 后带前导 `/`）。
pub const SIGNATURE_ENTRY: &str = "ci.signature";
/// 小程序项目类型（`project.config.json` 的 type 缺省值）。
const PROJECT_TYPE: &str = "miniProgram";
/// CI 上传的调试场景值（miniprogram-ci 固定 1011）。
const SCENE: u16 = 1011;
/// 代码保护位（T-M3-06 未开启代码保护）。
const CODE_PROTECT: u8 = 0;
/// access_token 提前过期窗口（避免边界失效）。
const TOKEN_MARGIN: Duration = Duration::from_secs(300);

/// 微信 publish 客户端。
pub struct WechatPublisher {
    /// 传输层（真网络 / mock 契约测试注入）。
    transport: Arc<dyn Transport>,
    /// 运行内 access_token 缓存（键 = appid；shortcut: 进程内、单进程假设）。
    tokens: Mutex<HashMap<String, CachedToken>>,
}

/// 缓存条目。
struct CachedToken {
    /// token 值。
    token: String,
    /// 过期时刻（已扣 [`TOKEN_MARGIN`]）。
    expires: Instant,
}

impl WechatPublisher {
    /// 装配客户端（传输层由 CLI / 测试注入）。
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            tokens: Mutex::new(HashMap::new()),
        }
    }

    /// 取凭据（引用模型：按引用经后端链现取）+ 对应脱敏器。
    fn credentials(&self, job: &Job<'_>) -> Result<(WechatCredentials, Redactor), PublishError> {
        let creds = wechat_credentials(job.credential, job.providers)?;
        let redactor = creds.redactor();
        Ok((creds, redactor))
    }

    /// 取 access_token（缓存命中即用）。
    fn access_token(
        &self,
        job: &Job<'_>,
        creds: &WechatCredentials,
        redactor: &Redactor,
    ) -> Result<String, PublishError> {
        if let Some(cached) = self.tokens.lock().expect("token 缓存锁").get(&creds.appid)
            && cached.expires > Instant::now()
        {
            return Ok(cached.token.clone());
        }
        let url = format!(
            "{TOKEN_URL}?grant_type=client_credential&appid={}&secret={}",
            transport::form_encode(&creds.appid),
            transport::form_encode(&creds.secret)
        );
        let response = transport::send(self.transport.as_ref(), &HttpRequest::get(url), redactor)?;
        let json = transport::ok_json(&response, TOKEN_URL, redactor)?;
        api_ok(&json, redactor)?;
        let token = json
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                credential_error(
                    job,
                    creds,
                    "响应缺 access_token 字段（AppSecret 或帐号权限有问题？）".to_string(),
                )
            })?
            .to_string();
        let expires_in = json
            .get("expires_in")
            .and_then(Value::as_u64)
            .unwrap_or(7200);
        let ttl = Duration::from_secs(expires_in).saturating_sub(TOKEN_MARGIN);
        let ttl = ttl.max(Duration::from_secs(60));
        self.tokens.lock().expect("token 缓存锁").insert(
            creds.appid.clone(),
            CachedToken {
                token: token.clone(),
                expires: Instant::now() + ttl,
            },
        );
        Ok(token)
    }
}

impl Publisher for WechatPublisher {
    fn channel(&self) -> Channel {
        Channel::Wechat
    }

    /// 上传：随机串 → 签名 → B 协议容器 + gzip → POST（口径见模块头）。
    fn upload(&self, job: &Job<'_>) -> Result<Uploaded, PublishError> {
        let (creds, redactor) = self.credentials(job)?;

        // 1) 取随机串（签名前置；防重放）。
        let rand_body = format!(
            "{{\"appid\":{},\"clientRand\":{}}}",
            json_str(job.appid),
            client_rand()
        );
        let response = transport::send(
            self.transport.as_ref(),
            &HttpRequest::post(RAND_URL, "application/json", rand_body),
            &redactor,
        )?;
        let rand_json = transport::ok_json(&response, RAND_URL, &redactor)?;
        ci_ok(&rand_json, &redactor)?;
        let random = rand_json
            .pointer("/data/randomString")
            .and_then(Value::as_str)
            .ok_or_else(|| PublishError::Platform {
                channel: Channel::Wechat.id(),
                code: "0".to_string(),
                message: "响应缺 data.randomString 字段".to_string(),
                hint: Some("平台响应形态变了：核对 miniprogram-ci 协议代际".to_string()),
            })?;

        // 2) 代码上传密钥签名（raw PKCS#1 v1.5；Node crypto.privateEncrypt 语义）。
        let message = format!(
            "{{\"appid\":{},\"rand_str\":{}}}",
            json_str(job.appid),
            json_str(random)
        );
        let signature = rsa_private_encrypt(job, &creds, message.as_bytes())?;

        // 3) 容器（文件清单 + ci.signature）→ gzip。
        let signature_entry = format!(
            "{{\"signature\":{},\"version\":{}}}",
            json_str(&signature),
            json_str(CI_PROTOCOL_VERSION)
        );
        let container = pack_container(
            job.package.files(),
            SIGNATURE_ENTRY,
            signature_entry.as_bytes(),
        );
        let body = gzip(&container)?;

        // 4) 上传。
        let debug_launch_info = serde_json::json!({"scene": SCENE}).to_string();
        let query = format!(
            "codeprotect={CODE_PROTECT}&type={PROJECT_TYPE}&appid={}&version={}&desc={}&robot=1&debugLaunchInfo={}",
            transport::form_encode(job.appid),
            transport::form_encode(job.version),
            transport::form_encode(job.description),
            transport::form_encode(&debug_launch_info)
        );
        let url = format!("{UPLOAD_URL}?{query}");
        // 口径同 miniprogram-ci：Buffer body、不显式设 Content-Type。
        let response = transport::send(
            self.transport.as_ref(),
            &HttpRequest::post(url, "", body),
            &redactor,
        )?;
        let json = transport::ok_json(&response, UPLOAD_URL, &redactor)?;
        ci_ok(&json, &redactor)?;
        Ok(Uploaded {
            raw: redactor.redact_value(&json),
        })
    }

    /// 提审：`{"item_list": materials}`；85009 等平台码带修复提示。
    fn submit(&self, job: &Job<'_>) -> Result<Submission, PublishError> {
        let (creds, redactor) = self.credentials(job)?;
        let token = self.access_token(job, &creds, &redactor)?;
        let item_list = job.materials.ok_or_else(|| PublishError::Materials {
            channel: Channel::Wechat.id(),
            reason: "微信提审必须提供类目：materials 需给 item_list 数组（1–5 项，每项含 \
                     address 与 first_class/second_class 等，类目须已在账号后台配置）"
                .to_string(),
        })?;
        validate_item_list(item_list)?;

        let body = serde_json::json!({ "item_list": item_list }).to_string();
        let redactor = with_token(redactor, &token);
        let url = format!(
            "{SUBMIT_URL}?access_token={}",
            transport::form_encode(&token)
        );
        let request = HttpRequest::post(url, "application/json", body).header(
            transport::IDEMPOTENCY_HEADER,
            crate::archive::submission_key(
                Channel::Wechat.id(),
                job.appid,
                job.version,
                job.package.digest(),
            ),
        );
        let response = transport::send(self.transport.as_ref(), &request, &redactor)?;
        let json = transport::ok_json(&response, SUBMIT_URL, &redactor)?;
        // 85009 = 该版本已有审核中的提审单：幂等重发（超时后重试 / 崩溃后重跑）的正常形态，
        // 平台只受理了一次——按 Accepted 续走，不让用户把「已经提审了」当成失败。
        if json.get("errcode").and_then(Value::as_i64) == Some(85009) {
            return Ok(Submission {
                raw: redactor.redact_value(&json),
            });
        }
        api_ok(&json, &redactor)?;
        Ok(Submission {
            raw: redactor.redact_value(&json),
        })
    }

    /// 轮询：最新一次提审单的审核状态（无需 auditid，断点续走友好）。
    fn poll(&self, job: &Job<'_>) -> Result<PollOutcome, PublishError> {
        let (creds, redactor) = self.credentials(job)?;
        let token = self.access_token(job, &creds, &redactor)?;
        let redactor = with_token(redactor, &token);
        let url = format!("{POLL_URL}?access_token={}", transport::form_encode(&token));
        let response = transport::send(self.transport.as_ref(), &HttpRequest::get(url), &redactor)?;
        let json = transport::ok_json(&response, POLL_URL, &redactor)?;
        api_ok(&json, &redactor)?;

        let status =
            json.get("status")
                .and_then(Value::as_i64)
                .ok_or_else(|| PublishError::Platform {
                    channel: Channel::Wechat.id(),
                    code: "0".to_string(),
                    message: "响应缺 status 字段".to_string(),
                    hint: Some("最新一次提审单可能不存在：先确认提审已受理".to_string()),
                })?;
        let state = map_status(status);
        let reason = json
            .get("reason")
            .and_then(Value::as_str)
            .map(|text| redactor.redact(text))
            .or_else(|| {
                (state == AuditState::Unknown)
                    .then(|| format!("未识别状态值 {status}（原始响应见回执 raw）"))
            });
        Ok(PollOutcome {
            state,
            reason,
            raw: redactor.redact_value(&json),
        })
    }
}

/// 把 token 也纳入脱敏集（token 走 query，可能被传输层错误带出）。
fn with_token(mut redactor: Redactor, token: &str) -> Redactor {
    redactor.add(token);
    redactor
}

/// errcode → 规范化状态（官方口径：0 通过 / 1 拒绝 / 2 审核中 / 3 已撤回 / 4 审核延后）。
fn map_status(status: i64) -> AuditState {
    match status {
        0 => AuditState::Approved,
        1 => AuditState::Rejected,
        2 => AuditState::InReview,
        3 => AuditState::Withdrawn,
        4 => AuditState::Delayed,
        _ => AuditState::Unknown,
    }
}

/// 微信 api 错误信封（`errcode` / `errmsg`；0 或缺失 = 成功）。
fn api_ok(json: &Value, redactor: &Redactor) -> Result<(), PublishError> {
    let code = json.get("errcode").and_then(Value::as_i64).unwrap_or(0);
    if code == 0 {
        return Ok(());
    }
    let message = json
        .get("errmsg")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Err(PublishError::Platform {
        channel: Channel::Wechat.id(),
        code: code.to_string(),
        message: redactor.redact(&message),
        hint: api_hint(code),
    })
}

/// CI 通道错误信封（`errCode` / `errMsg`，camelCase；0 或缺失 = 成功）。
fn ci_ok(json: &Value, redactor: &Redactor) -> Result<(), PublishError> {
    let code = json.get("errCode").and_then(Value::as_i64).unwrap_or(0);
    if code == 0 {
        return Ok(());
    }
    let message = json
        .get("errMsg")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Err(PublishError::Platform {
        channel: Channel::Wechat.id(),
        code: code.to_string(),
        message: redactor.redact(&message),
        hint: None,
    })
}

/// 常见错误码的修复提示（按解析到的文档口径）。
fn api_hint(code: i64) -> Option<String> {
    let hint = match code {
        40001 => "access_token 失效：重跑命令（token 缓存随进程重建）",
        40013 => "appid 非法：核对凭据载荷里的 appid 与小程序是否一致",
        85009 => {
            "该版本已有审核中的提审单（提审边界 at-least-once）：等审核结束；\
             或换新版本号重新上传提审（R4：审核结果以平台为准）"
        }
        85008 | 85010 | 85023 => {
            "类目材料不合法：item_list 须给 1–5 项、每项含 address，类目须已在账号后台配置\
             （可用「获取审核时可填写的类目」接口取合法类目）"
        }
        86001 => "平台侧不存在已提交的代码：确认先完成了代码包上传",
        _ => return None,
    };
    Some(hint.to_string())
}

/// 提审材料形态校验（数组、1–5 项、每项是含非空 address 的对象；其余交平台判定）。
fn validate_item_list(item_list: &Value) -> Result<(), PublishError> {
    let bad = |reason: &str| PublishError::Materials {
        channel: Channel::Wechat.id(),
        reason: reason.to_string(),
    };
    let items = item_list
        .as_array()
        .ok_or_else(|| bad("item_list 须是数组（每项一个审核项对象）"))?;
    if items.is_empty() || items.len() > 5 {
        return Err(bad("item_list 项数须在 1–5（平台口径）"));
    }
    for item in items {
        let address = item
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if address.is_empty() {
            return Err(bad("item_list 每项都要有非空 address（提审页面路径）"));
        }
    }
    Ok(())
}

/// 代码上传密钥签名：Node `crypto.privateEncrypt(RSA_PKCS1_PADDING)` 的等价原语。
///
/// 密钥 PEM 两种常见封装（PKCS#8 `BEGIN PRIVATE KEY` / PKCS#1 `BEGIN RSA PRIVATE KEY`）
/// 都接受；错误不携带密钥内容。
fn rsa_private_encrypt(
    job: &Job<'_>,
    creds: &WechatCredentials,
    message: &[u8],
) -> Result<String, PublishError> {
    use rsa::pkcs1::DecodeRsaPrivateKey as _;
    use rsa::pkcs8::DecodePrivateKey as _;

    let pem = creds.private_key.trim();
    let key = rsa::RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| rsa::RsaPrivateKey::from_pkcs1_pem(pem))
        .map_err(|err| {
            credential_error(
                job,
                creds,
                format!("代码上传密钥解析失败（须 PEM 私钥：PKCS#8 或 PKCS#1）：{err}"),
            )
        })?;
    let signature = key
        .sign(rsa::Pkcs1v15Sign::new_unprefixed(), message)
        .map_err(|err| {
            credential_error(
                job,
                creds,
                format!("代码上传密钥签名失败（密钥与消息长度不匹配？）：{err}"),
            )
        })?;
    Ok(base64::engine::general_purpose::STANDARD.encode(signature))
}

/// B 协议容器（miniprogram-ci `pack` 等价）。
///
/// 布局：头 14 字节 `0xBE | i32be(版本=0) | i32be(文件信息段长) | i32be(数据段长) | 0xED`；
/// 文件信息段 = `i32be(条目数)` + 每条 `i32be(名字长) | 名字 | i32be(绝对数据偏移) |
/// i32be(数据长)`（偏移自容器起点、数据段按同序紧排）；数据段 = 各条目内容顺序拼接。
/// 名字统一 `/` 前缀；`extra`（`ci.signature`）追加在尾部（与 miniprogram-ci 后加键同位）。
fn pack_container(files: &[(String, Vec<u8>)], extra_name: &str, extra_body: &[u8]) -> Vec<u8> {
    let mut entries: Vec<(String, &[u8])> = files
        .iter()
        .map(|(name, body)| (container_name(name), body.as_slice()))
        .collect();
    entries.push((container_name(extra_name), extra_body));

    let names_total: usize = entries.iter().map(|(name, _)| name.len()).sum();
    let data_total: usize = entries.iter().map(|(_, body)| body.len()).sum();
    // 文件信息段长含条目数（4B）与每条 12B 定长头。
    let info_len = 4 + 12 * entries.len() + names_total;

    let mut out = Vec::with_capacity(14 + info_len + data_total);
    out.push(0xBE);
    out.extend_from_slice(&0i32.to_be_bytes());
    out.extend_from_slice(&(info_len as i32).to_be_bytes());
    out.extend_from_slice(&(data_total as i32).to_be_bytes());
    out.push(0xED);
    out.extend_from_slice(&(entries.len() as i32).to_be_bytes());
    let mut offset = 14 + info_len;
    for (name, body) in &entries {
        out.extend_from_slice(&(name.len() as i32).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&(offset as i32).to_be_bytes());
        out.extend_from_slice(&(body.len() as i32).to_be_bytes());
        offset += body.len();
    }
    for (_, body) in &entries {
        out.extend_from_slice(body);
    }
    out
}

/// 容器内条目名：反斜杠归一为 `/`，无前导 `/` 则补上。
fn container_name(name: &str) -> String {
    let name = name.replace('\\', "/");
    if name.starts_with('/') {
        name
    } else {
        format!("/{name}")
    }
}

/// gzip（`zlib.gzipSync` 等价；平台只要求合法 gzip，不比对压缩字节）。
fn gzip(data: &[u8]) -> Result<Vec<u8>, PublishError> {
    use std::io::Write as _;

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(data)
        .and_then(|()| encoder.finish())
        .map_err(|source| PublishError::Input {
            path: std::path::PathBuf::from("<gzip>"),
            source,
        })
}

/// 客户端随机数（`clientRand`，< 1e8；系统 RNG，退化为时间）。
fn client_rand() -> u64 {
    use ring::rand::SecureRandom as _;

    let mut bytes = [0u8; 4];
    match ring::rand::SystemRandom::new().fill(&mut bytes) {
        Ok(()) => u64::from(u32::from_be_bytes(bytes)) % 100_000_000,
        Err(_) => crate::time::unix_now() % 100_000_000,
    }
}

/// JSON 字符串字面量（与 `JSON.stringify` 的字符串口径一致，消息字节级可复现）。
fn json_str(value: &str) -> String {
    serde_json::to_string(value).expect("字符串序列化不会失败")
}

/// 组装凭据域错误（名 = 引用名，provider = 命中后端）。
fn credential_error(job: &Job<'_>, creds: &WechatCredentials, reason: String) -> PublishError {
    PublishError::CredentialInvalid {
        name: job.credential.name.clone(),
        provider: creds.source,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping_matches_docs() {
        assert_eq!(map_status(0), AuditState::Approved);
        assert_eq!(map_status(1), AuditState::Rejected);
        assert_eq!(map_status(2), AuditState::InReview);
        assert_eq!(map_status(3), AuditState::Withdrawn);
        assert_eq!(map_status(4), AuditState::Delayed);
        assert_eq!(map_status(9), AuditState::Unknown);
    }

    #[test]
    fn pack_container_layout_matches_miniprogram_ci() {
        let files = vec![
            (
                "app.json".to_string(),
                br#"{"pages":["pages/index/index"]}"#.to_vec(),
            ),
            ("pages/index/index.js".to_string(), b"Page({})".to_vec()),
        ];
        let extra = br#"{"signature":"AA==","version":"2.1.31"}"#;
        let packed = pack_container(&files, SIGNATURE_ENTRY, extra);

        // 手算布局（与 miniprogram-ci pack.js 同式）：头 14B + 信息段 4+12*3+名字总长。
        let names: Vec<String> = vec![
            "/app.json".to_string(),
            "/pages/index/index.js".to_string(),
            "/ci.signature".to_string(),
        ];
        let names_total: usize = names.iter().map(String::len).sum();
        let info_len = 4 + 12 * 3 + names_total;
        assert_eq!(packed[0], 0xBE);
        assert_eq!(&packed[1..5], &0i32.to_be_bytes());
        assert_eq!(&packed[5..9], &(info_len as i32).to_be_bytes());
        let data_total = files[0].1.len() + files[1].1.len() + extra.len();
        assert_eq!(&packed[9..13], &(data_total as i32).to_be_bytes());
        assert_eq!(packed[13], 0xED);
        assert_eq!(&packed[14..18], &3i32.to_be_bytes());

        // 逐条目核对（名 / 绝对偏移 / 长度）。
        let mut cursor = 18usize;
        let mut offset = 14 + info_len;
        for (index, name) in names.iter().enumerate() {
            let name_len = i32::from_be_bytes(packed[cursor..cursor + 4].try_into().unwrap());
            assert_eq!(name_len as usize, name.len());
            assert_eq!(
                &packed[cursor + 4..cursor + 4 + name.len()],
                name.as_bytes()
            );
            let data_offset = i32::from_be_bytes(
                packed[cursor + 4 + name.len()..cursor + 8 + name.len()]
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(data_offset as usize, offset, "条目 {index} 数据偏移");
            let data_len = i32::from_be_bytes(
                packed[cursor + 8 + name.len()..cursor + 12 + name.len()]
                    .try_into()
                    .unwrap(),
            );
            let body = if index == 0 {
                files[0].1.as_slice()
            } else if index == 1 {
                files[1].1.as_slice()
            } else {
                extra.as_slice()
            };
            assert_eq!(data_len as usize, body.len());
            assert_eq!(
                &packed[offset..offset + body.len()],
                body,
                "条目 {index} 数据"
            );
            cursor += 12 + name.len();
            offset += body.len();
        }
        assert_eq!(offset, packed.len(), "数据段正好收尾");
    }

    #[test]
    fn gzip_roundtrips_container() {
        use std::io::Read as _;

        let container = pack_container(
            &[("app.json".to_string(), b"{}".to_vec())],
            "ci.signature",
            b"x",
        );
        let compressed = gzip(&container).expect("gzip");
        assert_eq!(&compressed[..2], &[0x1F, 0x8B], "gzip 魔数");
        let mut decoder = flate2::read::GzDecoder::new(compressed.as_slice());
        let mut restored = Vec::new();
        decoder.read_to_end(&mut restored).expect("gunzip");
        assert_eq!(restored, container);
    }

    #[test]
    fn validate_item_list_rejects_platform_invalid_shapes() {
        let ok = serde_json::json!([{"address": "pages/index/index", "first_class": "教育"}]);
        validate_item_list(&ok).expect("合法 1 项");
        for bad in [
            serde_json::json!({}),
            serde_json::json!([]),
            serde_json::json!([{"address": ""}]),
            serde_json::json!([{"tag": "t"}, {"tag": "t"}, {"tag": "t"}, {"tag": "t"}, {"tag": "t"}, {"tag": "t"}]),
        ] {
            assert!(validate_item_list(&bad).is_err(), "应拒：{bad}");
        }
    }
}
