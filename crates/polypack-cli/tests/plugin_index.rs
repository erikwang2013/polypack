//! T-M9-41 验收：ADR 0004 签名插件索引。
//!
//! 覆盖 `polypack plugin add` / `search` 的离线验签链：索引签名、过期、sequence 回滚 /
//! 镜像分叉、版本 / 发布者密钥吊销、制品 SRI、发布者指纹，以及错误脱敏。
//! 密钥与 GNUPGHOME 都是 `/tmp` 下的一次性短路径（gpg-agent unix socket 限制）；
//! 宿主没有可用 gpg 时打印 `SKIP` 并跳过（与仓库既有真跑契约测试同口径）。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

mod common;

use common::{Key, TempDir, has_gpg, keypair, private_dir, sign_file, trust_payload};

const INDEX_TRUST_ENV: &str = "POLYPACK_TEST_INDEX_TRUST";
const REVOCATIONS_TRUST_ENV: &str = "POLYPACK_TEST_REVOCATIONS_TRUST";
const PUBLISHER_KEY_ENV: &str = "POLYPACK_TEST_PUBLISHER_KEY";
const PLUGIN_ID: &str = "dev.example.pocket-notes";
const VERSION: &str = "1.2.3";
const FAR_FUTURE: &str = "2099-01-01T00:00:00Z";
const EXPIRED: &str = "2000-01-01T00:00:00Z";

// ---------------------------------------------------------------- 测试支架

fn canonical_bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(&polypack_publish::metadata::canonical_json(value)).expect("canonical JSON")
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("父目录可建");
    }
    std::fs::write(path, bytes).expect("测试文件可写");
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("测试文件可读")
}

// ---------------------------------------------------------------- 夹具

struct Fixture {
    root: TempDir,
    index_dir: PathBuf,
    state: PathBuf,
    index_key: Key,
    revocation_key: Key,
    publisher_key: Key,
}

impl Fixture {
    fn new(tag: &str) -> Option<Self> {
        // Windows 的 MSYS gpg 不认 Windows 绝对路径参数；与既有插件签名真跑同口径。
        if cfg!(windows) || !has_gpg() {
            eprintln!("SKIP：宿主没有可用的 gpg（Linux/macOS 会走真实验签分支）");
            return None;
        }
        let root = TempDir::new(tag);
        let index_key = keypair(root.path(), "index");
        let revocation_key = keypair(root.path(), "revocations");
        let publisher_key = keypair(root.path(), "publisher");
        let index_dir = root.path().join("mirror");
        let state = root.path().join("state");
        private_dir(&index_dir);
        Some(Self {
            root,
            index_dir,
            state,
            index_key,
            revocation_key,
            publisher_key,
        })
    }

    fn artifact_bytes(&self) -> Vec<u8> {
        b"\0asm fake plugin artifact for index tests".to_vec()
    }

    fn entry(&self, sri: &str, size: u64) -> Value {
        json!({
            "id": PLUGIN_ID,
            "name": "Pocket Notes",
            "summary": "Offline notes with a signed index",
            "version": VERSION,
            "wit_version": "target-spec/v1",
            "min_host_version": "1.0.0",
            "published_at": "2026-10-20T00:00:00Z",
            "artifact": {
                "url": "file:pocket-notes.wasm",
                "sri": sri,
                "size": size,
            },
            "publisher": {
                "key_id": self.publisher_key.fingerprint.clone(),
                "display_name": "Example Publisher",
            },
            "signature": {"url": "file:pocket-notes.wasm.asc"},
            "status": "active",
        })
    }

    /// 写入制品 + 发布者签名 + 吊销快照 + 索引及双方分离签名。
    fn write_bundle(&self, mut index: Value, revocations: Value) -> Value {
        let artifact = self.artifact_bytes();
        write(&self.index_dir.join("pocket-notes.wasm"), &artifact);
        sign_file(
            &self.publisher_key,
            &self.index_dir.join("pocket-notes.wasm"),
            &self.index_dir.join("pocket-notes.wasm.asc"),
        );

        let revocations_bytes = canonical_bytes(&revocations);
        write(&self.index_dir.join("revocations.json"), &revocations_bytes);
        sign_file(
            &self.revocation_key,
            &self.index_dir.join("revocations.json"),
            &self.index_dir.join("revocations.json.asc"),
        );

        index["revocations_digest"] =
            Value::String(polypack_plugin::content_digest(&revocations_bytes));
        index["signing_key_id"] = Value::String(self.index_key.fingerprint.clone());
        let index_bytes = canonical_bytes(&index);
        write(&self.index_dir.join("index.json"), &index_bytes);
        sign_file(
            &self.index_key,
            &self.index_dir.join("index.json"),
            &self.index_dir.join("index.json.asc"),
        );
        index
    }

    fn default_index(&self, sequence: u64, expires: &str, revocations_digest: &str) -> Value {
        let artifact = self.artifact_bytes();
        json!({
            "schema": "polypack-plugin-index/v1",
            "sequence": sequence,
            "generated_at": "2026-10-20T00:00:00Z",
            "expires_at": expires,
            "previous_digest": null,
            "signing_key_id": self.index_key.fingerprint.clone(),
            "plugins": [self.entry(
                &polypack_plugin::content_digest(&artifact),
                artifact.len() as u64,
            )],
            "revocations_digest": revocations_digest,
        })
    }

    fn empty_revocations(&self, sequence: u64) -> Value {
        json!({
            "schema": "polypack-plugin-revocations/v1",
            "sequence": sequence,
            "issued_at": FAR_FUTURE,
            "revocations": [],
        })
    }

    /// 正常索引 + 空吊销快照；返回已签名的索引值（供后续篡改 / 复用）。
    fn standard_bundle(&self, sequence: u64, expires: &str) -> Value {
        let revocations = self.empty_revocations(1);
        // revocations_digest 由 write_bundle 回填。
        let index = self.default_index(sequence, expires, "");
        self.write_bundle(index, revocations)
    }

    fn envs(&self) -> Vec<(&'static str, String)> {
        vec![
            (INDEX_TRUST_ENV, trust_payload(&self.index_key.public)),
            (
                REVOCATIONS_TRUST_ENV,
                trust_payload(&self.revocation_key.public),
            ),
            (PUBLISHER_KEY_ENV, trust_payload(&self.publisher_key.public)),
        ]
    }
}

// ---------------------------------------------------------------- CLI

fn base_command(fx: &Fixture, envs: &[(&'static str, String)]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_polypack"));
    command
        .current_dir(fx.root.path())
        .arg("--json")
        .env_remove(INDEX_TRUST_ENV)
        .env_remove(REVOCATIONS_TRUST_ENV)
        .env_remove(PUBLISHER_KEY_ENV);
    for (key, value) in envs {
        command.env(key, value);
    }
    command
}

fn add(fx: &Fixture, spec: &str, envs: &[(&'static str, String)]) -> Output {
    let mut command = base_command(fx, envs);
    command
        .args(["plugin", "add", spec])
        .arg("--index")
        .arg(&fx.index_dir)
        .arg("--trust-root")
        .arg(format!("env:{INDEX_TRUST_ENV}"))
        .arg("--revocations-trust-root")
        .arg(format!("env:{REVOCATIONS_TRUST_ENV}"))
        .arg("--publisher-key")
        .arg(format!("env:{PUBLISHER_KEY_ENV}"))
        .arg("--state")
        .arg(&fx.state);
    command.output().expect("polypack 可执行")
}

fn search(fx: &Fixture, query: &str, envs: &[(&'static str, String)]) -> Output {
    let mut command = base_command(fx, envs);
    command
        .args(["plugin", "search", query])
        .arg("--index")
        .arg(&fx.index_dir)
        .arg("--trust-root")
        .arg(format!("env:{INDEX_TRUST_ENV}"))
        .arg("--revocations-trust-root")
        .arg(format!("env:{REVOCATIONS_TRUST_ENV}"))
        .arg("--state")
        .arg(&fx.state);
    command.output().expect("polypack 可执行")
}

fn combined(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn event(output: &Output, name: &str) -> Value {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value["event"] == name)
        .unwrap_or_else(|| panic!("缺少 {name} 事件：{}", combined(output)))
}

fn assert_code(output: &Output, code: &str) {
    let text = combined(output);
    assert!(text.contains(code), "期望错误码 {code}，实际：{text}");
}

fn installed_wasm(fx: &Fixture) -> PathBuf {
    fx.state
        .join("plugins")
        .join(PLUGIN_ID)
        .join(VERSION)
        .join("plugin.wasm")
}

// ---------------------------------------------------------------- 契约

#[test]
fn add_and_search_offline_signed_index() {
    let Some(fx) = Fixture::new("happy") else {
        return;
    };
    let index = fx.standard_bundle(1, FAR_FUTURE);
    let envs = fx.envs();

    let added = add(&fx, &format!("{PLUGIN_ID}@{VERSION}"), &envs);
    assert!(added.status.success(), "add 应通过：{}", combined(&added));
    let add_event = event(&added, "plugin_add");
    assert_eq!(add_event["id"], PLUGIN_ID);
    assert_eq!(add_event["version"], VERSION);
    assert_eq!(add_event["sri"], index["plugins"][0]["artifact"]["sri"]);
    let wasm = installed_wasm(&fx);
    assert_eq!(read(&wasm), fx.artifact_bytes(), "安装制品字节必须一致");
    assert!(
        fx.state.join("state.json").is_file(),
        "安装后必须记录 sequence 基线"
    );
    assert!(
        wasm.parent()
            .expect("安装目录")
            .join("entry.json")
            .is_file(),
        "已验证条目必须落盘"
    );

    let found = search(&fx, "notes", &envs);
    assert!(
        found.status.success(),
        "search 应通过：{}",
        combined(&found)
    );
    let search_event = event(&found, "plugin_search");
    assert_eq!(search_event["results"][0]["id"], PLUGIN_ID);
    assert_eq!(search_event["results"][0]["version"], VERSION);
    assert_eq!(
        search_event["results"][0]["publisher"],
        fx.publisher_key.fingerprint
    );
}

#[test]
fn tampered_index_signature_is_rejected_and_does_not_leak_trust_material() {
    let Some(fx) = Fixture::new("tampered") else {
        return;
    };
    let mut index = fx.standard_bundle(1, FAR_FUTURE);
    index["plugins"][0]["summary"] = Value::String("tampered after signing".to_string());
    write(&fx.index_dir.join("index.json"), &canonical_bytes(&index));

    let envs = fx.envs();
    let rejected = add(&fx, PLUGIN_ID, &envs);
    assert!(!rejected.status.success(), "篡改索引必须拒绝");
    assert_code(&rejected, "polypack::E_PLUGIN_0011");
    assert!(!installed_wasm(&fx).exists(), "验签失败不得落盘");

    let text = combined(&rejected);
    for (_, payload) in &envs {
        assert!(
            !text.contains(payload),
            "错误输出不得回显信任根载荷：{text}"
        );
    }
    assert!(
        !text.contains("BEGIN PGP PUBLIC KEY BLOCK"),
        "错误输出不得回显公钥：{text}"
    );
    assert!(
        !text.contains("polypack-verify-"),
        "错误输出不得泄漏临时 GNUPGHOME：{text}"
    );
}

#[test]
fn expired_index_is_rejected() {
    let Some(fx) = Fixture::new("expired") else {
        return;
    };
    fx.standard_bundle(1, EXPIRED);
    let rejected = add(&fx, PLUGIN_ID, &fx.envs());
    assert!(!rejected.status.success(), "过期索引必须拒绝");
    assert_code(&rejected, "polypack::E_PLUGIN_0012");
    assert!(!installed_wasm(&fx).exists());
}

#[test]
fn sequence_rollback_and_same_sequence_fork_are_rejected() {
    let Some(fx) = Fixture::new("sequence") else {
        return;
    };
    let envs = fx.envs();
    fx.standard_bundle(2, FAR_FUTURE);
    let accepted = add(&fx, PLUGIN_ID, &envs);
    assert!(
        accepted.status.success(),
        "首个索引应通过：{}",
        combined(&accepted)
    );

    // 同 sequence、不同内容 = 镜像分叉。
    let mut fork = fx.default_index(2, FAR_FUTURE, "");
    fork["plugins"][0]["summary"] = Value::String("forked view".to_string());
    fx.write_bundle(fork, fx.empty_revocations(1));
    let forked = add(&fx, PLUGIN_ID, &envs);
    assert!(!forked.status.success(), "同 sequence 分叉必须拒绝");
    assert_code(&forked, "polypack::E_PLUGIN_0012");

    // sequence 低于本地已见最高值 = 回滚。
    fx.standard_bundle(1, FAR_FUTURE);
    let rolled_back = add(&fx, PLUGIN_ID, &envs);
    assert!(!rolled_back.status.success(), "sequence 回滚必须拒绝");
    assert_code(&rolled_back, "polypack::E_PLUGIN_0011");

    let state: Value =
        serde_json::from_slice(&read(&fx.state.join("state.json"))).expect("状态可读");
    assert_eq!(state["index_sequence"], 2, "失败不得推进回滚基线");
}

#[test]
fn search_advances_rollback_baseline() {
    let Some(fx) = Fixture::new("search-baseline") else {
        return;
    };
    let envs = fx.envs();
    fx.standard_bundle(2, FAR_FUTURE);
    let found = search(&fx, "", &envs);
    assert!(
        found.status.success(),
        "search 应通过：{}",
        combined(&found)
    );

    fx.standard_bundle(1, FAR_FUTURE);
    let rolled_back = add(&fx, PLUGIN_ID, &envs);
    assert!(!rolled_back.status.success(), "search 后旧索引仍必须拒绝");
    assert_code(&rolled_back, "polypack::E_PLUGIN_0011");
}

#[test]
fn revoked_version_is_rejected() {
    let Some(fx) = Fixture::new("revoked-version") else {
        return;
    };
    let revocations = json!({
        "schema": "polypack-plugin-revocations/v1",
        "sequence": 1,
        "issued_at": FAR_FUTURE,
        "revocations": [{
            "kind": "version",
            "id": PLUGIN_ID,
            "version": VERSION,
            "reason": "malicious",
            "revoked_at": "2026-10-20T00:00:00Z",
        }],
    });
    let index = fx.default_index(1, FAR_FUTURE, "");
    fx.write_bundle(index, revocations);

    let rejected = add(&fx, &format!("{PLUGIN_ID}@{VERSION}"), &fx.envs());
    assert!(!rejected.status.success(), "已吊销版本必须拒绝");
    assert_code(&rejected, "polypack::E_PLUGIN_0013");
    assert!(!installed_wasm(&fx).exists());
}

#[test]
fn revoked_publisher_key_is_rejected() {
    let Some(fx) = Fixture::new("revoked-key") else {
        return;
    };
    let revocations = json!({
        "schema": "polypack-plugin-revocations/v1",
        "sequence": 1,
        "issued_at": FAR_FUTURE,
        "revocations": [{
            "kind": "key",
            "fingerprint": fx.publisher_key.fingerprint,
            "reason": "compromised",
            "revoked_at": "2026-10-20T00:00:00Z",
        }],
    });
    let index = fx.default_index(1, FAR_FUTURE, "");
    fx.write_bundle(index, revocations);

    let rejected = add(&fx, PLUGIN_ID, &fx.envs());
    assert!(!rejected.status.success(), "已吊销发布者密钥必须拒绝");
    assert_code(&rejected, "polypack::E_PLUGIN_0013");
    assert!(!installed_wasm(&fx).exists());
}

#[test]
fn sri_mismatch_is_rejected() {
    let Some(fx) = Fixture::new("sri") else {
        return;
    };
    let artifact = fx.artifact_bytes();
    let wrong = polypack_plugin::content_digest(b"different artifact bytes");
    let index = fx.default_index(1, FAR_FUTURE, "");
    let mut index = index;
    index["plugins"][0]["artifact"]["sri"] = Value::String(wrong);
    index["plugins"][0]["artifact"]["size"] = Value::from(artifact.len() as u64);
    fx.write_bundle(index, fx.empty_revocations(1));

    let rejected = add(&fx, PLUGIN_ID, &fx.envs());
    assert!(!rejected.status.success(), "SRI 不符必须拒绝");
    assert_code(&rejected, "polypack::E_PLUGIN_0006");
    assert!(!installed_wasm(&fx).exists());
}

#[test]
fn search_without_matches_succeeds_with_empty_result() {
    let Some(fx) = Fixture::new("search-empty") else {
        return;
    };
    fx.standard_bundle(1, FAR_FUTURE);
    let found = search(&fx, "no-such-plugin", &fx.envs());
    assert!(
        found.status.success(),
        "无匹配不是错误：{}",
        combined(&found)
    );
    let event = event(&found, "plugin_search");
    assert_eq!(event["results"].as_array().expect("结果数组").len(), 0);
}
