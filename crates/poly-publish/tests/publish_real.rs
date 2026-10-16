//! T-M3-09 真平台腿（`POLY_PUBLISH_REAL=1`）：真账号上传 → 提审 → 轮询（自动化止步提审，R4）。
//!
//! 本仓库**没有**真平台测试账号 / 代码上传密钥（无可用凭据）→ 未设开关即打印 SKIP 并以通过
//! 结束（绝不伪绿）；mock 契约测试（tests/wechat.rs）覆盖同一契约的 HTTP 形态，口径取自
//! miniprogram-ci 2.1.31（上传通道无公开文档）。
//!
//! 真跑配置（开了开关却缺配置 = 配置错误，直接失败，不静默跳过）：
//! - `POLY_PUBLISH_APPID` / `POLY_PUBLISH_VERSION` / `POLY_PUBLISH_PACKAGE`（T-M3-06 产物 dist 目录）；
//! - `POLY_PUBLISH_CRED`：环境变量名，其值为 `base64(JSON)` 凭据载荷（引用模型注入）；
//! - `POLY_PUBLISH_MATERIALS`：提审材料 JSON 路径（微信 = `item_list` 数组；类目须已在账号后台配置）；
//! - 可选：`POLY_PUBLISH_POLL_INTERVAL_SECS`（默认 30）、`POLY_PUBLISH_POLL_TIMEOUT_SECS`（默认 120）、
//!   `POLY_PUBLISH_ARCHIVE_ROOT`（回执归档根，默认系统临时目录下 `poly-publish-real`）。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use poly_core::{CredentialRef, CredentialSource};
use poly_publish::{
    Archive, Channel, CredentialProvider, EnvProvider, PollPolicy, PublishError, PublishRequest,
    Transport, UreqTransport, builtin, publish,
};

#[test]
fn real_upload_and_submit_walk() {
    if std::env::var("POLY_PUBLISH_REAL").as_deref() != Ok("1") {
        eprintln!(
            "SKIP: 未设 POLY_PUBLISH_REAL=1——真上传 + 提审腿需要微信测试账号（appid + AppSecret \
             + 代码上传密钥）与已配类目；mock 契约测试见 tests/wechat.rs（口径取自 miniprogram-ci \
             2.1.31：B 协议容器 + raw PKCS#1 v1.5 签名 + GET get_latest_auditstatus 轮询）"
        );
        return;
    }

    let appid = require_env("POLY_PUBLISH_APPID");
    let version = require_env("POLY_PUBLISH_VERSION");
    let package = PathBuf::from(require_env("POLY_PUBLISH_PACKAGE"));
    let cred_variable = require_env("POLY_PUBLISH_CRED");
    let materials_path = require_env("POLY_PUBLISH_MATERIALS");
    let materials: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&materials_path).expect("读提审材料 JSON"))
            .expect("提审材料须是合法 JSON（微信 = item_list 数组）");

    let credential = CredentialRef {
        name: format!("{appid}-code-upload"),
        source: CredentialSource::Env {
            variable: cred_variable,
        },
    };
    let providers: Vec<Box<dyn CredentialProvider>> = vec![Box::new(EnvProvider)];
    let request = PublishRequest::new(
        appid,
        version,
        format!("poly-publish 真实走查（unix {}）", epoch_secs()),
        package,
        credential,
    )
    .with_materials(materials)
    .with_poll(PollPolicy {
        interval: secs_from_env("POLY_PUBLISH_POLL_INTERVAL_SECS", 30),
        timeout: secs_from_env("POLY_PUBLISH_POLL_TIMEOUT_SECS", 120),
    });

    let transport: Arc<dyn Transport> =
        Arc::new(UreqTransport::new().expect("装配传输层（系统信任库）"));
    let publisher = builtin(transport)
        .into_iter()
        .find(|publisher| publisher.channel() == Channel::Wechat)
        .expect("微信客户端在内置清单里");
    let archive_root = std::env::var("POLY_PUBLISH_ARCHIVE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("poly-publish-real"));
    let archive = Archive::new(&archive_root);

    match publish(publisher.as_ref(), &request, &providers, &archive) {
        Ok(outcome) => println!(
            "真实发布走到终态 {}（回执 {}；runs {}）",
            outcome.receipt.state,
            outcome.path.display(),
            outcome.receipt.runs
        ),
        // 上传 + 提审已受理、轮询超预算：R4 口径下这是「跑通」的正常收尾——
        // 审核结果以平台为准，回执已带状态流水落盘，重跑只续轮询。
        Err(PublishError::PollTimeout { last, .. }) => println!(
            "上传 + 提审已受理；轮询超预算（最后状态 {last}；归档 {}）——R4：审核结果以平台为准",
            archive.root().display()
        ),
        Err(err) => panic!("真实发布失败：{err}（code {}）", err.code()),
    }
}

/// 必填环境变量（缺一即失败）。
fn require_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("POLY_PUBLISH_REAL=1 需要环境变量 {name}（缺配置即失败，不静默跳过）")
    })
}

/// 秒数环境变量（缺省值兜底）。
fn secs_from_env(name: &str, default: u64) -> Duration {
    Duration::from_secs(
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(default),
    )
}

/// 当前 unix 秒（版本描述里缀上，便于平台后台辨认这一次走查）。
fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}
