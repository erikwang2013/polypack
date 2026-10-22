//! Subresource Integrity：`sha384-<base64>` 摘要与清单生成 / 校验（T-M1-08）。
//!
//! SRI（W3C）只认 sha256 / 384 / 512，这里固定用 sha384（浏览器推荐值）。
//! base64 为标准字母表 + `=` 填充，自行实现以避免为二十行代码引入依赖。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha384};

use crate::error::{BundleError, io_err};

/// 使用的摘要算法（写死，浏览器侧只接受 sha256/384/512）。
pub const ALGORITHM: &str = "sha384";

/// 一个文件的 SRI 摘要（`sha384-<base64>`）与字节数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// SRI 摘要串。
    pub integrity: String,
    /// 文件字节数。
    pub bytes: u64,
}

/// 站点 SRI 清单（`integrity.json`）：相对路径 → 摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// 摘要算法。
    pub algorithm: String,
    /// 文件清单（相对路径使用 `/` 分隔，排序稳定）。
    pub files: BTreeMap<String, Entry>,
}

/// 字节序列的 SRI 摘要。
pub fn integrity_of(bytes: &[u8]) -> String {
    let digest = Sha384::digest(bytes);
    format!("{ALGORITHM}-{}", base64(&digest))
}

/// 为目录树生成清单（跳过清单自身与隐藏文件）。
pub fn manifest_of(root: &Path) -> Result<Manifest, BundleError> {
    let mut files = BTreeMap::new();
    collect(root, root, &mut files)?;
    Ok(Manifest {
        algorithm: ALGORITHM.to_string(),
        files,
    })
}

/// 校验清单与目录树一致（sri-check 校验器）。
///
/// 返回不一致项（缺失 / 摘要不符 / 清单外新增）；空向量即通过。
pub fn verify(root: &Path, manifest: &Manifest) -> Result<Vec<String>, BundleError> {
    let actual = manifest_of(root)?;
    let mut problems = Vec::new();
    for (path, entry) in &manifest.files {
        match actual.files.get(path) {
            None => problems.push(format!("缺失: {path}")),
            Some(found) if found.integrity != entry.integrity => problems.push(format!(
                "摘要不符: {path}（清单 {}，实际 {}）",
                entry.integrity, found.integrity
            )),
            Some(found) if found.bytes != entry.bytes => {
                problems.push(format!("体积不符: {path}"));
            }
            Some(_) => {}
        }
    }
    for path in actual.files.keys() {
        if !manifest.files.contains_key(path) {
            problems.push(format!("清单外新增: {path}"));
        }
    }
    Ok(problems)
}

/// 递归收集文件（相对路径以 `/` 分隔）。
fn collect(root: &Path, dir: &Path, out: &mut BTreeMap<String, Entry>) -> Result<(), BundleError> {
    for entry in std::fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        let path: PathBuf = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "integrity.json" {
            continue;
        }
        let meta = entry.metadata().map_err(io_err(&path))?;
        if meta.is_dir() {
            collect(root, &path, out)?;
        } else {
            let bytes = std::fs::read(&path).map_err(io_err(&path))?;
            let rel = path
                .strip_prefix(root)
                .map_err(|source| BundleError::Io {
                    path: path.clone(),
                    source: std::io::Error::other(source),
                })?
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            out.insert(
                rel,
                Entry {
                    integrity: integrity_of(&bytes),
                    bytes: meta.len(),
                },
            );
        }
    }
    Ok(())
}

/// 标准 base64 编码（RFC 4648 §4，含 `=` 填充）。
fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_rfc4648_vectors() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), want, "输入 {input:?}");
        }
    }

    /// 期望值由 python3 hashlib/base64 独立算出（非本实现自证）。
    #[test]
    fn sha384_sri_vectors() {
        assert_eq!(
            integrity_of(b""),
            "sha384-OLBgp1GsljhM2TJ+sbHjaiH9txEUvgdDTAzHv2P24donTt6/529l+9Ua0vFImLlb"
        );
        assert_eq!(
            integrity_of(b"hello"),
            "sha384-WeF0h3dEjGnea4ANejO7+5/xtGPkQ1TDVTvNucZm+pASWjx5+QOXvfX2oT3oKGhP"
        );
        assert_eq!(
            integrity_of(b"<!doctype html>"),
            "sha384-iV7oG577QnYnZ5tI458SeWUkhLgotVFoY/3qAeQs3mac4niRM3mls+AJZ3OTMA/R"
        );
    }

    #[test]
    fn manifest_roundtrip_and_drift_detection() {
        let base = std::env::temp_dir().join(format!("polypack-sri-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        crate::fsutil::write_text(&base.join("index.html"), "<!doctype html>").unwrap();
        crate::fsutil::write_text(&base.join("assets/app.js"), "hello").unwrap();

        let manifest = manifest_of(&base).unwrap();
        assert_eq!(manifest.algorithm, "sha384");
        assert!(
            manifest
                .files
                .get("index.html")
                .unwrap()
                .integrity
                .starts_with("sha384-")
        );
        assert!(manifest.files.contains_key("assets/app.js"));
        assert!(verify(&base, &manifest).unwrap().is_empty());

        crate::fsutil::write_text(&base.join("assets/app.js"), "tampered").unwrap();
        let problems = verify(&base, &manifest).unwrap();
        assert!(problems.iter().any(|p| p.contains("assets/app.js")));

        crate::fsutil::write_text(&base.join("extra.css"), "x").unwrap();
        let problems = verify(&base, &manifest).unwrap();
        assert!(problems.iter().any(|p| p.contains("清单外新增")));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn integrity_json_is_not_hashed_into_itself() {
        let base = std::env::temp_dir().join(format!("polypack-sri-self-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        crate::fsutil::write_text(&base.join("index.html"), "x").unwrap();
        crate::fsutil::write_text(&base.join("integrity.json"), "{}").unwrap();
        let manifest = manifest_of(&base).unwrap();
        assert!(!manifest.files.contains_key("integrity.json"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
