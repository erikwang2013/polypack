//! rpm 文件头解析（T-M5-01）：打包器自检与 verify 腿 `rpmlint` 的本地结构面共用。
//!
//! 纯 Rust 解析 lead（3.0 魔数）+ signature header + main header，不依赖宿主 rpm
//! 命令：verify 腿在无 rpm 的 Debian 宿主上也能跑结构面；官方 rpmlint 在场时再叠加
//! 判词。头部字符串 / 整数数组的字节序与偏移按 rpm 格式规范（大端）。

use std::path::Path;

use crate::error::{BundleError, io_err};

/// rpm 结构检查结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpmStructure {
    /// `NAME`。
    pub name: String,
    /// `VERSION`。
    pub version: String,
    /// `RELEASE`。
    pub release: String,
    /// `ARCH`。
    pub arch: String,
    /// `LICENSE`。
    pub license: String,
    /// 头部重建的文件路径（`DIRNAMES` + `DIRINDEXES` + `BASENAMES`）。
    pub files: Vec<String>,
}

/// rpm 文件头常量（`rpmlead.h` / `rpmtag.h` 的稳定编号）。
const RPMTAG_NAME: u32 = 1000;
const RPMTAG_VERSION: u32 = 1001;
const RPMTAG_RELEASE: u32 = 1002;
const RPMTAG_LICENSE: u32 = 1014;
const RPMTAG_ARCH: u32 = 1022;
const RPMTAG_DIRINDEXES: u32 = 1116;
const RPMTAG_BASENAMES: u32 = 1117;
const RPMTAG_DIRNAMES: u32 = 1118;

/// 解析 rpm 头部并做结构面断言（desktop / AppStream / bin / lib 条目）。
pub fn check_rpm_structure(rpm: &Path) -> Result<RpmStructure, BundleError> {
    let bytes = std::fs::read(rpm).map_err(io_err(rpm))?;
    let bad = |reason: String| BundleError::Archive {
        path: rpm.to_path_buf(),
        reason,
    };
    if bytes.len() < 96 || bytes[0..4] != [0xed, 0xab, 0xee, 0xdb] {
        return Err(bad("缺 rpm lead 魔数（\\xed\\xab\\xee\\xdb）".to_string()));
    }
    if bytes[4] != 3 || bytes[5] != 0 {
        return Err(bad(format!(
            "lead 版本须为 3.0（实际 {}.{}）",
            bytes[4], bytes[5]
        )));
    }
    let (_, sig_end) = parse_header(&bytes, 96, rpm)?;
    // signature header 之后按 8 字节对齐（rpm 格式：padding 不计入 size）。
    let main_start = sig_end.div_ceil(8) * 8;
    let (header, _) = parse_header(&bytes, main_start, rpm)?;
    let string = |tag: u32, name: &str| {
        header
            .strings(tag)
            .and_then(|values| values.into_iter().next())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| bad(format!("头部缺 {name}（tag {tag}）")))
    };
    let name = string(RPMTAG_NAME, "NAME")?;
    let version = string(RPMTAG_VERSION, "VERSION")?;
    let release = string(RPMTAG_RELEASE, "RELEASE")?;
    let arch = string(RPMTAG_ARCH, "ARCH")?;
    let license = string(RPMTAG_LICENSE, "LICENSE")?;
    let basenames = header.strings(RPMTAG_BASENAMES).unwrap_or_default();
    let dirnames = header.strings(RPMTAG_DIRNAMES).unwrap_or_default();
    let dirindexes = header.ints(RPMTAG_DIRINDEXES).unwrap_or_default();
    if basenames.is_empty() {
        return Err(bad("头部缺文件列表（BASENAMES 为空）".to_string()));
    }
    let files: Vec<String> = basenames
        .iter()
        .enumerate()
        .filter(|(_, base)| !base.is_empty())
        .map(|(index, base)| {
            let dir = dirindexes
                .get(index)
                .and_then(|at| dirnames.get(*at as usize))
                .map(String::as_str)
                .unwrap_or("/");
            format!("{dir}{base}")
        })
        .collect();
    for (suffix, what) in [
        (".desktop", "desktop 入口"),
        (".metainfo.xml", "AppStream 元数据"),
    ] {
        if !files.iter().any(|path| path.ends_with(suffix)) {
            return Err(bad(format!("文件列表缺 {what}（*{suffix}）")));
        }
    }
    // FHS：接受 /usr 前缀（Fedora 现行布局）与旧式 /bin、/lib 根前缀。
    let under = |root: &str| {
        files
            .iter()
            .any(|path| path.starts_with(root) || path.starts_with(&format!("/usr{root}")))
    };
    if !under("/bin/") {
        return Err(bad("文件列表缺 /usr/bin/<可执行>".to_string()));
    }
    if !under("/lib/") {
        return Err(bad("文件列表缺 /usr/lib/<资源目录>".to_string()));
    }
    Ok(RpmStructure {
        name,
        version,
        release,
        arch,
        license,
        files,
    })
}

/// 解析一个 rpm header（魔数 + 索引 + 数据区），返回（标签表，结束偏移）。
fn parse_header(
    bytes: &[u8],
    at: usize,
    label: &Path,
) -> Result<(RpmHeader, usize), BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: label.to_path_buf(),
        reason,
    };
    if at + 16 > bytes.len() || bytes[at..at + 3] != [0x8e, 0xad, 0xe8] {
        return Err(bad(format!("偏移 {at} 处缺 rpm header 魔数")));
    }
    let count = be32(bytes, at + 8) as usize;
    let data_len = be32(bytes, at + 12) as usize;
    let index_start = at + 16;
    let data_start = index_start
        .checked_add(count * 16)
        .ok_or_else(|| bad("header 索引长度溢出".to_string()))?;
    let data_end = data_start
        .checked_add(data_len)
        .ok_or_else(|| bad("header 数据长度溢出".to_string()))?;
    if data_end > bytes.len() {
        return Err(bad("header 数据越界".to_string()));
    }
    let mut tags = std::collections::BTreeMap::new();
    for index in 0..count {
        let entry = index_start + index * 16;
        let tag = be32(bytes, entry);
        let kind = be32(bytes, entry + 4);
        let offset = be32(bytes, entry + 8) as usize;
        let items = be32(bytes, entry + 12) as usize;
        let start = data_start + offset;
        if start > data_end {
            return Err(bad(format!("标签 {tag} 的数据偏移越界")));
        }
        match kind {
            4 => {
                let mut values = Vec::with_capacity(items);
                for item in 0..items {
                    let pos = start + item * 4;
                    if pos + 4 > data_end {
                        return Err(bad(format!("标签 {tag} 的 INT32 数组越界")));
                    }
                    values.push(be32(bytes, pos));
                }
                tags.insert(tag, TagValue::Ints(values));
            }
            6 => {
                let end = bytes[start..data_end]
                    .iter()
                    .position(|byte| *byte == 0)
                    .map(|len| start + len)
                    .ok_or_else(|| bad(format!("标签 {tag} 的字符串未终止")))?;
                let value = String::from_utf8_lossy(&bytes[start..end]).into_owned();
                tags.insert(tag, TagValue::Strings(vec![value]));
            }
            8 | 9 => {
                let mut values = Vec::with_capacity(items);
                let mut cursor = start;
                for _ in 0..items {
                    let end = bytes[cursor..data_end]
                        .iter()
                        .position(|byte| *byte == 0)
                        .map(|len| cursor + len)
                        .ok_or_else(|| bad(format!("标签 {tag} 的字符串数组未终止")))?;
                    values.push(String::from_utf8_lossy(&bytes[cursor..end]).into_owned());
                    cursor = end + 1;
                }
                tags.insert(tag, TagValue::Strings(values));
            }
            _ => {}
        }
    }
    Ok((RpmHeader { tags }, data_end))
}

/// 解析后的 header 标签表。
struct RpmHeader {
    tags: std::collections::BTreeMap<u32, TagValue>,
}

enum TagValue {
    Ints(Vec<u32>),
    Strings(Vec<String>),
}

impl RpmHeader {
    fn ints(&self, tag: u32) -> Option<Vec<u32>> {
        match self.tags.get(&tag)? {
            TagValue::Ints(values) => Some(values.clone()),
            TagValue::Strings(_) => None,
        }
    }

    fn strings(&self, tag: u32) -> Option<Vec<String>> {
        match self.tags.get(&tag)? {
            TagValue::Strings(values) => Some(values.clone()),
            TagValue::Ints(_) => None,
        }
    }
}

fn be32(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 截断 / 错魔数输入必须响亮（不把空壳当 rpm）。
    #[test]
    fn malformed_rpm_is_rejected() {
        let base = std::env::temp_dir().join(format!("poly-rpm-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("bad.rpm");
        std::fs::write(&path, b"not an rpm").unwrap();
        assert!(check_rpm_structure(&path).is_err());
        std::fs::write(&path, [0xed, 0xab, 0xee, 0xdb, 3, 0]).unwrap();
        assert!(check_rpm_structure(&path).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
