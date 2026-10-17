use super::*;
use poly_targets::Host;

/// 最小工程骨架（口径同 publish 测试）。
fn fixture(tag: &str) -> (PathBuf, Project) {
    let root = std::env::temp_dir().join(format!("polypack-release-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("工程目录可建");
    let root = root.canonicalize().expect("工程根可规范化");
    std::fs::write(
        root.join("polypack.toml"),
        "[package]\nname = \"fixture-app\"\nversion = \"0.1.0\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("写配置");
    let project = crate::project::load(&root.join("polypack.toml")).expect("配置可加载");
    (root, project)
}

fn spec(id: &str, format: &str) -> TargetSpec {
    let mut spec = TargetSpec::new_stub(id, 2, &[Host::Linux]);
    spec.package.format = format.to_string();
    spec
}

fn seed(root: &Path, target: &str, name: &str, bytes: &[u8]) -> PathBuf {
    let dir = root.join(".polypack").join("packages").join(target);
    std::fs::create_dir_all(&dir).expect("产物目录可建");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("写产物");
    path
}

fn prepared(root: &Path, project: &Project, target: &str, format: &str) -> Prepared {
    prepare(
        project,
        &spec(target, format),
        &root.join(".polypack").join("repo"),
    )
    .expect("准备须成功")
}

#[derive(Default)]
struct MockBackend {
    calls: std::cell::Cell<usize>,
}

impl RepoBackend for MockBackend {
    fn publish(&self, request: &RepoRequest<'_>) -> Result<(), Diagnostic> {
        assert_eq!(request.kind, RepoKind::Rpm);
        assert_eq!(request.credential, Some("keyring:release"));
        assert!(request.repo.ends_with(".polypack/repo"));
        assert!(!request.artifacts.is_empty());
        self.calls.set(self.calls.get() + 1);
        Ok(())
    }
}

fn artifact(name: &str, digest: &str) -> Artifact {
    Artifact {
        name: name.to_string(),
        bytes: 1,
        digest: digest.to_string(),
        path: PathBuf::new(),
    }
}

#[test]
fn prepare_collects_sorted_artifacts_and_fixed_receipt_path() {
    let (root, project) = fixture("prepare");
    seed(&root, "fedora", "b.rpm", b"b");
    seed(&root, "fedora", "a.rpm", b"aa");
    seed(&root, "fedora", "notes.txt", b"ignored");
    let prepared = prepared(&root, &project, "fedora", "rpm");
    let names: Vec<&str> = prepared
        .artifacts
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(names, ["a.rpm", "b.rpm"], "只收目标扩展名，按名排序");
    let short = &prepared.manifest_digest[..16];
    assert_eq!(
        prepared.receipt,
        root.join(".polypack")
            .join("release")
            .join("fedora")
            .join(format!("0.1.0-{short}.json"))
    );
    assert_eq!(
        Digest::parse(&prepared.manifest_digest)
            .expect("摘要合法")
            .to_string(),
        prepared.manifest_digest
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn manifest_digest_is_order_independent_but_content_bound() {
    let a = artifact("a.rpm", "aa");
    let b = artifact("b.rpm", "bb");
    assert_eq!(
        manifest_digest(&[a.clone(), b.clone()]),
        manifest_digest(&[b.clone(), a.clone()]),
        "文件系统顺序不影响清单摘要"
    );
    let mut changed = b.clone();
    changed.digest = "cc".to_string();
    assert_ne!(
        manifest_digest(&[a.clone(), b]),
        manifest_digest(&[a, changed]),
        "内容变化必须换键"
    );
}

#[test]
fn first_run_writes_receipt_and_same_manifest_rerun_is_noop() {
    let (root, project) = fixture("idempotent");
    seed(&root, "fedora", "app-0.1.0.rpm", b"rpm-bytes");
    let prepared = prepared(&root, &project, "fedora", "rpm");
    let backend = MockBackend::default();

    let first =
        release_target(&prepared, Some("keyring:release"), &backend, 100).expect("首跑须成功");
    assert!(!first.cache_hit);
    assert_eq!(first.runs, 1);
    assert_eq!(backend.calls.get(), 1);
    assert!(
        !prepared.receipt.with_extension("json.tmp").exists(),
        "原子落盘无临时文件残留"
    );

    let stored: Receipt =
        serde_json::from_slice(&std::fs::read(&prepared.receipt).expect("回执存在"))
            .expect("回执可解析");
    assert_eq!(stored.schema_version, SCHEMA_VERSION);
    assert_eq!(stored.manifest_digest, prepared.manifest_digest);
    assert_eq!(stored.artifacts.len(), 1);

    let second =
        release_target(&prepared, Some("keyring:release"), &backend, 200).expect("二跑须 no-op");
    assert!(second.cache_hit, "同清单摘要 + 同仓库 = 命中");
    assert_eq!(second.runs, 2);
    assert_eq!(backend.calls.get(), 1, "no-op 不触仓库");
    let stored: Receipt =
        serde_json::from_slice(&std::fs::read(&prepared.receipt).expect("回执存在"))
            .expect("回执可解析");
    assert_eq!(stored.runs, 2);
    assert_eq!(stored.first_unix, 100);
    assert_eq!(stored.last_unix, 200);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn changed_artifact_set_gets_new_receipt_key() {
    let (root, project) = fixture("changed");
    seed(&root, "fedora", "app-0.1.0.rpm", b"v1");
    let first_prepared = prepared(&root, &project, "fedora", "rpm");
    let backend = MockBackend::default();
    release_target(&first_prepared, Some("keyring:release"), &backend, 100).expect("首跑");

    seed(&root, "fedora", "app-0.1.0.rpm", b"v2");
    let second_prepared = prepared(&root, &project, "fedora", "rpm");
    assert_ne!(
        first_prepared.receipt, second_prepared.receipt,
        "产物变化 → 新清单摘要 → 新回执键"
    );
    let second =
        release_target(&second_prepared, Some("keyring:release"), &backend, 200).expect("二跑");
    assert!(!second.cache_hit);
    assert_eq!(second.runs, 1, "runs 按清单键计数");
    assert_eq!(backend.calls.get(), 2);
    assert!(
        first_prepared.receipt.exists() && second_prepared.receipt.exists(),
        "两份回执均永久保留"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unknown_receipt_schema_is_an_error_not_silent_rebuild() {
    let (root, project) = fixture("schema");
    seed(&root, "fedora", "app-0.1.0.rpm", b"bytes");
    let prepared = prepared(&root, &project, "fedora", "rpm");
    std::fs::create_dir_all(prepared.receipt.parent().expect("父目录")).expect("建目录");
    std::fs::write(&prepared.receipt, r#"{"schema_version":99}"#).expect("写回执");
    let backend = MockBackend::default();
    let err = release_target(&prepared, None, &backend, 1).expect_err("陌生 schema 须报错");
    assert_eq!(err.code.to_string(), "poly::E_RELEASE_0003");
    assert_eq!(backend.calls.get(), 0, "读回执失败不得触仓库");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unsupported_format_and_missing_artifacts_have_stable_codes() {
    let (root, project) = fixture("codes");
    let repo = root.join(".polypack").join("repo");
    let err = prepare(&project, &spec("linux", "appimage"), &repo).expect_err("非仓库格式");
    assert_eq!(err.code.to_string(), "poly::E_RELEASE_0002");

    let err = prepare(&project, &spec("fedora", "rpm"), &repo).expect_err("无产物");
    assert_eq!(err.code.to_string(), "poly::E_RELEASE_0001");
    assert!(
        err.hint
            .as_deref()
            .unwrap_or_default()
            .contains("poly build --target fedora"),
        "修复提示点名出包命令"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn resolve_targets_dedupes_and_reports_unknown_ids() {
    let registry =
        Registry::from_specs(vec![spec("fedora", "rpm"), spec("freebsd", "pkg")]).expect("注册");
    let ids = vec![
        "freebsd".to_string(),
        "fedora".to_string(),
        "freebsd".to_string(),
    ];
    let resolved = resolve_targets(&ids, &registry).expect("解析");
    let ids: Vec<&str> = resolved.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, ["freebsd", "fedora"], "去重保序");

    let err = resolve_targets(&["nope".to_string()], &registry).expect_err("未知目标");
    assert_eq!(err.code.to_string(), "poly::E_TGT_0001");
}

/// T-M5-06：pkg 已接本地后端；本机无 pkg(8) 时如实报 E_RELEASE_0005 且不落回执。
/// 有 pkg 的宿主（FreeBSD）跳过该分支断言——真跑与安装冒烟归 T-M5-09 VM。
#[test]
fn pkg_backend_requires_pkg_tool_and_leaves_no_receipt() {
    if poly_bundle::exec::tool_present("pkg") {
        eprintln!("SKIP：本机有 pkg(8)，缺工具分支不可复现（FreeBSD 真跑归 T-M5-09）");
        return;
    }
    let (root, project) = fixture("pkg-backend");
    seed(&root, "freebsd", "app-0.1.0.pkg", b"bytes");
    let prepared = prepared(&root, &project, "freebsd", "pkg");
    let err = release_target(&prepared, None, &repo::LocalBackend, 1).expect_err("缺 pkg 须失败");
    assert_eq!(err.code.to_string(), "poly::E_RELEASE_0005");
    assert!(err.message.contains("FreeBSD"), "{}", err.message);
    assert!(!prepared.receipt.exists(), "失败不落回执");
    let _ = std::fs::remove_dir_all(&root);
}

/// T-M5-08：pkgsrc 已接本地后端；本机缺 pkg_info / pkg_admin 时如实报 E_RELEASE_0005
/// 且不落回执。有 pkg_install 的宿主（NetBSD）跳过该分支断言——真跑归 T-M5-09 VM。
#[test]
fn pkgsrc_backend_requires_netbsd_tools_and_leaves_no_receipt() {
    if ["pkg_info", "pkg_admin"]
        .iter()
        .all(|tool| poly_bundle::exec::tool_present(tool))
    {
        eprintln!("SKIP：本机有 pkg_install，缺工具分支不可复现（NetBSD 真跑归 T-M5-09）");
        return;
    }
    let (root, project) = fixture("pkgsrc-backend");
    seed(&root, "netbsd", "app-0.1.0.tgz", b"tgz-bytes");
    let prepared = prepared(&root, &project, "netbsd", "pkgsrc");
    let err =
        release_target(&prepared, None, &repo::LocalBackend, 1).expect_err("缺 pkg_install 须失败");
    assert_eq!(err.code.to_string(), "poly::E_RELEASE_0005");
    assert!(err.message.contains("NetBSD"), "{}", err.message);
    assert!(!prepared.receipt.exists(), "失败不落回执");
    let _ = std::fs::remove_dir_all(&root);
}
