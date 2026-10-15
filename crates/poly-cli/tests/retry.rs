//! T-M2-11 验收（CLI 侧）：`poly retry --from sign` 的产物定位、凭据引用与「独立重跑」。
//!
//! - 缺未签名 APK / 缺 apksigner 两条如实报告路径在三宿主都确定性触发（不依赖本机装了什么）；
//! - 真实签名链（aapt2 造未签名 APK + keytool 造 keystore + apksigner 签名）在本机工具齐备时
//!   真跑两轮（第二轮 = 独立重跑、重新拉取凭据），缺工具时按 skip-if-missing 明示跳过。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 测试用 keystore 口令（仅存在于测试进程与临时目录，绝不进仓库）。
const STORE_PASS: &str = "polypack-retry-test-pass";

/// 凭据注入变量名（`--credential env:…` 指向它）。
const CRED_VAR: &str = "POLYPACK_RETRY_TEST_CRED";

/// 文本清单（aapt2 link 会把它编译成二进制清单，apksigner 需要 minSdkVersion）。
const MANIFEST: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.polypack.retrytest">
  <uses-sdk android:minSdkVersion="24" android:targetSdkVersion="36"/>
</manifest>
"#;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-cli-retry-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可创建");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 最小工程配置。
fn fixture(root: &Path) -> PathBuf {
    let config = root.join("polypack.toml");
    std::fs::write(
        &config,
        "[package]\nname = \"Retry Fixture\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
    )
    .expect("写配置");
    config
}

/// 运行 `poly` 二进制（cargo 测试自动注入 `CARGO_BIN_EXE_poly`）。
fn poly(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("poly 可执行")
}

/// 按 JSON Lines 解析 stdout。
fn json_lines(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("stdout 每行应为合法 JSON"))
        .collect()
}

#[test]
fn retry_without_unsigned_apk_reports_build_hint() {
    let temp = TempDir::new("no-apk");
    fixture(temp.path());

    let out = poly(&["retry", "--from", "sign"], temp.path());
    assert_eq!(out.status.code(), Some(1), "无产物可重跑必须非零退出");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_RETRY_0001"), "{stderr}");
    assert!(
        stderr.contains("poly build --target android"),
        "提示应指向先产出未签名 APK 的那条路：{stderr}"
    );
    assert!(
        !temp.path().join(".polypack/repro").exists() && !temp.path().join("signed.apk").exists(),
        "找不到产物时不产任何假产物"
    );
}

#[test]
fn retry_sign_reports_missing_apksigner_before_touching_credentials() {
    let temp = TempDir::new("no-tool");
    fixture(temp.path());
    let apk = temp.path().join("app-unsigned.apk");
    std::fs::write(&apk, b"not-a-real-apk").expect("占位未签名 APK");

    // 显式把 apksigner 指到不存在的路径：本机装没装都走「缺工具」分支
    let out = Command::new(env!("CARGO_BIN_EXE_poly"))
        .args(["retry", "--from", "sign", "--input"])
        .arg(&apk)
        .arg("--json")
        .current_dir(temp.path())
        .env("POLYPACK_APKSIGNER", temp.path().join("no-such-apksigner"))
        .output()
        .expect("poly 可执行");
    assert_eq!(out.status.code(), Some(1), "缺工具必须非零退出");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("poly::E_SIG_0001"), "{stderr}");
    assert!(stderr.contains("POLYPACK_APKSIGNER"), "{stderr}");
    let lines = json_lines(&out);
    assert!(
        lines.iter().all(|line| line["event"] != "retry-signed"),
        "缺工具时不得产出 retry-signed 事件：{lines:?}"
    );
}

/// 本机可用的真实签名工具链（缺任一项即跳过真跑）。
struct Tools {
    aapt2: PathBuf,
    apksigner: PathBuf,
    android_jar: PathBuf,
    keytool: PathBuf,
}

/// PATH 上的候选可执行文件（Windows 上 JDK / SDK 给的是 `.bat`）。
fn on_path(stem: &str) -> Option<PathBuf> {
    let names = if cfg!(windows) {
        vec![
            format!("{stem}.bat"),
            format!("{stem}.exe"),
            format!("{stem}.cmd"),
            stem.to_string(),
        ]
    } else {
        vec![stem.to_string()]
    };
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// Android SDK 根候选（与 poly-sign 的解析顺序同口径）。
fn sdk_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for key in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(value) = std::env::var_os(key) {
            roots.push(PathBuf::from(value));
        }
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        roots.push(PathBuf::from(home).join("Android").join("Sdk"));
    }
    roots.push(PathBuf::from("/home/component/Android/sdk"));
    roots
}

/// `build-tools/<版本>/<stem>`（版本降序取第一个存在的）。
fn in_build_tools(stem: &str) -> Option<PathBuf> {
    for root in sdk_roots() {
        let Ok(entries) = std::fs::read_dir(root.join("build-tools")) else {
            continue;
        };
        let mut versions: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        versions.sort_by_key(|path| std::cmp::Reverse(version_key(path)));
        for dir in versions {
            let candidate = dir.join(stem);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// build-tools 目录名的数值版本键（`36.1.0` → `[36, 1, 0]`）。
fn version_key(dir: &Path) -> Vec<u64> {
    dir.file_name()
        .map(|name| {
            name.to_string_lossy()
                .split('.')
                .map(|part| part.parse().unwrap_or(0))
                .collect()
        })
        .unwrap_or_default()
}

/// 最高版本 `platforms/android-*/android.jar`。
fn android_jar() -> Option<PathBuf> {
    let mut best: Option<(Vec<u64>, PathBuf)> = None;
    for root in sdk_roots() {
        let Ok(entries) = std::fs::read_dir(root.join("platforms")) else {
            continue;
        };
        for dir in entries.flatten().map(|entry| entry.path()) {
            let jar = dir.join("android.jar");
            if !jar.is_file() {
                continue;
            }
            let key: Vec<u64> = dir
                .file_name()
                .map(|name| {
                    name.to_string_lossy()
                        .rsplit('-')
                        .next()
                        .unwrap_or_default()
                        .parse::<u64>()
                        .map(|n| vec![n])
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            if best.as_ref().is_none_or(|(seen, _)| key > *seen) {
                best = Some((key, jar));
            }
        }
    }
    best.map(|(_, jar)| jar)
}

/// 发现完整工具链；缺任一项返回 `None`。
fn discover() -> Option<Tools> {
    Some(Tools {
        aapt2: on_path("aapt2").or_else(|| in_build_tools("aapt2"))?,
        apksigner: on_path("apksigner").or_else(|| in_build_tools("apksigner"))?,
        android_jar: android_jar()?,
        keytool: on_path("keytool")?,
    })
}

/// 标准 base64（RFC 4648 §4，含填充）：测试内自备，不给 CLI 引依赖。
fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[test]
fn retry_sign_really_signs_and_reruns_independently() {
    let Some(tools) = discover() else {
        eprintln!("SKIP：缺 aapt2 / apksigner / keytool / android.jar，真实签名链无从跑");
        return;
    };
    let temp = TempDir::new("real-sign");
    fixture(temp.path());

    // 1) 未签名 APK：文本清单经 aapt2 link 出真实（带二进制清单的）未签名件
    let manifest = temp.path().join("AndroidManifest.xml");
    std::fs::write(&manifest, MANIFEST).expect("写清单");
    let apk = temp.path().join("app-unsigned.apk");
    let status = Command::new(&tools.aapt2)
        .arg("link")
        .arg("-o")
        .arg(&apk)
        .arg("--manifest")
        .arg(&manifest)
        .arg("-I")
        .arg(&tools.android_jar)
        .status()
        .expect("aapt2 可执行");
    assert!(status.success(), "aapt2 link 应成功");

    // 2) keystore → base64(JSON) 载荷：凭据值只经环境变量注入，不写盘、不进日志
    let keystore = temp.path().join("ks.p12");
    let status = Command::new(&tools.keytool)
        .args(["-genkeypair", "-keystore"])
        .arg(&keystore)
        .args([
            "-storetype",
            "PKCS12",
            "-alias",
            "pax",
            "-storepass",
            STORE_PASS,
            "-keypass",
            STORE_PASS,
            "-dname",
            "CN=PolyPack Retry Fixture, O=PolyPack, C=CN",
            "-keyalg",
            "RSA",
            "-validity",
            "1",
        ])
        .status()
        .expect("keytool 可执行");
    assert!(status.success(), "keytool genkeypair 应成功");
    let payload = base64(
        format!(
            "{{\"keystore\":\"{}\",\"store_password\":\"{STORE_PASS}\",\"key_alias\":\"pax\",\"key_password\":\"{STORE_PASS}\"}}",
            base64(&std::fs::read(&keystore).expect("读 keystore"))
        )
        .as_bytes(),
    );

    // 3) 两轮同一命令 = 独立重跑（每轮重新拉取凭据）；第二轮覆写同一产物
    let signed = temp.path().join("signed.apk");
    for round in 1..=2 {
        let out = Command::new(env!("CARGO_BIN_EXE_poly"))
            .args(["retry", "--from", "sign", "--input"])
            .arg(&apk)
            .arg("--credential")
            .arg(format!("env:{CRED_VAR}"))
            .arg("--out")
            .arg(&signed)
            .arg("--json")
            .current_dir(temp.path())
            .env(CRED_VAR, &payload)
            .env("POLYPACK_APKSIGNER", &tools.apksigner)
            .output()
            .expect("poly 可执行");
        assert_eq!(
            out.status.code(),
            Some(0),
            "第 {round} 轮签名应成功：stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            signed.is_file() && signed.metadata().expect("stat").len() > 0,
            "第 {round} 轮应产出非空签名件"
        );

        let lines = json_lines(&out);
        let signed_event = lines
            .iter()
            .find(|line| line["event"] == "retry-signed")
            .unwrap_or_else(|| panic!("第 {round} 轮缺 retry-signed 事件：{lines:?}"));
        assert_eq!(signed_event["rerun"], true, "输出须如实标注是重跑");
        assert_eq!(signed_event["target"], "android");
        assert!(
            signed_event["evidence"]
                .as_str()
                .is_some_and(|evidence| evidence.contains("Verifies")),
            "apksigner verify 证据须进事件流：{signed_event}"
        );
        assert!(
            lines.iter().any(|line| line["event"] == "signed"),
            "签名完成事件须进事件流：{lines:?}"
        );

        // 脱敏断言：凭据值（载荷与口令）不得出现在任何输出通道
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        for secret in [payload.as_str(), STORE_PASS] {
            assert!(
                !stdout.contains(secret) && !stderr.contains(secret),
                "第 {round} 轮输出泄露了凭据值"
            );
        }
    }
}
