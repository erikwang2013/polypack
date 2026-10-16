//! 支付宝开放平台 publish 客户端（T-M3-10）：版本创建（基于模板）→ 平台云构建等待 → 提审
//! → 审核状态轮询。
//!
//! 协议口径（官方文档镜像 apiId=1794/1795/1799/1806 逐字 + alipay-sdk v2 请求类源码级印证，
//! 交叉见提交说明；无真账号联调——mock 契约测试钉形态，真跑腿 SKIP-with-print）：
//!
//! 1. **网关**：`POST https://openapi.alipay.com/gateway.do`。公共参数（`app_id` / `method` /
//!    `format` / `charset` / `sign_type` / `timestamp` / `version` / `app_auth_token`? / `sign`）
//!    走 URL query；业务参数走 body（`application/x-www-form-urlencoded`，普通接口只有
//!    `biz_content=<紧凑 JSON>` 一项；`audit.apply` 例外——业务字段平铺、无 `biz_content`，
//!    见下）。签名 = **RSA2（RSA-SHA256，PKCS#1 v1.5，base64）** over「全部参数（除 `sign`）
//!    按键升序 `k=v` 以 `&` 拼接，值不做 URL 编码」；`timestamp` 为 GMT+8
//!    `yyyy-MM-dd HH:mm:ss`（本 crate 显式按 UTC+8 换算，不依赖本机时区）。
//! 2. **版本创建** `alipay.open.mini.version.upload`（「基于模板上传版本」——代码本体在
//!    平台模板通道，本客户端传 `template_id` + `app_version`；`VERSION_HAS_EXISTED`
//!    视为已上传（断点续走正常形态）。materials = `{"template_id":…[, "template_version",
//!    "ext", "bundle_id"]}`）。
//! 3. **构建等待** `alipay.open.mini.version.build.query`：`build_status` 0 排队 / 1 构建中 /
//!    2 成功 / 3 失败 / 5 超时；等待预算复用 [`crate::PollPolicy`]（`job.poll`）。
//! 4. **提审** `alipay.open.mini.version.audit.apply`：业务字段**平铺为 form 字段**（该接口
//!    无 `biz_content`——文档表单行 + v2 SDK `getTextParams` 两处印证）；`app_version` =
//!    请求版本号、`version_desc` = 请求描述、必需文本字段（`app_name` / `app_english_name` /
//!    `app_slogan` / `app_desc` / `app_category_ids`）从 materials 取，其余文档字段透传。
//!    `CAN_NOT_SUBMIT_WITH_AUDIT(_GRAY)`（已有审核中 / 灰度版本）视为已在提审队列——
//!    提审边界 at-least-once 的正常形态，续走轮询。**文件类材料**（`app_logo` / 截图 /
//!    license 图等 23 个文件段，v2 SDK `getFileParams` 名单）传输未实现：materials 里给了
//!    就**明拒**（E_PUB_0004），绝不静默漏发。
//! 5. **轮询** `alipay.open.mini.version.detail.query` → `status`：INIT 开发中 / AUDITING
//!    审核中 / WAIT_RELEASE 审核通过待上架 / AUDIT_REJECT 审核驳回 / RELEASE 已上架 /
//!    GRAY 灰度中 / OFFLINE 下架 / ONLINE 上架（文档镜像逐字）。
//!
//! 已知边界（如实注记）：响应签（平台公钥验签）未实现（凭据契约未含支付宝公钥）；
//! v3 模型新增状态 `BASE_AUDIT_PASS` / `AUDIT_OFFLINE` 未逐字确证 → 归 Unknown 不硬猜。

use std::time::Instant;

use serde_json::Value;

use crate::archive::AuditState;
use crate::error::PublishError;
use crate::secret::{AlipayCredentials, Redactor, alipay_credentials};
use crate::transport::{self, HttpRequest, Transport};
use crate::{Channel, Job, PollOutcome, Publisher, Submission, Uploaded};

mod proto;

use proto::{
    API_VERSION, CHARSET, FILE_FIELDS, FORM_TYPE, FORMAT, GATEWAY_URL, METHOD_AUDIT_APPLY,
    METHOD_BUILD_QUERY, METHOD_DETAIL_QUERY, METHOD_UPLOAD, OPTIONAL_TEXT_FIELDS,
    REQUIRED_TEXT_FIELDS, SIGN_TYPE, encode_pairs, local_platform_error, map_status, node_ok,
    node_sub_code, platform_error, response_node, rsa2_sign, scalar_field, scalar_text, sign_base,
};

/// 支付宝 publish 客户端。
pub struct AlipayPublisher {
    /// 传输层（真网络 / mock 契约测试注入）。
    transport: std::sync::Arc<dyn Transport>,
}

impl AlipayPublisher {
    /// 装配客户端（传输层由 CLI / 测试注入）。
    pub fn new(transport: std::sync::Arc<dyn Transport>) -> Self {
        Self { transport }
    }

    /// 取凭据（引用模型：按引用经后端链现取）+ 对应脱敏器。
    fn credentials(&self, job: &Job<'_>) -> Result<(AlipayCredentials, Redactor), PublishError> {
        let creds = alipay_credentials(job.credential, job.providers)?;
        let redactor = creds.redactor();
        Ok((creds, redactor))
    }

    /// 网关调用：拼公共参数 + 业务参数 → RSA2 签名 → POST（query = 公共参数，body = 业务参数）。
    ///
    /// 返回**未经信封检查**的网关 JSON——成功 / 可恢复 / 硬错误的分流留给调用方。
    fn call(
        &self,
        job: &Job<'_>,
        creds: &AlipayCredentials,
        redactor: &Redactor,
        method: &str,
        business: &[(String, String)],
    ) -> Result<Value, PublishError> {
        let mut common: Vec<(String, String)> = vec![
            ("app_id".to_string(), creds.app_id.clone()),
            ("method".to_string(), method.to_string()),
            ("format".to_string(), FORMAT.to_string()),
            ("charset".to_string(), CHARSET.to_string()),
            ("sign_type".to_string(), SIGN_TYPE.to_string()),
            (
                "timestamp".to_string(),
                crate::time::gateway_timestamp(crate::time::unix_now()),
            ),
            ("version".to_string(), API_VERSION.to_string()),
        ];
        if let Some(token) = &creds.app_auth_token {
            common.push(("app_auth_token".to_string(), token.clone()));
        }
        let mut signed: Vec<(String, String)> = common.clone();
        signed.extend(business.iter().cloned());
        let sign = rsa2_sign(job, creds, &sign_base(&signed))?;
        common.push(("sign".to_string(), sign));

        let url = format!("{GATEWAY_URL}?{}", encode_pairs(&common));
        let body = encode_pairs(business);
        let response = transport::send(
            self.transport.as_ref(),
            &HttpRequest::post(url, FORM_TYPE, body).header("Accept", "application/json"),
            redactor,
        )?;
        transport::ok_json(&response, GATEWAY_URL, redactor)
    }
}

impl Publisher for AlipayPublisher {
    fn channel(&self) -> Channel {
        Channel::Alipay
    }

    /// 版本创建：`biz_content = {app_version, template_id[, template_version, ext, bundle_id]}`。
    fn upload(&self, job: &Job<'_>) -> Result<Uploaded, PublishError> {
        let (creds, redactor) = self.credentials(job)?;
        let materials = job.materials.ok_or_else(|| PublishError::Materials {
            channel: Channel::Alipay.id(),
            reason: "支付宝版本创建（基于模板上传版本）需要模板 id：materials 需给 \
                     {\"template_id\":\"…\"}（template_version / ext / bundle_id 可选）"
                .to_string(),
        })?;

        let mut biz = serde_json::Map::new();
        biz.insert(
            "app_version".to_string(),
            Value::String(job.version.to_string()),
        );
        biz.insert(
            "template_id".to_string(),
            Value::String(scalar_field(materials, "template_id", true)?.expect("必填")),
        );
        for key in ["template_version", "ext", "bundle_id"] {
            if let Some(value) = scalar_field(materials, key, false)? {
                biz.insert(key.to_string(), Value::String(value));
            }
        }
        let business = vec![("biz_content".to_string(), Value::Object(biz).to_string())];

        let json = self.call(job, &creds, &redactor, METHOD_UPLOAD, &business)?;
        let node = response_node(&json, METHOD_UPLOAD)?;
        if !node_ok(node) && node_sub_code(node) != Some("VERSION_HAS_EXISTED") {
            return Err(platform_error(node, &redactor));
        }
        Ok(Uploaded {
            raw: redactor.redact_value(node),
        })
    }

    /// 构建等待：`build.query` 轮询（0 / 1 → 按 `job.poll` 预算等待；2 完成；3 / 5 硬错）。
    fn await_ready(&self, job: &Job<'_>) -> Result<(), PublishError> {
        let (creds, redactor) = self.credentials(job)?;
        let business = vec![(
            "biz_content".to_string(),
            serde_json::json!({ "app_version": job.version }).to_string(),
        )];
        let started = Instant::now();
        loop {
            let json = self.call(job, &creds, &redactor, METHOD_BUILD_QUERY, &business)?;
            let node = response_node(&json, METHOD_BUILD_QUERY)?;
            if !node_ok(node) {
                return Err(platform_error(node, &redactor));
            }
            let status = node
                .get("build_status")
                .and_then(scalar_text)
                .ok_or_else(|| {
                    local_platform_error(
                        "0",
                        "构建状态响应缺 build_status 字段",
                        "响应形态变了：核对 build.query 文档（0/1/2/3/5）",
                    )
                })?;
            match status.as_str() {
                "2" => return Ok(()),
                "0" | "1" => {
                    if started.elapsed() >= job.poll.timeout {
                        return Err(PublishError::PollTimeout {
                            channel: Channel::Alipay.id(),
                            last: format!("平台云构建未就绪（build_status={status}）"),
                        });
                    }
                    std::thread::sleep(job.poll.interval);
                }
                "3" => {
                    return Err(local_platform_error(
                        "3",
                        "平台云构建失败（build_status=3）",
                        "到开放平台后台看构建日志（模板 / 依赖问题）；修复后换版本号重跑",
                    ));
                }
                "5" => {
                    return Err(local_platform_error(
                        "5",
                        "平台云构建超时（build_status=5）",
                        "稍后重跑同键命令：只重试构建等待，不重建版本",
                    ));
                }
                other => {
                    return Err(local_platform_error(
                        "0",
                        &format!("未识别的 build_status：{other}"),
                        "协议口径变了：核对 build.query 文档（0 排队 / 1 构建中 / 2 成功 / 3 失败 / 5 超时）",
                    ));
                }
            }
        }
    }

    /// 提审：业务字段平铺（`app_version` + `version_desc` + 必需 / 可选文本字段）。
    fn submit(&self, job: &Job<'_>) -> Result<Submission, PublishError> {
        let (creds, redactor) = self.credentials(job)?;
        let materials = job.materials.ok_or_else(|| PublishError::Materials {
            channel: Channel::Alipay.id(),
            reason: "支付宝提审需要材料：app_name / app_english_name / app_slogan / app_desc / \
                     app_category_ids（类别用「一级_二级;…」格式；region_type / service_region_info \
                     / service_phone|service_email 等平台要求项按账号实际透传）"
                .to_string(),
        })?;
        let files: Vec<&str> = FILE_FIELDS
            .iter()
            .copied()
            .filter(|key| materials.get(key).is_some())
            .collect();
        if !files.is_empty() {
            return Err(PublishError::Materials {
                channel: Channel::Alipay.id(),
                reason: format!(
                    "文件类材料（{}）传输未实现（multipart 文件段；段名已确证但本客户端未发文件）\
                     ——绝不静默漏发：请从 materials 移除，或在平台后台补齐后重跑",
                    files.join(" / ")
                ),
            });
        }
        if job.description.trim().is_empty() {
            return Err(PublishError::Materials {
                channel: Channel::Alipay.id(),
                reason:
                    "version_desc（版本描述）为空：请求的版本描述字段必填（平台要求 30–500 字）"
                        .to_string(),
            });
        }

        // 该接口无 biz_content：字段平铺（文档表单行 + v2 SDK getTextParams 两处印证）。
        let mut fields: Vec<(String, String)> = vec![
            ("app_version".to_string(), job.version.to_string()),
            ("version_desc".to_string(), job.description.to_string()),
        ];
        for key in REQUIRED_TEXT_FIELDS {
            fields.push((
                key.to_string(),
                scalar_field(materials, key, true)?.expect("必填"),
            ));
        }
        for key in OPTIONAL_TEXT_FIELDS {
            if let Some(value) = scalar_field(materials, key, false)? {
                fields.push((key.to_string(), value));
            }
        }

        let json = self.call(job, &creds, &redactor, METHOD_AUDIT_APPLY, &fields)?;
        let node = response_node(&json, METHOD_AUDIT_APPLY)?;
        if !node_ok(node)
            && !matches!(
                node_sub_code(node),
                Some("CAN_NOT_SUBMIT_WITH_AUDIT") | Some("CAN_NOT_SUBMIT_WITH_AUDIT_GRAY")
            )
        {
            return Err(platform_error(node, &redactor));
        }
        Ok(Submission {
            raw: redactor.redact_value(node),
        })
    }

    /// 轮询：`detail.query` → `status`（INIT / AUDITING / WAIT_RELEASE / AUDIT_REJECT /
    /// RELEASE / GRAY / OFFLINE / ONLINE）。
    fn poll(&self, job: &Job<'_>) -> Result<PollOutcome, PublishError> {
        let (creds, redactor) = self.credentials(job)?;
        let mut biz = serde_json::Map::new();
        biz.insert(
            "app_version".to_string(),
            Value::String(job.version.to_string()),
        );
        if let Some(materials) = job.materials
            && let Some(bundle) = scalar_field(materials, "bundle_id", false)?
        {
            biz.insert("bundle_id".to_string(), Value::String(bundle));
        }
        let business = vec![("biz_content".to_string(), Value::Object(biz).to_string())];

        let json = self.call(job, &creds, &redactor, METHOD_DETAIL_QUERY, &business)?;
        let node = response_node(&json, METHOD_DETAIL_QUERY)?;
        if !node_ok(node) {
            return Err(platform_error(node, &redactor));
        }
        let status = node.get("status").and_then(Value::as_str).ok_or_else(|| {
            local_platform_error(
                "0",
                "版本详情响应缺 status 字段",
                "版本可能不存在：先完成版本创建；响应形态变了就核对 detail.query 文档",
            )
        })?;
        let state = map_status(status);
        let reason = node
            .get("reject_reason")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| redactor.redact(text))
            .or_else(|| match state {
                AuditState::Rejected => {
                    Some("平台驳回但未给 reject_reason（原始响应见回执 raw）".to_string())
                }
                AuditState::Unknown => Some(format!("未识别状态值 {status}（原始响应见回执 raw）")),
                _ => None,
            });
        Ok(PollOutcome {
            state,
            reason,
            raw: redactor.redact_value(node),
        })
    }
}
