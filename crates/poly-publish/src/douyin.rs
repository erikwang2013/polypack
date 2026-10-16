//! 抖音开放平台 publish 客户端（T-M3-10）：测试版本创建 → 提审 → 审核状态轮询。
//!
//! 协议口径（官方文档 SSR 原文 + tt-ide-cli 0.1.33 交叉印证，见提交说明；无真账号联调——
//! mock 契约测试钉形态，真跑腿 SKIP-with-print）：
//!
//! 1. **信封**：HTTP 恒 200；`err_no == 0` 为成功；`log_id` 恒有（随回执留档便于对平台排查）。
//! 2. **上传** = `POST /api/apps/v1/package_version/upload/`：body
//!    `{ext_json?, template_id, user_desc, user_version, tag?}`——按 `template_id`
//!    **创建测试版本**（代码包先经开发工具 / 模板库通道，本接口不传包体）；
//!    `template_id` 为 Int64（materials 必填，接受数字或数字字符串）。
//!    幂等：同一 `user_version` 已有版本在审核中 → 11303（本客户端按已在队列续走轮询）。
//! 3. **提审** = 先 `GET /api/apps/v1/package_version/get_audit_hosts/` 取
//!    `data.host_names`（平台前置要求），再 `POST /api/apps/v2/package_version/audit/`
//!    `{host_names, ...}`；11303 视为已在提审队列（at-least-once 续走）。
//! 4. **轮询** = `GET /api/apps/v1/package_version/versions/`（无查询参数）→
//!    `data.audit.status`：0 审核中 / 1 通过 / 2 不通过 / 3 撤回审核；`data.audit` 可为空
//!    （无审核记录 → Unknown，不硬猜）。
//!
//! **凭据**：`access-token` 头（服务商 / 代开发链 `isvact.*`）。token **铸造**链路文档口径
//! 互斥（自研 `developer.toutiao.com` vs 服务商 tpapp 两个候选 URL）且未实测 → 本客户端
//! **不做铸造**，凭据载荷直接注入 `invoke` token：`{"access_token":"…"}`（never fake）。
//!
//! 已知边界（如实注记）：`attach_info` / `attachInfo` 拼写在文档间矛盾——本客户端只解析
//! `status` / `reason`，原始响应整体进回执，不受拼写影响。

use serde_json::Value;

use crate::archive::AuditState;
use crate::error::PublishError;
use crate::secret::{DouyinCredentials, Redactor, douyin_credentials};
use crate::transport::{self, HttpRequest, Method, Transport};
use crate::{Channel, Job, PollOutcome, Publisher, Submission, Uploaded};

/// 测试版本创建（按模板）。
const UPLOAD_URL: &str = "https://open.douyin.com/api/apps/v1/package_version/upload/";
/// 提审前置：可用审核域名列表。
const AUDIT_HOSTS_URL: &str =
    "https://open.douyin.com/api/apps/v1/package_version/get_audit_hosts/";
/// 提审（v2）。
const AUDIT_URL: &str = "https://open.douyin.com/api/apps/v2/package_version/audit/";
/// 版本列表（含审核状态；无查询参数）。
const VERSIONS_URL: &str = "https://open.douyin.com/api/apps/v1/package_version/versions/";
/// 鉴权头（连字符形态，非 Authorization）。
const TOKEN_HEADER: &str = "access-token";
/// 请求体类型。
const JSON_TYPE: &str = "application/json";
/// 信封成功码。
const OK_CODE: i64 = 0;
/// 提审可选透传字段（materials 里给了就进 body）。
const OPTIONAL_AUDIT_FIELDS: [&str; 6] = [
    "audit_note",
    "audit_way",
    "tag",
    "is_force_skip_plugin_app_check",
    "is_force_skip_plugin_jssdk_check",
    "main_category_check_option_material",
];

/// 抖音 publish 客户端。
pub struct DouyinPublisher {
    /// 传输层（真网络 / mock 契约测试注入）。
    transport: std::sync::Arc<dyn Transport>,
}

impl DouyinPublisher {
    /// 装配客户端（传输层由 CLI / 测试注入）。
    pub fn new(transport: std::sync::Arc<dyn Transport>) -> Self {
        Self { transport }
    }

    /// 取凭据（引用模型：按引用经后端链现取）+ 对应脱敏器。
    fn credentials(&self, job: &Job<'_>) -> Result<(DouyinCredentials, Redactor), PublishError> {
        let creds = douyin_credentials(job.credential, job.providers)?;
        let redactor = creds.redactor();
        Ok((creds, redactor))
    }

    /// 平台调用（信封**不**在此检查——可恢复码需按接口分流）。
    fn call(
        &self,
        creds: &DouyinCredentials,
        redactor: &Redactor,
        method: Method,
        url: &str,
        body: Option<String>,
    ) -> Result<Value, PublishError> {
        let request = match method {
            Method::Get => HttpRequest::get(url).header(TOKEN_HEADER, &creds.access_token),
            Method::Post => HttpRequest::post(url, JSON_TYPE, body.unwrap_or_default())
                .header(TOKEN_HEADER, &creds.access_token),
        };
        let response = transport::send(self.transport.as_ref(), &request, redactor)?;
        transport::ok_json(&response, url, redactor)
    }
}

impl Publisher for DouyinPublisher {
    fn channel(&self) -> Channel {
        Channel::Douyin
    }

    /// 测试版本创建（`user_version` = 请求版本号；11303 留给驱动按幂等续走）。
    fn upload(&self, job: &Job<'_>) -> Result<Uploaded, PublishError> {
        let (creds, redactor) = self.credentials(job)?;
        let materials = job.materials.ok_or_else(|| PublishError::Materials {
            channel: Channel::Douyin.id(),
            reason:
                "抖音创建测试版本需要模板 id：materials 需给 {\"template_id\":123}（代码包先经 \
                     开发工具 / 模板库通道；ext_json / tag 可选）"
                    .to_string(),
        })?;

        let mut body = serde_json::Map::new();
        body.insert("template_id".to_string(), template_id(materials)?);
        body.insert(
            "user_version".to_string(),
            Value::String(job.version.to_string()),
        );
        body.insert(
            "user_desc".to_string(),
            Value::String(job.description.to_string()),
        );
        // ext_json 文档口径为 ext.json 内容的字符串形式（严格：须是字符串）。
        if let Some(value) = materials.get("ext_json") {
            match value {
                Value::String(text) => {
                    body.insert("ext_json".to_string(), Value::String(text.clone()));
                }
                _ => {
                    return Err(PublishError::Materials {
                        channel: Channel::Douyin.id(),
                        reason: "materials.ext_json 须是字符串（ext.json 内容的字符串形式）"
                            .to_string(),
                    });
                }
            }
        }
        if let Some(Value::String(tag)) = materials.get("tag") {
            body.insert("tag".to_string(), Value::String(tag.clone()));
        }
        let json = self.call(
            &creds,
            &redactor,
            Method::Post,
            UPLOAD_URL,
            Some(Value::Object(body).to_string()),
        )?;
        // 11303（已有版本在审核中）= 版本已创建 → 断点续走的正常形态，按已上传续走。
        if !envelope_ok(&json) && err_no(&json) != Some(11303) {
            return Err(platform_error(&json, &redactor));
        }
        Ok(Uploaded {
            raw: redactor.redact_value(&json),
        })
    }

    /// 无平台侧构建等待（测试版本创建即就绪）：no-op（驱动在每条路径都会调用，须可重入）。
    fn await_ready(&self, job: &Job<'_>) -> Result<(), PublishError> {
        // 仍先解析凭据：凭据不可用要在触网前暴露（与其它客户端口径一致）。
        let _ = self.credentials(job)?;
        Ok(())
    }

    /// 提审：先取可用审核域名，再提交（11303 = 已在提审队列，按 at-least-once 续走）。
    fn submit(&self, job: &Job<'_>) -> Result<Submission, PublishError> {
        let (creds, redactor) = self.credentials(job)?;
        let hosts = self.call(&creds, &redactor, Method::Get, AUDIT_HOSTS_URL, None)?;
        douyin_ok(&hosts, &redactor)?;
        let host_names: Vec<Value> = hosts
            .pointer("/data/host_names")
            .and_then(Value::as_array)
            .filter(|names| !names.is_empty())
            .cloned()
            .ok_or_else(|| {
                local_platform_error(
                    "0",
                    "提审前置接口未返回可用审核域名（data.host_names 为空或缺失）",
                    "到抖音开放平台确认该应用已配置服务器域名；响应形态变了就核对 get_audit_hosts 文档",
                )
            })?;

        let mut body = serde_json::Map::new();
        body.insert("host_names".to_string(), Value::Array(host_names));
        if let Some(materials) = job.materials {
            for key in OPTIONAL_AUDIT_FIELDS {
                if let Some(value) = materials.get(key)
                    && !value.is_null()
                {
                    body.insert(key.to_string(), value.clone());
                }
            }
        }
        let json = self.call(
            &creds,
            &redactor,
            Method::Post,
            AUDIT_URL,
            Some(Value::Object(body).to_string()),
        )?;
        if !envelope_ok(&json) && err_no(&json) != Some(11303) {
            return Err(platform_error(&json, &redactor));
        }
        Ok(Submission {
            raw: redactor.redact_value(&json),
        })
    }

    /// 轮询：`versions/` → `data.audit.status`（0 审核中 / 1 通过 / 2 不通过 / 3 撤回审核）。
    fn poll(&self, job: &Job<'_>) -> Result<PollOutcome, PublishError> {
        let (creds, redactor) = self.credentials(job)?;
        let json = self.call(&creds, &redactor, Method::Get, VERSIONS_URL, None)?;
        douyin_ok(&json, &redactor)?;
        let Some(Value::Object(audit)) = json.pointer("/data/audit") else {
            return Ok(PollOutcome {
                state: AuditState::Unknown,
                reason: Some("平台无审核版本记录（data.audit 为空）".to_string()),
                raw: redactor.redact_value(&json),
            });
        };
        let status = audit.get("status").and_then(Value::as_i64).ok_or_else(|| {
            local_platform_error(
                "0",
                "版本列表响应的 data.audit 缺 status 字段",
                "响应形态变了：核对 versions 文档（status 0/1/2/3）",
            )
        })?;
        let state = map_status(status);
        let reason = audit
            .get("reason")
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(|text| redactor.redact(text))
            .or_else(|| match state {
                AuditState::Rejected => {
                    Some("平台驳回但未给 reason（原始响应见回执 raw）".to_string())
                }
                AuditState::Unknown => {
                    Some(format!("未识别审核状态值 {status}（原始响应见回执 raw）"))
                }
                _ => None,
            });
        Ok(PollOutcome {
            state,
            reason,
            raw: redactor.redact_value(&json),
        })
    }
}

/// `data.audit.status` → 规范化状态（文档逐字：0 审核中 / 1 通过 / 2 不通过 / 3 撤回审核）。
fn map_status(status: i64) -> AuditState {
    match status {
        0 => AuditState::InReview,
        1 => AuditState::Approved,
        2 => AuditState::Rejected,
        3 => AuditState::Withdrawn,
        _ => AuditState::Unknown,
    }
}

/// 信封检查（`err_no == 0` 为成功；`err_msg` 脱敏后进错误）。
fn douyin_ok(json: &Value, redactor: &Redactor) -> Result<(), PublishError> {
    if envelope_ok(json) {
        return Ok(());
    }
    Err(platform_error(json, redactor))
}

/// 信封成功判据。
fn envelope_ok(json: &Value) -> bool {
    err_no(json) == Some(OK_CODE)
}

/// `err_no` 取值（缺省 None——缺字段视同失败，见 [`douyin_ok`] 的调用方）。
fn err_no(json: &Value) -> Option<i64> {
    json.get("err_no").and_then(Value::as_i64)
}

/// 信封 → 平台错误（`log_id` 进消息便于对平台排查；文本脱敏）。
fn platform_error(json: &Value, redactor: &Redactor) -> PublishError {
    let code = err_no(json);
    let message = json
        .get("err_msg")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let log_id = json
        .get("log_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    PublishError::Platform {
        channel: Channel::Douyin.id(),
        code: code.map(|value| value.to_string()).unwrap_or_default(),
        message: redactor.redact(&format!("{message}（log_id={log_id}）")),
        hint: douyin_hint(code),
    }
}

/// 客户端自产的平台域错误（响应缺字段等）。
fn local_platform_error(code: &str, message: &str, hint: &str) -> PublishError {
    PublishError::Platform {
        channel: Channel::Douyin.id(),
        code: code.to_string(),
        message: message.to_string(),
        hint: Some(hint.to_string()),
    }
}

/// 常见 `err_no` 的修复提示（文档 / tt-ide-cli 口径）。
fn douyin_hint(code: Option<i64>) -> Option<String> {
    let hint = match code? {
        11300 => "系统繁忙：稍后重跑同键命令",
        11301 => "提审过于频繁（平台限制约 10 秒一次）：稍后重跑同键命令（从回执断点续走）",
        11302 => "测试版本未找到：先完成上传阶段（upload）再提审",
        11303 => "已有版本在审核中：本客户端按「已在提审队列」续走轮询（断点续走正常形态）",
        11305 => "主体认证未完成：到开放平台完成主体认证",
        11306 => "小程序基本信息未填写完成：到开放平台补齐基本信息",
        11307 => "请选择正确的应审端：核对应用形态配置",
        11312 => "未配置隐私协议：到开放平台补齐隐私协议",
        28001008 => "access_token 已过期 / 无效：经凭据引用重新注入有效 token（本客户端不做铸造）",
        28001018 => "未授权该接口：核对应用权限（服务商 / 代开发链的 isvact.* 授权范围）",
        10104 => "小程序被封禁：到开放平台申诉",
        10107 => "小程序被封禁：到开放平台申诉",
        10108 => "绑定的开发小程序不能提审：换正式主体的小程序",
        10109 => "小程序名称被重置：到开放平台核对名称后重跑",
        40001 => "参数错误：核对 materials（template_id / user_desc / ext_json 形态）",
        40003 => "找不到第三方应用：核对 access-token 对应的服务商应用",
        40004 => "找不到授权小程序：核对授权关系与 appid",
        40029 => "找不到模板：核对 materials.template_id",
        _ => return None,
    };
    Some(hint.to_string())
}

/// `template_id` 取值：Int64（真实现校验 `^[1-9][0-9]{0,18}$`）——数字或数字字符串都收，
/// 按数字发出；非法形态在触网前明拒。
fn template_id(materials: &Value) -> Result<Value, PublishError> {
    let text = match materials.get("template_id") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => {
            return Err(PublishError::Materials {
                channel: Channel::Douyin.id(),
                reason: "materials.template_id 缺失或形态非法（须为 Int64：数字或数字字符串）"
                    .to_string(),
            });
        }
    };
    let bytes = text.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= 19
        && bytes[0].is_ascii_digit()
        && bytes[0] != b'0'
        && bytes.iter().all(u8::is_ascii_digit);
    let Ok(number) = text.parse::<u64>() else {
        return Err(PublishError::Materials {
            channel: Channel::Douyin.id(),
            reason: format!(
                "materials.template_id 形态非法：{text}（须匹配 ^[1-9][0-9]{{0,18}}$）"
            ),
        });
    };
    if !valid {
        return Err(PublishError::Materials {
            channel: Channel::Douyin.id(),
            reason: format!(
                "materials.template_id 形态非法：{text}（须匹配 ^[1-9][0-9]{{0,18}}$）"
            ),
        });
    }
    Ok(Value::Number(number.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping_matches_docs() {
        assert_eq!(map_status(0), AuditState::InReview);
        assert_eq!(map_status(1), AuditState::Approved);
        assert_eq!(map_status(2), AuditState::Rejected);
        assert_eq!(map_status(3), AuditState::Withdrawn);
        assert_eq!(map_status(9), AuditState::Unknown);
    }

    #[test]
    fn envelope_ok_requires_err_no_zero_and_missing_key_fails() {
        assert!(envelope_ok(
            &serde_json::json!({"err_no": 0, "err_msg": ""})
        ));
        assert!(!envelope_ok(&serde_json::json!({"err_no": 11303})));
        assert!(!envelope_ok(&serde_json::json!({"err_msg": "缺字段"})));
    }

    #[test]
    fn template_id_accepts_int_and_digit_string_rejects_sloppy_forms() {
        assert_eq!(
            template_id(&serde_json::json!({"template_id": 123})).expect("数字"),
            serde_json::json!(123)
        );
        assert_eq!(
            template_id(&serde_json::json!({"template_id": "987"})).expect("数字字符串"),
            serde_json::json!(987)
        );
        for bad in [
            serde_json::json!({}),
            serde_json::json!({"template_id": "0123"}),
            serde_json::json!({"template_id": "12a3"}),
            serde_json::json!({"template_id": "12345678901234567890"}),
            serde_json::json!({"template_id": 1.5}),
        ] {
            let err = template_id(&bad).expect_err("非法模板 id 须拒");
            assert_eq!(err.code(), crate::codes::E_PUB_0004, "{bad}");
        }
    }

    #[test]
    fn platform_error_carries_log_id_and_hint() {
        let redactor = Redactor::new([""]);
        let err = platform_error(
            &serde_json::json!({"err_no": 28001008, "err_msg": "invalid token", "log_id": "20261016xxxx"}),
            &redactor,
        );
        let text = err.to_string();
        assert!(text.contains("28001008"), "{text}");
        assert!(text.contains("log_id=20261016xxxx"), "{text}");
        let hint = err.hint().expect("带提示");
        assert!(hint.contains("access_token"), "{hint}");
    }
}
