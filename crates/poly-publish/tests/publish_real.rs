//! T-M3-09 / T-M3-10 真平台腿：真账号上传 → 提审 → 轮询（自动化止步提审，R4）。
//!
//! 本仓库**没有**真平台测试账号 / 凭据（无可用凭据）→ 未设开关即打印 SKIP 并以通过结束
//! （绝不伪绿）；mock 契约测试覆盖同一契约的 HTTP 形态（微信 tests/wechat.rs，口径取自
//! miniprogram-ci 2.1.31；支付宝 tests/alipay.rs；抖音 tests/douyin.rs）。
//!
//! 开关按渠道分立，**同一时刻只开一个渠道开关**（三个腿读同一组 `POLY_PUBLISH_*` 环境变量，
//! cargo test 并行执行——同时开多个渠道开关会把同一份配置喂给多个平台）：
//! - 微信：`POLY_PUBLISH_REAL=1`；支付宝：`POLY_PUBLISH_REAL_ALIPAY=1`；抖音：`POLY_PUBLISH_REAL_DOUYIN=1`。
//!
//! 真跑配置（开了开关却缺配置 = 配置错误，直接失败，不静默跳过）：
//! - `POLY_PUBLISH_APPID` / `POLY_PUBLISH_VERSION` / `POLY_PUBLISH_PACKAGE`（T-M3-06 产物 dist 目录）；
//! - `POLY_PUBLISH_CRED`：环境变量名，其值为 `base64(JSON)` 凭据载荷（引用模型注入；
//!   微信 {appid, secret, private_key} / 支付宝 {app_id, private_key[, app_auth_token]} /
//!   抖音 {access_token}——token 外部注入，客户端不做铸造）；
//! - `POLY_PUBLISH_MATERIALS`：材料 JSON 路径（微信 = `item_list` 数组；支付宝 = template_id +
//!   提审文本字段，文件类字段未实现勿带；抖音 = template_id；类目 / 模板须已在平台后台配置）；
//! - 可选：`POLY_PUBLISH_POLL_INTERVAL_SECS`（默认 30）、`POLY_PUBLISH_POLL_TIMEOUT_SECS`（默认 120）、
//!   `POLY_PUBLISH_ARCHIVE_ROOT`（回执归档根，默认系统临时目录下 `poly-publish-real`）。
//!
//! 各渠道真腿的未确证边界（如实注记）：支付宝包体不上传（版本创建走平台模板通道，
//! PACKAGE 仅用于回执键摘要；minidev 私有协议备选不做）、响应签未校验；
//! 抖音 token 铸造链路未实测。

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
    run_real(Channel::Wechat, "微信真上传 + 提审腿");
}

#[test]
fn real_alipay_walk() {
    if std::env::var("POLY_PUBLISH_REAL_ALIPAY").as_deref() != Ok("1") {
        eprintln!(
            "SKIP: 未设 POLY_PUBLISH_REAL_ALIPAY=1——支付宝真腿需要开放平台应用（app_id + RSA2 \
             应用私钥 [+ app_auth_token]）与模板（materials {{\"template_id\":…}}）；\n\
             未确证边界（如实）：①包体不上传——版本创建走平台模板通道，PACKAGE 仅用于回执键摘要\
             （minidev 私有协议备选不做——不造未验证的第二条路）；\n\
             ②响应签未校验（凭据契约未含支付宝公钥）；③文件类材料（app_logo / 截图等）未实现，\
             materials 带了即明拒；\n\
             mock 契约测试见 tests/alipay.rs（网关封套 / RSA2 签名独立复算 / 平铺提审字段 / \
             build.query 构建等待）"
        );
        return;
    }
    run_real(
        Channel::Alipay,
        "支付宝真腿：版本创建 → 云构建等待 → 提审（materials 勿含文件类字段）",
    );
}

#[test]
fn real_douyin_walk() {
    if std::env::var("POLY_PUBLISH_REAL_DOUYIN").as_deref() != Ok("1") {
        eprintln!(
            "SKIP: 未设 POLY_PUBLISH_REAL_DOUYIN=1——抖音真腿需要开放平台 access_token（外部注入：\n\
             凭据载荷 {{\"access_token\":\"…\"}}，token 铸造链路文档口径互斥且未实测——客户端不做铸造）\n\
             与模板（materials {{\"template_id\":N}}）；mock 契约测试见 tests/douyin.rs（err_no 信封 / \
             access-token 头 / get_audit_hosts 前置 / data.audit.status 枚举 / 11303 幂等）"
        );
        return;
    }
    run_real(Channel::Douyin, "抖音真腿：测试版本创建 → 提审 → 轮询");
}

/// 共用的真跑走查体：同组 env 配置 + 指定渠道客户端，跑 上传 → 提审 → 轮询。
fn run_real(channel: Channel, note: &str) {
    let appid = require_env("POLY_PUBLISH_APPID");
    let version = require_env("POLY_PUBLISH_VERSION");
    let package = PathBuf::from(require_env("POLY_PUBLISH_PACKAGE"));
    let cred_variable = require_env("POLY_PUBLISH_CRED");
    let materials_path = require_env("POLY_PUBLISH_MATERIALS");
    let materials: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&materials_path).expect("读提审材料 JSON"))
            .expect("提审材料须是合法 JSON（平台口径见文件头）");

    let credential = CredentialRef {
        name: format!("{appid}-publish"),
        source: CredentialSource::Env {
            variable: cred_variable,
        },
    };
    let providers: Vec<Box<dyn CredentialProvider>> = vec![Box::new(EnvProvider)];
    let request = PublishRequest::new(
        appid,
        version,
        // 描述同时充当提审版本描述（支付宝 version_desc 要求 30–500 字，故写足）。
        format!(
            "poly-publish 真实走查：上传 + 提审链路验证（unix {}）——自动化止步提审，审核结果以平台为准",
            epoch_secs()
        ),
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
        .find(|publisher| publisher.channel() == channel)
        .expect("渠道客户端在内置清单里");
    let archive_root = std::env::var("POLY_PUBLISH_ARCHIVE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("poly-publish-real"));
    let archive = Archive::new(&archive_root);

    match publish(publisher.as_ref(), &request, &providers, &archive) {
        Ok(outcome) => println!(
            "{note}：走到终态 {}（回执 {}；runs {}）",
            outcome.receipt.state,
            outcome.path.display(),
            outcome.receipt.runs
        ),
        // 上传 + 提审已受理、轮询（或构建等待）超预算：R4 口径下这是「跑通」的正常收尾——
        // 审核结果以平台为准，回执已带状态流水落盘，重跑只续轮询 / 只重等构建。
        Err(PublishError::PollTimeout { last, .. }) => println!(
            "{note}：上传 + 提审已受理；等待超预算（最后状态 {last}；归档 {}）——R4：审核结果以平台为准",
            archive.root().display()
        ),
        Err(err) => panic!("{note}真实发布失败：{err}（code {}）", err.code()),
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
