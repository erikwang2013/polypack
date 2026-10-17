//! T-M5-05 仓库元数据：结构 / 幂等（FakeSigner）+ 真跑（隔离 GNUPGHOME + apt / dnf）。
//!
//! 结构腿不依赖外部工具；真跑腿在缺 gpg / apt-get / dnf 的宿主打印 SKIP 并返回，
//! 绝不伪造成真跑。密钥只在内存 payload 与一次性 GNUPGHOME 中存在，结束断言零残留。

use std::io::Read as _;
use std::path::{Path, PathBuf};

use poly_bundle::arwrite::{self, Member};
use poly_cache::Digest;
use poly_sign::{RepoSigner, SignError};

use super::{deb, rfc2822, rpm};
use crate::release::{Artifact, RepoKind, RepoRequest};

const RPM_FIXTURE: &str = "tests/fixtures/repo/polypack-repo-probe-1.0-1.noarch.rpm";

/// 确定性假签名器：只验元数据装配 / 幂等，不碰密钥。
struct FakeSigner;

impl RepoSigner for FakeSigner {
    fn detach_sign(&self, _data: &[u8]) -> Result<Vec<u8>, SignError> {
        Ok(b"-----BEGIN PGP SIGNATURE-----\nFAKE\n-----END PGP SIGNATURE-----\n".to_vec())
    }

    fn clear_sign(&self, data: &[u8]) -> Result<Vec<u8>, SignError> {
        let mut out = b"-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\n".to_vec();
        out.extend_from_slice(data);
        out.extend_from_slice(
            b"\n-----BEGIN PGP SIGNATURE-----\nFAKE\n-----END PGP SIGNATURE-----\n",
        );
        Ok(out)
    }

    fn public_key(&self, armored: bool) -> Result<Vec<u8>, SignError> {
        Ok(if armored {
            b"-----BEGIN PGP PUBLIC KEY BLOCK-----\nFAKE\n-----END PGP PUBLIC KEY BLOCK-----\n"
                .to_vec()
        } else {
            b"FAKEBIN".to_vec()
        })
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("polypack-repo-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch 目录可建");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn request<'a>(repo: &'a Path, artifacts: &'a [Artifact], kind: RepoKind) -> RepoRequest<'a> {
    RepoRequest {
        target: "stable",
        version: "1.0-1",
        repo,
        kind,
        signing: None,
        artifacts,
        credential: None,
    }
}

fn artifact(path: &Path) -> Artifact {
    let bytes = std::fs::read(path).expect("产物可读");
    Artifact {
        name: path
            .file_name()
            .expect("产物有基名")
            .to_string_lossy()
            .into_owned(),
        bytes: bytes.len() as u64,
        digest: Digest::of_bytes(&bytes).to_string(),
        path: path.to_path_buf(),
    }
}

/// 手工 deb（复用 `arwrite` 的 ar / tar / gzip：测试不依赖 dpkg-deb）。
fn make_deb(dir: &Path, package: &str, version: &str, arch: &str) -> PathBuf {
    let control = format!(
        "Package: {package}\nVersion: {version}\nArchitecture: {arch}\n\
         Maintainer: PolyPack <test@polypack.invalid>\nDescription: repo fixture\n"
    );
    let control_tar = arwrite::tar_bytes(&[("./control".to_string(), control.into_bytes())])
        .expect("control tar");
    let control_gz =
        arwrite::gzip_bytes(&control_tar, &dir.join("control.tar.gz")).expect("control gzip");
    let data_tar =
        arwrite::tar_bytes(&[("./usr/share/fixture.txt".to_string(), b"fixture\n".to_vec())])
            .expect("data tar");
    let data_gz = arwrite::gzip_bytes(&data_tar, &dir.join("data.tar.gz")).expect("data gzip");
    let members = vec![
        Member {
            name: "debian-binary".to_string(),
            bytes: b"2.0\n".to_vec(),
        },
        Member {
            name: "control.tar.gz".to_string(),
            bytes: control_gz,
        },
        Member {
            name: "data.tar.gz".to_string(),
            bytes: data_gz,
        },
    ];
    let path = dir.join(format!("{package}_{version}_{arch}.deb"));
    arwrite::write_ar(&members, &path).expect("写 deb");
    path
}

fn gunzip(bytes: &[u8]) -> String {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut out = String::new();
    decoder.read_to_string(&mut out).expect("gzip 可解");
    out
}

#[test]
fn deb_repository_is_idempotent_and_apt_shaped() {
    let scratch = Scratch::new("deb");
    let artifacts_dir = scratch.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("产物目录");
    let app = make_deb(&artifacts_dir, "fixture-app", "1.0-1", "amd64");
    let lib = make_deb(&artifacts_dir, "libfixture", "1.0-1", "all");
    let artifacts = vec![artifact(&app), artifact(&lib)];
    let repo = scratch.path().join("repo");
    let request = request(&repo, &artifacts, RepoKind::Deb);

    deb::generate(&request, &FakeSigner).expect("首跑");
    let packages = repo.join("dists/stable/main/binary-amd64/Packages");
    let release = repo.join("dists/stable/Release");
    let first_packages = std::fs::read(&packages).expect("Packages");
    let first_release = std::fs::read(&release).expect("Release");
    let first_in_release = std::fs::read(repo.join("dists/stable/InRelease")).expect("InRelease");

    deb::generate(&request, &FakeSigner).expect("二跑");
    assert_eq!(
        first_packages,
        std::fs::read(&packages).expect("Packages"),
        "Packages 二跑字节一致"
    );
    assert_eq!(
        first_release,
        std::fs::read(&release).expect("Release"),
        "Release 二跑字节一致"
    );
    assert_eq!(
        first_in_release,
        std::fs::read(repo.join("dists/stable/InRelease")).expect("InRelease"),
        "假签名器下 InRelease 也字节一致（真跑仅签名时间戳可变）"
    );

    let packages = String::from_utf8(first_packages).expect("Packages UTF-8");
    assert!(packages.contains("Package: fixture-app\n"));
    assert!(
        packages.contains("Filename: pool/main/f/fixture-app/fixture-app_1.0-1_amd64.deb\n"),
        "pool 前缀取首字符：{packages}"
    );
    assert!(packages.contains("SHA256: "));
    assert!(repo.join("dists/stable/main/binary-all/Packages").exists());

    let release = String::from_utf8(first_release).expect("Release UTF-8");
    assert!(release.contains("Suite: stable\n"));
    assert!(release.contains("Architectures: all amd64\n"));
    assert!(release.contains("Components: main\n"));
    assert!(
        release
            .lines()
            .any(|line| line.starts_with(' ') && line.ends_with("main/binary-all/Packages")),
        "Release 的 SHA256 路径相对 dists/<suite>/：{release}"
    );
    let in_release = String::from_utf8(first_in_release).expect("InRelease UTF-8");
    assert!(
        in_release.contains(release.as_str()),
        "清签正文包含 Release"
    );

    assert_eq!(
        std::fs::read(repo.join("pool/main/libf/libfixture/libfixture_1.0-1_all.deb"))
            .expect("lib pool 产物"),
        std::fs::read(&lib).expect("源产物"),
        "pool 复制内容一致（lib* 取前 4 字符前缀）"
    );
    assert_eq!(
        std::fs::read(repo.join("polypack-repo.gpg")).expect("二进制公钥"),
        b"FAKEBIN"
    );
    assert!(repo.join("polypack-repo.asc").exists());
    assert!(repo.join("dists/stable/Release.gpg").exists());

    let report = poly_core::scan::scan_dir(&repo).expect("secret-scan 可执行");
    assert!(
        report.findings.is_empty(),
        "仓库内不得有密钥材料：{:?}",
        report.findings
    );
}

#[test]
fn rpm_repository_is_idempotent_and_repomd_shaped() {
    let scratch = Scratch::new("rpm");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join(RPM_FIXTURE);
    assert!(fixture.exists(), "缺 rpm 夹具：{}", fixture.display());
    let artifacts = vec![artifact(&fixture)];
    let repo = scratch.path().join("repo");
    let request = request(&repo, &artifacts, RepoKind::Rpm);

    rpm::generate(&request, &FakeSigner).expect("首跑");
    let paths = [
        repo.join("repodata/repomd.xml"),
        repo.join("repodata/primary.xml.gz"),
        repo.join("repodata/filelists.xml.gz"),
        repo.join("repodata/other.xml.gz"),
    ];
    let first: Vec<Vec<u8>> = paths
        .iter()
        .map(|path| std::fs::read(path).expect("repodata 产物"))
        .collect();

    rpm::generate(&request, &FakeSigner).expect("二跑");
    for (path, before) in paths.iter().zip(&first) {
        assert_eq!(
            *before,
            std::fs::read(path).expect("repodata 产物"),
            "{} 二跑字节一致",
            path.display()
        );
    }

    let primary = gunzip(&first[1]);
    assert!(primary.contains("<name>polypack-repo-probe</name>"));
    assert!(primary.contains("<arch>noarch</arch>"));
    assert!(primary.contains("epoch=\"0\" ver=\"1.0\" rel=\"1\""));
    assert!(primary.contains("<location href=\"rpm/polypack-repo-probe-1.0-1.noarch.rpm\"/>"));
    assert!(primary.contains("<rpm:license>Apache-2.0</rpm:license>"));
    assert!(primary.contains("<file>/usr/share/polypack/probe.txt</file>"));
    assert!(primary.contains("<rpm:provides>"), "应有 provides 段");
    let filelists = gunzip(&first[2]);
    assert!(filelists.contains("<file>/usr/share/doc/polypack-repo-probe/LICENSE</file>"));
    let other = gunzip(&first[3]);
    assert!(other.contains("<otherdata"));

    let repomd = String::from_utf8(first[0].clone()).expect("repomd UTF-8");
    for kind in ["primary", "filelists", "other"] {
        assert!(
            repomd.contains(&format!("<data type=\"{kind}\">")),
            "repomd 缺 {kind} 段：{repomd}"
        );
    }
    assert!(repomd.contains("open-checksum"));
    assert!(
        repomd.contains(&super::sha256_hex(&first[1])),
        "repomd 压缩摘要与 primary.xml.gz 同源"
    );
    assert!(
        repo.join("rpm/polypack-repo-probe-1.0-1.noarch.rpm")
            .exists()
    );
    assert!(repo.join("repodata/repomd.xml.asc").exists());

    let report = poly_core::scan::scan_dir(&repo).expect("secret-scan 可执行");
    assert!(
        report.findings.is_empty(),
        "仓库内不得有密钥材料：{:?}",
        report.findings
    );
    let _ = scratch;
}

#[test]
fn deb_control_filename_mismatch_is_e_release_0005() {
    let scratch = Scratch::new("deb-mismatch");
    let artifacts_dir = scratch.path().join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("产物目录");
    let deb_path = make_deb(&artifacts_dir, "fixture-app", "1.0-1", "amd64");
    let mut entry = artifact(&deb_path);
    entry.name = "renamed.deb".to_string();
    let artifacts = vec![entry];
    let repo = scratch.path().join("repo");
    let request = request(&repo, &artifacts, RepoKind::Deb);
    let err = deb::generate(&request, &FakeSigner).expect_err("文件名与 control 不一致须拒绝");
    assert_eq!(err.code.to_string(), "poly::E_RELEASE_0005");
}

#[test]
fn rfc2822_dates_are_utc_and_known() {
    assert_eq!(rfc2822(0), "Thu, 01 Jan 1970 00:00:00 UTC");
    assert_eq!(rfc2822(1_700_000_000), "Tue, 14 Nov 2023 22:13:20 UTC");
}

#[test]
fn rpm_evr_and_flags_are_derived_from_header_conventions() {
    let evr = |value: &str| {
        let (epoch, ver, rel) = rpm::split_evr(value);
        (epoch, ver, rel)
    };
    assert_eq!(
        evr("1.0-1"),
        ("0".to_string(), "1.0".to_string(), "1".to_string())
    );
    assert_eq!(
        evr("2:1.0-1"),
        ("2".to_string(), "1.0".to_string(), "1".to_string())
    );
    assert_eq!(evr(""), ("0".to_string(), String::new(), String::new()));
    assert_eq!(rpm::flags_text(0), "");
    assert_eq!(rpm::flags_text(2), "LT");
    assert_eq!(rpm::flags_text(4), "GT");
    assert_eq!(rpm::flags_text(8), "EQ");
    assert_eq!(rpm::flags_text(10), "LE");
    assert_eq!(rpm::flags_text(12), "GE");
    assert_eq!(rpm::flags_text(0x48), "EQ", "非版本位被掩掉");
}

mod real;
