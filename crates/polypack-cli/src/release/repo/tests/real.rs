//! T-M5-05 真跑：隔离 GNUPGHOME 签名 + apt update / dnf makecache 消费。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use base64::Engine as _;
use polypack_core::{CredentialRef, CredentialSource};
use polypack_sign::secret::{self, MemoryBackend};
use polypack_sign::{GpgSigner, RepoSigner};

use super::{RPM_FIXTURE, Scratch, artifact, deb, make_deb, request, rpm};
use crate::release::RepoKind;

const PASS: &str = "repo-cli-test-pass-123";
const UID: &str = "PolyPack Repo CLI Test <repo-cli@polypack.invalid>";

fn has_tool(tool: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(tool).is_file()))
}

fn private_dir(path: &Path) {
    std::fs::create_dir_all(path).expect("目录可建");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).expect("目录 0700");
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

/// 一次性受口令保护签名密钥（私钥导出字节）；缺 gpg → SKIP。
fn key_fixture(scratch: &Path) -> Option<Vec<u8>> {
    if !has_tool("gpg") {
        eprintln!("SKIP：宿主没有 gpg，仓库签名真跑跳过");
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
    assert!(!secret.is_empty(), "私钥导出非空");
    let _ = Command::new("gpgconf")
        .arg("--homedir")
        .arg(&home)
        .args(["--kill", "gpg-agent"])
        .output();
    Some(secret)
}

fn payload(secret_key: &[u8], passphrase: &str) -> Vec<u8> {
    const B64: base64::engine::general_purpose::GeneralPurpose =
        base64::engine::general_purpose::STANDARD;
    let json = serde_json::json!({
        "keystore": B64.encode(secret_key),
        "store_password": passphrase,
    });
    B64.encode(serde_json::to_vec(&json).expect("payload JSON"))
        .into_bytes()
}

/// 真签名器：凭据引用 + 内存后端（与 polypack-sign 真跑同口径）。
fn real_signer(scratch: &Path, name: &str) -> Option<GpgSigner> {
    let secret = key_fixture(scratch)?;
    let backend =
        MemoryBackend::with_entry(name, payload(&secret, PASS)).with_id(secret::KEYRING_ID);
    let reference = CredentialRef {
        name: name.to_string(),
        source: CredentialSource::Keyring,
    };
    Some(GpgSigner::new(&reference, &[Box::new(backend)]).expect("签名器装配"))
}

fn sign_temp_leftovers() -> Vec<PathBuf> {
    let prefix = format!("polypack-sign-{}-", std::process::id());
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix(&prefix))
                .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect()
}

fn native_deb_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "i386",
        "arm" => "armhf",
        "powerpc64" => "ppc64el",
        "s390x" => "s390x",
        "riscv64" => "riscv64",
        _ => "all",
    }
}

fn apt_update(scratch: &Path, repo: &Path) -> Output {
    let empty = scratch.join("apt-empty");
    let lists = scratch.join("apt-lists");
    let cache = scratch.join("apt-cache");
    let log = scratch.join("apt-log");
    for dir in [&empty, &lists, &cache, &log] {
        std::fs::create_dir_all(dir).expect("apt 目录可建");
    }
    let source = scratch.join("sources.list");
    std::fs::write(
        &source,
        format!(
            "deb [signed-by={}] file:{} stable main\n",
            repo.join("polypack-repo.gpg").display(),
            repo.display()
        ),
    )
    .expect("写 sources.list");
    let options = [
        format!("Dir::Etc::sourcelist={}", source.display()),
        format!("Dir::Etc::sourceparts={}", empty.display()),
        format!("Dir::Etc::parts={}", empty.display()),
        format!("Dir::State::Lists={}", lists.display()),
        format!("Dir::Cache={}", cache.display()),
        format!("Dir::Log={}", log.display()),
        format!(
            "Dir::State::extended_states={}",
            scratch.join("extended").display()
        ),
        "Dir::State::status=/dev/null".to_string(),
        "Debug::NoLocking=1".to_string(),
        "APT::Get::List-Cleanup=0".to_string(),
        "Acquire::Languages=none".to_string(),
    ];
    let mut command = Command::new("apt-get");
    for option in &options {
        command.arg("-o").arg(option);
    }
    command.arg("update").output().expect("apt-get 可执行")
}

fn verify_release_signatures(scratch: &Path, repo: &Path, public_asc: &[u8]) {
    let home = scratch.join("verify-home");
    private_dir(&home);
    let public = scratch.join("public.asc");
    std::fs::write(&public, public_asc).expect("写公钥");
    gpg_ok(&home, &["--import", &public.to_string_lossy()]);
    let detached = repo.join("dists/stable/Release.gpg");
    let release = repo.join("dists/stable/Release");
    let inline = repo.join("dists/stable/InRelease");
    gpg_ok(
        &home,
        &[
            "--verify",
            &detached.to_string_lossy(),
            &release.to_string_lossy(),
        ],
    );
    gpg_ok(&home, &["--verify", &inline.to_string_lossy()]);
}

#[test]
fn apt_update_consumes_signed_repository_and_rejects_tamper() {
    if !has_tool("apt-get") {
        eprintln!("SKIP：宿主没有 apt-get，apt update 真跑跳过");
        return;
    }
    let scratch = Scratch::new("apt");
    let Some(signer) = real_signer(scratch.path(), "repo-apt") else {
        return;
    };
    let artifacts_dir = scratch.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("产物目录");
    let arch = native_deb_arch();
    let deb_path = make_deb(&artifacts_dir, "fixture-app", "1.0-1", arch);
    let artifacts = vec![artifact(&deb_path)];
    let repo = scratch.path().join("repo");
    let request = request(&repo, &artifacts, RepoKind::Deb);
    deb::generate(&request, &signer).expect("生成签名仓库");

    let public = signer.public_key(true).expect("ASCII 公钥");
    verify_release_signatures(scratch.path(), &repo, &public);

    let first = apt_update(scratch.path(), &repo);
    assert!(
        first.status.success(),
        "apt update 应通过签名校验：\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );

    let packages = repo.join(format!("dists/stable/main/binary-{arch}/Packages"));
    let mut tampered = std::fs::read(&packages).expect("Packages");
    tampered.extend_from_slice(b"Tampered: yes\n");
    std::fs::write(&packages, tampered).expect("篡改 Packages");
    let _ = std::fs::remove_dir_all(scratch.path().join("apt-lists"));
    let second = apt_update(scratch.path(), &repo);
    assert!(
        !second.status.success(),
        "篡改 Packages 后 apt update 必须失败（Release 哈希不再匹配）：\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );

    drop(signer);
    assert!(
        sign_temp_leftovers().is_empty(),
        "GNUPGHOME 与口令文件零残留：{:?}",
        sign_temp_leftovers()
    );
}

#[test]
fn dnf_makecache_consumes_signed_repository_when_available() {
    if !has_tool("dnf") {
        eprintln!(
            "SKIP：宿主没有 dnf，dnf makecache 真跑跳过（容器口径见 crates/polypack-cli/README.md）"
        );
        return;
    }
    let scratch = Scratch::new("dnf");
    let Some(signer) = real_signer(scratch.path(), "repo-dnf") else {
        return;
    };
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join(RPM_FIXTURE);
    assert!(fixture.exists(), "缺 rpm 夹具：{}", fixture.display());
    let artifacts = vec![artifact(&fixture)];
    let repo = scratch.path().join("repo");
    let request = request(&repo, &artifacts, RepoKind::Rpm);
    rpm::generate(&request, &signer).expect("生成签名 rpm 仓库");

    let empty = scratch.path().join("dnf-empty");
    let cache = scratch.path().join("dnf-cache");
    let persist = scratch.path().join("dnf-persist");
    for dir in [&empty, &cache, &persist] {
        std::fs::create_dir_all(dir).expect("dnf 目录可建");
    }
    let conf = scratch.path().join("dnf.conf");
    std::fs::write(
        &conf,
        format!(
            "[main]\n\
             gpgcheck=0\n\
             cachedir={}\n\
             persistdir={}\n\
             reposdir={}\n\
             [polypack]\n\
             name=PolyPack\n\
             baseurl=file://{}\n\
             enabled=1\n\
             repo_gpgcheck=1\n\
             gpgcheck=0\n\
             gpgkey=file://{}\n",
            cache.display(),
            persist.display(),
            empty.display(),
            repo.display(),
            repo.join("polypack-repo.asc").display()
        ),
    )
    .expect("写 dnf.conf");
    let output = Command::new("dnf")
        .args([
            "-c",
            &conf.to_string_lossy(),
            "--disablerepo=*",
            "--enablerepo=polypack",
            "-y",
            "makecache",
        ])
        .output()
        .expect("dnf 可执行");
    assert!(
        output.status.success(),
        "dnf makecache 应通过 repo_gpgcheck：\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    drop(signer);
    assert!(sign_temp_leftovers().is_empty());
}
