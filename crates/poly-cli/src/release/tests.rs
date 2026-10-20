use super::report;
use super::*;
use poly_targets::Host;

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
    assert!(
        stored
            .sbom_digest
            .as_deref()
            .is_some_and(|digest| !digest.is_empty()),
        "发布回执须记录产物集合的 SBOM 摘要（T-M9-03）"
    );

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

fn credential_values(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

fn credential_specs() -> Vec<TargetSpec> {
    vec![
        spec("fedora", "rpm"),
        spec("freebsd", "pkg"),
        spec("netbsd", "pkgsrc"),
    ]
}

#[test]
fn bare_credential_is_default_for_every_target() {
    let creds = parse_credentials(
        &credential_values(&["keyring:release"]),
        &credential_specs(),
    )
    .expect("裸引用");
    assert_eq!(creds.resolve("fedora"), Some("keyring:release"));
    assert_eq!(creds.resolve("freebsd"), Some("keyring:release"));
}

#[test]
fn target_mapping_overrides_bare_default() {
    let values = credential_values(&[
        "vault:shared",
        "fedora=keyring:fedora-gpg",
        "freebsd=env:FREEBSD_KEY",
    ]);
    let creds = parse_credentials(&values, &credential_specs()).expect("裸引用 + 映射");
    assert_eq!(creds.resolve("fedora"), Some("keyring:fedora-gpg"));
    assert_eq!(creds.resolve("freebsd"), Some("env:FREEBSD_KEY"));
    assert_eq!(
        creds.resolve("netbsd"),
        Some("vault:shared"),
        "未映射目标用缺省"
    );
}

#[test]
fn credential_ref_with_colon_is_not_a_mapping() {
    let creds = parse_credentials(
        &credential_values(&["keyring:release"]),
        &credential_specs(),
    )
    .expect("冒号引用");
    assert_eq!(creds.resolve("fedora"), Some("keyring:release"));
}

#[test]
fn env_ref_with_equals_is_not_misparsed_as_mapping() {
    let values = credential_values(&["env:FREEBSD_KEY=value", "fedora=env:FOO=bar"]);
    let creds = parse_credentials(&values, &credential_specs()).expect("env 变量名可含 =");
    assert_eq!(
        creds.resolve("fedora"),
        Some("env:FOO=bar"),
        "映射右值保留 ="
    );
    assert_eq!(
        creds.resolve("freebsd"),
        Some("env:FREEBSD_KEY=value"),
        "左侧含冒号按整值引用"
    );
}

#[test]
fn duplicate_target_mapping_is_rejected() {
    let values = credential_values(&["fedora=keyring:a", "fedora=keyring:b"]);
    let err = parse_credentials(&values, &credential_specs()).expect_err("重复映射须报错");
    assert_eq!(err.code.to_string(), "poly::E_PLAN_0002");
    assert!(err.message.contains("fedora"), "{}", err.message);
    assert!(
        err.hint.as_deref().unwrap_or_default().contains("--target"),
        "修复提示点名 --target"
    );
}

#[test]
fn unknown_target_mapping_is_rejected() {
    let err = parse_credentials(
        &credential_values(&["fodora=keyring:typo"]),
        &credential_specs(),
    )
    .expect_err("未知目标须报错");
    assert_eq!(err.code.to_string(), "poly::E_PLAN_0002");
    assert!(err.message.contains("fodora"), "{}", err.message);
    assert!(
        err.hint.as_deref().unwrap_or_default().contains("--target"),
        "修复提示点名 --target"
    );
}

#[derive(Default)]
struct RecordingBackend {
    seen: std::cell::RefCell<Vec<(String, Option<String>)>>,
}

impl RepoBackend for RecordingBackend {
    fn publish(&self, request: &RepoRequest<'_>) -> Result<(), Diagnostic> {
        self.seen.borrow_mut().push((
            request.target.to_string(),
            request.credential.map(str::to_string),
        ));
        Ok(())
    }
}

#[test]
fn different_targets_receive_their_own_credentials() {
    let (root, project) = fixture("per-target-credential");
    seed(&root, "fedora", "app-0.1.0.rpm", b"rpm");
    seed(&root, "freebsd", "app-0.1.0.pkg", b"pkg");
    let specs = vec![spec("fedora", "rpm"), spec("freebsd", "pkg")];
    let values = credential_values(&["fedora=keyring:fedora-gpg", "freebsd=env:FREEBSD_KEY"]);
    let creds = parse_credentials(&values, &specs).expect("映射合法");
    let backend = RecordingBackend::default();
    let repo = root.join(".polypack").join("repo");
    for spec in &specs {
        let prepared = prepare(&project, spec, &repo).expect("准备须成功");
        release_target(&prepared, creds.resolve(&prepared.target), &backend, 1)
            .expect("发布须成功");
    }
    let seen = backend.seen.borrow();
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[0],
        ("fedora".to_string(), Some("keyring:fedora-gpg".to_string()))
    );
    assert_eq!(
        seen[1],
        ("freebsd".to_string(), Some("env:FREEBSD_KEY".to_string()))
    );
    drop(seen);
    let _ = std::fs::remove_dir_all(&root);
}

/// T-M5-06：无 pkg(8) 时报 E_RELEASE_0005 且不落回执；FreeBSD 宿主的真跑归 T-M5-09 VM。
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

/// T-M5-08：缺 pkg_info / pkg_admin 时报 E_RELEASE_0005 且不落回执；NetBSD 真跑归 T-M5-09 VM。
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

/// 宽松后端：接受任意仓库格式，只记录发布目标（`--changed` 选择测试用）。
#[derive(Default)]
struct CountBackend {
    seen: std::cell::RefCell<Vec<String>>,
}

impl RepoBackend for CountBackend {
    fn publish(&self, request: &RepoRequest<'_>) -> Result<(), Diagnostic> {
        self.seen.borrow_mut().push(request.target.to_string());
        Ok(())
    }
}

fn release_changed_with(prepared: &Prepared, backend: &CountBackend) -> ChangedOutcome {
    release_changed(prepared, Some("keyring:release"), backend, 100).expect("--changed 执行须成功")
}

#[test]
fn changed_skips_matching_receipt_and_publishes_missing_one() {
    let (root, project) = fixture("changed-mixed");
    seed(&root, "fedora", "app-0.1.0.rpm", b"rpm-bytes");
    seed(&root, "freebsd", "app-0.1.0.pkg", b"pkg-bytes");
    let fedora = prepared(&root, &project, "fedora", "rpm");
    let freebsd = prepared(&root, &project, "freebsd", "pkg");
    let backend = CountBackend::default();
    for prepared in [&fedora, &freebsd] {
        assert!(matches!(
            release_changed_with(prepared, &backend),
            ChangedOutcome::Published(_)
        ));
    }
    assert_eq!(backend.seen.borrow().as_slice(), ["fedora", "freebsd"]);

    // fedora：改成无 cache_key 的旧回执（runs=7），必须按 manifest_digest 降级命中。
    let mut old: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&fedora.receipt).expect("回执存在"))
            .expect("回执可解析");
    old.as_object_mut().expect("对象").remove("cache_key");
    old["runs"] = serde_json::json!(7);
    let old_bytes = serde_json::to_vec_pretty(&old).expect("序列化");
    std::fs::write(&fedora.receipt, &old_bytes).expect("写旧回执");
    // freebsd：删掉回执，模拟从未发布。
    std::fs::remove_file(&freebsd.receipt).expect("删回执");

    let ChangedOutcome::Skipped {
        receipt,
        key_source,
    } = release_changed_with(&fedora, &backend)
    else {
        panic!("fedora 有匹配回执，须 skipped");
    };
    assert_eq!(key_source, KeySource::ManifestDigest, "旧回执降级判定");
    let event = report::skipped_event(&fedora, &receipt, key_source);
    assert_eq!(event["event"], "release-skipped", "skipped 是独立事件");
    assert_eq!(event["target"], "fedora");
    assert_eq!(event["key_source"], "manifest_digest");
    assert_eq!(event["runs"], 7, "事件反映既有 runs，不递增");
    assert_eq!(
        std::fs::read(&fedora.receipt).expect("回执存在"),
        old_bytes,
        "skipped 不重写回执"
    );
    assert_eq!(backend.seen.borrow().len(), 2, "skipped 不触仓库");

    assert!(matches!(
        release_changed_with(&freebsd, &backend),
        ChangedOutcome::Published(_)
    ));
    assert_eq!(
        backend.seen.borrow().as_slice(),
        ["fedora", "freebsd", "freebsd"]
    );
    let stored: Receipt =
        serde_json::from_slice(&std::fs::read(&freebsd.receipt).expect("新回执存在"))
            .expect("回执可解析");
    assert_eq!(stored.runs, 1, "新目标首跑 runs=1");
    assert!(stored.cache_key.is_some(), "新回执写入 cache_key");
    // 新回执带 cache_key：产物未变即命中，回执零重写、runs 不递增。
    let before = std::fs::read(&freebsd.receipt).expect("回执存在");
    let ChangedOutcome::Skipped {
        receipt,
        key_source,
    } = release_changed_with(&freebsd, &backend)
    else {
        panic!("freebsd 未变，须 skipped");
    };
    assert_eq!(key_source, KeySource::CacheKey);
    assert_eq!(receipt.runs, 1);
    assert_eq!(
        std::fs::read(&freebsd.receipt).expect("回执存在"),
        before,
        "cache_key 命中同样不重写回执"
    );
    assert_eq!(backend.seen.borrow().len(), 3, "skipped 不触仓库");
    let _ = std::fs::remove_dir_all(&root);
}

/// T-M9-09：5 目标二次 `--changed` 只触达后端 1 次（首跑 5 次），其余 4 个
/// `release-skipped` 且回执字节零变化。
#[test]
fn five_target_changed_run_publishes_one_and_skips_four() {
    let (root, project) = fixture("changed-five");
    let targets = [
        ("fedora", "rpm", "rpm"),
        ("ubuntu", "deb", "deb"),
        ("mint", "deb", "deb"),
        ("freebsd", "pkg", "pkg"),
        ("netbsd", "pkgsrc", "tgz"),
    ];
    let mut first_run = Vec::new();
    for (target, format, extension) in targets {
        seed(
            &root,
            target,
            &format!("fixture-0.1.0.{extension}"),
            target.as_bytes(),
        );
        first_run.push(prepared(&root, &project, target, format));
    }
    let backend = CountBackend::default();
    for prepared in &first_run {
        assert!(matches!(
            release_changed_with(prepared, &backend),
            ChangedOutcome::Published(_)
        ));
    }
    assert_eq!(
        backend.seen.borrow().len(),
        5,
        "首跑 5 个目标各 1 次后端调用"
    );

    // 只改 mint 的产物：第二轮恰 1 个 published + 4 个 skipped。
    seed(&root, "mint", "fixture-0.1.0.deb", b"mint-v2");
    let changed = prepared(&root, &project, "mint", "deb");
    let untouched: Vec<(String, PathBuf, Vec<u8>)> = first_run
        .iter()
        .filter(|prepared| prepared.target != "mint")
        .map(|prepared| {
            (
                prepared.target.clone(),
                prepared.receipt.clone(),
                std::fs::read(&prepared.receipt).expect("首跑回执存在"),
            )
        })
        .collect();

    let mut published = Vec::new();
    let mut skipped = 0;
    let second = first_run
        .iter()
        .filter(|prepared| prepared.target != "mint")
        .chain([&changed]);
    for prepared in second {
        match release_changed_with(prepared, &backend) {
            ChangedOutcome::Published(_) => published.push(prepared.target.clone()),
            ChangedOutcome::Skipped { .. } => skipped += 1,
        }
    }
    assert_eq!(published, ["mint"], "只有变更目标发布");
    assert_eq!(skipped, 4, "其余四个目标 skipped");
    assert_eq!(
        backend.seen.borrow().len(),
        6,
        "第二轮只多 1 次后端调用（5 → 1）"
    );
    for (target, receipt, bytes) in &untouched {
        assert_eq!(
            &std::fs::read(receipt).expect("回执存在"),
            bytes,
            "{target} 的 skipped 回执不得重写"
        );
    }
    assert!(first_run[2].receipt.exists(), "变更前的旧回执永久保留");
    assert!(changed.receipt.exists(), "变更后的新回执已归档");
    let _ = std::fs::remove_dir_all(&root);
}
