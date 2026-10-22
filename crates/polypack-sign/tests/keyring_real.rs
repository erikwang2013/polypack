//! T-M2-11 真环境闭环：真 Secret Service 总线上的**真写 → 真读 → 真删**。
//!
//! 门控：`POLYPACK_KEYRING_REAL=1` **且**总线上有 `org.freedesktop.secrets` 才真跑；缺则
//! eprintln SKIP 并以通过结束（CI 三宿主无会话总线属预期形态，同 `POLYPACK_VAULT_REAL` 口径）。
//!
//! headless 真跑配方（实测可用；隔离库目录 = 不碰本机真实钥匙串）：
//! ```text
//! mkdir -p -m 700 /tmp/polypack-keyring-rt /tmp/polypack-keyring-home/keyrings
//! dbus-run-session -- bash -c '
//!   export HOME=/tmp/polypack-keyring-home XDG_RUNTIME_DIR=/tmp/polypack-keyring-rt \
//!          XDG_DATA_HOME=/tmp/polypack-keyring-home/.local/share \
//!          GNOME_KEYRING_CUSTOM_LOCAL_PATH=/tmp/polypack-keyring-home/keyrings
//!   printf "polypack-it-pass\n" | gnome-keyring-daemon --unlock --components=secrets --foreground &
//!   sleep 3; POLYPACK_KEYRING_REAL=1 cargo test -p polypack-sign --test keyring_real -- --nocapture'
//! ```
//! 三个卡点（都实测踩过）：
//! 1. `--foreground` 必加——否则 daemon 静默后台化，`org.freedesktop.secrets` 不上总线；
//! 2. 密码必须**非空**（空密码不被接受，daemon 报 `failed to unlock login keyring on startup`
//!    并转弹 prompt）；
//! 3. 库目录必须指到空目录——本机 gnome-keyring 是 **Deepin 打补丁版**，密钥库目录被硬覆盖为
//!    `~/.local/share/deepin-keyrings-wb`（无视 `HOME` / `XDG_DATA_HOME`），里面已有一个密码
//!    未知的 `login.keyring` → 解锁失败后写条目会弹 `gcr-prompter`，headless 下直接挂死。
//!    `GNOME_KEYRING_CUSTOM_LOCAL_PATH` 是该补丁的覆盖入口；指向空目录后 `--unlock` 会用给定
//!    密码**真创建** `login.keyring`（上游 gnome-keyring 无此变量，靠 `HOME` 隔离即可）。
//!
//! 未按配方隔离就真跑，条目会落在**当前用户的真实钥匙串**里（跑完会自动删掉，但仍应避免）。
//!
//! 角色分工：写 / 删走 keyring crate（dev-dependency，与 [`KeyringBackend`] 同一 crate
//! 同版本——后端本体是只读的，没有写 / 删 API），读走生产读路径 [`KeyringBackend::fetch`]。
//!
//! **独立性**：条目在不在另用 `gdbus` 直查总线协议层 `SearchItems` 交叉确认（不经过本
//! crate 读路径）；gdbus 缺失则该子检查 SKIP。
//!
//! 平台门控：真跑形态（headless dbus + secret-service）只在 Linux；其他平台整文件不编译
//! ——不假装在 macOS / Windows 上跑过（那两处需各自的真钥匙串环境）。

#![cfg(target_os = "linux")]

mod common;

use polypack_sign::secret::{KEYRING_ID, KeyringBackend, SecretBackend};

/// 真跑用的条目名（= `CredentialRef.name` = 条目的 `username` 属性）。
const NAME: &str = "polypack-it-keyring";
/// 载荷原文里的密钥库字节（写入 → 读回的比对基准）。
const KEYSTORE: &[u8] = b"pkcs12-bytes-of-keyring-it";

/// 门控：`true` 才真跑。
fn real_env() -> bool {
    if std::env::var("POLYPACK_KEYRING_REAL").ok().as_deref() != Some("1") {
        eprintln!(
            "SKIP：真环境 keyring 闭环需 POLYPACK_KEYRING_REAL=1\
             （headless 起 `dbus-run-session` + gnome-keyring，配方见本文件头）"
        );
        return false;
    }
    if std::env::var("DBUS_SESSION_BUS_ADDRESS").is_err() {
        eprintln!(
            "SKIP：没有会话总线（DBUS_SESSION_BUS_ADDRESS 未设置）——用 dbus-run-session 包一层"
        );
        return false;
    }
    // 运行记录：隔离线索进输出（--nocapture 下可见），便于事后核对跑的是哪个库。
    eprintln!(
        "真跑：HOME={} GNOME_KEYRING_CUSTOM_LOCAL_PATH={}",
        std::env::var("HOME").unwrap_or_else(|_| "<未设置>".into()),
        std::env::var("GNOME_KEYRING_CUSTOM_LOCAL_PATH")
            .unwrap_or_else(|_| "<未设置（非 Deepin 补丁版可忽略）>".into())
    );
    true
}

/// 总线协议层直查：`SearchItems({"service": "polypack", "username": <name>})` 命中条数。
///
/// `Ok(None)` = 本机没有 `gdbus`（该子检查 SKIP）；`Err` = 真查失败（测试应炸）。
fn bus_item_count(name: &str) -> Result<Option<usize>, String> {
    let output = std::process::Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.secrets",
            "--object-path",
            "/org/freedesktop/secrets",
            "--method",
            "org.freedesktop.Secret.Service.SearchItems",
        ])
        .arg(format!(
            "{{\"service\": \"polypack\", \"username\": \"{name}\"}}"
        ))
        .output();
    let output = match output {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("gdbus 起不来：{err}")),
    };
    if !output.status.success() {
        return Err(format!(
            "SearchItems 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    // 形如 `(@ao [objectpath '/org/freedesktop/secrets/collection/login/1'], @ao [])`：
    // 数 `objectpath` 出现次数即命中条数（空结果 `(@ao [], @ao [])` 为 0）。
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(Some(stdout.matches("objectpath").count()))
}

/// 总线层交叉确认（gdbus 缺失则 SKIP 该子检查）。
fn assert_bus_count(label: &str, expected: usize) {
    match bus_item_count(NAME) {
        Ok(Some(count)) => {
            eprintln!("总线层交叉确认（{label}）：命中 {count} 条（期望 {expected}）");
            assert_eq!(count, expected, "总线层命中条数（{label}）");
        }
        Ok(None) => eprintln!("SKIP：本机无 gdbus，跳过总线层交叉确认（{label}）"),
        Err(reason) => panic!("总线层交叉确认（{label}）失败：{reason}"),
    }
}

/// 真写 → 真读 → 真删一整轮（顺序有依赖，单测试串行跑）。
#[test]
fn real_keyring_write_read_delete_round_trip() {
    if !real_env() {
        return;
    }

    let entry = keyring::Entry::new(KeyringBackend::SERVICE, NAME).expect("条目句柄");
    // 幂等前置：上一轮残留先清掉（无条目时删除报 NoEntry，忽略）。
    let _ = entry.delete_credential();

    let backend = KeyringBackend::new();
    assert_eq!(backend.id(), KEYRING_ID);
    backend.available().expect("真守护进程在总线上即应可用");
    assert_bus_count("写入前", 0);

    // 真写：载荷契约 = base64(JSON) 原文（与 Vault / Env 后端同一形态）。
    let payload = common::payload(
        KEYSTORE,
        common::PASS,
        Some(common::ALIAS),
        Some(common::PASS),
    );
    let text = String::from_utf8(payload.clone()).expect("载荷是 base64 文本");
    entry.set_password(&text).expect("真写进 Secret Service");
    assert_bus_count("写入后", 1);

    // 真读：走生产读路径（KeyringBackend::fetch）。
    let fetched = backend.fetch(NAME).expect("读回成功");
    eprintln!("读回 {} 字节（写入 {} 字节）", fetched.len(), payload.len());
    assert_eq!(fetched, payload, "密钥库往返逐字节一致");

    // 真删：删完总线层必须查不到。
    entry.delete_credential().expect("真删");
    assert_bus_count("删除后", 0);

    // 删后读：明确报「无该条目」（不是泛化的后端不可用）。
    let err = backend.fetch(NAME).expect_err("删掉后必读不到");
    assert!(
        err.to_string().contains("无该条目"),
        "删后应是缺条目而非后端故障：{err}"
    );
}
