//! blake3 摘要、确定性目录树摘要与缓存键（ROADMAP §5.2）。

use core::fmt;

#[cfg(feature = "std")]
use std::fs;
#[cfg(feature = "std")]
use std::path::{Path, PathBuf};

use alloc::borrow::ToOwned;
use alloc::string::{String, ToString};
#[cfg(feature = "std")]
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::error::CacheError;

/// 缓存键域前缀 + 键格式版本：改动键的构造方式时必须递增。
const KEY_DOMAIN: &str = "polypack-cache/v1";
/// 目录树摘要域前缀：与字节摘要隔离，避免不同用途的摘要互相碰撞。
#[cfg(feature = "std")]
const TREE_DOMAIN: &str = "polypack-tree/v1";

/// blake3 摘要（小写十六进制，64 字符）。
#[derive(Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Digest(String);

impl Digest {
    /// 字节串摘要。
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self::from_hash(blake3::hash(bytes))
    }

    /// 目录树确定性摘要：按相对路径（字节序）排序后逐节点混入路径与内容。
    ///
    /// - `exclude` 为相对路径分量：不含 `/` 的条目排除任意层级同名项（如 `node_modules`），
    ///   含 `/` 的条目按相对路径前缀排除（如 `target/debug`）；
    /// - 空目录不参与摘要；符号链接按目标路径文本参与、不跟随（避免目录环）；
    ///   fifo / 设备 / socket 等其他类型不参与。
    #[cfg(feature = "std")]
    pub fn of_tree(root: &Path, exclude: &[&str]) -> Result<Self, CacheError> {
        let mut nodes = Vec::new();
        collect_nodes(root, b"", exclude, &mut nodes)?;
        nodes.sort_by(|a, b| a.rel.cmp(&b.rel)); // 排序：与文件系统的遍历顺序无关
        let mut hasher = blake3::Hasher::new();
        hasher.update(TREE_DOMAIN.as_bytes());
        for node in &nodes {
            hash_field(&mut hasher, &node.rel);
            match node.kind {
                NodeKind::File => {
                    let bytes = fs::read(&node.path).map_err(|e| CacheError::io(&node.path, e))?;
                    hasher.update(b"F");
                    hash_field(&mut hasher, &bytes);
                }
                NodeKind::Link => {
                    let target =
                        fs::read_link(&node.path).map_err(|e| CacheError::io(&node.path, e))?;
                    hasher.update(b"L");
                    hash_field(&mut hasher, target.as_os_str().as_encoded_bytes());
                }
            }
        }
        Ok(Self::from_hash(hasher.finalize()))
    }

    /// 解析十六进制摘要文本；大写归一化为小写，长度或字符非法则报错。
    pub fn parse(s: &str) -> Result<Self, CacheError> {
        if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CacheError::Digest(s.to_owned()));
        }
        Ok(Self(s.to_ascii_lowercase()))
    }

    /// 十六进制文本（小写）。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn from_hash(hash: blake3::Hash) -> Self {
        Self(hash.to_hex().to_string())
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({})", self.0)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        Self::parse(&String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

/// §5.2 缓存键输入。
///
/// 本 crate 为叶层，故除摘要外一律接收字符串（不引用 `TargetSpec` / `Profile` 类型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheInputs {
    /// 源文件树摘要（`Digest::of_tree`）。
    pub sources: Digest,
    /// 依赖锁文件内容摘要（`Digest::of_bytes`）。
    pub lockfile: Digest,
    /// 目标声明序列化文本（调用方给）。
    pub target_spec: String,
    /// 工具链版本串（调用方给）。
    pub toolchain: String,
    /// 构建参数 profile（`"ci"` / `"local"`）。
    pub profile: String,
    /// 多应用工作区的应用 id；单应用为 `None`。
    ///
    /// 仅在 `Some` 时混入键：单应用（`None`）键与 M9 之前逐字节一致；共享 core 源树的
    /// compile 键由调用方传 `None`，从而跨应用复用同一缓存条目。
    pub app_id: Option<String>,
}

/// 计算缓存键：`blake3(域前缀 ‖ 各字段 长度前缀‖值)`。
///
/// 域前缀做版本隔离，长度前缀消除字段拼接歧义（`"ab"+"c"` 与 `"a"+"bc"` 不同键）。
/// `app_id = None` 时不追加任何字节，单应用键保持向后兼容。
pub fn cache_key(inputs: &CacheInputs) -> Digest {
    let mut hasher = blake3::Hasher::new();
    hasher.update(KEY_DOMAIN.as_bytes());
    for field in [
        inputs.sources.as_str().as_bytes(),
        inputs.lockfile.as_str().as_bytes(),
        inputs.target_spec.as_bytes(),
        inputs.toolchain.as_bytes(),
        inputs.profile.as_bytes(),
    ] {
        hash_field(&mut hasher, field);
    }
    if let Some(app_id) = &inputs.app_id {
        hash_field(&mut hasher, app_id.as_bytes());
    }
    Digest::from_hash(hasher.finalize())
}

/// 长度前缀 + 内容：消除字段拼接歧义。
fn hash_field(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

/// 目录树中的一个节点（普通文件或符号链接），`rel` 为 `/` 分隔的相对路径字节。
#[cfg(feature = "std")]
struct Node {
    rel: Vec<u8>,
    path: PathBuf,
    kind: NodeKind,
}

/// 节点类型。
#[cfg(feature = "std")]
enum NodeKind {
    File,
    Link,
}

/// 递归收集节点；`prefix` 是已带尾部 `/` 的相对路径前缀（根目录为空）。
#[cfg(feature = "std")]
fn collect_nodes(
    dir: &Path,
    prefix: &[u8],
    exclude: &[&str],
    out: &mut Vec<Node>,
) -> Result<(), CacheError> {
    let entries = fs::read_dir(dir).map_err(|e| CacheError::io(dir, e))?;
    for item in entries {
        let item = item.map_err(|e| CacheError::io(dir, e))?;
        let mut rel = prefix.to_vec();
        rel.extend_from_slice(item.file_name().as_encoded_bytes());
        if is_excluded(&rel, exclude) {
            continue;
        }
        let path = item.path();
        // file_type 不跟随符号链接：目录环不会被走进
        let file_type = item.file_type().map_err(|e| CacheError::io(&path, e))?;
        if file_type.is_dir() {
            rel.push(b'/');
            collect_nodes(&path, &rel, exclude, out)?;
        } else if file_type.is_file() {
            out.push(Node {
                rel,
                path,
                kind: NodeKind::File,
            });
        } else if file_type.is_symlink() {
            out.push(Node {
                rel,
                path,
                kind: NodeKind::Link,
            });
        }
    }
    Ok(())
}

/// 排除匹配规则见 `Digest::of_tree` 文档。
#[cfg(feature = "std")]
fn is_excluded(rel: &[u8], exclude: &[&str]) -> bool {
    exclude.iter().any(|pattern| {
        let bytes = pattern.trim_matches('/').as_bytes();
        if bytes.contains(&b'/') {
            rel == bytes
                || rel
                    .strip_prefix(bytes)
                    .is_some_and(|rest| rest.starts_with(b"/"))
        } else {
            // 单段名字只看最后一段：祖先目录在进入前已按同一规则判过
            rel.rsplit(|&b| b == b'/').next() == Some(bytes)
        }
    })
}
