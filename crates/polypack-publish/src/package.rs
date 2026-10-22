//! 代码包载具（T-M3-09 / T-M3-10；T-M3-12 快应用 `.rpk` 接缝）：目录载具（T-M3-06 产物）
//! 与单文件载具 → 文件清单 + **容器无关** blake3 摘要。
//!
//! 摘要口径（确定性，测试锁定）：
//! - 目录载具：域分隔串 + 逐条（相对路径、长度、字节），清单排序 → 内容一致 → 摘要一致；
//! - 单文件载具：域分隔串 + 长度 + 字节，**文件名不参与**（内容即身份，重命名不换回执键）；
//! - 两种载具域分隔，互不相撞；与各平台上传容器（微信 B 协议 / zip）无关。
//!
//! 回执键 / 断点续走（`渠道 + appid + 版本 + 摘要前 16`）见 [`crate::flow`]。

use std::path::{Path, PathBuf};

use crate::error::PublishError;

/// 代码包载具（目录载具 = T-M3-06 产物目录；单文件载具 = 单文件产物如 `.rpk`）→
/// 文件清单 + 容器无关摘要。
pub struct Package {
    /// 相对路径（`/` 分隔、升序）→ 内容；单文件载具恰一条（文件名不参与摘要）。
    files: Vec<(String, Vec<u8>)>,
    /// 单文件载具的文件名（目录载具为 `None`；不参与摘要——内容即身份）。
    single: Option<String>,
    /// 内容摘要（blake3 hex，64 字符）。
    digest: String,
}

impl Package {
    /// 读代码包载具（按路径形态自动判别）：目录 → 目录载具语义（T-M3-06 产物
    /// `<…>/miniapp/dist`，根须有 `app.json`）；常规文件 → 单文件载具（[`Self::load_file`]，
    /// 如快应用 `.rpk`）。驱动只认本入口——载具形态不进四步，平台侧零感知。
    pub fn load(path: &Path) -> Result<Self, PublishError> {
        let meta = std::fs::metadata(path).map_err(|source| PublishError::Input {
            path: path.to_path_buf(),
            source,
        })?;
        if meta.is_dir() {
            return Self::load_dir(path);
        }
        if meta.is_file() {
            return Self::load_file(path);
        }
        Err(input_err(
            path,
            std::io::ErrorKind::InvalidData,
            "代码包载具应是目录（T-M3-06 产物 <…>/miniapp/dist）或单文件（如快应用 .rpk）",
        ))
    }

    /// 读目录载具（T-M3-06 产物 `<…>/miniapp/dist`；根须有 `app.json`——四端产物同款）。
    fn load_dir(dir: &Path) -> Result<Self, PublishError> {
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
            single: None,
        })
    }

    /// 读**单文件载具**（单文件产物：快应用 `.rpk` 等——hap-toolkit 真跑产物，根目录即文件）。
    ///
    /// 摘要 = 域分隔串 + 长度 + 字节——**不含文件名**：单文件的身份就是内容本身（重命名 /
    /// 换产物文件名不换回执键，断点续走更稳）；与目录载具摘要域分隔，互不相撞。
    pub fn load_file(path: &Path) -> Result<Self, PublishError> {
        let meta = std::fs::metadata(path).map_err(|source| PublishError::Input {
            path: path.to_path_buf(),
            source,
        })?;
        if !meta.is_file() {
            return Err(input_err(
                path,
                std::io::ErrorKind::InvalidInput,
                "单文件载具应是常规文件（目录走 Package::load 的目录分支）",
            ));
        }
        let body = std::fs::read(path).map_err(|source| PublishError::Input {
            path: path.to_path_buf(),
            source,
        })?;
        if body.is_empty() {
            return Err(input_err(
                path,
                std::io::ErrorKind::InvalidData,
                "单文件载具是空文件（构建尚未完成？）",
            ));
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| "package".to_string());
        Ok(Self {
            digest: digest_of_single(&body),
            files: vec![(name.clone(), body)],
            single: Some(name),
        })
    }

    /// 文件清单（升序；单文件载具恰一条）。
    pub fn files(&self) -> &[(String, Vec<u8>)] {
        &self.files
    }

    /// 单文件载具视图：`Some((文件名, 内容))`（目录载具 → `None`）。
    pub fn single_file(&self) -> Option<(&str, &[u8])> {
        let name = self.single.as_deref()?;
        self.files.first().map(|(_, body)| (name, body.as_slice()))
    }

    /// 内容摘要（hex）。
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// 确定性 zip（条目排序、Stored、时间戳 1980-01-01、权限 0644；口径同
    /// `polypack-bundle::zipwrite`——同内容二跑字节一致）。支付宝 / 抖音客户端用。
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
    hasher.update(b"polypack-publish/package/1");
    for (name, body) in files {
        hasher.update(name.as_bytes());
        hasher.update(b"\0");
        hasher.update(&(body.len() as u64).to_le_bytes());
        hasher.update(body);
    }
    hasher.finalize().to_hex().to_string()
}

/// 单文件载具摘要：域分隔串 + 长度 + 字节；**不含文件名**（内容即身份，重命名不换键）。
fn digest_of_single(body: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"polypack-publish/package-file/1");
    hasher.update(&(body.len() as u64).to_le_bytes());
    hasher.update(body);
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
        let dir = std::env::temp_dir().join(format!("polypack-publish-{tag}-{}", std::process::id()));
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
    fn single_file_carrier_is_content_addressed_and_rename_insensitive() {
        let base = scratch("single");
        let first = base.join("pax-notes.debug.1.2.3.rpk");
        std::fs::write(&first, b"PK\x03\x04polypack-rpk-sample").expect("写单文件载具");
        std::fs::create_dir_all(base.join("nested")).expect("建目录");
        let renamed = base.join("nested/renamed.rpk");
        std::fs::write(&renamed, b"PK\x03\x04polypack-rpk-sample").expect("写同内容异名");

        let a = Package::load_file(&first).expect("单文件载具");
        let b = Package::load_file(&renamed).expect("单文件载具");
        assert_eq!(a.digest(), b.digest(), "同内容（异名 / 异目录）→ 摘要一致");
        assert_eq!(
            a.single_file(),
            Some((
                "pax-notes.debug.1.2.3.rpk",
                b"PK\x03\x04polypack-rpk-sample".as_slice()
            )),
            "单文件视图 =（文件名, 内容）"
        );
        assert_eq!(a.files().len(), 1, "清单恰一条");
        assert_eq!(
            Package::load(&first).expect("同一入口自动判别").digest(),
            a.digest(),
            "load 与 load_file 等价"
        );

        std::fs::write(&renamed, b"PK\x03\x04other").expect("改内容");
        assert_ne!(
            Package::load_file(&renamed).expect("重读").digest(),
            a.digest(),
            "内容变 → 摘要变"
        );

        // 与目录载具域分隔：同字节文件进目录后的树摘要 ≠ 单文件摘要。
        let dir = base.join("dist");
        write_miniapp(&dir);
        std::fs::write(dir.join("payload.bin"), b"PK\x03\x04polypack-rpk-sample").expect("写");
        let as_dir = Package::load(&dir).expect("目录载具");
        assert_eq!(as_dir.single_file(), None, "目录载具没有单文件视图");
        assert_ne!(as_dir.digest(), a.digest(), "两种载具摘要域分隔，互不相撞");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn single_file_rejects_empty_directory_and_missing_paths() {
        let base = scratch("single-bad");
        let empty = base.join("empty.rpk");
        std::fs::write(&empty, b"").expect("写空文件");
        let err = Package::load(&empty).err().expect("空单文件须拒");
        assert!(err.to_string().contains("空文件"), "{err}");

        let dir = base.join("dist");
        write_miniapp(&dir);
        let err = Package::load_file(&dir).err().expect("目录不是单文件载具");
        assert!(err.to_string().contains("单文件"), "{err}");

        let err = Package::load(&base.join("missing.rpk"))
            .err()
            .expect("缺路径须拒");
        assert_eq!(err.code(), crate::codes::E_PUB_0004, "{err}");
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
