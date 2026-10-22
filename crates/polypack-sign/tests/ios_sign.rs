//! T-M2-10 契约测试（mock 模式）：codesign / notarytool / stapler / spctl 命令组装、
//! 凭据零明文契约与宿主工具缺失契约。
//!
//! 本宿主（Linux）无 macOS 工具链：执行函数必须返回 [`SignError::ToolMissing`]
//! （`polypack::E_SIG_0001`）——「真机执行阻塞（宿主）」的证据；`plan_*` 为纯组装，
//! 本宿主全程真跑断言。macos-14 真机（T-M2-14 nightly）执行签名 / 公证真跑。
//! 凭据域错误统一归 `polypack::E_SIG_0002`（缺配置 / 形态非法）。

use std::path::Path;

use polypack_core::{CredentialRef, CredentialSource};
use polypack_sign::SignError;
use polypack_sign::codes;
use polypack_sign::ios::{self, IosCredentials, NotaryAuth, REF_SIGNING_PROFILE};

/// 凭据引用（约定名 + 来源）。
fn reference(name: &str, source: CredentialSource) -> CredentialRef {
    CredentialRef {
        name: name.to_string(),
        source,
    }
}

/// 钥匙串档案形态的完整凭据（签名标识 + notarytool 档案 + 团队）。
fn keychain_credentials() -> IosCredentials {
    IosCredentials {
        identity: Some("Apple Distribution: Pax (TEAM123456)".to_string()),
        team_id: Some("TEAM123456".to_string()),
        notary: Some(NotaryAuth::KeychainProfile(REF_SIGNING_PROFILE.to_string())),
        provisioning_ref: None,
    }
}

// ---------------------------------------------------------------- 签名

/// 签名计划：硬化运行时 + 安全时间戳（公证前置），参数逐个传参。
#[test]
fn sign_plan_is_hardened_and_timestamped() {
    let creds = keychain_credentials();
    let plan =
        ios::plan_sign(Path::new("/out/export/Payload/Pax Notes.app"), &creds).expect("plan");
    assert_eq!(plan.commands.len(), 1);
    assert_eq!(
        plan.commands[0].args,
        vec![
            "--force",
            "--timestamp",
            "--options",
            "runtime",
            "--sign",
            "Apple Distribution: Pax (TEAM123456)",
            "/out/export/Payload/Pax Notes.app",
        ],
        "{}",
        plan.commands[0].line()
    );
    assert_eq!(plan.commands[0].program, "codesign");
}

/// 缺签名标识 → 缺凭据（`E_SIG_0002`），不产出半成品计划。
#[test]
fn sign_requires_identity() {
    let mut creds = keychain_credentials();
    creds.identity = None;
    let err = ios::plan_sign(Path::new("/out/App.app"), &creds).expect_err("缺标识必须失败");
    assert_eq!(err.code(), codes::E_SIG_0002);
    assert!(matches!(
        err,
        SignError::CredentialUnavailable {
            name,
            backend: "config",
            ..
        } if name == "ios.identity"
    ));
}

// ---------------------------------------------------------------- 公证

/// 公证打包：`ditto` 保符号链接 / 扩展属性（notarytool 只收 zip / pkg / dmg）。
#[test]
fn notarize_zip_uses_ditto() {
    let plan = ios::plan_zip(
        Path::new("/out/Pax Notes.app"),
        Path::new("/out/notarize/Pax Notes.zip"),
    );
    assert_eq!(
        plan.commands[0].args,
        vec![
            "-c",
            "-k",
            "--keepParent",
            "/out/Pax Notes.app",
            "/out/notarize/Pax Notes.zip",
        ]
    );
    assert_eq!(plan.commands[0].program, "ditto");
}

/// 钥匙串档案优先（Apple 推荐；材料留在钥匙串，命令里只有档案名）。
#[test]
fn notary_auth_prefers_keychain_profile() {
    let creds = keychain_credentials();
    let plan = ios::plan_notarize(Path::new("/out/notarize/Pax Notes.zip"), &creds).expect("plan");
    assert_eq!(plan.commands.len(), 1);
    let command = &plan.commands[0];
    assert_eq!(command.program, "xcrun");
    assert_eq!(
        command.args,
        vec![
            "notarytool",
            "submit",
            "/out/notarize/Pax Notes.zip",
            "--keychain-profile",
            REF_SIGNING_PROFILE,
            "--wait",
        ],
        "{}",
        command.line()
    );
}

/// env 形态：认证参数**只含变量名**（`@env:VAR`，notarytool 自行取值）——明文永不进命令。
#[test]
fn notary_env_form_never_inlines_secrets() {
    let creds = IosCredentials {
        identity: Some("Apple Distribution: Pax (TEAM123456)".to_string()),
        team_id: Some("TEAM123456".to_string()),
        notary: Some(NotaryAuth::Env {
            apple_id: "POLYPACK_APPLE_ID".to_string(),
            password: "POLYPACK_APPLE_APP_PASSWORD".to_string(),
        }),
        provisioning_ref: None,
    };
    let plan = ios::plan_notarize(Path::new("/out/app.zip"), &creds).expect("plan");
    assert_eq!(
        plan.commands[0].args,
        vec![
            "notarytool",
            "submit",
            "/out/app.zip",
            "--apple-id",
            "@env:POLYPACK_APPLE_ID",
            "--password",
            "@env:POLYPACK_APPLE_APP_PASSWORD",
            "--team-id",
            "TEAM123456",
            "--wait",
        ],
        "认证参数只允许变量名：{}",
        plan.commands[0].line()
    );

    // 变量名非法（注入形态 / 空名）→ 响亮拒绝。
    for bad in ["BAD NAME", "1BAD", "-x"] {
        let mut broken = creds.clone();
        broken.notary = Some(NotaryAuth::Env {
            apple_id: "POLYPACK_APPLE_ID".to_string(),
            password: bad.to_string(),
        });
        let err = ios::plan_notarize(Path::new("/out/app.zip"), &broken).expect_err("变量名非法");
        assert_eq!(err.code(), codes::E_SIG_0002, "{bad:?}");
    }

    // 缺口令 / 缺团队 ID / 缺认证形态 → 缺凭据。
    let mut no_password = creds.clone();
    no_password.notary = Some(NotaryAuth::Env {
        apple_id: "POLYPACK_APPLE_ID".to_string(),
        password: String::new(),
    });
    assert_eq!(
        ios::plan_notarize(Path::new("/out/app.zip"), &no_password)
            .expect_err("缺口令")
            .code(),
        codes::E_SIG_0002
    );

    let mut no_team = creds.clone();
    no_team.team_id = None;
    assert_eq!(
        ios::plan_notarize(Path::new("/out/app.zip"), &no_team)
            .expect_err("缺团队")
            .code(),
        codes::E_SIG_0002
    );

    let mut no_auth = creds.clone();
    no_auth.notary = None;
    assert_eq!(
        ios::plan_notarize(Path::new("/out/app.zip"), &no_auth)
            .expect_err("缺认证")
            .code(),
        codes::E_SIG_0002
    );
}

// ---------------------------------------------------------------- 装订与校验

/// 装订：`staple` 后 `validate`；校验：`codesign --verify` + `spctl`（安装包用 install 类型）。
#[test]
fn staple_and_verify_plans() {
    let plan = ios::plan_staple(Path::new("/out/Pax Notes.app"));
    assert_eq!(plan.commands.len(), 2, "装订须复核：{:#?}", plan.lines());
    assert_eq!(
        plan.commands[0].args,
        vec!["stapler", "staple", "/out/Pax Notes.app"]
    );
    assert_eq!(
        plan.commands[1].args,
        vec!["stapler", "validate", "/out/Pax Notes.app"]
    );

    let plan = ios::plan_verify(Path::new("/out/Pax Notes.app"));
    assert_eq!(plan.commands.len(), 2);
    assert_eq!(plan.commands[0].program, "codesign");
    assert_eq!(
        plan.commands[0].args,
        vec![
            "--verify",
            "--deep",
            "--strict",
            "--verbose=2",
            "/out/Pax Notes.app",
        ]
    );
    assert_eq!(plan.commands[1].program, "spctl");
    assert_eq!(
        plan.commands[1].args,
        vec![
            "--assess",
            "--type",
            "execute",
            "--verbose=4",
            "/out/Pax Notes.app",
        ]
    );

    // 安装包形态：spctl 走 install 类型。
    let plan = ios::plan_verify(Path::new("/out/Pax Notes.pkg"));
    assert_eq!(plan.commands[1].args[2], "install");
}

// ---------------------------------------------------------------- 凭据装配

/// 凭据引用装配：钥匙串档案 / env 形态 / 口令档案 / 描述文件；Vault 与非法形态响亮拒绝。
#[test]
fn credentials_from_credential_ref() {
    // 钥匙串来源 → notarytool 档案名 = 引用名。
    let creds = IosCredentials::from_credential_ref(&[reference(
        REF_SIGNING_PROFILE,
        CredentialSource::Keyring,
    )])
    .expect("keyring");
    assert_eq!(
        creds.notary,
        Some(NotaryAuth::KeychainProfile(REF_SIGNING_PROFILE.to_string()))
    );
    assert_eq!(creds.provisioning_ref, None);

    // env 来源 → Apple ID 变量名；配套口令档案并入；描述文件引用登记在案。
    let creds = IosCredentials::from_credential_ref(&[
        reference(
            REF_SIGNING_PROFILE,
            CredentialSource::Env {
                variable: "POLYPACK_APPLE_ID".to_string(),
            },
        ),
        reference(
            "apple-codesign.password",
            CredentialSource::Env {
                variable: "POLYPACK_APPLE_APP_PASSWORD".to_string(),
            },
        ),
        reference("apple-codesign.provisioning", CredentialSource::Keyring),
        // 其他目标的引用（如 Android 题述档案）必须被忽略。
        reference("android-keystore", CredentialSource::Keyring),
    ])
    .expect("env");
    assert_eq!(
        creds.notary,
        Some(NotaryAuth::Env {
            apple_id: "POLYPACK_APPLE_ID".to_string(),
            password: "POLYPACK_APPLE_APP_PASSWORD".to_string(),
        })
    );
    assert_eq!(
        creds.provisioning_ref,
        Some(reference(
            "apple-codesign.provisioning",
            CredentialSource::Keyring
        ))
    );

    // Vault 来源：材料须先注入钥匙串（notarytool 只认钥匙串 / env）。
    let err = IosCredentials::from_credential_ref(&[reference(
        REF_SIGNING_PROFILE,
        CredentialSource::Vault,
    )])
    .expect_err("Vault 必须拒绝");
    assert_eq!(err.code(), codes::E_SIG_0002);
    assert!(matches!(
        err,
        SignError::CredentialInvalid {
            backend: "vault",
            ..
        }
    ));

    // 口令档案不接受钥匙串来源（否则无法以 `@env:` 传递）。
    let err = IosCredentials::from_credential_ref(&[
        reference(
            REF_SIGNING_PROFILE,
            CredentialSource::Env {
                variable: "POLYPACK_APPLE_ID".to_string(),
            },
        ),
        reference("apple-codesign.password", CredentialSource::Keyring),
    ])
    .expect_err("钥匙串口令档案必须拒绝");
    assert_eq!(err.code(), codes::E_SIG_0002);
}

/// 收据槽位（提审级缓存）：`<out>/receipts/<产品名>-<版本>-notarization.json`。
#[test]
fn receipt_slot_is_reserved() {
    assert_eq!(
        ios::receipt_path(Path::new("/out"), "Pax Notes", "1.2.3"),
        Path::new("/out/receipts/Pax Notes-1.2.3-notarization.json")
    );
}

// ---------------------------------------------------------------- 执行门控

/// 宿主门控：非 macOS 宿主执行函数在动手前返回 ToolMissing（收据不落盘）。
#[test]
fn execution_is_gated_to_macos_hosts() {
    if cfg!(target_os = "macos") {
        eprintln!("SKIP：macOS 宿主上真跑（实得真执行为准），门控契约仅在非 macOS 宿主断言");
        return;
    }
    let creds = keychain_credentials();
    let app = Path::new("/out/Pax Notes.app");
    let receipt = ios::receipt_path(
        Path::new("/tmp/polypack-sign-ios-gate"),
        "Pax Notes",
        "1.2.3",
    );
    let _ = std::fs::remove_dir_all("/tmp/polypack-sign-ios-gate");

    for err in [
        ios::sign(app, &creds).expect_err("非 macOS：sign"),
        ios::notarize(Path::new("/out/app.zip"), &creds, &receipt).expect_err("非 macOS：notarize"),
        ios::staple(app).expect_err("非 macOS：staple"),
        ios::verify(app).expect_err("非 macOS：verify"),
    ] {
        assert_eq!(err.code(), codes::E_SIG_0001, "{err:?}");
        match err {
            SignError::ToolMissing { tool, hint } => {
                assert!(!tool.is_empty() && hint.contains("macOS"), "{tool}：{hint}");
            }
            other => panic!("期望 ToolMissing，实得 {other:?}"),
        }
    }
    assert!(!receipt.exists(), "门控须先于落盘：{}", receipt.display());
}
