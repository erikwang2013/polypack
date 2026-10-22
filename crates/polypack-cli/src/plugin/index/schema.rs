//! ADR 0004 签名插件索引的文档模型、规范化与新鲜度 / 吊销规则。
//!
//! 规范化复用 `polypack-publish` 既有的确定性 canonical JSON（递归键排序、数组顺序保留、
//! 紧凑 UTF-8）；本 schema 的数值只有整数，字符串按 JSON 标准转义，因此对这里会
//! 出现的文档与 RFC 8785 JCS 输出一致。签名验证针对规范化字节，而不是文件排版。

use polypack_core::codes;
use polypack_core::diagnostics::Diagnostic;
use serde::{Deserialize, Serialize};

/// 索引 schema 版本。
pub const INDEX_SCHEMA: &str = "polypack-plugin-index/v1";
/// 吊销快照 schema 版本。
pub const REVOCATIONS_SCHEMA: &str = "polypack-plugin-revocations/v1";
/// 宿主当前支持的插件接口版本（复用宿主沙箱常量，避免漂移）。
pub const SUPPORTED_WIT: &str = polypack_plugin::TARGET_SPEC_VERSION;
/// 吊销快照最长未刷新时间（ADR 0004 §7：超过 30 天拒绝新安装）。
pub const MAX_REVOCATION_AGE_SECS: i64 = 30 * 24 * 60 * 60;

/// `index.json` 顶层结构。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexDocument {
    /// 固定 `polypack-plugin-index/v1`。
    pub schema: String,
    /// 单调递增序列。
    pub sequence: u64,
    /// 生成时刻（RFC 3339 UTC）。
    pub generated_at: String,
    /// 过期时刻（RFC 3339 UTC）。
    pub expires_at: String,
    /// 上一版索引规范化字节的 `blake3-<hex>`；首版为 null。
    pub previous_digest: Option<String>,
    /// 索引签名密钥指纹（40 / 64 hex）。
    pub signing_key_id: String,
    /// 插件条目。
    pub plugins: Vec<PluginEntry>,
    /// 配套吊销快照规范化字节的 `blake3-<hex>`。
    pub revocations_digest: String,
}

/// 插件条目（`plugins[]`）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginEntry {
    /// 稳定 id（小写反向域名）。
    pub id: String,
    /// 展示名。
    pub name: String,
    /// 展示摘要。
    pub summary: String,
    /// SemVer 2.0.0。
    pub version: String,
    /// 插件接口版本（当前只安装 `target-spec/v1`）。
    pub wit_version: String,
    /// 兼容的最低 PolyPack 版本。
    pub min_host_version: String,
    /// 发布时刻（RFC 3339 UTC）。
    pub published_at: String,
    /// 制品位置与 SRI。
    pub artifact: Artifact,
    /// 发布者身份。
    pub publisher: Publisher,
    /// 发布者对制品的分离签名位置。
    pub signature: SignatureRef,
    /// `active` / `yanked`。
    pub status: String,
    /// `status = yanked` 时必填。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yank_reason: Option<String>,
    /// 可选展示字段（固定 schema，未知键拒绝）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
}

/// 制品引用。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    /// `https://` 或 `file:`；URL 本身不参与信任判定。
    pub url: String,
    /// `blake3-<64 hex>`，与宿主加载同口径。
    pub sri: String,
    /// 字节数。
    pub size: u64,
}

/// 发布者身份。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publisher {
    /// 发布者 GPG 指纹（40 / 64 hex），不参与展示排序。
    pub key_id: String,
    /// 展示名，不参与安全判定。
    pub display_name: String,
}

/// 分离签名引用。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureRef {
    /// `https://` 或 `file:`（ASCII armor）。
    pub url: String,
}

/// 可选展示元数据。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    /// 主页。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// 许可证。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// 关键词。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
}

/// `revocations.json` 顶层结构。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevocationsDocument {
    /// 固定 `polypack-plugin-revocations/v1`。
    pub schema: String,
    /// 单调递增序列。
    pub sequence: u64,
    /// 签发时刻（RFC 3339 UTC）。
    pub issued_at: String,
    /// 只追加的吊销条目。
    pub revocations: Vec<Revocation>,
}

/// 单条吊销记录。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revocation {
    /// `version` / `plugin` / `key`。
    pub kind: String,
    /// 插件 id（`version` / `plugin`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 版本（仅 `version`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 发布者指纹（仅 `key`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// `compromised` / `malicious` / `withdrawn` / `superseded`。
    pub reason: String,
    /// 吊销时刻（RFC 3339 UTC）。
    pub revoked_at: String,
    /// 可选替代版本。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
}

/// 解析索引 JSON；未知字段拒绝。
pub fn parse_index(bytes: &[u8]) -> Result<IndexDocument, Diagnostic> {
    serde_json::from_slice(bytes).map_err(|err| {
        index_error(format!(
            "插件索引不是合法 schema（第 {} 行第 {} 列）",
            err.line(),
            err.column()
        ))
    })
}

/// 解析吊销快照 JSON；未知字段拒绝。
pub fn parse_revocations(bytes: &[u8]) -> Result<RevocationsDocument, Diagnostic> {
    serde_json::from_slice(bytes).map_err(|err| {
        index_error(format!(
            "吊销快照不是合法 schema（第 {} 行第 {} 列）",
            err.line(),
            err.column()
        ))
    })
}

/// 文档的规范化 UTF-8 字节（签名字节）。
pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, Diagnostic> {
    let value =
        serde_json::to_value(value).map_err(|_| index_error("索引文档无法序列化为 JSON"))?;
    let canonical = polypack_publish::metadata::canonical_json(&value);
    serde_json::to_vec(&canonical).map_err(|_| index_error("索引文档无法规范化"))
}

/// 索引 / 吊销文档的格式错误（E_PLUGIN_0011）。
pub fn index_error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(codes::E_PLUGIN_0011, message)
        .with_hint("核对签名索引与吊销快照的 schema、签名信任根与 sequence")
}
