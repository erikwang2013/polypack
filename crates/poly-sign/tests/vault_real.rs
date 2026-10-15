//! T-M2-11 真环境闭环：真 Vault（`vault server -dev`，KV v2）读写往返。
//!
//! 门控：`POLY_VAULT_REAL=1` **且** `VAULT_ADDR` / `VAULT_TOKEN` 齐备才真跑；缺任一
//! eprintln SKIP 并以通过结束（CI 三宿主没有 Vault 属预期形态，同 poly-bundle 的
//! `POLY_CONTRACT_REAL` 口径）。
//!
//! **脱敏**：token 只从环境读、只进请求头；本文件对「错误串里不出现 token」做显式断言
//! （含服务端回显请求头这一假设形态），任何断言消息都不带 token 值。

mod common;

use poly_core::{CredentialRef, CredentialSource};
use poly_sign::secret::{
    self, SecretBackend, SigningMaterial, VAULT_ID, VaultBackend, VaultClient,
};
use poly_sign::vault::HttpVaultClient;

/// 真跑用的凭据档案名（嵌套路径形态，顺带覆盖 Vault 的层级路径）。
const NAME: &str = "polypack-it/pax-release";
/// 载荷原文里的密钥库字节（写入 → 读回的比对基准）。
const KEYSTORE: &[u8] = b"pkcs12-bytes-of-vault-it";

/// 门控 + 环境读取：`Some((addr, token))` 才真跑。
fn real_env() -> Option<(String, String)> {
    if std::env::var("POLY_VAULT_REAL").ok().as_deref() != Some("1") {
        eprintln!("SKIP：真环境 Vault 闭环需 POLY_VAULT_REAL=1（本地起 `vault server -dev`）");
        return None;
    }
    let (Ok(addr), Ok(token)) = (std::env::var("VAULT_ADDR"), std::env::var("VAULT_TOKEN")) else {
        eprintln!("SKIP：真跑需 VAULT_ADDR + VAULT_TOKEN（dev 服务器启动横幅里有 root token）");
        return None;
    };
    Some((addr, token))
}

/// 真读写闭环：写 base64(JSON) 载荷 → 经 [`VaultBackend`] 读回 → `resolve()` 出签名材料。
#[test]
fn real_vault_kv_v2_round_trip_through_backend() {
    let Some(_real_env) = real_env() else {
        return;
    };
    // 走 from_env：地址 / token / VAULT_CACERT 全从环境读（https + 私有 CA 场景即此路）。
    let client = HttpVaultClient::from_env().expect("环境齐备即可构造");

    // 写入：载荷契约 = base64(JSON) 原文（与 keyring / Env 后端同一形态，本层不解析）。
    let payload = common::payload(
        KEYSTORE,
        common::PASS,
        Some(common::ALIAS),
        Some(common::PASS),
    );
    let payload_text = String::from_utf8(payload.clone()).expect("载荷是 base64 文本");
    client
        .put_payload(NAME, &payload_text)
        .expect("KV v2 写入成功");

    // 读回：走 VaultBackend（真实后端链上的那一个）→ fetch 原文一致。
    let backend = VaultBackend::new(client);
    assert_eq!(backend.id(), VAULT_ID);
    assert!(backend.available().is_ok(), "client 已构造即视为可用");
    let fetched = backend.fetch(NAME).expect("读回成功");
    assert_eq!(fetched, payload, "写进 Vault 的载荷与读回逐字节一致");

    // 解析：同一份载荷 → SigningMaterial（口令 / 别名都对齐）。
    let reference = CredentialRef {
        name: NAME.to_string(),
        source: CredentialSource::Vault,
    };
    let backends: Vec<Box<dyn SecretBackend>> = vec![Box::new(backend)];
    let material: SigningMaterial = secret::resolve(&reference, &backends).expect("解析成签名材料");
    assert_eq!(material.keystore, KEYSTORE, "keystore 字节往返一致");
    assert_eq!(material.store_password, common::PASS);
    assert_eq!(material.key_alias.as_deref(), Some(common::ALIAS));
}

/// 条目缺失：404 明确成「路径不存在」并给播种提示，不出现 token。
#[test]
fn missing_entry_is_reported_with_path_and_no_token_leak() {
    let Some((_addr, token)) = real_env() else {
        return;
    };
    let client = HttpVaultClient::from_env().expect("构造");
    let err = client
        .read("polypack-it/definitely-missing")
        .expect_err("不存在的条目必失败");
    let text = err.to_string();
    assert_eq!(err.code(), poly_sign::codes::E_SIG_0002);
    assert!(text.contains("404") && text.contains("vault"), "{text}");
    assert!(
        text.contains("polypack-it/definitely-missing"),
        "报错应带路径便于定位：{text}"
    );
    assert!(!text.contains(&token), "报错不得含 token");
}

/// 错 token：真服务器给 403；错误串里既没有错 token 也没有真 token。
#[test]
fn bad_token_is_403_and_errors_stay_redacted() {
    let Some((addr, token)) = real_env() else {
        return;
    };
    let bogus = "poly-bogus-token-must-not-leak";
    // 错 token 用显式构造（from_env 只认环境里的真 token）；addr / CA 沿用环境值。
    let cacert = std::env::var("VAULT_CACERT").ok();
    let client = HttpVaultClient::new(&addr, bogus, cacert.as_deref().map(std::path::Path::new))
        .expect("构造");
    let err = client.read(NAME).expect_err("错 token 必失败");
    let text = err.to_string();
    assert!(
        text.contains("403"),
        "dev 服务器对错 token 应回 403：{text}"
    );
    assert!(text.contains("policy"), "403 应带修复提示：{text}");
    assert!(!text.contains(bogus), "错误串不得回显错 token：{text}");
    assert!(!text.contains(&token), "错误串不得含真 token：{text}");
    // Debug 同样脱敏（日志 / panic 消息里也不该出现）。
    assert!(!format!("{client:?}").contains(bogus));
}
