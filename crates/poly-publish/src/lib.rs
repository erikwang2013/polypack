//! PolyPack 商店与平台发布客户端（T-M3-09 / T-M3-10）：代码包上传 → 提审 → 审核状态轮询
//! + 回执归档。
//!
//! **自动化边界（ROADMAP R4）**：止步「提审」——轮询只记录平台审核结果，绝不自动发布；
//! 回执永久保留（[`archive`]）。
//!
//! **零分叉**：三平台共用同一 [`Publisher`] 接口 + 共享驱动 [`flow::publish`]；平台差异
//! （端点 / 签名 / 容器格式 / 状态码语义 / 构建等待）全部收敛在客户端实现内
//! （[`wechat`] / [`alipay`] / [`douyin`]；快应用联盟商店 T-M3-12 复用本骨架）。
//! 驱动只认 [`Job`] 与四步（upload / await_ready / submit / poll），不认识任何平台名词。
//!
//! **凭据**：只给引用（[`poly_core::CredentialRef`]），值经 [`CredentialProvider`] 后端链
//! 现取（[`MemoryProvider`] / [`EnvProvider`]，keyring / Vault 由 CLI 桥接——语义同
//! T-M2-11）；仓库与回执零明文，错误与日志里的平台文本先过 [`Redactor`]。
//!
//! 上手（真实模式；测试走 mock 传输层，见 `tests/`）：
//!
//! ```no_run
//! use std::sync::Arc;
//! use poly_core::{CredentialRef, CredentialSource};
//! use poly_publish::{
//!     builtin, publish, Archive, Channel, MemoryProvider, PublishRequest,
//! };
//!
//! # fn main() -> Result<(), poly_publish::PublishError> {
//! let reference = CredentialRef {
//!     name: "wechat-mini".to_string(),
//!     source: CredentialSource::Env { variable: "POLY_WECHAT_CRED".to_string() },
//! };
//! // 载荷 = base64(JSON)；env 不可用时给内存后端注入（CI 场景）。
//! let providers: Vec<Box<dyn poly_publish::CredentialProvider>> =
//!     vec![Box::new(MemoryProvider::new())];
//! let request = PublishRequest::new("wx123", "1.0.0", "首个版本", "dist", reference);
//! let archive = Archive::new(".");
//! // 真传输：`Arc::new(UreqTransport::new()?)`；测试注入 FakeTransport。
//! # let transport: Arc<dyn poly_publish::Transport> = unreachable!();
//! for publisher in builtin(transport) {
//!     if publisher.channel() == Channel::Wechat {
//!         let outcome = publish(publisher.as_ref(), &request, &providers, &archive)?;
//!         println!("{} → {}", outcome.path.display(), outcome.receipt.state);
//!     }
//! }
//! # Ok(()) }
//! ```

pub mod alipay;
pub mod archive;
pub mod douyin;
pub mod error;
pub mod flow;
pub mod oem;
pub mod retry;
pub mod secret;
pub mod transport;
pub mod wechat;

mod package;
mod time;

use poly_core::CredentialRef;

pub use alipay::AlipayPublisher;
pub use archive::{
    Archive, AuditState, PollEntry, Receipt, SCHEMA_VERSION, SUBMIT_SCHEMA_VERSION, SubmitJournal,
    SubmitStatus, receipt_key, submission_key,
};
pub use douyin::DouyinPublisher;
pub use error::{PublishError, codes};
pub use flow::{PollPolicy, PublishOutcome, PublishRequest, publish, publish_checked};
pub use oem::OemPublisher;
pub use package::Package;
pub use retry::{PublishFailure, RetryPolicy, Stage};
pub use secret::{CredentialProvider, EnvProvider, MemoryProvider, Redactor};
pub use transport::{HttpRequest, HttpResponse, Method, Transport, UreqTransport};
pub use wechat::WechatPublisher;

/// 发布渠道（与 poly-targets `TargetSpec.publish` registry 名对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// 微信开放平台（小程序）。
    Wechat,
    /// 支付宝开放平台（小程序）。
    Alipay,
    /// 抖音开放平台（小程序）。
    Douyin,
    /// 快应用联盟商店（T-M3-12 复用本骨架的提审脚本；客户端 impl 由 packagers 侧补）。
    QuickApp,
    /// 车厂 / OEM 渠道（T-M6-11 占位：无统一公开 API，需 OEM 凭据引用）。
    Oem,
}

impl Channel {
    /// 渠道 id（回执目录名 / 事件流 / registry 三处同口径）。
    pub fn id(&self) -> &'static str {
        match self {
            Self::Wechat => "wechat-open-platform",
            Self::Alipay => "alipay-open-platform",
            Self::Douyin => "douyin-open-platform",
            Self::QuickApp => "quickapp-union-store",
            Self::Oem => "oem-channel",
        }
    }

    /// 由渠道 id 反解（CLI 派发用）。
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "wechat-open-platform" => Some(Self::Wechat),
            "alipay-open-platform" => Some(Self::Alipay),
            "douyin-open-platform" => Some(Self::Douyin),
            "quickapp-union-store" => Some(Self::QuickApp),
            "oem-channel" => Some(Self::Oem),
            _ => None,
        }
    }
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// 单次发布任务的共享上下文（驱动组装一次，各步同用；借出而非拥有）。
///
/// 凭据经 [`Job::credential`] + [`Job::providers`] 按引用现取（各平台客户端自解析）；
/// 提审材料语义由平台解释（[`Job::materials`]）。
pub struct Job<'a> {
    /// 平台应用 id。
    pub appid: &'a str,
    /// 版本号。
    pub version: &'a str,
    /// 版本描述。
    pub description: &'a str,
    /// 提审材料（平台各自解释）。
    pub materials: Option<&'a serde_json::Value>,
    /// 代码包（容器无关文件清单 + 内容摘要）。
    pub package: &'a Package,
    /// 凭据引用。
    pub credential: &'a CredentialRef,
    /// 凭据后端链。
    pub providers: &'a [Box<dyn CredentialProvider>],
    /// 平台侧等待 / 轮询预算（审核轮询与构建等待共用；来自 [`PublishRequest::poll`]）。
    pub poll: &'a PollPolicy,
}

/// 上传结果。
pub struct Uploaded {
    /// 平台响应快照（已脱敏；原样进回执）。
    pub raw: serde_json::Value,
}

/// 提审结果。
pub struct Submission {
    /// 平台响应快照（已脱敏；原样进回执）。
    pub raw: serde_json::Value,
}

/// 一次轮询结果。
pub struct PollOutcome {
    /// 规范化状态。
    pub state: AuditState,
    /// 平台自述原因（拒绝原因等；已脱敏）。
    pub reason: Option<String>,
    /// 平台原始状态片段（已脱敏；原样进回执）。
    pub raw: serde_json::Value,
}

/// 平台发布客户端（ROADMAP §4.1）：共享驱动只依赖本接口，平台差异收敛在实现内。
///
/// 实现约定：
/// - 四步自身幂等可重入（[`Publisher::await_ready`] 默认空实现）；不改本地状态，
///   快照经返回值交给驱动落回执；
/// - `raw` 快照必须已脱敏（经本 crate 的 [`Redactor`]）——它们会落盘进回执；
/// - 凭据经 [`Job`] 的引用 + 后端链现取，绝不写入错误文本。
pub trait Publisher: Send + Sync {
    /// 渠道。
    fn channel(&self) -> Channel;

    /// 上传代码包（容器由实现决定）。
    fn upload(&self, job: &Job<'_>) -> Result<Uploaded, PublishError>;

    /// 上传后平台侧就绪等待（默认无此阶段；异步构建类平台可复写）。
    fn await_ready(&self, job: &Job<'_>) -> Result<(), PublishError> {
        let _ = job;
        Ok(())
    }

    /// 提审（自动化边界：此后只轮询，不发布）。
    fn submit(&self, job: &Job<'_>) -> Result<Submission, PublishError>;

    /// 单次审核状态查询。
    fn poll(&self, job: &Job<'_>) -> Result<PollOutcome, PublishError>;
}

/// 内置客户端清单（传输层共享）。CLI 按 `channel()` 派发；T-M3-10 起含三平台，
/// T-M6-11 起含车厂 / OEM 占位客户端（`oem-channel`，不触网）。
pub fn builtin(transport: std::sync::Arc<dyn Transport>) -> Vec<Box<dyn Publisher>> {
    vec![
        Box::new(WechatPublisher::new(transport.clone())),
        Box::new(AlipayPublisher::new(transport.clone())),
        Box::new(DouyinPublisher::new(transport)),
        Box::new(OemPublisher),
    ]
}
