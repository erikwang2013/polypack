//! 支付宝客户端协议层（T-M3-10）：网关封套 / RSA2 签名 / 字段名单 / 状态映射——
//! 协议常量与纯函数全部收敛在本模块（客户端实现见 [`super`]）。

use serde_json::Value;

use crate::archive::AuditState;
use crate::error::PublishError;
use crate::secret::{AlipayCredentials, Redactor};
use crate::transport;
use crate::{Channel, Job};

/// 开放平台网关（沙箱为 openapi.alipaydev.com；本客户端只走生产网关）。
pub(super) const GATEWAY_URL: &str = "https://openapi.alipay.com/gateway.do";
/// 版本创建（基于模板上传版本）。
pub(super) const METHOD_UPLOAD: &str = "alipay.open.mini.version.upload";
/// 构建状态查询（平台云构建）。
pub(super) const METHOD_BUILD_QUERY: &str = "alipay.open.mini.version.build.query";
/// 提审（字段平铺，无 biz_content）。
pub(super) const METHOD_AUDIT_APPLY: &str = "alipay.open.mini.version.audit.apply";
/// 版本详情 / 审核状态查询。
pub(super) const METHOD_DETAIL_QUERY: &str = "alipay.open.mini.version.detail.query";
/// 网关接口版本（固定）。
pub(super) const API_VERSION: &str = "1.0";
/// 请求格式（仅支持 JSON）。
pub(super) const FORMAT: &str = "JSON";
/// 编码（官方示例口径）。
pub(super) const CHARSET: &str = "utf-8";
/// 签名算法（RSA2 = RSA-SHA256）。
pub(super) const SIGN_TYPE: &str = "RSA2";
/// 网关成功码。
const SUCCESS_CODE: &str = "10000";
/// 网关级错误的顶层节点名（与 `<method>_response` 二选一）。
const ERROR_KEY: &str = "error_response";
/// 网关表单类型（普通 / 平铺业务参数）。
pub(super) const FORM_TYPE: &str = "application/x-www-form-urlencoded";

/// 提审必需文本字段（文档镜像「必须」列；文件类字段见 [`FILE_FIELDS`]）。
pub(super) const REQUIRED_TEXT_FIELDS: [&str; 5] = [
    "app_name",
    "app_english_name",
    "app_slogan",
    "app_desc",
    "app_category_ids",
];
/// 提审可选文本字段（v2 SDK `getTextParams` 名单，原样透传不走样）。
pub(super) const OPTIONAL_TEXT_FIELDS: [&str; 15] = [
    "bundle_id",
    "memo",
    "region_type",
    "service_region_info",
    "service_phone",
    "service_email",
    "license_name",
    "license_no",
    "license_valid_date",
    "audit_rule",
    "auto_online",
    "mini_category_ids",
    "speed_up",
    "test_accout",
    "test_password",
];
/// 提审文件类字段（v2 SDK `getFileParams` 逐字名单）：multipart 文件段传输未实现 → 明拒。
pub(super) const FILE_FIELDS: [&str; 23] = [
    "app_logo",
    "first_screen_shot",
    "second_screen_shot",
    "third_screen_shot",
    "fourth_screen_shot",
    "fifth_screen_shot",
    "first_license_pic",
    "second_license_pic",
    "third_license_pic",
    "fourth_license_pic",
    "fifth_license_pic",
    "first_special_license_pic",
    "second_special_license_pic",
    "third_special_license_pic",
    "fourth_special_license_pic",
    "fifth_special_license_pic",
    "sixth_special_license_pic",
    "seventh_special_license_pic",
    "eighth_special_license_pic",
    "ninth_special_license_pic",
    "tenth_special_license_pic",
    "out_door_pic",
    "test_file_name",
];

/// 签名串：全部参数（除 `sign`）按键升序 `k=v` 以 `&` 连接；**值不做 URL 编码**（官方口径）。
pub(super) fn sign_base(params: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = params.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    sorted
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// 参数列表 → 百分号编码表单串（发送用；签名串另见 [`sign_base`]）。
pub(super) fn encode_pairs(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                transport::form_encode(key),
                transport::form_encode(value)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// RSA2 签名（RSA-SHA256 / PKCS#1 v1.5 / base64）；应用私钥支持 PEM PKCS#8 / PKCS#1
/// 及去头尾的一行 base64（支付宝控制台常见交付形态）。错误不携带密钥材料。
pub(super) fn rsa2_sign(
    job: &Job<'_>,
    creds: &AlipayCredentials,
    message: &str,
) -> Result<String, PublishError> {
    use base64::Engine as _;
    use rsa::pkcs8::EncodePrivateKey as _;

    let key = parse_private_key(job, creds)?;
    let der = key
        .to_pkcs8_der()
        .map_err(|err| credential_error(job, creds, format!("应用私钥 PKCS#8 编码失败：{err}")))?;
    let pair = ring::signature::RsaKeyPair::from_pkcs8(der.as_bytes()).map_err(|_| {
        credential_error(
            job,
            creds,
            "应用私钥不是可用 RSA 密钥（ring 拒绝）".to_string(),
        )
    })?;
    let mut signature = vec![0u8; pair.public().modulus_len()];
    pair.sign(
        &ring::signature::RSA_PKCS1_SHA256,
        &ring::rand::SystemRandom::new(),
        message.as_bytes(),
        &mut signature,
    )
    .map_err(|_| credential_error(job, creds, "RSA2 签名失败".to_string()))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(signature))
}

/// 私钥解析（多形态探测；失败只给形态提示、不回显任何密钥内容）。
fn parse_private_key(
    job: &Job<'_>,
    creds: &AlipayCredentials,
) -> Result<rsa::RsaPrivateKey, PublishError> {
    use base64::Engine as _;
    use rsa::pkcs1::DecodeRsaPrivateKey as _;
    use rsa::pkcs8::DecodePrivateKey as _;

    let pem = creds.private_key.trim();
    if let Ok(key) = rsa::RsaPrivateKey::from_pkcs8_pem(pem) {
        return Ok(key);
    }
    if let Ok(key) = rsa::RsaPrivateKey::from_pkcs1_pem(pem) {
        return Ok(key);
    }
    if let Ok(der) = base64::engine::general_purpose::STANDARD.decode(pem) {
        if let Ok(key) = rsa::RsaPrivateKey::from_pkcs8_der(&der) {
            return Ok(key);
        }
        if let Ok(key) = rsa::RsaPrivateKey::from_pkcs1_der(&der) {
            return Ok(key);
        }
    }
    Err(credential_error(
        job,
        creds,
        "应用私钥解析失败（支持 PEM PKCS#8 / PKCS#1，或去头尾回车的一行 base64）".to_string(),
    ))
}

/// 取网关信封节点：`<method 的 . 全替换为 _>_response`，网关级错误回退 `error_response`。
pub(super) fn response_node<'a>(json: &'a Value, method: &str) -> Result<&'a Value, PublishError> {
    let key = format!("{}_response", method.replace('.', "_"));
    json.get(&key)
        .or_else(|| json.get(ERROR_KEY))
        .ok_or_else(|| {
            local_platform_error(
                "0",
                &format!("网关响应缺少 {key} 节点"),
                "响应形态变了：核对开放平台封套（<method>_response / error_response）",
            )
        })
}

/// 信封成功判据：`code == "10000"`。
pub(super) fn node_ok(node: &Value) -> bool {
    node.get("code").and_then(Value::as_str) == Some(SUCCESS_CODE)
}

/// 业务返回码（`sub_code`；缺省 None）。
pub(super) fn node_sub_code(node: &Value) -> Option<&str> {
    node.get("sub_code").and_then(Value::as_str)
}

/// 信封 → 平台错误（`sub_code` 优先作码；文本脱敏）。
pub(super) fn platform_error(node: &Value, redactor: &Redactor) -> PublishError {
    let code = node.get("code").and_then(Value::as_str).unwrap_or_default();
    let sub_code = node_sub_code(node);
    let msg = node.get("msg").and_then(Value::as_str).unwrap_or_default();
    let sub_msg = node
        .get("sub_msg")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let message = if sub_msg.is_empty() {
        msg.to_string()
    } else {
        format!("{msg}：{sub_msg}")
    };
    PublishError::Platform {
        channel: Channel::Alipay.id(),
        code: sub_code.unwrap_or(code).to_string(),
        message: redactor.redact(&message),
        hint: alipay_hint(sub_code),
    }
}

/// 客户端自产的平台域错误（构建状态异常 / 响应缺字段等）。
pub(super) fn local_platform_error(code: &str, message: &str, hint: &str) -> PublishError {
    PublishError::Platform {
        channel: Channel::Alipay.id(),
        code: code.to_string(),
        message: message.to_string(),
        hint: Some(hint.to_string()),
    }
}

/// 常见 `sub_code` 的修复提示（文档镜像 / SDK 源码口径）。
fn alipay_hint(sub_code: Option<&str>) -> Option<String> {
    let hint = match sub_code? {
        "VERSION_HAS_EXISTED" => {
            "该版本号已存在：本客户端按「已上传」续走（断点续走的正常形态）；\
             若平台侧版本的代码内容非本次构建产物，以平台侧为准（换版本号重跑）"
        }
        "LARGER_VERSION_HAS_EXISTED" => {
            "已存在更大的版本号：版本号必须高于历史最新（换版本号重跑）"
        }
        "CAN_NOT_SUBMIT_WITH_AUDIT" | "CAN_NOT_SUBMIT_WITH_AUDIT_GRAY" => {
            "已有审核中 / 灰度中的版本（提审边界 at-least-once）：本客户端按「已在提审队列」\
             续走轮询；若此处仍报错，说明版本被其它流程占用"
        }
        "CAN_NOT_SUBMIT_WITHOUT_INIT" => {
            "非开发版不能提交审核：确认先完成版本创建（upload 阶段）且未被其它流程改状态"
        }
        "CAN_NOT_SUBMIT_WITHOUT_SCAN_PASS" => {
            "安全扫描未通过：等扫描通过（detail.query 的 scan_result）后重跑同键命令"
        }
        "EXIST_LARGER_ONLINED_VERSION" => "存在更大的已上架版本：换更高版本号",
        "NO_AUTH_RELATION" => {
            "托管小程序与三方应用无授权关系：该接口需应用授权 token（app_auth_token）——\
             检查凭据载荷与平台授权状态"
        }
        "ISV_APP_ID_IS_BLANK" => "该接口需要带应用授权 token 调用：凭据载荷补 app_auth_token",
        "CODE_TEMPLATE_NOT_EXIST" => {
            "代码模板不存在：核对 materials.template_id（须是该应用可用的模板）"
        }
        "CODE_TEMPLATE_NOT_SAFE" => "代码模板未过安全扫描：等模板安全扫描通过后重试",
        "CODE_TEMPLATE_DELETED" => "代码模板已删除：换可用模板",
        "APP_VERSION_IS_BLANK" => "版本号为空：核对请求的版本号字段",
        "SYSTEM_ERROR" => "平台系统繁忙：稍后重跑同键命令（从回执断点续走）",
        _ => return None,
    };
    Some(hint.to_string())
}

/// `status` → 规范化状态（文档镜像逐字：INIT/AUDITING/WAIT_RELEASE/AUDIT_REJECT/
/// RELEASE/GRAY/OFFLINE/ONLINE；v3 新增 BASE_AUDIT_PASS / AUDIT_OFFLINE 未逐字确证 → Unknown）。
pub(super) fn map_status(status: &str) -> AuditState {
    match status {
        "INIT" => AuditState::Uploaded,
        "AUDITING" => AuditState::InReview,
        "WAIT_RELEASE" => AuditState::Approved,
        "AUDIT_REJECT" => AuditState::Rejected,
        "RELEASE" | "GRAY" | "ONLINE" => AuditState::Released,
        "OFFLINE" => AuditState::Withdrawn,
        _ => AuditState::Unknown,
    }
}

/// materials 标量字段取值：字符串原样（必填须非空）；数字 / 布尔转字符串；
/// 对象 / 数组按紧凑 JSON 字符串平铺（v2 SDK `JSONWriter` 同口径，如 `service_region_info`）。
pub(super) fn scalar_field(
    materials: &Value,
    key: &str,
    required: bool,
) -> Result<Option<String>, PublishError> {
    let bad = |reason: String| PublishError::Materials {
        channel: Channel::Alipay.id(),
        reason,
    };
    match materials.get(key) {
        None | Some(Value::Null) => {
            if required {
                Err(bad(format!("materials 缺必填字段 {key}")))
            } else {
                Ok(None)
            }
        }
        Some(Value::String(text)) => {
            if required && text.trim().is_empty() {
                Err(bad(format!("materials.{key} 为空")))
            } else {
                Ok(Some(text.clone()))
            }
        }
        Some(value @ (Value::Number(_) | Value::Bool(_) | Value::Array(_) | Value::Object(_))) => {
            Ok(Some(value.to_string()))
        }
    }
}

/// `build_status` / 其他标量取值（文档为字符串 `"0"`…；容忍数字形态）。
pub(super) fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(_) => Some(value.to_string()),
        _ => None,
    }
}

/// 组装凭据域错误（名 = 引用名，provider = 命中后端）。
fn credential_error(job: &Job<'_>, creds: &AlipayCredentials, reason: String) -> PublishError {
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
        assert_eq!(map_status("INIT"), AuditState::Uploaded);
        assert_eq!(map_status("AUDITING"), AuditState::InReview);
        assert_eq!(map_status("WAIT_RELEASE"), AuditState::Approved);
        assert_eq!(map_status("AUDIT_REJECT"), AuditState::Rejected);
        assert_eq!(map_status("RELEASE"), AuditState::Released);
        assert_eq!(map_status("GRAY"), AuditState::Released);
        assert_eq!(map_status("ONLINE"), AuditState::Released);
        assert_eq!(map_status("OFFLINE"), AuditState::Withdrawn);
        // v3 新增值未逐字确证 → 不硬猜。
        assert_eq!(map_status("BASE_AUDIT_PASS"), AuditState::Unknown);
        assert_eq!(map_status("AUDIT_OFFLINE"), AuditState::Unknown);
        assert_eq!(map_status("WHAT"), AuditState::Unknown);
    }

    #[test]
    fn sign_base_sorts_and_keeps_values_raw() {
        let params = vec![
            ("sign_type".to_string(), "RSA2".to_string()),
            ("b".to_string(), "v b".to_string()),
            ("app_id".to_string(), "2021000000000000".to_string()),
        ];
        assert_eq!(
            sign_base(&params),
            "app_id=2021000000000000&b=v b&sign_type=RSA2"
        );
        // 发送时值做编码，但签名串保持原样（两边分口径）。
        assert_eq!(
            encode_pairs(&[("b".to_string(), "v b".to_string())]),
            "b=v%20b"
        );
    }

    #[test]
    fn response_node_prefers_method_key_and_falls_back_to_error_response() {
        let ok = serde_json::json!({
            "alipay_open_mini_version_detail_query_response": {"code": "10000", "status": "INIT"},
            "sign": "x",
        });
        let node = response_node(&ok, METHOD_DETAIL_QUERY).expect("取节点");
        assert!(node_ok(node));
        assert_eq!(node["status"], "INIT");

        let gateway_error = serde_json::json!({
            "error_response": {"code": "40002", "msg": "Invalid Arguments", "sub_code": "isv.invalid-app-id"},
        });
        let node = response_node(&gateway_error, METHOD_UPLOAD).expect("回退 error_response");
        assert!(!node_ok(node));
        assert_eq!(node_sub_code(node), Some("isv.invalid-app-id"));

        let missing = serde_json::json!({"other": 1});
        let err = response_node(&missing, METHOD_UPLOAD).expect_err("缺节点须拒");
        assert_eq!(err.code(), crate::codes::E_PUB_0003);
    }

    #[test]
    fn scalar_field_names_missing_required_and_missing_files_are_named() {
        let materials = serde_json::json!({"template_id": 123456, "ext": {"a": 1}});
        assert_eq!(
            scalar_field(&materials, "template_id", true).expect("数字转字符串"),
            Some("123456".to_string())
        );
        assert_eq!(
            scalar_field(&materials, "ext", false).expect("对象转紧凑 JSON"),
            Some("{\"a\":1}".to_string())
        );
        let err = scalar_field(&materials, "app_name", true).expect_err("缺必填须拒");
        assert!(err.to_string().contains("app_name"), "{err}");

        let with_file = serde_json::json!({"app_logo": "logo.png"});
        let files: Vec<&str> = FILE_FIELDS
            .iter()
            .copied()
            .filter(|key| with_file.get(key).is_some())
            .collect();
        assert_eq!(files, ["app_logo"], "文件类字段按名单识别");
    }
}
