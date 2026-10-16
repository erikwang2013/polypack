//! 零分叉硬约束（ROADMAP §4.1）：三平台共用同一 [`Publisher`] 接口 + 共享驱动，
//! 平台差异收敛在客户端实现内。
//!
//! 手法：测试内实现一个**纯内存 Publisher 替身**（不走 HTTP、不含任何平台名词），让同一段
//! 驱动代码把它按三个渠道各走一遍——四步调用序与回执落点只随 `channel()` 变化，
//! 核心不认识任何平台。真客户端（`builtin`）只做清单不变量断言；T-M3-10 补齐三平台后
//! 本文件的渠道表自然扩展为端到端矩阵。

mod common;

use std::sync::Mutex;

use common::TempRoot;
use poly_core::{CredentialRef, CredentialSource};
use poly_publish::{
    Archive, AuditState, Channel, CredentialProvider, Job, PollOutcome, PollPolicy, PublishError,
    PublishRequest, Publisher, Submission, Uploaded, builtin, publish,
};

/// 三渠道（渠道 id 与 poly-targets `TargetSpec.publish` registry 名同口径）。
const CHANNELS: [Channel; 3] = [Channel::Wechat, Channel::Alipay, Channel::Douyin];

/// 记录调用序的 Publisher 替身（零平台名词、零网络）。
struct LoopPublisher {
    /// 自报渠道（唯一的分叉点）。
    channel: Channel,
    /// 四步调用序。
    calls: Mutex<Vec<&'static str>>,
}

impl LoopPublisher {
    fn new(channel: Channel) -> Self {
        Self {
            channel,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().expect("调用序锁").clone()
    }

    fn record(&self, step: &'static str) {
        self.calls.lock().expect("调用序锁").push(step);
    }
}

impl Publisher for LoopPublisher {
    fn channel(&self) -> Channel {
        self.channel
    }

    fn upload(&self, _job: &Job<'_>) -> Result<Uploaded, PublishError> {
        self.record("upload");
        Ok(Uploaded {
            raw: serde_json::json!({"ok": true}),
        })
    }

    fn await_ready(&self, _job: &Job<'_>) -> Result<(), PublishError> {
        self.record("await_ready");
        Ok(())
    }

    fn submit(&self, _job: &Job<'_>) -> Result<Submission, PublishError> {
        self.record("submit");
        Ok(Submission {
            raw: serde_json::json!({"ok": true}),
        })
    }

    fn poll(&self, _job: &Job<'_>) -> Result<PollOutcome, PublishError> {
        self.record("poll");
        Ok(PollOutcome {
            state: AuditState::Approved,
            reason: None,
            raw: serde_json::json!({"status": 0}),
        })
    }
}

/// 同一段驱动代码走完三渠道：四步调用序一致、回执只按 `channel()` 落位。
#[test]
fn one_driver_walks_every_channel_with_identical_call_sequence() {
    for channel in CHANNELS {
        let root = TempRoot::new(&format!("zero-fork-{}", channel.id()));
        let publisher = LoopPublisher::new(channel);
        // 替身不解凭据：给空链 + 任意引用，驱动原样传进 Job（凭据解析是客户端内部事）。
        let reference = CredentialRef {
            name: "loop".to_string(),
            source: CredentialSource::Keyring,
        };
        let providers: Vec<Box<dyn CredentialProvider>> = Vec::new();
        let request = PublishRequest::new("app-1", "1.0.0", "零分叉", root.dist(), reference)
            .with_poll(PollPolicy {
                interval: std::time::Duration::ZERO,
                timeout: std::time::Duration::from_secs(1),
            });
        let archive = Archive::new(root.path());

        let outcome =
            publish(&publisher, &request, &providers, &archive).expect("替身在共享驱动上走通");

        assert_eq!(
            publisher.calls(),
            ["upload", "await_ready", "submit", "poll"],
            "{channel} 的四步调用序固定（驱动不认识平台）"
        );
        assert_eq!(outcome.resumed_from, None);
        assert_eq!(
            outcome.receipt.channel,
            channel.id(),
            "回执渠道取自 channel()"
        );
        assert_eq!(outcome.receipt.state, AuditState::Approved);
        assert_eq!(
            outcome.path,
            archive.receipt_path(
                channel.id(),
                &poly_publish::receipt_key(
                    "app-1",
                    "1.0.0",
                    poly_publish::Package::load(&root.dist())
                        .expect("读包")
                        .digest(),
                )
            ),
            "回执只随 channel() 落位：<root>/.polypack/publish/<channel>/…"
        );
    }
}

/// 渠道 id 是不变量：唯一、可反解、与 registry 名同口径。
#[test]
fn channel_ids_are_unique_and_round_trip() {
    let fake = common::FakeTransport::new(Vec::new());
    let publishers = builtin(fake);
    let ids: Vec<&str> = publishers.iter().map(|p| p.channel().id()).collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "内置清单渠道不重复：{ids:?}");
    for id in &ids {
        assert_eq!(
            Channel::from_id(id).map(|channel| channel.id()),
            Some(*id),
            "渠道 id 可反解：{id}"
        );
    }
    assert!(
        ids.contains(&Channel::Wechat.id()),
        "微信客户端在内置清单内：{ids:?}"
    );
    assert_eq!(Channel::Wechat.id(), "wechat-open-platform");
    assert_eq!(Channel::Alipay.id(), "alipay-open-platform");
    assert_eq!(Channel::Douyin.id(), "douyin-open-platform");
    assert_eq!(Channel::from_id("nope"), None, "陌生渠道 id 反解失败");
}
