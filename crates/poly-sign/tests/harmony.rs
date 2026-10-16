//! T-M3-04 验收（计划层）：HAP 签名的工具解析、`app_cert` / `profile` 强校验、输出命名。
//!
//! 真跑签名（sign-app → verify-app）在 `poly-cli` 验收走查里以真实 HAP 执行并存证
//! （见 T-M3-04 报告）；本文件只锚定不执行工具的契约面，宿主缺 java / hap-sign-tool 时显式
//! SKIP（见 `common` 原则），绝不伪造真跑。

mod common;

use std::path::{Path, PathBuf};

use base64::Engine as _;
use poly_core::{CredentialRef, CredentialSource};
use poly_sign::codes;
use poly_sign::harmony::{HarmonySignRequest, HarmonySigner, signed_output};
use poly_sign::secret::{KEYRING_ID, MemoryBackend, SecretBackend};

/// 测试口令（仅测试用，非真实凭据）。
const PASS: &str = "hap-sign-pass";

/// hap-sign-tool 工具对可用则返回 true，否则打印 SKIP 原因（调用方随即 `return`）。
fn hap_tools_or_skip() -> bool {
    match poly_sign::harmony::tool_paths() {
        Ok(_) => true,
        Err(err) => {
            eprintln!("SKIP：{err}（java + hap-sign-tool 就绪的宿主上会走真跑分支）");
            false
        }
    }
}

/// HAP 载荷（base64(JSON)）：`app_cert` / `profile` 按需注入，值只求字段在场。
fn hap_payload(alias: Option<&str>, app_cert: bool, profile: bool) -> Vec<u8> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut json = serde_json::json!({
        "keystore": b64.encode(b"p12-fixture"),
        "store_password": PASS,
    });
    if let Some(alias) = alias {
        json["key_alias"] = serde_json::Value::from(alias);
    }
    if app_cert {
        json["app_cert"] = serde_json::Value::from(b64.encode(b"cert-fixture"));
    }
    if profile {
        json["profile"] = serde_json::Value::from(b64.encode(b"p7b-fixture"));
    }
    b64.encode(serde_json::to_vec(&json).expect("载荷 JSON 可序列化"))
        .into_bytes()
}

/// 签名器：内存后端冒充 keyring 注入载荷（headless CI 无 Secret Service 的注入路径）。
fn signer_for(name: &str, payload: Vec<u8>) -> HarmonySigner {
    HarmonySigner::new(vec![
        Box::new(MemoryBackend::with_entry(name, payload).with_id(KEYRING_ID))
            as Box<dyn SecretBackend>,
    ])
}

fn request(input: &Path, out: Option<PathBuf>) -> HarmonySignRequest {
    HarmonySignRequest {
        input: input.to_path_buf(),
        out,
        credential: CredentialRef {
            name: "hap-release".to_string(),
            source: CredentialSource::Keyring,
        },
    }
}

/// 未签名 HAP 占位夹具（计划层不解析 zip，只要求普通文件在场）。
fn write_hap_stub(path: &Path) {
    std::fs::write(path, b"hap-stub").expect("夹具可写");
}

#[test]
fn plan_rejects_missing_cert_and_profile_before_running() {
    if !hap_tools_or_skip() {
        return;
    }
    let scratch = common::Scratch::new("hap-plan-material");
    let hap = scratch.path().join("app-unsigned.hap");
    write_hap_stub(&hap);

    // 只有 keystore：必须点名 app_cert，绝不空手签名
    let signer = signer_for("hap-release", hap_payload(Some("k"), false, false));
    let err = signer
        .plan(&request(&hap, None))
        .expect_err("缺 app_cert / profile 必须在 plan 阶段快速失败");
    assert_eq!(err.code(), codes::E_SIG_0002);
    assert!(err.to_string().contains("app_cert"), "{err}");

    // key_alias 也是 HAP 必需（hap-sign-tool 的 -keyAlias 无省略档）
    let signer = signer_for("hap-release", hap_payload(None, true, true));
    let err = signer
        .plan(&request(&hap, None))
        .expect_err("缺 key_alias 必须快速失败");
    assert_eq!(err.code(), codes::E_SIG_0002);
    assert!(err.to_string().contains("key_alias"), "{err}");
}

#[test]
fn plan_rejects_missing_input_hap() {
    if !hap_tools_or_skip() {
        return;
    }
    let scratch = common::Scratch::new("hap-plan-input");
    let signer = signer_for("hap-release", hap_payload(Some("k"), true, true));
    let err = signer
        .plan(&request(&scratch.path().join("missing.hap"), None))
        .expect_err("输入缺失必须在 plan 阶段快速失败");
    assert_eq!(err.code(), codes::E_SIG_0004);
    assert!(err.hint().is_some());
}

#[test]
fn plan_names_default_output_and_resolves_tool() {
    if !hap_tools_or_skip() {
        return;
    }
    let scratch = common::Scratch::new("hap-plan-ok");
    let hap = scratch.path().join("polypack-app-0.1.0-unsigned.hap");
    write_hap_stub(&hap);
    let signer = signer_for("hap-release", hap_payload(Some("openHarmony release"), true, true));
    let plan = signer.plan(&request(&hap, None)).expect("材料齐备应出计划");
    assert_eq!(plan.output, signed_output(&hap));
    assert_eq!(
        plan.output.file_name().and_then(|name| name.to_str()),
        Some("polypack-app-0.1.0-unsigned-signed.hap")
    );
    assert!(
        plan.tool.ends_with("hap-sign-tool.jar"),
        "解析出的 jar 不符：{}",
        plan.tool.display()
    );
    // 计划展示串不含任何口令槽位（口令只存在于临时的执行参数里）
    let shown = plan.to_string();
    assert!(shown.contains("****"), "计划应注明口令脱敏：{shown}");
}
