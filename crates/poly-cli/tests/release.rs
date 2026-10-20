//! T-M5-11 准备面：`poly release` 参数面与稳定错误码（deb/rpm 已接线 T-M5-05；pkg 待 T-M5-06）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

use base64::Engine as _;

/// 最小工程骨架（只需配置可装载）。
fn fixture(tag: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("polypack-release-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("工程目录可建");
    std::fs::write(
        root.join("polypack.toml"),
        "[package]\nname = \"fixture-app\"\nversion = \"0.1.0\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("写配置");
    root
}

fn poly(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("poly 可执行")
}

#[test]
fn help_lists_release_surface() {
    let root = fixture("help");
    let out = poly(&root, &["release", "--help"]);
    assert_eq!(out.status.code(), Some(0), "--help 恒 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    for flag in [
        "--target",
        "--repo",
        "--credential",
        "--dry-run",
        "--changed",
        "--since",
    ] {
        assert!(stdout.contains(flag), "help 须含 {flag}：{stdout}");
    }
    assert!(
        stdout.contains("release-skipped"),
        "help 须说明 skipped 事件：{stdout}"
    );
    for syntax in ["TARGET=REF", "env:VAR"] {
        assert!(
            stdout.contains(syntax),
            "help 须解释按目标凭据语法 {syntax}：{stdout}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn comma_separated_targets_are_split_and_rejected_by_format() {
    let root = fixture("comma");
    let out = poly(&root, &["release", "--target", "linux,web", "--dry-run"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("poly::E_RELEASE_0002"),
        "非仓库格式报稳定码：{stderr}"
    );
    assert!(
        stderr.contains("appimage"),
        "逗号拆分后首个目标 = linux（appimage）：{stderr}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn repeated_target_flag_is_accepted() {
    let root = fixture("repeat");
    let out = poly(
        &root,
        &[
            "release",
            "--target",
            "linux",
            "--target",
            "web",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("poly::E_RELEASE_0002"),
        "重复 --target 被接受并走到格式校验：{stderr}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unknown_target_uses_target_code_and_hint() {
    let root = fixture("unknown");
    let out = poly(&root, &["release", "--target", "no-such", "--dry-run"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_TGT_0001"), "{stderr}");
    assert!(
        stderr.contains("poly doctor"),
        "修复提示指向目标矩阵：{stderr}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 用既有 `linux` 目标 + `[targets.linux].format` 覆盖模拟 rpm 目标（真实 fedora 目标
/// 待 T-M5-05/06/07 入 main；本测试只验 poly-cli 面，不依赖其实现）。
fn fixture_rpm(tag: &str) -> PathBuf {
    let root = fixture(tag);
    let mut config = std::fs::read_to_string(root.join("polypack.toml")).expect("读配置");
    config.push_str("[targets.linux]\nformat = \"rpm\"\n");
    std::fs::write(root.join("polypack.toml"), config).expect("写配置");
    let dir = root.join(".polypack").join("packages").join("linux");
    std::fs::create_dir_all(&dir).expect("产物目录可建");
    std::fs::write(dir.join("fixture-app-0.1.0-1.x86_64.rpm"), b"rpm-bytes").expect("写产物");
    root
}

/// `--json` 输出恰一行，便于整行断言（stdout 必须纯 JSON Lines）。
fn only_json_line(stdout: &[u8]) -> serde_json::Value {
    let text = String::from_utf8_lossy(stdout);
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 1, "stdout 应恰一行 JSON：{text}");
    serde_json::from_str(lines[0])
        .unwrap_or_else(|err| panic!("行不是合法 JSON：{}（{err}）", lines[0]))
}

#[test]
fn dry_run_reports_manifest_and_receipt_anchor() {
    // 回执路径由 CLI canonicalize 产出（macOS /private/var、Windows \\?\ 长名 / 8.3 别名），
    // 期望锚点按同一形态取，否则裸比较必假红。
    let root = std::fs::canonicalize(fixture_rpm("dry")).expect("工程根可解析");
    let out = poly(
        &root,
        &["release", "--target", "linux", "--dry-run", "--json"],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "dry-run 须成功：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value = only_json_line(&out.stdout);
    assert_eq!(value["event"], "release-dry-run");
    assert_eq!(value["target"], "linux");
    assert_eq!(value["repo_kind"], "rpm", "配置 format 覆盖生效");
    assert_eq!(value["artifact_count"], 1);
    assert_eq!(value["cache_hit"].as_bool(), Some(false));
    let receipt = Path::new(value["receipt"].as_str().expect("回执路径"));
    assert_eq!(
        receipt.parent(),
        Some(
            root.join(".polypack")
                .join("release")
                .join("linux")
                .as_path()
        ),
        "回执落点"
    );
    assert!(
        receipt
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("0.1.0-") && name.ends_with(".json")),
        "回执文件名 = 版本-摘要前 16：{}",
        receipt.display()
    );
    assert!(!receipt.exists(), "dry-run 不落回执");
    let _ = std::fs::remove_dir_all(&root);
}

/// 两个仓库格式目标（linux / web 均覆盖为 rpm），供 per-target 凭据测试。
fn fixture_two_rpm(tag: &str) -> PathBuf {
    let root = fixture(tag);
    let mut config = std::fs::read_to_string(root.join("polypack.toml")).expect("读配置");
    config.push_str("[targets.linux]\nformat = \"rpm\"\n[targets.web]\nformat = \"rpm\"\n");
    std::fs::write(root.join("polypack.toml"), config).expect("写配置");
    for target in ["linux", "web"] {
        let dir = root.join(".polypack").join("packages").join(target);
        std::fs::create_dir_all(&dir).expect("产物目录可建");
        std::fs::write(
            dir.join(format!("fixture-app-0.1.0-1.{target}.rpm")),
            b"rpm-bytes",
        )
        .expect("写产物");
    }
    root
}

#[test]
fn dry_run_accepts_per_target_credential_mappings() {
    let root = fixture_two_rpm("per-target-cred");
    let out = poly(
        &root,
        &[
            "release",
            "--target",
            "linux,web",
            "--dry-run",
            "--json",
            "--credential",
            "linux=keyring:linux-gpg",
            "--credential",
            "web=env:WEB_KEY",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "凭据映射须解析成功：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let events: Vec<serde_json::Value> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|err| panic!("行不是合法 JSON：{line}（{err}）"))
        })
        .collect();
    assert_eq!(events.len(), 2, "每个目标一条 dry-run 事件：{text}");
    assert_eq!(events[0]["target"], "linux");
    assert_eq!(events[1]["target"], "web");
    for event in &events {
        assert_eq!(event["event"], "release-dry-run");
        assert_eq!(event["credential_given"].as_bool(), Some(true));
    }
    assert!(
        !text.contains("linux-gpg") && !text.contains("WEB_KEY"),
        "报告不得泄漏凭据引用值：{text}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// pkg 格式走 T-M5-06 本地后端；无 pkg(8) 的宿主只验稳定错误码与不落回执。
fn fixture_pkg(tag: &str) -> PathBuf {
    let root = fixture(tag);
    let mut config = std::fs::read_to_string(root.join("polypack.toml")).expect("读配置");
    config.push_str("[targets.linux]\nformat = \"pkg\"\n");
    std::fs::write(root.join("polypack.toml"), config).expect("写配置");
    let dir = root.join(".polypack").join("packages").join("linux");
    std::fs::create_dir_all(&dir).expect("产物目录可建");
    std::fs::write(dir.join("fixture-app-0.1.0.pkg"), b"pkg-bytes").expect("写产物");
    root
}

#[test]
fn pkg_backend_without_tool_reports_stable_code_and_leaves_no_receipt() {
    if poly_bundle::exec::tool_present("pkg") {
        eprintln!("SKIP：本机有 pkg(8)，缺工具分支不可复现（FreeBSD 真跑归 T-M5-09）");
        return;
    }
    let root = fixture_pkg("pending");
    let out = poly(&root, &["release", "--target", "linux"]);
    assert_eq!(out.status.code(), Some(1), "缺 pkg(8) 须失败");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_RELEASE_0005"), "{stderr}");
    assert!(
        stderr.contains("FreeBSD"),
        "修复提示点名 FreeBSD 用户态：{stderr}"
    );
    assert!(
        !root.join(".polypack").join("release").exists(),
        "失败不落回执"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// pkgsrc 格式走 T-M5-08 本地后端；无 pkg_install 的宿主只验稳定错误码与不落回执。
#[test]
fn pkgsrc_backend_without_tools_reports_stable_code_and_leaves_no_receipt() {
    if ["pkg_info", "pkg_admin"]
        .iter()
        .all(|tool| poly_bundle::exec::tool_present(tool))
    {
        eprintln!("SKIP：本机有 pkg_install，缺工具分支不可复现（NetBSD 真跑归 T-M5-09）");
        return;
    }
    let root = fixture("pkgsrc-pending");
    let dir = root.join(".polypack").join("packages").join("netbsd");
    std::fs::create_dir_all(&dir).expect("产物目录可建");
    std::fs::write(dir.join("fixture-app-0.1.0.tgz"), b"tgz-bytes").expect("写产物");
    let out = poly(&root, &["release", "--target", "netbsd"]);
    assert_eq!(out.status.code(), Some(1), "缺 pkg_install 须失败");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_RELEASE_0005"), "{stderr}");
    assert!(
        stderr.contains("NetBSD"),
        "修复提示点名 NetBSD 用户态：{stderr}"
    );
    assert!(
        !root.join(".polypack").join("release").exists(),
        "失败不落回执"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rerun_with_matching_receipt_is_published_noop() {
    let root = fixture_rpm("noop");
    let dry = poly(
        &root,
        &["release", "--target", "linux", "--dry-run", "--json"],
    );
    assert_eq!(dry.status.code(), Some(0));
    let value = only_json_line(&dry.stdout);
    let receipt_path = PathBuf::from(value["receipt"].as_str().expect("回执路径"));
    // 模拟前一次成功发布留下的回执（真实首跑要等仓库后端接线）。
    let receipt = serde_json::json!({
        "schema_version": 1,
        "target": "linux",
        "version": "0.1.0",
        "repo": value["repo"].clone(),
        "repo_kind": "rpm",
        "manifest_digest": value["manifest_digest"].clone(),
        "artifacts": value["artifacts"].clone(),
        "runs": 1,
        "first_unix": 100,
        "last_unix": 100,
    });
    std::fs::create_dir_all(receipt_path.parent().expect("父目录")).expect("建目录");
    std::fs::write(
        &receipt_path,
        serde_json::to_vec_pretty(&receipt).expect("序列化"),
    )
    .expect("写回执");

    let out = poly(&root, &["release", "--target", "linux", "--json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "命中回执即 no-op：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value = only_json_line(&out.stdout);
    assert_eq!(value["event"], "published");
    assert_eq!(value["cache_hit"].as_bool(), Some(true));
    assert_eq!(value["runs"], 2, "no-op 也如实 runs+1");
    assert_eq!(value["receipt"], receipt_path.display().to_string());
    let stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&receipt_path).expect("回执存在"))
            .expect("回执可解析");
    assert_eq!(stored["runs"], 2);
    assert_eq!(stored["first_unix"], 100, "首跑时刻保留");
    let _ = std::fs::remove_dir_all(&root);
}

/// 两个目标都写旧格式回执（无 cache_key）：`--changed` 按 manifest_digest 全部跳过。
#[test]
fn changed_skips_all_unchanged_targets_and_leaves_receipts_untouched() {
    let root = fixture_two_rpm("changed-all-skip");
    let dry = poly(
        &root,
        &["release", "--target", "linux,web", "--dry-run", "--json"],
    );
    assert_eq!(dry.status.code(), Some(0));
    let mut receipts = Vec::new();
    for line in String::from_utf8_lossy(&dry.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let value: serde_json::Value = serde_json::from_str(line).expect("dry-run JSON");
        assert_eq!(value["event"], "release-dry-run");
        let receipt_path = PathBuf::from(value["receipt"].as_str().expect("回执路径"));
        let receipt = serde_json::json!({
            "schema_version": 1,
            "target": value["target"].clone(),
            "version": value["version"].clone(),
            "repo": value["repo"].clone(),
            "repo_kind": value["repo_kind"].clone(),
            "manifest_digest": value["manifest_digest"].clone(),
            "artifacts": value["artifacts"].clone(),
            "runs": 4,
            "first_unix": 100,
            "last_unix": 200,
        });
        std::fs::create_dir_all(receipt_path.parent().expect("父目录")).expect("建目录");
        let bytes = serde_json::to_vec_pretty(&receipt).expect("序列化");
        std::fs::write(&receipt_path, &bytes).expect("写回执");
        receipts.push((receipt_path, bytes, value["target"].clone()));
    }
    assert_eq!(receipts.len(), 2, "两个目标各一条 dry-run 事件");

    let out = poly(
        &root,
        &["release", "--target", "linux,web", "--changed", "--json"],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "全部 skipped 须退出 0：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let events: Vec<serde_json::Value> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("JSON Lines"))
        .collect();
    assert_eq!(events.len(), 2, "{text}");
    for (event, (_, _, target)) in events.iter().zip(&receipts) {
        assert_eq!(event["event"], "release-skipped");
        assert_eq!(event["target"].as_str(), target.as_str());
        assert_eq!(event["key_source"], "manifest_digest", "旧回执降级");
        assert_eq!(event["runs"], 4, "skipped 不递增 runs");
    }
    for (path, bytes, _) in &receipts {
        assert_eq!(
            &std::fs::read(path).expect("回执存在"),
            bytes,
            "skipped 不重写回执"
        );
    }

    // 人读通道同样输出独立 skipped 事件，不伪装成 published。
    let human = poly(&root, &["release", "--target", "linux,web", "--changed"]);
    assert_eq!(human.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert_eq!(stdout.matches("release-skipped").count(), 2, "{stdout}");
    assert!(
        !stdout.contains("· published"),
        "不得混入 published：{stdout}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn since_reports_clear_diagnostic_instead_of_silent_ignore() {
    let root = fixture("since");
    let out = poly(
        &root,
        &[
            "release",
            "--target",
            "linux",
            "--changed",
            "--since",
            "HEAD",
            "--dry-run",
        ],
    );
    assert_eq!(out.status.code(), Some(1), "--since 不支持须失败");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_PLAN_0002"), "{stderr}");
    assert!(stderr.contains("--since"), "诊断须点名 --since：{stderr}");
    assert!(
        stderr.contains("--changed"),
        "修复提示指向 --changed：{stderr}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// T-M9-09 基准目标：均以 `[targets.<id>].format = "deb"` 覆盖为仓库格式。
const FIVE_TARGETS: [&str; 5] = ["linux", "web", "windows", "macos", "android"];

const BENCH_PASS: &str = "release-bench-pass";

/// 5 目标夹具：每个目标一份合法 deb（data 内嵌 marker，改 marker 即换清单摘要）。
fn fixture_five_deb(tag: &str) -> PathBuf {
    let root = fixture(tag);
    let mut config = std::fs::read_to_string(root.join("polypack.toml")).expect("读配置");
    for target in FIVE_TARGETS {
        config.push_str(&format!("[targets.{target}]\nformat = \"deb\"\n"));
    }
    std::fs::write(root.join("polypack.toml"), config).expect("写配置");
    for target in FIVE_TARGETS {
        write_deb(&root, target, &format!("{target}-v1"));
    }
    root
}

/// 用 `poly_bundle::arwrite` 造合法 deb（不依赖 dpkg-deb；文件名与 control 同源）。
fn write_deb(root: &Path, target: &str, marker: &str) -> PathBuf {
    use poly_bundle::arwrite::{self, Member};
    let dir = root.join(".polypack").join("packages").join(target);
    std::fs::create_dir_all(&dir).expect("产物目录可建");
    let control = "Package: fixture-app\nVersion: 0.1.0\nArchitecture: amd64\n\
                   Maintainer: PolyPack <bench@polypack.invalid>\n\
                   Description: release changed benchmark\n";
    let control_tar = arwrite::tar_bytes(&[("./control".to_string(), control.as_bytes().to_vec())])
        .expect("control tar");
    let control_gz =
        arwrite::gzip_bytes(&control_tar, &dir.join("control.tar.gz")).expect("control gzip");
    let data_tar = arwrite::tar_bytes(&[(
        "./usr/share/fixture.txt".to_string(),
        marker.as_bytes().to_vec(),
    )])
    .expect("data tar");
    let data_gz = arwrite::gzip_bytes(&data_tar, &dir.join("data.tar.gz")).expect("data gzip");
    let path = dir.join("fixture-app_0.1.0_amd64.deb");
    arwrite::write_ar(
        &[
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
        ],
        &path,
    )
    .expect("写 deb");
    path
}

/// 一次性 GPG 私钥 → `env:` 凭据载荷（base64(JSON)）；缺 gpg 时跳过整条测试。
fn gpg_env_payload(root: &Path) -> Option<String> {
    if !poly_bundle::exec::tool_present("gpg") {
        eprintln!("SKIP：宿主没有 gpg，5 目标 --changed 真跑跳过");
        return None;
    }
    let home = root.join("gnupg");
    std::fs::create_dir_all(&home).expect("GNUPGHOME 可建");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700))
            .expect("GNUPGHOME 0700");
    }
    let gpg = |args: &[&str]| {
        Command::new("gpg")
            .args(["--batch", "--no-options", "--homedir"])
            .arg(&home)
            .args(args)
            .output()
            .expect("gpg 可执行")
    };
    let generated = gpg(&[
        "--pinentry-mode",
        "loopback",
        "--passphrase",
        BENCH_PASS,
        "--quick-generate-key",
        "PolyPack Release Bench <bench@polypack.invalid>",
        "ed25519",
        "sign",
        "0",
    ]);
    assert!(
        generated.status.success(),
        "gpg 生成密钥失败：{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let exported = gpg(&[
        "--pinentry-mode",
        "loopback",
        "--passphrase",
        BENCH_PASS,
        "--export-secret-keys",
    ]);
    assert!(
        exported.status.success(),
        "gpg 导出私钥失败：{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    assert!(!exported.stdout.is_empty(), "私钥导出非空");
    let _ = Command::new("gpgconf")
        .arg("--homedir")
        .arg(&home)
        .args(["--kill", "gpg-agent"])
        .output();
    let key = base64::engine::general_purpose::STANDARD.encode(&exported.stdout);
    let json = format!(r#"{{"keystore":"{key}","store_password":"{BENCH_PASS}"}}"#);
    Some(base64::engine::general_purpose::STANDARD.encode(json))
}

fn poly_with_env(cwd: &Path, args: &[&str], key: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .current_dir(cwd)
        .env("POLY_RELEASE_KEY", key)
        .args(args)
        .output()
        .expect("poly 可执行")
}

fn json_events(stdout: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|err| panic!("行不是合法 JSON：{line}（{err}）"))
        })
        .collect()
}

/// T-M9-09：5 目标首跑 5 次后端调用；改 1 个产物后 `--changed` 只发 1 个、
/// 跳过 4 个（回执零重写），后端调用 5 → 1（省 80%）。墙钟只打印不设阈值。
#[test]
fn five_target_changed_run_publishes_one_and_skips_four() {
    let root = fixture_five_deb("changed-five");
    let Some(payload) = gpg_env_payload(&root) else {
        let _ = std::fs::remove_dir_all(&root);
        return;
    };
    let target_list = FIVE_TARGETS.join(",");
    let first_start = Instant::now();
    let first = poly_with_env(
        &root,
        &[
            "release",
            "--target",
            &target_list,
            "--changed",
            "--credential",
            "env:POLY_RELEASE_KEY",
            "--json",
        ],
        &payload,
    );
    let first_ms = first_start.elapsed().as_millis();
    assert_eq!(
        first.status.code(),
        Some(0),
        "首跑须成功：{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_events = json_events(&first.stdout);
    assert_eq!(first_events.len(), 5, "每个目标一条 published 事件");
    let mut receipts = BTreeMap::new();
    for event in &first_events {
        assert_eq!(event["event"], "published");
        assert_eq!(event["cache_hit"].as_bool(), Some(false), "首跑须真发布");
        let target = event["target"].as_str().expect("目标").to_string();
        receipts.insert(
            target,
            PathBuf::from(event["receipt"].as_str().expect("回执路径")),
        );
    }
    assert_eq!(receipts.len(), 5);

    // 只改 android 的产物，其余 4 个目标的回执字节必须原样。
    write_deb(&root, "android", "android-v2");
    let untouched: Vec<(String, PathBuf, Vec<u8>)> = FIVE_TARGETS
        .iter()
        .filter(|target| **target != "android")
        .map(|target| {
            let path = receipts.get(*target).expect("首跑回执").clone();
            let bytes = std::fs::read(&path).expect("回执存在");
            ((*target).to_string(), path, bytes)
        })
        .collect();

    let second_start = Instant::now();
    let second = poly_with_env(
        &root,
        &[
            "release",
            "--target",
            &target_list,
            "--changed",
            "--credential",
            "env:POLY_RELEASE_KEY",
            "--json",
        ],
        &payload,
    );
    let second_ms = second_start.elapsed().as_millis();
    assert_eq!(
        second.status.code(),
        Some(0),
        "1 published + 4 skipped 须退出 0：{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let events = json_events(&second.stdout);
    assert_eq!(events.len(), 5);
    let mut published = Vec::new();
    let mut skipped = Vec::new();
    for event in &events {
        let target = event["target"].as_str().expect("目标").to_string();
        match event["event"].as_str() {
            Some("published") => published.push(target),
            Some("release-skipped") => {
                assert_eq!(event["reason"], "unchanged");
                assert_eq!(event["key_source"], "cache_key", "新回执走 cache_key 判定");
                skipped.push(target);
            }
            other => panic!("意外事件 {other:?}：{event}"),
        }
    }
    assert_eq!(published, ["android"], "只发布变更目标");
    assert_eq!(
        skipped,
        ["linux", "web", "windows", "macos"],
        "其余四个目标 skipped"
    );
    for (target, path, bytes) in &untouched {
        assert_eq!(
            &std::fs::read(path).expect("回执存在"),
            bytes,
            "{target} 的 skipped 回执不得重写"
        );
    }
    let published_event = events
        .iter()
        .find(|event| event["event"] == "published")
        .expect("published 事件");
    let new_receipt = PathBuf::from(published_event["receipt"].as_str().expect("回执路径"));
    assert_ne!(new_receipt, receipts["android"], "产物变化 → 新回执键");
    assert!(new_receipt.exists(), "新回执已归档");
    assert!(receipts["android"].exists(), "旧回执永久保留");

    // 后端调用 = published 事件数（--changed 命中 skipped 时不触后端）。
    let first_calls = first_events.len();
    let second_calls = published.len();
    let call_saving = (first_calls - second_calls) * 100 / first_calls;
    assert!(
        call_saving >= 50,
        "后端调用节省须 ≥ 50%（{first_calls} → {second_calls}）"
    );
    let wall_saving = (first_ms.saturating_sub(second_ms) * 100)
        .checked_div(first_ms)
        .unwrap_or(0);
    eprintln!(
        "T-M9-09 first_published={first_calls} second_published={second_calls} \
         second_skipped={} call_saving={call_saving}% \
         first_ms={first_ms} second_ms={second_ms} wall_saving={wall_saving}%",
        skipped.len()
    );
    let _ = std::fs::remove_dir_all(&root);
}
