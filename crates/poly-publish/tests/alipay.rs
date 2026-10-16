//! T-M3-10 验收（mock HTTP 契约）：支付宝 publish 客户端 版本创建 → 构建等待 → 提审 → 轮询
//! + 回执归档。
//!
//! 口径锚点：官方文档镜像（apiId=1794/1795/1799/1806）+ alipay-sdk v2 请求类源码——本文件
//! 断言网关封套（query 公共参数 / body 业务参数）、**RSA2 签名可被独立复算**（测试侧用公钥
//! 按「去 sign、按键升序 k=v、值不编码」重拼签名串后 RSA-SHA256 验签）、平铺提审字段、
//! `timestamp` 为 GMT+8 `yyyy-MM-dd HH:mm:ss` 形态。
//!
//! 无真平台凭据 / 账号：本文件全部走 [`FakeTransport`]（零网络）；真跑腿见 `publish_real.rs`
//! （`POLY_PUBLISH_REAL_ALIPAY=1`，缺配置即 SKIP 并打印口径）。

mod common;

use std::time::Duration;

use common::{FakeTransport, Recorded, TempRoot};
use poly_core::CredentialRef;
use poly_publish::{
    AlipayPublisher, Archive, AuditState, Channel, HttpResponse, Package, PollPolicy, PublishError,
    PublishRequest, Receipt, SCHEMA_VERSION, codes, publish, receipt_key,
};

/// 形态样本（非真实账号）。
const APP_ID: &str = "2021000000000000";
/// 版本号。
const VERSION: &str = "1.0.0";
/// 版本描述（必进提审 `version_desc`）。
const DESC: &str = "首个版本 v1 描述";
/// 应用授权 token 样本（≥6 字节：会被脱敏器收录并走 query）。
const AUTH_TOKEN: &str = "app-auth-token-abcdef";

const GATEWAY_URL: &str = "https://openapi.alipay.com/gateway.do";
const M_UPLOAD: &str = "alipay.open.mini.version.upload";
const M_BUILD: &str = "alipay.open.mini.version.build.query";
const M_AUDIT: &str = "alipay.open.mini.version.audit.apply";
const M_DETAIL: &str = "alipay.open.mini.version.detail.query";

/// 材料样本（模板 + 提审文本字段；文件类字段一律不出现——给了即拒，见专项用例）。
fn materials() -> serde_json::Value {
    serde_json::json!({
        "template_id": "2012345678901234",
        "template_version": "0.1.0",
        "ext": "{\"env\":\"prod\"}",
        "bundle_id": "com.alipay.alipaywallet",
        "app_name": "聚盒小程序",
        "app_english_name": "Polypack Mini",
        "app_slogan": "一句话标语",
        "app_desc": "这是用于契约测试的应用描述文本。",
        "app_category_ids": "工具_效率;",
        "region_type": "CHINA",
        "service_region_info": {"region": "310000", "area": "110100"},
        "service_phone": "0571-87654321",
        "memo": "契约测试",
    })
}

/// 版本创建受理。
fn upload_ok() -> HttpResponse {
    common::json_ok(serde_json::json!({
        "alipay_open_mini_version_upload_response": {
            "code": "10000", "msg": "Success",
            "version_created": "h1", "build_status": "0", "need_rotation": "false",
        },
        "sign": "x",
    }))
}

/// 构建状态查询响应。
fn build(status: &str) -> HttpResponse {
    common::json_ok(serde_json::json!({
        "alipay_open_mini_version_build_query_response": {
            "code": "10000", "msg": "Success", "build_status": status,
        },
        "sign": "x",
    }))
}

/// 提审受理。
fn audit_ok() -> HttpResponse {
    common::json_ok(serde_json::json!({
        "alipay_open_mini_version_audit_apply_response": {"code": "10000", "msg": "Success"},
        "sign": "x",
    }))
}

/// 版本详情响应。
fn detail(status: &str, reject_reason: &str) -> HttpResponse {
    common::json_ok(serde_json::json!({
        "alipay_open_mini_version_detail_query_response": {
            "code": "10000", "msg": "Success", "status": status,
            "reject_reason": reject_reason,
        },
        "sign": "x",
    }))
}

/// 一次完整流程的平台侧响应（版本创建 → 构建中 ×2 → 提审受理 → 审核中 → 待上架）。
///
/// 审核中那轮的 `reject_reason` 里嵌入 app_auth_token：压「平台自述文本进回执前须脱敏」。
fn approved_script() -> Vec<HttpResponse> {
    vec![
        upload_ok(),
        build("1"),
        build("2"),
        audit_ok(),
        detail(
            "AUDITING",
            &format!("审核中（含 token={AUTH_TOKEN} 压脱敏）"),
        ),
        detail("WAIT_RELEASE", ""),
    ]
}

/// 标准发布请求（轮询间隔 0 → 测试即时；超预算默认 30s）。
fn request(root: &TempRoot, credential: CredentialRef) -> PublishRequest {
    PublishRequest::new(APP_ID, VERSION, DESC, root.dist(), credential)
        .with_materials(materials())
        .with_poll(PollPolicy {
            interval: Duration::ZERO,
            timeout: Duration::from_secs(30),
        })
}

/// 当前代码包的回执键。
fn receipt_key_of(root: &TempRoot) -> String {
    let digest = Package::load(&root.dist())
        .expect("读代码包")
        .digest()
        .to_string();
    receipt_key(APP_ID, VERSION, &digest)
}

/// 支付宝凭据 + 内存后端（引用模型注入；带 app_auth_token 压三方应用形态）。
fn alipay_credential(
    name: &str,
) -> (
    CredentialRef,
    Vec<Box<dyn poly_publish::CredentialProvider>>,
) {
    common::credential(
        name,
        serde_json::json!({
            "app_id": APP_ID,
            "private_key": common::test_private_key_pem(),
            "app_auth_token": AUTH_TOKEN,
        }),
    )
}

/// `k=v&…` 表单串 → 百分号解码后的参数表。
fn params(text: &str) -> Vec<(String, String)> {
    text.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').expect("表单为 k=v");
            (
                percent_encoding::percent_decode_str(key)
                    .decode_utf8()
                    .expect("键为 UTF-8")
                    .into_owned(),
                percent_encoding::percent_decode_str(value)
                    .decode_utf8()
                    .expect("值为 UTF-8")
                    .into_owned(),
            )
        })
        .collect()
}

/// 参数表取值。
fn value_of(pairs: &[(String, String)], key: &str) -> String {
    pairs
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| panic!("缺参数 {key}：{pairs:?}"))
}

/// 独立复算网关签名：query（去 sign）+ body 参数 → 按键升序 `k=v` 以 `&` 连接 → RSA-SHA256 验签。
fn assert_signature(recorded: &Recorded) {
    let query = params(recorded.query());
    let mut sign_pairs: Vec<(String, String)> = query
        .iter()
        .filter(|(key, _)| key != "sign")
        .cloned()
        .collect();
    sign_pairs.extend(params(&recorded.body_text()));
    sign_pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let base = sign_pairs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let sign = value_of(&query, "sign");
    common::verify_rsa_sha256(base.as_bytes(), &sign);
}

/// 公共参数断言（每个网关请求都该有的形态）。
fn assert_common_params(recorded: &Recorded, method: &str) {
    assert_eq!(recorded.endpoint(), GATEWAY_URL, "全部走网关");
    let query = params(recorded.query());
    assert_eq!(value_of(&query, "app_id"), APP_ID);
    assert_eq!(value_of(&query, "method"), method);
    assert_eq!(value_of(&query, "format"), "JSON");
    assert_eq!(value_of(&query, "charset"), "utf-8");
    assert_eq!(value_of(&query, "sign_type"), "RSA2");
    assert_eq!(value_of(&query, "version"), "1.0");
    assert_eq!(
        value_of(&query, "app_auth_token"),
        AUTH_TOKEN,
        "三方应用形态走 query"
    );
    let timestamp = value_of(&query, "timestamp");
    assert_gateway_timestamp(&timestamp);
    // sign 不进 body、也不进签名串；body 只承载业务参数。
    assert_eq!(recorded.header("Accept"), Some("application/json"));
    assert_eq!(
        recorded.header("Content-Type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_signature(recorded);
}

/// `yyyy-MM-dd HH:mm:ss` 形态（GMT+8 换算在 `time` 模块单测里逐日钉死）。
fn assert_gateway_timestamp(value: &str) {
    let bytes = value.as_bytes();
    assert_eq!(bytes.len(), 19, "timestamp 形态：{value}");
    for (index, byte) in bytes.iter().enumerate() {
        match index {
            4 | 7 => assert_eq!(*byte, b'-', "{value}"),
            10 => assert_eq!(*byte, b' ', "{value}"),
            13 | 16 => assert_eq!(*byte, b':', "{value}"),
            _ => assert!(byte.is_ascii_digit(), "{value}"),
        }
    }
}

#[test]
fn full_flow_creates_builds_submits_and_polls_with_signed_gateway_requests() {
    let root = TempRoot::new("ali-full");
    let fake = FakeTransport::new(approved_script());
    let (reference, providers) = alipay_credential("alipay-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    let publisher = AlipayPublisher::new(fake.clone());

    let outcome = publish(&publisher, &request, &providers, &archive).expect("全流程走通");
    assert_eq!(outcome.resumed_from, None, "全新执行");
    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(
        outcome.path,
        archive.receipt_path(Channel::Alipay.id(), &receipt_key_of(&root)),
        "回执路径口径固定"
    );

    // ---- 请求时序：版本创建 → 构建等待 ×2 → 提审 → 轮询 ×2 ----
    let requests = fake.requests();
    assert_eq!(requests.len(), 6, "六次请求");
    for (recorded, method) in requests
        .iter()
        .zip([M_UPLOAD, M_BUILD, M_BUILD, M_AUDIT, M_DETAIL, M_DETAIL])
    {
        assert_common_params(recorded, method);
    }

    // ① 版本创建：biz_content = {app_version, template_id[, template_version, ext, bundle_id]}。
    let upload_body = params(&requests[0].body_text());
    assert_eq!(
        upload_body.len(),
        1,
        "普通接口 body 只有 biz_content 一项：{upload_body:?}"
    );
    let biz: serde_json::Value =
        serde_json::from_str(&value_of(&upload_body, "biz_content")).expect("biz_content 为 JSON");
    assert_eq!(
        biz,
        serde_json::json!({
            "app_version": VERSION,
            "template_id": "2012345678901234",
            "template_version": "0.1.0",
            "ext": "{\"env\":\"prod\"}",
            "bundle_id": "com.alipay.alipaywallet",
        }),
        "版本创建业务参数原样"
    );

    // ② 构建等待 ×2：biz_content 只带 app_version。
    for build in &requests[1..3] {
        let body = params(&build.body_text());
        let biz: serde_json::Value =
            serde_json::from_str(&value_of(&body, "biz_content")).expect("biz_content 为 JSON");
        assert_eq!(biz, serde_json::json!({"app_version": VERSION}));
    }

    // ③ 提审：字段平铺（无 biz_content）、无文件段；version_desc = 请求描述。
    let audit_body = params(&requests[3].body_text());
    assert!(
        !audit_body.iter().any(|(key, _)| key == "biz_content"),
        "audit.apply 无 biz_content（平铺口径）"
    );
    assert_eq!(value_of(&audit_body, "app_version"), VERSION);
    assert_eq!(value_of(&audit_body, "version_desc"), DESC);
    assert_eq!(value_of(&audit_body, "app_name"), "聚盒小程序");
    assert_eq!(value_of(&audit_body, "app_english_name"), "Polypack Mini");
    assert_eq!(value_of(&audit_body, "app_slogan"), "一句话标语");
    assert_eq!(
        value_of(&audit_body, "app_desc"),
        "这是用于契约测试的应用描述文本。"
    );
    assert_eq!(value_of(&audit_body, "app_category_ids"), "工具_效率;");
    assert_eq!(value_of(&audit_body, "region_type"), "CHINA");
    assert_eq!(value_of(&audit_body, "service_phone"), "0571-87654321");
    let region: serde_json::Value =
        serde_json::from_str(&value_of(&audit_body, "service_region_info"))
            .expect("区域为 JSON 串");
    assert_eq!(
        region,
        materials()["service_region_info"],
        "对象按紧凑 JSON 平铺"
    );

    // ④ 轮询：biz_content = {app_version, bundle_id}（materials 有则带）。
    for poll in &requests[4..] {
        assert_eq!(poll.method, poly_publish::Method::Post, "网关恒为 POST");
        let body = params(&poll.body_text());
        let biz: serde_json::Value =
            serde_json::from_str(&value_of(&body, "biz_content")).expect("biz_content 为 JSON");
        assert_eq!(
            biz,
            serde_json::json!({"app_version": VERSION, "bundle_id": "com.alipay.alipaywallet"})
        );
    }

    // ---- 回执：状态流水 + 快照 + 计数 ----
    let receipt = &outcome.receipt;
    assert_eq!(receipt.schema_version, SCHEMA_VERSION);
    assert_eq!(receipt.channel, "alipay-open-platform");
    assert_eq!(receipt.appid, APP_ID);
    assert_eq!(receipt.version, VERSION);
    assert_eq!(receipt.runs, 1);
    assert!(receipt.upload.is_some() && receipt.submit.is_some());
    assert_eq!(
        receipt.upload.as_ref().expect("上传快照")["version_created"],
        "h1",
        "平台版本句柄随快照留档"
    );
    assert_eq!(receipt.polls.len(), 2);
    assert_eq!(receipt.polls[0].state, AuditState::InReview);
    assert_eq!(receipt.polls[0].raw["status"], "AUDITING");
    assert_eq!(receipt.polls[1].state, AuditState::Approved);
    assert_eq!(receipt.polls[1].raw["status"], "WAIT_RELEASE");
    assert_eq!(receipt.polls[1].reason, None, "通过轮不带原因");

    // ---- 脱敏：token 出现在平台自述文本里 → 回执必须已抹除；回执不含任何凭据材料 ----
    let reason = receipt.polls[0].reason.as_deref().expect("审核中带 reason");
    assert!(reason.contains("<脱敏>"), "平台文本先过脱敏器：{reason}");
    assert!(!reason.contains(AUTH_TOKEN), "token 不得落盘：{reason}");
    let file = std::fs::read_to_string(&outcome.path).expect("读回执");
    let key_line = common::test_private_key_pem()
        .lines()
        .nth(1)
        .expect("PEM 有正文行")
        .to_string();
    for needle in [AUTH_TOKEN, key_line.as_str()] {
        assert!(!file.contains(needle), "回执不得含凭据材料");
    }
}

#[test]
fn rerun_after_terminal_is_noop_without_network() {
    let root = TempRoot::new("ali-noop");
    let fake = FakeTransport::new(approved_script());
    let (reference, providers) = alipay_credential("alipay-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    publish(
        &AlipayPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("首跑");
    assert_eq!(fake.count(), 6);

    // 二跑：同键同内容。新传输层脚本为空——一旦触网即报错。
    let second = FakeTransport::new(Vec::new());
    let outcome = publish(
        &AlipayPublisher::new(second.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("终态重跑 = no-op");
    assert_eq!(outcome.resumed_from, Some(AuditState::Approved));
    assert_eq!(outcome.receipt.runs, 2, "重跑如实计数");
    assert_eq!(outcome.receipt.polls.len(), 2, "流水不追加");
    assert_eq!(second.count(), 0, "终态重跑零触网");
}

#[test]
fn resume_from_uploaded_runs_build_wait_then_submits() {
    let root = TempRoot::new("ali-resume-uploaded");
    let archive = Archive::new(root.path());
    // 预置断点：版本已创建、构建未确认（创建后崩溃的恢复路径）。
    let digest = Package::load(&root.dist())
        .expect("读包")
        .digest()
        .to_string();
    let mut seeded = Receipt::new(Channel::Alipay.id(), APP_ID, VERSION, &digest);
    seeded.upload = Some(serde_json::json!({
        "code": "10000", "version_created": "h1", "build_status": "0",
    }));
    archive.save(&seeded).expect("预置回执");
    let key = receipt_key_of(&root);

    let fake = FakeTransport::new(vec![
        common::json_ok(serde_json::json!({
            "alipay_open_mini_version_build_query_response": {"code": "10000", "build_status": "2"},
        })),
        common::json_ok(serde_json::json!({
            "alipay_open_mini_version_audit_apply_response": {"code": "10000", "msg": "Success"},
        })),
        common::json_ok(serde_json::json!({
            "alipay_open_mini_version_detail_query_response": {"code": "10000", "status": "WAIT_RELEASE"},
        })),
    ]);
    let (reference, providers) = alipay_credential("alipay-mini");
    let request = request(&root, reference);
    let outcome = publish(
        &AlipayPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("从 uploaded 续走");

    assert_eq!(outcome.resumed_from, Some(AuditState::Uploaded));
    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(outcome.receipt.runs, 2);

    // 构建等待 → 提审 → 轮询：不重建版本（upload 未出现；await_ready 在续走路径照常经过）。
    let requests = fake.requests();
    assert_eq!(requests.len(), 3);
    for (recorded, method) in requests.iter().zip([M_BUILD, M_AUDIT, M_DETAIL]) {
        assert_common_params(recorded, method);
    }
    assert_eq!(
        archive
            .load(Channel::Alipay.id(), &key)
            .expect("读")
            .map(|r| r.state),
        Some(AuditState::Approved)
    );
}

#[test]
fn version_already_exists_is_treated_as_created() {
    let root = TempRoot::new("ali-exists");
    let mut script = approved_script();
    script[0] = common::json_ok(serde_json::json!({
        "alipay_open_mini_version_upload_response": {
            "code": "40004", "msg": "Business Failed",
            "sub_code": "VERSION_HAS_EXISTED", "sub_msg": "版本已存在",
        },
    }));
    let fake = FakeTransport::new(script);
    let (reference, providers) = alipay_credential("alipay-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    let outcome = publish(
        &AlipayPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("同版本已存在 = 断点续走正常形态");

    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(
        outcome.receipt.upload.as_ref().expect("上传快照")["sub_code"],
        "VERSION_HAS_EXISTED",
        "幂等受理原样留档"
    );
    assert_eq!(fake.count(), 6, "续走全链路");
}

#[test]
fn audit_conflict_sub_code_is_treated_as_already_submitted() {
    let root = TempRoot::new("ali-audit-conflict");
    let mut script = approved_script();
    script[3] = common::json_ok(serde_json::json!({
        "alipay_open_mini_version_audit_apply_response": {
            "code": "40004", "msg": "Business Failed",
            "sub_code": "CAN_NOT_SUBMIT_WITH_AUDIT", "sub_msg": "已有审核中版本",
        },
    }));
    let fake = FakeTransport::new(script);
    let (reference, providers) = alipay_credential("alipay-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    let outcome = publish(
        &AlipayPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("已有审核中版本 = 已在提审队列（at-least-once）");

    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(
        outcome.receipt.submit.as_ref().expect("提审快照")["sub_code"],
        "CAN_NOT_SUBMIT_WITH_AUDIT",
        "幂等受理原样留档"
    );
}

#[test]
fn build_failure_aborts_with_receipt_kept_at_uploaded() {
    let root = TempRoot::new("ali-build-fail");
    let fake = FakeTransport::new(vec![upload_ok(), build("3")]);
    let (reference, providers) = alipay_credential("alipay-mini");
    let request = request(&root, reference);
    let archive = Archive::new(root.path());
    let err = publish(
        &AlipayPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect_err("构建失败须硬错");
    assert_eq!(err.code(), codes::E_PUB_0003);
    match &err {
        PublishError::Platform { code, hint, .. } => {
            assert_eq!(code, "3");
            let hint = hint.as_deref().expect("带修复提示");
            assert!(hint.contains("构建"), "{hint}");
        }
        other => panic!("应为 Platform 错误：{other}"),
    }

    // 止步构建等待：没有提审请求；回执停在 uploaded（重跑只重等构建，不重建版本）。
    assert_eq!(fake.count(), 2, "无提审请求");
    let receipt = archive
        .load(Channel::Alipay.id(), &receipt_key_of(&root))
        .expect("读回执")
        .expect("回执存在");
    assert_eq!(receipt.state, AuditState::Uploaded);
    assert!(receipt.submit.is_none());
}

#[test]
fn file_materials_are_refused_loudly() {
    let root = TempRoot::new("ali-files");
    let fake = FakeTransport::new(vec![upload_ok(), build("2")]);
    let (reference, providers) = alipay_credential("alipay-mini");
    let mut materials = materials();
    materials["app_logo"] = serde_json::json!("logo.png");
    materials["first_screen_shot"] = serde_json::json!("shot.png");
    let request = PublishRequest::new(APP_ID, VERSION, DESC, root.dist(), reference)
        .with_materials(materials)
        .with_poll(PollPolicy {
            interval: Duration::ZERO,
            timeout: Duration::from_secs(30),
        });
    let archive = Archive::new(root.path());
    let err = publish(
        &AlipayPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect_err("文件类材料须明拒（绝不静默漏发）");
    assert_eq!(err.code(), codes::E_PUB_0004);
    let text = err.to_string();
    assert!(text.contains("app_logo"), "{text}");
    assert!(text.contains("first_screen_shot"), "{text}");

    // 止步提审前：没有 audit.apply 请求；回执停在 uploaded。
    assert_eq!(fake.count(), 2, "仅版本创建 + 构建等待");
    let receipt = archive
        .load(Channel::Alipay.id(), &receipt_key_of(&root))
        .expect("读回执")
        .expect("回执存在");
    assert_eq!(receipt.state, AuditState::Uploaded);
}

#[test]
fn missing_template_refuses_before_any_network() {
    let root = TempRoot::new("ali-no-template");
    let fake = FakeTransport::new(Vec::new());
    let (reference, providers) = alipay_credential("alipay-mini");
    let request =
        PublishRequest::new(APP_ID, VERSION, DESC, root.dist(), reference).with_poll(PollPolicy {
            interval: Duration::ZERO,
            timeout: Duration::from_secs(30),
        });
    let archive = Archive::new(root.path());
    let err = publish(
        &AlipayPublisher::new(fake.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect_err("缺 template_id 须拒");
    assert_eq!(err.code(), codes::E_PUB_0004);
    assert!(err.to_string().contains("template_id"), "{err}");
    assert_eq!(fake.count(), 0, "材料问题零触网");
    assert!(
        archive
            .load(Channel::Alipay.id(), &receipt_key_of(&root))
            .expect("读")
            .is_none(),
        "未触网不落回执"
    );
}

#[test]
fn build_wait_timeout_reports_e005_and_resumes_without_recreating_version() {
    let root = TempRoot::new("ali-timeout");
    let fake = FakeTransport::new(vec![upload_ok(), build("1")]);
    let (reference, providers) = alipay_credential("alipay-mini");
    // 预算 0：第一次构建查询（构建中）后即超预算——确定性、不靠睡等。
    let request = request(&root, reference).with_poll(PollPolicy {
        interval: Duration::ZERO,
        timeout: Duration::ZERO,
    });
    let archive = Archive::new(root.path());
    let err = publish(&AlipayPublisher::new(fake), &request, &providers, &archive)
        .expect_err("构建等待超预算须报 E_PUB_0005");
    assert!(matches!(err, PublishError::PollTimeout { .. }), "{err}");
    assert_eq!(err.code(), codes::E_PUB_0005);
    let PublishError::PollTimeout { last, .. } = &err else {
        unreachable!()
    };
    assert!(last.contains("构建"), "{last}");

    // 回执停在 uploaded（构建等待不进 polls 流水——版本状态仍以轮询为准）。
    let key = receipt_key_of(&root);
    let receipt = archive
        .load(Channel::Alipay.id(), &key)
        .expect("读回执")
        .expect("回执存在");
    assert_eq!(receipt.state, AuditState::Uploaded);
    assert!(receipt.polls.is_empty());

    // 续跑：直接重等构建（获批）→ 提审 → 轮询——不重建版本。
    let resume = FakeTransport::new(vec![
        common::json_ok(serde_json::json!({
            "alipay_open_mini_version_build_query_response": {"code": "10000", "build_status": "2"},
        })),
        common::json_ok(serde_json::json!({
            "alipay_open_mini_version_audit_apply_response": {"code": "10000", "msg": "Success"},
        })),
        common::json_ok(serde_json::json!({
            "alipay_open_mini_version_detail_query_response": {"code": "10000", "status": "WAIT_RELEASE"},
        })),
    ]);
    let outcome = publish(
        &AlipayPublisher::new(resume.clone()),
        &request,
        &providers,
        &archive,
    )
    .expect("续走获批");
    assert_eq!(outcome.resumed_from, Some(AuditState::Uploaded));
    assert_eq!(outcome.receipt.state, AuditState::Approved);
    assert_eq!(outcome.receipt.runs, 2);
    let methods: Vec<String> = resume
        .requests()
        .iter()
        .map(|recorded| value_of(&params(recorded.query()), "method"))
        .collect();
    assert_eq!(methods, [M_BUILD, M_AUDIT, M_DETAIL], "续走不重建版本");
}
