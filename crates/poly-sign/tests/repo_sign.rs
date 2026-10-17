//! T-M5-05 真跑：隔离 GNUPGHOME 的仓库签名 / 清签 / 公钥导出与外部验签。
//!
//! 凭据走与 T-M2-11 相同的 `CredentialRef` + 内存后端注入（headless CI 无 Secret
//! Service）；临时私钥只在测试进程内存与一次性 GNUPGHOME 中存在，结束后断言零残留。
//! 宿主没有 gpg 时打印 SKIP 并以通过结束（绝不伪造真跑）。

mod common;

use std::path::Path;
use std::process::{Command, Output};

use poly_core::{CredentialRef, CredentialSource};
use poly_sign::secret::MemoryBackend;
use poly_sign::{GpgSigner, RepoSigner, secret};

/// 测试口令（仅测试用；与 `common::PASS` 区分，证明显式口令文件生效）。
const PASS: &str = "repo-sign-pass-123";
/// 测试密钥 UID。
const UID: &str = "PolyPack Repo Test <repo-test@polypack.invalid>";

fn private_dir(path: &Path) {
    std::fs::create_dir_all(path).expect("目录可建");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .expect("目录可设 0700");
    }
}

fn gpg(home: &Path, args: &[&str]) -> Output {
    Command::new("gpg")
        .arg("--batch")
        .arg("--no-options")
        .arg("--homedir")
        .arg(home)
        .args(args)
        .output()
        .expect("gpg 可执行")
}

fn gpg_ok(home: &Path, args: &[&str]) -> Output {
    let output = gpg(home, args);
    assert!(
        output.status.success(),
        "gpg {args:?} 失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// 生成一次性受口令保护的签名密钥，返回（私钥导出字节，公钥导出字节）。
fn key_fixture(scratch: &Path) -> Option<(Vec<u8>, Vec<u8>)> {
    // Git for Windows 的 gpg 是 MSYS 程序：`--homedir C:\...` 会被当 POSIX 相对路径拼接
    // （CI windows-2022 实证），Windows 真跑腿按环境槽 SKIP-with-print，不伪造。
    if cfg!(windows) {
        eprintln!("SKIP：Windows 的 MSYS gpg 不认 Windows 路径参数，跳过仓库签名真跑");
        return None;
    }
    if !common::has_tool("gpg") {
        eprintln!("SKIP：宿主没有 gpg，跳过仓库签名真跑");
        return None;
    }
    let home = scratch.join("keygen-home");
    private_dir(&home);
    let pass_file = scratch.join("keygen.pass");
    std::fs::write(&pass_file, PASS).expect("写口令文件");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&pass_file, std::fs::Permissions::from_mode(0o600))
            .expect("口令文件 0600");
    }
    let pass = pass_file.to_string_lossy().to_string();
    gpg_ok(
        &home,
        &[
            "--pinentry-mode",
            "loopback",
            "--passphrase-file",
            &pass,
            "--quick-generate-key",
            UID,
            "rsa2048",
            "sign",
            "0",
        ],
    );
    let secret = gpg_ok(
        &home,
        &[
            "--pinentry-mode",
            "loopback",
            "--passphrase-file",
            &pass,
            "--export-secret-keys",
        ],
    )
    .stdout;
    let public = gpg_ok(&home, &["--export"]).stdout;
    assert!(!secret.is_empty() && !public.is_empty(), "密钥导出非空");
    Some((secret, public))
}

/// 外部验签：全新 GNUPGHOME 只导入公钥，验证分离签名与明文签名。
fn verify_externally(scratch: &Path, public: &[u8], data: &[u8], detached: &[u8], inline: &[u8]) {
    let home = scratch.join("verify-home");
    private_dir(&home);
    let public_file = scratch.join("public.asc");
    let data_file = scratch.join("data.txt");
    let sig_file = scratch.join("data.sig");
    let inline_file = scratch.join("data.inline");
    std::fs::write(&public_file, public).expect("写公钥");
    std::fs::write(&data_file, data).expect("写数据");
    std::fs::write(&sig_file, detached).expect("写分离签名");
    std::fs::write(&inline_file, inline).expect("写明文签名");
    gpg_ok(&home, &["--import", &public_file.to_string_lossy()]);
    // gpg --verify 只在签名有效时 0 退出（locale 无关；文案随语言变化，不作断言）。
    gpg_ok(
        &home,
        &[
            "--verify",
            &sig_file.to_string_lossy(),
            &data_file.to_string_lossy(),
        ],
    );
    gpg_ok(&home, &["--verify", &inline_file.to_string_lossy()]);
}

#[test]
fn gpg_signer_isolated_home_signs_and_exports_verifiable_public_key() {
    let _guard = common::temp_lock();
    let scratch = common::Scratch::new("repo-sign");
    let Some((secret_key, public_key)) = key_fixture(scratch.path()) else {
        return;
    };

    let payload = common::payload(&secret_key, PASS, None, None);
    let backend = MemoryBackend::with_entry("repo-sign", payload).with_id(secret::KEYRING_ID);
    let reference = CredentialRef {
        name: "repo-sign".to_string(),
        source: CredentialSource::Keyring,
    };
    let signer = GpgSigner::new(&reference, &[Box::new(backend)]).expect("签名器装配");

    let data = b"Origin: Polypack\nSuite: stable\nComponent: main\n";
    let detached = signer.detach_sign(data).expect("分离签名");
    let inline = signer.clear_sign(data).expect("明文签名");
    assert!(detached.starts_with(b"-----BEGIN PGP SIGNATURE-----"));
    assert!(inline.starts_with(b"-----BEGIN PGP SIGNED MESSAGE-----"));

    let key_bin = signer.public_key(false).expect("二进制公钥");
    let key_asc = signer.public_key(true).expect("ASCII 公钥");
    assert_eq!(key_bin, public_key, "二进制公钥与生成侧一致");
    assert!(key_asc.starts_with(b"-----BEGIN PGP PUBLIC KEY BLOCK-----"));

    verify_externally(scratch.path(), &key_asc, data, &detached, &inline);

    drop(signer);
    assert!(
        common::sign_temp_leftovers().is_empty(),
        "GNUPGHOME 与口令文件零残留：{:?}",
        common::sign_temp_leftovers()
    );
}

/// 口令错误必须在装配 / 签名阶段响亮失败（E_SIG_0003），不得产出空签名。
#[test]
fn wrong_passphrase_is_an_error_not_an_empty_signature() {
    let _guard = common::temp_lock();
    let scratch = common::Scratch::new("repo-bad-pass");
    let Some((secret_key, _)) = key_fixture(scratch.path()) else {
        return;
    };
    let payload = common::payload(&secret_key, "wrong-pass", None, None);
    let backend = MemoryBackend::with_entry("repo-bad", payload).with_id(secret::KEYRING_ID);
    let reference = CredentialRef {
        name: "repo-bad".to_string(),
        source: CredentialSource::Keyring,
    };
    let signer = GpgSigner::new(&reference, &[Box::new(backend)]).expect("导入不校验口令");
    let err = signer.detach_sign(b"payload").expect_err("错误口令须失败");
    assert_eq!(err.code(), poly_sign::codes::E_SIG_0003);
    drop(signer);
    assert!(common::sign_temp_leftovers().is_empty());
}
