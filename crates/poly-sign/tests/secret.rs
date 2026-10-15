//! T-M2-11 验收：凭据后端链（keyring / Vault trait / Env / CI 内存注入）与脱敏。
//!
//! 测试绝不联网：Vault 侧用 [`VaultClient`] 的内存假实现；keyring 侧用
//! [`MemoryBackend::with_id`] 冒充；真实 keyring 只做「探测不 panic」断言。

mod common;

use poly_core::{CredentialRef, CredentialSource, JobSpec, Profile};
use poly_sign::secret::{
    self, EnvBackend, KEYRING_ID, KeyringBackend, MEMORY_ID, MemoryBackend, SecretBackend,
    SigningMaterial, VAULT_ID, VaultBackend, VaultClient,
};
use poly_sign::{SignError, codes};

/// 测试口令（仅测试用，非真实凭据）。
const PASS: &str = "s3cret-keystore-pass";

fn keyring_ref(name: &str) -> CredentialRef {
    CredentialRef {
        name: name.to_string(),
        source: CredentialSource::Keyring,
    }
}

/// 内存后端冒充 keyring（headless CI 无 Secret Service 的注入路径）。
fn memory_keyring(name: &str, payload: Vec<u8>) -> Vec<Box<dyn SecretBackend>> {
    vec![
        Box::new(MemoryBackend::with_entry(name, payload).with_id(KEYRING_ID))
            as Box<dyn SecretBackend>,
    ]
}

fn resolve_memory(name: &str, payload: Vec<u8>) -> Result<SigningMaterial, SignError> {
    secret::resolve(&keyring_ref(name), &memory_keyring(name, payload))
}

/// Vault 的测试实现（内存假 Vault；验收要求「测试绝不联网」）。
struct FakeVault {
    payload: Vec<u8>,
}

impl VaultClient for FakeVault {
    fn read(&self, name: &str) -> Result<Vec<u8>, SignError> {
        assert_eq!(name, "pax-release", "Vault 按凭据档案名读取");
        Ok(self.payload.clone())
    }
}

/// 未接线的假 Vault：读取必定失败。
struct UnwiredVault;

impl VaultClient for UnwiredVault {
    fn read(&self, name: &str) -> Result<Vec<u8>, SignError> {
        Err(SignError::CredentialUnavailable {
            name: name.to_string(),
            backend: VAULT_ID,
            reason: "假 Vault 未接线".to_string(),
        })
    }
}

#[test]
fn same_job_resolves_via_keyring_and_vault_backends() {
    // 同一份载荷、同一档案名：先是 keyring 来源，再是 Vault 来源。
    let payload = common::payload(b"pkcs12-bytes-of-pax", PASS, Some("pax"), Some(PASS));
    let job = JobSpec {
        id: "android-release".to_string(),
        profile: Profile::Ci,
        targets: Vec::new(),
        excluded: Vec::new(),
        secrets: vec![keyring_ref("pax-release")],
        budget: Default::default(),
    };

    let from_keyring = secret::resolve(
        &job.secrets[0],
        &memory_keyring("pax-release", payload.clone()),
    )
    .expect("keyring 后端（内存注入）解析成功");

    let mut vault_job = job.clone();
    vault_job.secrets[0].source = CredentialSource::Vault;
    let vault_backends = vec![Box::new(VaultBackend::new(FakeVault {
        payload: payload.clone(),
    })) as Box<dyn SecretBackend>];
    let from_vault =
        secret::resolve(&vault_job.secrets[0], &vault_backends).expect("Vault 后端解析成功");

    assert_eq!(from_keyring.keystore, from_vault.keystore);
    assert_eq!(from_keyring.keystore, b"pkcs12-bytes-of-pax");
    assert_eq!(from_keyring.store_password, from_vault.store_password);
    assert_eq!(from_keyring.store_password, PASS);
    assert_eq!(from_keyring.key_alias.as_deref(), Some("pax"));
    assert_eq!(from_keyring.key_password.as_deref(), Some(PASS));
}

#[test]
fn vault_errors_propagate_from_client() {
    let mut reference = keyring_ref("pax-release");
    reference.source = CredentialSource::Vault;
    let backends = vec![Box::new(VaultBackend::new(UnwiredVault)) as Box<dyn SecretBackend>];
    let err = secret::resolve(&reference, &backends).expect_err("未接线的 Vault 必须失败");
    assert_eq!(err.code(), codes::E_SIG_0002);
    assert!(err.hint().is_some(), "须带修复提示");
}

#[test]
fn missing_backend_and_missing_entry_fail_fast() {
    // 后端链里没有 keyring 实现。
    let err = secret::resolve(&keyring_ref("pax-release"), &[]).expect_err("无后端必须失败");
    assert_eq!(err.code(), codes::E_SIG_0002);
    assert!(err.to_string().contains("keyring"), "错误须点名后端：{err}");
    assert!(err.hint().is_some());

    // 后端在，但条目不存在。
    let other_entry = memory_keyring("another-entry", common::payload(b"other", PASS, None, None));
    let err =
        secret::resolve(&keyring_ref("pax-release"), &other_entry).expect_err("条目名不同必须失败");
    assert_eq!(err.code(), codes::E_SIG_0002);

    // 有 memory 后端但来源是 Vault：同样快速失败。
    let mut reference = keyring_ref("pax-release");
    reference.source = CredentialSource::Vault;
    let backends = vec![Box::new(MemoryBackend::new()) as Box<dyn SecretBackend>];
    let err = secret::resolve(&reference, &backends).expect_err("缺 vault 后端必须失败");
    assert_eq!(err.code(), codes::E_SIG_0002);
    assert!(err.to_string().contains(VAULT_ID));
}

#[test]
fn env_source_without_variable_reports_hint() {
    let reference = CredentialRef {
        name: "pax-release".to_string(),
        source: CredentialSource::Env {
            variable: "POLYPACK_DEFINITELY_MISSING_VARIABLE".to_string(),
        },
    };
    let err = secret::resolve(&reference, &[]).expect_err("变量缺失必须失败");
    assert_eq!(err.code(), codes::E_SIG_0002);
    assert!(
        err.to_string()
            .contains("POLYPACK_DEFINITELY_MISSING_VARIABLE"),
        "错误须点名环境变量：{err}"
    );
    let hint = err.hint().expect("须带修复提示");
    assert!(
        hint.contains("Env") || hint.contains("Memory"),
        "提示应指向注入方式：{hint}"
    );
}

#[test]
fn env_source_resolves_injected_payload() {
    const CHILD: &str = "POLYPACK_SIGN_ENV_CHILD";
    const VARIABLE: &str = "POLYPACK_SIGN_TEST_PAYLOAD";
    if std::env::var_os(CHILD).is_some() {
        let reference = CredentialRef {
            name: "pax-release".to_string(),
            source: CredentialSource::Env {
                variable: VARIABLE.to_string(),
            },
        };
        let material = secret::resolve(&reference, &[]).expect("子进程注入的变量应解析成功");
        assert_eq!(material.keystore, b"env-keystore-bytes");
        assert_eq!(material.store_password, PASS);
        assert_eq!(material.key_alias.as_deref(), Some("pax"));
        return;
    }
    // Rust 2024 中进程内 `set_var` 是 unsafe（本仓库 forbid unsafe），
    // 因此按「子进程重跑本测试」的方式注入环境变量，语义与真实 CI 注入一致。
    let payload = common::payload(b"env-keystore-bytes", PASS, Some("pax"), None);
    let exe = std::env::current_exe().expect("测试可执行路径");
    let status = std::process::Command::new(exe)
        .args([
            "env_source_resolves_injected_payload",
            "--exact",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env(
            VARIABLE,
            String::from_utf8(payload).expect("载荷为 base64 文本"),
        )
        .status()
        .expect("子进程可执行");
    assert!(status.success(), "子进程断言失败（见上方输出）");
}

#[test]
fn invalid_payloads_are_rejected_with_stable_code() {
    let cases: [(&str, Vec<u8>); 5] = [
        ("载荷不是 base64", b"%%%not-base64%%%".to_vec()),
        ("JSON 缺字段", base64_of(b"{}")),
        (
            "keystore 字段不是 base64",
            base64_of(br#"{"keystore":"???","store_password":"p"}"#),
        ),
        (
            "keystore 为空",
            base64_of(br#"{"keystore":"","store_password":"p"}"#),
        ),
        (
            "store_password 为空",
            base64_of(br#"{"keystore":"a2V5","store_password":""}"#),
        ),
    ];
    for (case, payload) in cases {
        let err = resolve_memory("pax-release", payload).expect_err(case);
        assert_eq!(err.code(), codes::E_SIG_0002, "{case} 应是稳定错误码");
        assert!(err.hint().is_some(), "{case} 须带修复提示");
    }
}

#[test]
fn secrets_never_reach_debug_or_error_text() {
    let material = resolve_memory(
        "pax-release",
        common::payload(b"keystore-bytes", PASS, Some("pax"), Some(PASS)),
    )
    .expect("解析成功");
    let debug = format!("{material:?}");
    assert!(
        !debug.contains(PASS),
        "SigningMaterial 的 Debug 不得带出口令：{debug}"
    );
    assert!(debug.contains("脱敏"), "Debug 应显式脱敏：{debug}");
    assert!(
        format!("{material:?}").contains("pax"),
        "别名可留在 Debug（非凭据值）"
    );

    // 载荷里带口令但结构非法：错误文本同样不得回显载荷内容。
    let broken = common::payload(b"keystore-bytes", PASS, Some("pax"), None)
        .into_iter()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<u8>>();
    let mut truncated = broken;
    truncated.truncate(truncated.len() / 2);
    let err = resolve_memory("pax-release", truncated).expect_err("截断载荷必须报错");
    assert_eq!(err.code(), codes::E_SIG_0002);
    for text in [err.to_string(), format!("{err:?}")] {
        assert!(!text.contains(PASS), "错误文本不得带出口令：{text}");
        assert!(
            !text.contains("store_password"),
            "错误文本不得回显载荷字段：{text}"
        );
    }
}

#[test]
fn keyring_probe_reports_without_panicking() {
    let backend = KeyringBackend::new();
    assert_eq!(backend.id(), KEYRING_ID);
    match backend.available() {
        Ok(()) => eprintln!("本机密钥库可用，探测通过（headless CI 通常走不可用分支）"),
        Err(err) => {
            assert_eq!(err.code(), codes::E_SIG_0002, "不可用须是稳定错误码");
            let hint = err.hint().expect("不可用须带修复提示");
            assert!(
                hint.contains("Env") || hint.contains("Memory"),
                "提示应指向 Env / Memory 注入：{hint}"
            );
        }
    }
    // 条目缺失（或后端不可达）同样只报错、不 panic。
    let err = backend
        .fetch("polypack-definitely-missing-entry")
        .expect_err("缺失条目必须报错");
    assert_eq!(err.code(), codes::E_SIG_0002);
}

#[test]
fn default_backends_expose_keyring() {
    let backends = secret::default_backends();
    assert_eq!(backends.len(), 1);
    assert_eq!(backends[0].id(), KEYRING_ID);
    assert_eq!(MemoryBackend::new().id(), MEMORY_ID);
}

#[test]
fn env_backend_is_addressable_by_id() {
    // Env 后端由 resolve 按来源变量现构；此处锁定其 id 与探测语义。
    let backend = EnvBackend::new("POLYPACK_DEFINITELY_MISSING_VARIABLE");
    assert_eq!(backend.id(), "env");
    let err = backend.available().expect_err("变量缺失时探测必须失败");
    assert_eq!(err.code(), codes::E_SIG_0002);
}

/// 整段 base64（载荷字段用）。
fn base64_of(bytes: &[u8]) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .encode(bytes)
        .into_bytes()
}
