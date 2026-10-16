//! 共享发布驱动（T-M3-09 / T-M3-10）：三渠道零分叉的
//! 「建包 → 查回执 → 上传 / 提审 / 轮询（断点续走）→ 归档」。
//!
//! 幂等 / 去重口径：
//! - [`Package`] 把代码包目录读成**容器无关**的文件清单 + blake3 内容摘要（路径、长度、
//!   字节逐条混入；排序后调用 → 内容一致 → 摘要一致），与各平台上传容器（微信 B 协议 /
//!   zip）无关——换容器不改摘要；
//! - 回执键 = 渠道 + appid + 版本 + 摘要前 16 hex，重跑同键：
//!   终态 → no-op（不触网，只加 `runs`）；`uploaded` → 只提审；`submitted` / `in_review`
//!   / `delayed` → 只轮询；
//! - 提审边界是 at-least-once：平台受理后本地落盘前崩溃，重跑会重发提审——平台以自有
//!   错误码点破（如微信 85009「已有审核中版本」），客户端给修复提示，不假装成功。
//!
//! R4：自动化止步提审；轮询超预算返回 [`PublishError::PollTimeout`]（回执已带状态流水
//! 落盘，重跑只续轮询）。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use poly_core::CredentialRef;

use crate::archive::{Archive, AuditState, PollEntry, Receipt, receipt_key};
use crate::error::PublishError;
use crate::secret::CredentialProvider;
use crate::time::unix_now;
use crate::{Job, Publisher};

/// 代码包（T-M3-06 产物目录 → 文件清单 + 容器无关摘要）。
pub struct Package {
    /// 相对路径（`/` 分隔、升序）→ 内容。
    files: Vec<(String, Vec<u8>)>,
    /// 内容摘要（blake3 hex，64 字符）。
    digest: String,
}

impl Package {
    /// 读代码包目录（T-M3-06 产物 `<…>/miniapp/dist`；根须有 `app.json`——四端产物同款）。
    pub fn load(dir: &Path) -> Result<Self, PublishError> {
        if !dir.is_dir() {
            return Err(input_err(
                dir,
                std::io::ErrorKind::NotFound,
                "代码包应是目录（T-M3-06 产物 <…>/miniapp/dist）",
            ));
        }
        let mut files = Vec::new();
        collect(dir, dir, &mut files)?;
        if files.is_empty() {
            return Err(input_err(
                dir,
                std::io::ErrorKind::InvalidData,
                "代码包目录里没有任何文件",
            ));
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        if !files.iter().any(|(name, _)| name == "app.json") {
            return Err(input_err(
                dir,
                std::io::ErrorKind::InvalidData,
                "代码包根缺 app.json（应指向 Taro 产物 dist 目录，而不是其上级）",
            ));
        }
        Ok(Self {
            digest: digest_of(&files),
            files,
        })
    }

    /// 文件清单（升序）。
    pub fn files(&self) -> &[(String, Vec<u8>)] {
        &self.files
    }

    /// 内容摘要（hex）。
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// 确定性 zip（条目排序、Stored、时间戳 1980-01-01、权限 0644；口径同
    /// `poly-bundle::zipwrite`——同内容二跑字节一致）。支付宝 / 抖音客户端用。
    pub fn zip_bytes(&self) -> Result<Vec<u8>, PublishError> {
        use std::io::Write as _;

        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .last_modified_time(zip::DateTime::DEFAULT)
            .unix_permissions(0o644);
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in &self.files {
            writer
                .start_file(name.as_str(), options)
                .map_err(|err| zip_err(Path::new(name), err))?;
            writer
                .write_all(body)
                .map_err(|source| PublishError::Input {
                    path: PathBuf::from(name),
                    source,
                })?;
        }
        let cursor = writer
            .finish()
            .map_err(|err| zip_err(Path::new("<zip>"), err))?;
        Ok(cursor.into_inner())
    }
}

/// 轮询策略（默认 60s × 15min；重跑只续轮询、不重复提审）。
#[derive(Debug, Clone)]
pub struct PollPolicy {
    /// 两次查询间隔。
    pub interval: Duration,
    /// 轮询总预算；耗尽 → [`PublishError::PollTimeout`]（回执已落盘）。
    pub timeout: Duration,
}

impl Default for PollPolicy {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            timeout: Duration::from_secs(900),
        }
    }
}

/// 一次发布请求（字段齐备即复现；凭据只给引用，值经后端链现取）。
pub struct PublishRequest {
    /// 平台应用 id。
    pub appid: String,
    /// 版本号（平台侧唯一性由平台保证）。
    pub version: String,
    /// 版本描述（上传 / 提审都会带上）。
    pub description: String,
    /// 代码包目录（T-M3-06 产物）。
    pub package: PathBuf,
    /// 凭据引用（T-M2-11 引用模型）。
    pub credential: CredentialRef,
    /// 提审材料（平台各自解释：微信 = `item_list` 数组（类目；真提审必填，且须已在账号
    /// 后台配好）；无 → 客户端按平台要求报缺）。
    pub materials: Option<serde_json::Value>,
    /// 轮询策略。
    pub poll: PollPolicy,
}

impl PublishRequest {
    /// 最小构造（materials = None、轮询用默认策略）。
    pub fn new(
        appid: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
        package: impl Into<PathBuf>,
        credential: CredentialRef,
    ) -> Self {
        Self {
            appid: appid.into(),
            version: version.into(),
            description: description.into(),
            package: package.into(),
            credential,
            materials: None,
            poll: PollPolicy::default(),
        }
    }

    /// 带提审材料。
    pub fn with_materials(mut self, materials: serde_json::Value) -> Self {
        self.materials = Some(materials);
        self
    }

    /// 换轮询策略。
    pub fn with_poll(mut self, poll: PollPolicy) -> Self {
        self.poll = poll;
        self
    }
}

/// 发布结果（回执已脱敏，可安全 Debug）。
#[derive(Debug)]
pub struct PublishOutcome {
    /// 最终回执（已落盘）。
    pub receipt: Receipt,
    /// 回执路径。
    pub path: PathBuf,
    /// 续走起点：`None` = 全新执行；`Some(终态)` = no-op 重跑；`Some(其余)` = 从该状态续走。
    pub resumed_from: Option<AuditState>,
}

/// 共享驱动：建包 → 查回执 → 上传 / 提审 / 轮询（续走）→ 归档。
///
/// 渠道取自 `publisher.channel()`（单一事实源）；`providers` 为凭据后端链（引用模型）。
pub fn publish(
    publisher: &dyn Publisher,
    request: &PublishRequest,
    providers: &[Box<dyn CredentialProvider>],
    archive: &Archive,
) -> Result<PublishOutcome, PublishError> {
    let channel = publisher.channel().id();
    let package = Package::load(&request.package)?;
    let key = receipt_key(&request.appid, &request.version, package.digest());

    let (mut receipt, resumed_from) = match archive.load(channel, &key)? {
        Some(mut receipt) if receipt.state.is_terminal() => {
            // 终态重跑 = no-op：不触网，只如实记一次执行。
            receipt.runs += 1;
            receipt.last_unix = unix_now();
            let state = receipt.state;
            let path = archive.save(&receipt)?;
            return Ok(PublishOutcome {
                receipt,
                path,
                resumed_from: Some(state),
            });
        }
        Some(mut receipt) => {
            receipt.runs += 1;
            receipt.last_unix = unix_now();
            let state = receipt.state;
            (receipt, Some(state))
        }
        None => (
            Receipt::new(channel, &request.appid, &request.version, package.digest()),
            None,
        ),
    };

    let job = Job {
        appid: &request.appid,
        version: &request.version,
        description: &request.description,
        materials: request.materials.as_ref(),
        package: &package,
        credential: &request.credential,
        providers,
    };

    // 阶段 1：上传（仅全新执行；已有回执视为已上传）。
    if resumed_from.is_none() {
        receipt.upload = Some(publisher.upload(&job)?.raw);
        receipt.state = AuditState::Uploaded;
        receipt.last_unix = unix_now();
        archive.save(&receipt)?;
    }

    // 就绪等待（默认 no-op，幂等可重入：上传后与续走时都会经过）。
    publisher.await_ready(&job)?;

    // 阶段 2：提审（全新，或停在 uploaded）。
    if matches!(resumed_from, None | Some(AuditState::Uploaded)) {
        receipt.submit = Some(publisher.submit(&job)?.raw);
        receipt.state = AuditState::Submitted;
        receipt.last_unix = unix_now();
        archive.save(&receipt)?;
    }

    // 阶段 3：轮询（到终态，或预算耗尽——每轮流水都先落回执再判断）。
    let started = Instant::now();
    loop {
        let outcome = publisher.poll(&job)?;
        receipt.state = outcome.state;
        receipt.last_unix = unix_now();
        receipt.polls.push(PollEntry {
            at_unix: receipt.last_unix,
            state: outcome.state,
            reason: outcome.reason,
            raw: outcome.raw,
        });
        let path = archive.save(&receipt)?;
        if outcome.state.is_terminal() {
            return Ok(PublishOutcome {
                receipt,
                path,
                resumed_from,
            });
        }
        if started.elapsed() >= request.poll.timeout {
            return Err(PublishError::PollTimeout {
                channel,
                last: outcome.state.to_string(),
            });
        }
        std::thread::sleep(request.poll.interval);
    }
}

/// 递归收集（跟随软链；shortcut: 不设环检测——Taro 产物无软链环）。
fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), PublishError> {
    let entries = std::fs::read_dir(dir).map_err(|source| PublishError::Input {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| PublishError::Input {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let meta = std::fs::metadata(&path).map_err(|source| PublishError::Input {
            path: path.clone(),
            source,
        })?;
        if meta.is_dir() {
            collect(root, &path, out)?;
        } else if meta.is_file() {
            let rel = path.strip_prefix(root).map_err(|err| PublishError::Input {
                path: path.clone(),
                source: std::io::Error::other(format!("相对路径：{err}")),
            })?;
            let name = rel.to_string_lossy().replace('\\', "/");
            let body = std::fs::read(&path).map_err(|source| PublishError::Input {
                path: path.clone(),
                source,
            })?;
            out.push((name, body));
        }
    }
    Ok(())
}

/// 容器无关摘要：逐条混入（路径、长度、字节）+ 头部版本串；清单已排序 → 确定性。
fn digest_of(files: &[(String, Vec<u8>)]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"poly-publish/package/1");
    for (name, body) in files {
        hasher.update(name.as_bytes());
        hasher.update(b"\0");
        hasher.update(&(body.len() as u64).to_le_bytes());
        hasher.update(body);
    }
    hasher.finalize().to_hex().to_string()
}

/// 输入错误（带路径与原因的 [`PublishError::Input`]）。
pub(crate) fn input_err(
    path: &Path,
    kind: std::io::ErrorKind,
    message: impl std::fmt::Display,
) -> PublishError {
    PublishError::Input {
        path: path.to_path_buf(),
        source: std::io::Error::new(kind, message.to_string()),
    }
}

/// zip crate 错误 → [`PublishError::Input`]（IO 原样，结构类转 `io::Error::other`）。
fn zip_err(path: &Path, err: zip::result::ZipError) -> PublishError {
    match err {
        zip::result::ZipError::Io(source) => PublishError::Input {
            path: path.to_path_buf(),
            source,
        },
        other => PublishError::Input {
            path: path.to_path_buf(),
            source: std::io::Error::other(other),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poly-publish-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    fn write_miniapp(dir: &Path) {
        std::fs::create_dir_all(dir.join("pages/index")).expect("建目录");
        std::fs::write(dir.join("app.json"), r#"{"pages":["pages/index/index"]}"#).expect("写");
        std::fs::write(dir.join("app.js"), "App({})").expect("写");
        std::fs::write(dir.join("pages/index/index.js"), "Page({})").expect("写");
    }

    #[test]
    fn digest_is_content_addressed_and_order_free() {
        let base = scratch("digest");
        let a = base.join("a");
        let b = base.join("b");
        write_miniapp(&a);
        write_miniapp(&b);
        let pa = Package::load(&a).expect("读 a");
        let pb = Package::load(&b).expect("读 b");
        assert_eq!(
            pa.digest(),
            pb.digest(),
            "内容一致 → 摘要一致（与目录无关）"
        );

        std::fs::write(a.join("app.js"), "App({v:2})").expect("改内容");
        let changed = Package::load(&a).expect("重读");
        assert_ne!(pa.digest(), changed.digest(), "内容变 → 摘要变");

        std::fs::write(a.join("app.js"), "App({})").expect("改回");
        assert_eq!(
            Package::load(&a).expect("三读").digest(),
            pa.digest(),
            "改回 → 摘要复原（确定性）"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn package_requires_app_json_at_root() {
        let base = scratch("appjson");
        let dir = base.join("dist");
        std::fs::create_dir_all(dir.join("inner")).expect("建目录");
        std::fs::write(dir.join("inner/app.json"), "{}").expect("写");
        let err = Package::load(&dir).err().expect("根缺 app.json 须拒");
        assert!(
            err.to_string().contains("app.json"),
            "报错要点名 app.json：{err}"
        );

        std::fs::write(dir.join("app.json"), "{}").expect("写");
        Package::load(&dir).expect("补齐后通过");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn zip_bytes_is_deterministic_with_sorted_entries() {
        let base = scratch("zip");
        let dir = base.join("dist");
        write_miniapp(&dir);
        let package = Package::load(&dir).expect("读");
        let first = package.zip_bytes().expect("zip 一");
        let second = package.zip_bytes().expect("zip 二");
        assert_eq!(first, second, "同内容二跑字节一致");

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(first)).expect("读 zip");
        let names: Vec<String> = (0..archive.len())
            .map(|i| {
                archive
                    .by_index(i)
                    .expect("条目")
                    .name()
                    .expect("名")
                    .into_owned()
            })
            .collect();
        assert_eq!(
            names,
            ["app.js", "app.json", "pages/index/index.js"],
            "升序"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
