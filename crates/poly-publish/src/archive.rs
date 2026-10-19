//! 回执归档（T-M3-09）：发布档案 = 断点续走依据 + 审计证据。
//!
//! 落点固定 `<root>/.polypack/publish/<channel>/<key>.json`；R4 / ROADMAP §5.5：回执永久
//! 保留（不清理），schema 版本固定（格式变更加版本号，读侧拒绝陌生版本）。
//! 键 = appid + version + 摘要前 16 hex——同键重跑要么 no-op（终态），要么断点续走
//! （`uploaded` → 只提审；`submitted` / `in_review` / `delayed` → 只轮询）。
//!
//! shortcut: 同键并发双跑按单写者假设（后写赢）——回执是证据不是锁，发布按单脚本串行跑。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::PublishError;
use crate::time::unix_now;

/// 回执 schema 版本（格式变更即升；读侧拒绝非本版本）。v2 = T-M8-11 提交号 / 时间戳。
pub const SCHEMA_VERSION: u32 = 2;

/// 规范化审核状态（平台原始响应原样存 `raw`，不丢信息）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditState {
    /// 代码包已上传，尚未提审（续走起点：提审）。
    Uploaded,
    /// 提审已受理（续走起点：轮询）。
    Submitted,
    /// 审核中。
    InReview,
    /// 审核通过（终态）。
    Approved,
    /// 审核被拒（终态；原因见 `PollEntry.reason`）。
    Rejected,
    /// 已撤回（终态）。
    Withdrawn,
    /// 审核延后（非终态：继续轮询）。
    Delayed,
    /// 已发布（终态；支付宝 RELEASE / GRAY 映射到此）。
    Released,
    /// 平台状态无法识别（非终态：继续轮询到预算耗尽，原始值在 `raw` 里）。
    Unknown,
}

impl AuditState {
    /// 终态（轮询可停 / 重跑 no-op）。
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Approved | Self::Rejected | Self::Withdrawn | Self::Released
        )
    }

    /// 稳定文本（回执 / 错误 / 事件流统一口径）。
    pub fn label(&self) -> &'static str {
        match self {
            Self::Uploaded => "uploaded",
            Self::Submitted => "submitted",
            Self::InReview => "in_review",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Withdrawn => "withdrawn",
            Self::Delayed => "delayed",
            Self::Released => "released",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for AuditState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// 一次轮询的流水（状态时间线；终态原因随行）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollEntry {
    /// 查询时刻（unix 秒）。
    pub at_unix: u64,
    /// 规范化状态。
    pub state: AuditState,
    /// 平台自述原因（拒绝原因等；已脱敏）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// 平台原始状态片段（已脱敏；不识别状态时是唯一线索）。
    pub raw: serde_json::Value,
}

/// 发布回执（一个「渠道 + appid + 版本 + 内容摘要」一份）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    /// schema 版本（= [`SCHEMA_VERSION`]）。
    pub schema_version: u32,
    /// 渠道 id（`wechat-open-platform` 等）。
    pub channel: String,
    /// 平台应用 id。
    pub appid: String,
    /// 用户提交的版本号（平台侧唯一性由平台保证）。
    pub version: String,
    /// 代码包摘要（容器无关口径；hex）。
    pub digest: String,
    /// 提审材料摘要（`metadata::materials_digest` 口径：canonical JSON 的 blake3；旧回执 /
    /// 无材料时缺省）。可选字段——不升 schema 版本，旧回执照常读取。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materials_digest: Option<String>,
    /// 已走到 / 停在的阶段。
    pub state: AuditState,
    /// 本键累计执行次数（首跑 1；重跑 / no-op 也 +1——如实记录被跑过几次）。
    pub runs: u64,
    /// 首次执行时刻（unix 秒）。
    pub first_unix: u64,
    /// 最近一次执行时刻（unix 秒）。
    pub last_unix: u64,
    /// 平台提交号（提审受理回执号；平台未给则缺省，原始响应仍在 `submit` / `polls`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission_id: Option<String>,
    /// 提审受理时刻（unix 秒；与 `SubmitJournal.submitted_unix` 同源，崩溃续跑可复原）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_unix: Option<u64>,
    /// 终态判定时刻（unix 秒；approved / rejected / withdrawn / released 落盘时）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_unix: Option<u64>,
    /// 上传响应快照（各平台客户端自述形态；已脱敏）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload: Option<serde_json::Value>,
    /// 提审响应快照（同上）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submit: Option<serde_json::Value>,
    /// 轮询流水（按时间追加）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub polls: Vec<PollEntry>,
}

impl Receipt {
    /// 新建（首跑；state 从 [`AuditState::Uploaded`] 起，上传成功后首次落盘）。
    pub fn new(channel: &str, appid: &str, version: &str, digest: &str) -> Self {
        let now = unix_now();
        Self {
            schema_version: SCHEMA_VERSION,
            channel: channel.to_string(),
            appid: appid.to_string(),
            version: version.to_string(),
            digest: digest.to_string(),
            materials_digest: None,
            state: AuditState::Uploaded,
            runs: 1,
            first_unix: now,
            last_unix: now,
            submission_id: None,
            submitted_unix: None,
            decided_unix: None,
            upload: None,
            submit: None,
            polls: Vec::new(),
        }
    }
}

/// 提审去重状态 schema 版本（T-M8-10 落盘；v2 = T-M8-11 扩展提交号 / 受理时刻）。
pub const SUBMIT_SCHEMA_VERSION: u32 = 2;

/// 提审去重状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmitStatus {
    /// 已记录提交意图，尚无受理结果（重跑同键重发，平台 / 网关按幂等键去重）。
    Pending,
    /// 平台已受理（崩溃窗口后重跑不再提审，直接续轮询）。
    Accepted,
    /// 结果未知（超时 / 响应丢失）：重跑同键重发，平台原生去重码兜底。
    Unknown,
}

/// 提审幂等状态（`<root>/<channel>/<回执键>.submit.json`）：**发送提审前**写 `Pending`，
/// 受理后改 `Accepted`——「平台已受理、回执未落盘」间崩溃，重跑看到 `Accepted` 即跳过提审
/// （重复提审不产生二次提交的本地锚点；平台侧另以幂等键 + 原生去重码兜底）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitJournal {
    /// schema 版本（= [`SUBMIT_SCHEMA_VERSION`]）。
    pub schema_version: u32,
    /// 幂等键（提审请求头 `Idempotency-Key`；同键重发由平台 / 网关去重）。
    pub key: String,
    /// 去重状态。
    pub status: SubmitStatus,
    /// 到达提审阶段的执行次数（首跑 1；重跑也如实计数）。
    pub attempts: u64,
    /// 首次执行时刻（unix 秒）。
    pub first_unix: u64,
    /// 最近一次执行时刻（unix 秒）。
    pub last_unix: u64,
    /// 平台提交号（受理回执里的提审单号；平台未给则缺省，受理快照仍进回执 `submit`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission_id: Option<String>,
    /// 平台受理时刻（unix 秒；`Accepted` 时写入，崩溃续跑据此复原回执时间线）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submitted_unix: Option<u64>,
}

impl SubmitJournal {
    /// 新建（`Pending`；`attempts` 由驱动在落盘前自增）。
    pub fn new(key: &str) -> Self {
        let now = unix_now();
        Self {
            schema_version: SUBMIT_SCHEMA_VERSION,
            key: key.to_string(),
            status: SubmitStatus::Pending,
            attempts: 0,
            first_unix: now,
            last_unix: now,
            submission_id: None,
            submitted_unix: None,
        }
    }
}

/// 提审幂等键：渠道 + appid + 版本 + 内容摘要的 blake3 hex（跨进程稳定）。
pub fn submission_key(channel: &str, appid: &str, version: &str, digest: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in [channel, appid, version, digest] {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    hasher.finalize().to_hex().to_string()
}

/// 回执归档目录（`<root>/.polypack/publish`）。
pub struct Archive {
    /// 归档根。
    root: PathBuf,
}

impl Archive {
    /// `project_root` = 项目根（回执落 `<project_root>/.polypack/publish/…`）。
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            root: project_root.into().join(".polypack").join("publish"),
        }
    }

    /// 归档根目录（CLI 打印用）。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 回执路径（`<root>/<channel>/<key>.json`；键由 [`receipt_key`] 生成）。
    pub fn receipt_path(&self, channel: &str, key: &str) -> PathBuf {
        self.root.join(channel).join(format!("{key}.json"))
    }

    /// 读回执：不存在 → `Ok(None)`；存在但形态非法 / 版本陌生 → 明确报错（不静默重建）。
    pub fn load(&self, channel: &str, key: &str) -> Result<Option<Receipt>, PublishError> {
        let path = self.receipt_path(channel, key);
        let Some(bytes) = read_optional(&path)? else {
            return Ok(None);
        };
        let receipt: Receipt =
            serde_json::from_slice(&bytes).map_err(|err| PublishError::Archive {
                path: path.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("回执 JSON 非法：{err}"),
                ),
            })?;
        if receipt.schema_version != SCHEMA_VERSION {
            return Err(PublishError::Archive {
                path,
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "回执 schema 版本 {} 与本期 {} 不符（格式固定；迁移后重跑）",
                        receipt.schema_version, SCHEMA_VERSION
                    ),
                ),
            });
        }
        Ok(Some(receipt))
    }

    /// 原子落盘（同目录临时文件 + rename；返回回执路径）。
    pub fn save(&self, receipt: &Receipt) -> Result<PathBuf, PublishError> {
        let key = receipt_key(&receipt.appid, &receipt.version, &receipt.digest);
        let path = self.receipt_path(&receipt.channel, &key);
        write_atomic(
            &path,
            &serde_json::to_vec_pretty(receipt).expect("回执序列化不会失败"),
        )?;
        Ok(path)
    }

    /// 提审去重状态路径（`<root>/<channel>/<key>.submit.json`；T-M8-11 回执归档的接口）。
    pub fn submit_path(&self, channel: &str, key: &str) -> PathBuf {
        self.root.join(channel).join(format!("{key}.submit.json"))
    }

    /// 读提审去重状态：不存在 → `Ok(None)`；形态 / 版本非法 → 明确报错（不静默重建）。
    pub fn load_submit(
        &self,
        channel: &str,
        key: &str,
    ) -> Result<Option<SubmitJournal>, PublishError> {
        let path = self.submit_path(channel, key);
        let Some(bytes) = read_optional(&path)? else {
            return Ok(None);
        };
        let journal: SubmitJournal =
            serde_json::from_slice(&bytes).map_err(|err| PublishError::Archive {
                path: path.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("提审去重状态 JSON 非法：{err}"),
                ),
            })?;
        if journal.schema_version != SUBMIT_SCHEMA_VERSION {
            return Err(PublishError::Archive {
                path,
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "提审去重状态 schema 版本 {} 与本期 {} 不符（格式固定；迁移后重跑）",
                        journal.schema_version, SUBMIT_SCHEMA_VERSION
                    ),
                ),
            });
        }
        Ok(Some(journal))
    }

    /// 原子落盘提审去重状态（返回路径）。
    pub fn save_submit(
        &self,
        channel: &str,
        key: &str,
        journal: &SubmitJournal,
    ) -> Result<PathBuf, PublishError> {
        let path = self.submit_path(channel, key);
        write_atomic(
            &path,
            &serde_json::to_vec_pretty(journal).expect("去重状态序列化不会失败"),
        )?;
        Ok(path)
    }
}

/// 读可选文件：不存在 → `Ok(None)`；其余 IO 失败 → [`PublishError::Archive`]。
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, PublishError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(PublishError::Archive {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// 原子落盘（同目录临时文件 + rename）。
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), PublishError> {
    let dir = path.parent().expect("归档路径必有父目录");
    std::fs::create_dir_all(dir).map_err(|source| PublishError::Archive {
        path: dir.to_path_buf(),
        source,
    })?;
    let tmp = path.with_extension(format!(
        "{}tmp",
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!("{ext}."))
            .unwrap_or_default()
    ));
    std::fs::write(&tmp, bytes).map_err(|source| PublishError::Archive {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| PublishError::Archive {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// 回执键：`{appid}-{version}-{digest 前 16 hex}`（文件名安全子集，其余字符 `-`）。
pub fn receipt_key(appid: &str, version: &str, digest: &str) -> String {
    let short = digest.get(..16).unwrap_or(digest);
    format!(
        "{}-{}-{}",
        sanitize_part(appid),
        sanitize_part(version),
        sanitize_part(short)
    )
}

/// 文件名安全化（保留 `[A-Za-z0-9._]`，其余替换为 `-`；空 → `empty`）。
fn sanitize_part(part: &str) -> String {
    let cleaned: String = part
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "empty".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_label_matches_serde() {
        for state in [
            AuditState::Uploaded,
            AuditState::Submitted,
            AuditState::InReview,
            AuditState::Approved,
            AuditState::Rejected,
            AuditState::Withdrawn,
            AuditState::Delayed,
            AuditState::Released,
            AuditState::Unknown,
        ] {
            assert_eq!(
                serde_json::to_string(&state).expect("序列化"),
                format!("\"{}\"", state.label()),
                "回执里的状态文本必须与 label 一致（格式固定）"
            );
        }
    }

    #[test]
    fn terminal_states_are_exactly_four() {
        let terminal: Vec<&str> = [
            AuditState::Uploaded,
            AuditState::Submitted,
            AuditState::InReview,
            AuditState::Approved,
            AuditState::Rejected,
            AuditState::Withdrawn,
            AuditState::Delayed,
            AuditState::Released,
            AuditState::Unknown,
        ]
        .iter()
        .filter(|state| state.is_terminal())
        .map(AuditState::label)
        .collect();
        assert_eq!(terminal, ["approved", "rejected", "withdrawn", "released"]);
    }

    #[test]
    fn receipt_key_is_filename_safe() {
        assert_eq!(
            receipt_key("wx1234", "1.2.0", "abcdef0123456789ff"),
            "wx1234-1.2.0-abcdef0123456789"
        );
        assert_eq!(receipt_key("wx/12 34", "v1", "dead"), "wx-12-34-v1-dead");
        assert_eq!(sanitize_part(""), "empty");
    }

    #[test]
    fn save_load_roundtrip_and_atomic_path() {
        let root = std::env::temp_dir().join(format!("poly-pub-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let archive = Archive::new(&root);
        let mut receipt =
            Receipt::new("wechat-open-platform", "wx1", "1.0.0", "aa11bb22cc33dd44ee");
        receipt.state = AuditState::InReview;
        receipt.polls.push(PollEntry {
            at_unix: 1,
            state: AuditState::InReview,
            reason: None,
            raw: serde_json::json!({"status": 2}),
        });
        let path = archive.save(&receipt).expect("落盘");
        assert_eq!(
            path,
            root.join(".polypack/publish/wechat-open-platform/wx1-1.0.0-aa11bb22cc33dd44.json")
        );
        assert!(
            !path.with_extension("json.tmp").exists(),
            "临时文件须被 rename 掉"
        );
        let loaded = archive
            .load("wechat-open-platform", "wx1-1.0.0-aa11bb22cc33dd44")
            .expect("读");
        let loaded = loaded.expect("存在");
        assert_eq!(loaded.state, AuditState::InReview);
        assert_eq!(loaded.polls.len(), 1);
        assert!(
            archive
                .load("wechat-open-platform", "不存在")
                .expect("缺省")
                .is_none(),
            "缺回执返回 None 而非错误"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
