//! T-M6-11 契约：车厂 / OEM 渠道占位 Publisher——明确错误码、零伪造提交、凭据只经引用。
//!
//! 车厂渠道无统一公开 API，占位客户端不得假装上传 / 提审：四步一律返回
//! `polypack::E_PUB_0006`（需 OEM 凭据引用），且不落回执。凭据只以 `CredentialRef` 形式
//! 进入 `Job`，错误文本不得出现凭据值。

mod common;

use polypack_core::{CredentialRef, CredentialSource};
use polypack_publish::{
    Archive, Channel, CredentialProvider, Job, MemoryProvider, OemPublisher, PollPolicy,
    PublishRequest, Publisher, codes, publish, receipt_key,
};

/// 凭据值哨兵：错误 / 回执里出现即泄漏。
const SECRET: &str = "oem-secret-never-surfaces";

fn reference() -> CredentialRef {
    CredentialRef {
        name: "oem-vendor-x".to_string(),
        source: CredentialSource::Env {
            variable: "POLYPACK_OEM_CRED".to_string(),
        },
    }
}

fn providers() -> Vec<Box<dyn CredentialProvider>> {
    vec![Box::new(MemoryProvider::with_entry(
        "oem-vendor-x",
        common::payload(&serde_json::json!({ "vendor_token": SECRET })),
    ))]
}

/// 四步全部拒绝（不伪造任一步），错误码稳定、文案点名「需 OEM 凭据引用」且不含凭据值。
#[test]
fn oem_placeholder_rejects_every_step_with_the_credential_requirement() {
    let root = common::TempRoot::new("oem-steps");
    let package = polypack_publish::Package::load(&root.dist()).expect("读包");
    let credential = reference();
    let providers = providers();
    let poll = PollPolicy::default();
    let job = Job {
        appid: "vendor-app",
        version: "1.0.0",
        description: "OEM 占位",
        materials: None,
        package: &package,
        credential: &credential,
        providers: &providers,
        poll: &poll,
    };

    let publisher = OemPublisher;
    assert_eq!(publisher.channel(), Channel::Oem);
    assert_eq!(publisher.channel().id(), "oem-channel");
    // 成功类型未实现 Debug，`expect_err` 不可用；归一为 `Result<(), _>` 后取值。
    let errors = [
        publisher
            .upload(&job)
            .map(|_| ())
            .expect_err("upload 应拒绝"),
        publisher.await_ready(&job).expect_err("await_ready 应拒绝"),
        publisher
            .submit(&job)
            .map(|_| ())
            .expect_err("submit 应拒绝"),
        publisher.poll(&job).map(|_| ()).expect_err("poll 应拒绝"),
    ];
    for err in errors {
        assert_eq!(err.code(), codes::E_PUB_0006, "{err}");
        let message = err.to_string();
        assert!(message.contains("需 OEM 凭据引用"), "{message}");
        assert!(
            message.contains("oem-vendor-x"),
            "错误须点名凭据引用：{message}"
        );
        assert!(!message.contains(SECRET), "凭据值不得进入错误：{message}");
        let hint = err.hint().expect("占位错误须给替代路径提示");
        assert!(hint.contains("CredentialRef"), "{hint}");
    }
}

/// 共享驱动走占位客户端：首步即失败，回执不落盘（没有「上传成功」的伪造状态）。
#[test]
fn oem_placeholder_writes_no_receipt_and_no_fake_submission() {
    let root = common::TempRoot::new("oem-driver");
    let archive = Archive::new(root.path());
    let providers = providers();
    let request = PublishRequest::new("vendor-app", "1.0.0", "OEM 占位", root.dist(), reference());

    let err =
        publish(&OemPublisher, &request, &providers, &archive).expect_err("占位客户端不得走通发布");
    assert_eq!(err.code(), codes::E_PUB_0006, "{err}");

    let package = polypack_publish::Package::load(&root.dist()).expect("读包");
    let receipt = archive.receipt_path(
        Channel::Oem.id(),
        &receipt_key("vendor-app", "1.0.0", package.digest()),
    );
    assert!(
        !receipt.exists(),
        "占位客户端不得写回执：{}",
        receipt.display()
    );
}

/// 渠道 id 与 polypack-targets `TargetSpec.publish` 同口径，可反解（CLI 派发面）。
#[test]
fn oem_channel_id_round_trips() {
    assert_eq!(Channel::Oem.id(), "oem-channel");
    assert_eq!(Channel::from_id("oem-channel"), Some(Channel::Oem));
}
