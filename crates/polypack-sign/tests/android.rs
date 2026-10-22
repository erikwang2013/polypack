//! T-M2-09 验收：apksigner 计划 / 执行、`verify --print-certs` 证据、临时密钥零残留、脱敏。
//!
//! 真跑用例在宿主缺 apksigner / keytool / openssl 时显式 SKIP（见 `common`），绝不伪造真跑。

mod common;

use std::io::Write as _;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use polypack_core::{CredentialRef, CredentialSource};
use polypack_sign::android::{AndroidSigner, KEY_PASS_VAR, STORE_PASS_VAR, SignRequest};
use polypack_sign::codes;
use polypack_sign::secret::{KEYRING_ID, MemoryBackend, SecretBackend};

/// 最小二进制 AndroidManifest.xml（AXML）夹具。
///
/// apksigner 必须从**二进制**清单读 minSdkVersion（纯文本清单会报
/// `malformed binary resource: AndroidManifest.xml`）。字节为本地一次性生成后固定的夹具：
/// `aapt package -M AndroidManifest.xml -I $ANDROID_HOME/platforms/android-36/android.jar -F dummy.apk`
/// 源清单：`<manifest package="dev.polypack.pax"><uses-sdk android:minSdkVersion="24"
/// android:targetSdkVersion="34"/></manifest>`。
const AXML_MANIFEST_B64: &str = "AwAIAJADAAABABwAPAIAAA8AAAAAAAAAAAAAAFgAAAAAAAAAAAAAACYAAABcAAAAegAAAJ4AAACwAAAACAEAAAwBAAAeAQAAUgEAAIYBAACaAQAAvgEAAMYBAADOAQAAEQBjAG8AbQBwAGkAbABlAFMAZABrAFYAZQByAHMAaQBvAG4AAAAZAGMAbwBtAHAAaQBsAGUAUwBkAGsAVgBlAHIAcwBpAG8AbgBDAG8AZABlAG4AYQBtAGUAAAANAG0AaQBuAFMAZABrAFYAZQByAHMAaQBvAG4AAAAQAHQAYQByAGcAZQB0AFMAZABrAFYAZQByAHMAaQBvAG4AAAAHAGEAbgBkAHIAbwBpAGQAAAAqAGgAdAB0AHAAOgAvAC8AcwBjAGgAZQBtAGEAcwAuAGEAbgBkAHIAbwBpAGQALgBjAG8AbQAvAGEAcABrAC8AcgBlAHMALwBhAG4AZAByAG8AaQBkAAAAAAAAAAcAcABhAGMAawBhAGcAZQAAABgAcABsAGEAdABmAG8AcgBtAEIAdQBpAGwAZABWAGUAcgBzAGkAbwBuAEMAbwBkAGUAAAAYAHAAbABhAHQAZgBvAHIAbQBCAHUAaQBsAGQAVgBlAHIAcwBpAG8AbgBOAGEAbQBlAAAACABtAGEAbgBpAGYAZQBzAHQAAAAQAGQAZQB2AC4AcABvAGwAeQBwAGEAYwBrAC4AcABhAHgAAAACADEANgAAAAIAMwA2AAAACAB1AHMAZQBzAC0AcwBkAGsAAAAAAIABCAAYAAAAcgUBAXMFAQEMAgEBcAIBAQABEAAYAAAAAgAAAP////8EAAAABQAAAAIBEACIAAAAAgAAAP//////////CgAAABQAFAAFAAAAAAAAAAUAAAAAAAAA/////wgAABAkAAAABQAAAAEAAAAMAAAACAAAAwwAAAD/////BwAAAAsAAAAIAAADCwAAAP////8IAAAADQAAAAgAABAkAAAA/////wkAAAAMAAAACAAAEBAAAAACARAATAAAAAMAAAD//////////w4AAAAUABQAAgAAAAAAAAAFAAAAAgAAAP////8IAAAQGAAAAAUAAAADAAAA/////wgAABAiAAAAAwEQABgAAAADAAAA//////////8OAAAAAwEQABgAAAAEAAAA//////////8KAAAAAQEQABgAAAAEAAAA/////wQAAAAFAAAA";

/// 测试口令（仅测试用，非真实凭据）。
const PASS: &str = "sign-and-verify-pass";

/// 写一个最小未签名 APK（zip 容器 + AXML 清单）。
fn write_minimal_apk(path: &Path) {
    let manifest = base64::engine::general_purpose::STANDARD
        .decode(AXML_MANIFEST_B64)
        .expect("夹具 base64 合法");
    let file = std::fs::File::create(path).expect("APK 可创建");
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file(
        "AndroidManifest.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .expect("清单条目可写");
    zip.write_all(&manifest).expect("清单写入");
    zip.finish().expect("zip 收尾");
}

/// 签名器：内存后端冒充 keyring 注入载荷（headless CI 无 Secret Service 的注入路径）。
fn signer_for(name: &str, payload: Vec<u8>) -> AndroidSigner {
    AndroidSigner::new(vec![
        Box::new(MemoryBackend::with_entry(name, payload).with_id(KEYRING_ID))
            as Box<dyn SecretBackend>,
    ])
}

fn request(input: &Path, out: Option<PathBuf>) -> SignRequest {
    SignRequest {
        input: input.to_path_buf(),
        out,
        credential: CredentialRef {
            name: "pax-release".to_string(),
            source: CredentialSource::Keyring,
        },
    }
}

#[test]
fn plan_rejects_missing_credential_before_running() {
    if common::apksigner_or_skip().is_none() {
        return;
    }
    let scratch = common::Scratch::new("plan-cred");
    let apk = scratch.path().join("app.apk");
    write_minimal_apk(&apk);

    let signer = AndroidSigner::new(Vec::new()); // 空后端链
    let err = signer
        .plan(&request(&apk, None))
        .expect_err("缺失凭据引用必须在 plan 阶段快速失败");
    assert_eq!(err.code(), codes::E_SIG_0002);
    let hint = err.hint().expect("须带修复提示");
    assert!(!hint.is_empty());
}

#[test]
fn plan_rejects_missing_input_apk() {
    if common::apksigner_or_skip().is_none() {
        return;
    }
    let scratch = common::Scratch::new("plan-input");
    let payload = common::payload(b"keystore", PASS, Some("pax"), None);
    let signer = signer_for("pax-release", payload);
    let err = signer
        .plan(&request(&scratch.path().join("missing.apk"), None))
        .expect_err("输入缺失必须在 plan 阶段快速失败");
    assert_eq!(err.code(), codes::E_SIG_0004);
    assert!(err.hint().is_some());
}

#[test]
fn plan_never_carries_password_values() {
    if common::apksigner_or_skip().is_none() {
        return;
    }
    let scratch = common::Scratch::new("plan-redact");
    let apk = scratch.path().join("app.apk");
    write_minimal_apk(&apk);
    let payload = common::payload(b"keystore", PASS, Some("pax"), None);
    let signer = signer_for("pax-release", payload);

    let plan = signer.plan(&request(&apk, None)).expect("计划可生成");
    let shown = format!("{plan}\n{plan:?}");
    assert!(
        shown.contains(STORE_PASS_VAR) && shown.contains(KEY_PASS_VAR),
        "计划应引用环境变量名而非口令：{shown}"
    );
    assert!(
        !shown.contains(PASS),
        "计划文本 / Debug 不得带出口令：{shown}"
    );
    assert_eq!(
        plan.output,
        scratch.path().join("app-signed.apk"),
        "缺省产物名"
    );
    assert_eq!(plan.key_alias.as_deref(), Some("pax"));
}

#[test]
fn signs_minimal_apk_and_captures_verification_evidence() {
    let _guard = common::temp_lock();
    if common::apksigner_or_skip().is_none() {
        return;
    }
    let scratch = common::Scratch::new("sign-ok");
    let Some(keystore) = common::keystore_fixture(scratch.path()) else {
        return;
    };
    let apk = scratch.path().join("app.apk");
    write_minimal_apk(&apk);
    let payload = common::payload(
        &keystore.bytes,
        common::PASS,
        keystore.alias.as_deref(),
        Some(common::PASS),
    );
    let signer = signer_for("pax-release", payload);

    let out = scratch.path().join("app-signed.apk");
    let artifact = signer
        .sign(&request(&apk, Some(out.clone())))
        .expect("签名 + 校验应成功");
    assert_eq!(artifact.path, out);
    assert!(artifact.bytes > 0, "产物字节数应 > 0");
    assert!(out.is_file(), "产物应落盘");
    assert!(
        artifact.verify_evidence.contains("Verifies"),
        "verify 证据缺 Verifies：{}",
        artifact.verify_evidence
    );
    assert!(
        artifact
            .verify_evidence
            .contains("Verified using v2 scheme (APK Signature Scheme v2): true"),
        "verify 证据缺 v2 方案：{}",
        artifact.verify_evidence
    );
    // 标签措辞随 apksigner 版本变（36.x `Signer #1 …` / 37.x `V3.0 Signer: …`），只锚定 DN 值。
    assert!(
        artifact
            .verify_evidence
            .contains(&format!("certificate DN: {}", keystore.dn)),
        "verify 证据缺签名者 DN：{}",
        artifact.verify_evidence
    );
    assert!(
        common::sign_temp_leftovers().is_empty(),
        "签名成功后不得残留临时密钥：{:?}",
        common::sign_temp_leftovers()
    );

    // 可重复调用（`polypack retry --from sign` 语义）：缺省产物名 + 每次重新拉取凭据。
    let again = signer.sign(&request(&apk, None)).expect("重复调用应成功");
    assert_eq!(again.path, scratch.path().join("app-signed.apk"));
    assert!(
        common::sign_temp_leftovers().is_empty(),
        "重复签名后仍不得残留：{:?}",
        common::sign_temp_leftovers()
    );
}

#[test]
fn failed_signing_leaves_no_temp_residue_and_no_secret_in_error() {
    let _guard = common::temp_lock();
    if common::apksigner_or_skip().is_none() {
        return;
    }
    let scratch = common::Scratch::new("sign-fail");
    let Some(keystore) = common::keystore_fixture(scratch.path()) else {
        return;
    };
    let apk = scratch.path().join("app.apk");
    write_minimal_apk(&apk);
    // 错误别名：apksigner 加载 signer 时非零退出（真实失败路径）。
    let payload = common::payload(
        &keystore.bytes,
        common::PASS,
        Some("definitely-not-an-alias"),
        None,
    );
    let signer = signer_for("pax-release", payload);
    let out = scratch.path().join("app-signed.apk");

    let err = signer
        .sign(&request(&apk, Some(out.clone())))
        .expect_err("错误别名必须失败");
    assert_eq!(err.code(), codes::E_SIG_0003, "外部命令失败：{err}");
    for text in [err.to_string(), format!("{err:?}")] {
        assert!(!text.contains(common::PASS), "错误文本不得带出口令：{text}");
    }
    assert!(!out.exists(), "失败路径不应留下半成品产物");
    assert!(
        common::sign_temp_leftovers().is_empty(),
        "失败路径同样不得残留临时密钥：{:?}",
        common::sign_temp_leftovers()
    );
}

#[test]
fn default_output_name_is_derived_from_input() {
    // 不依赖 apksigner 的纯逻辑断言（缺省产物路径由 plan 组装）。
    let dir = std::env::temp_dir().join(format!("polypack-sign-it-{}-naming", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("目录可建");
    let apk = dir.join("pax-notes.apk");
    write_minimal_apk(&apk);
    if common::apksigner_or_skip().is_none() {
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let payload = common::payload(b"keystore", PASS, None, None);
    let signer = signer_for("pax-release", payload);
    let plan = signer.plan(&request(&apk, None)).expect("计划可生成");
    assert_eq!(plan.output, dir.join("pax-notes-signed.apk"));
    let _ = std::fs::remove_dir_all(&dir);
}
