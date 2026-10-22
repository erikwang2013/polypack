//! rpm header 只读解析（repodata 所需最小标签视图）。
//!
//! 后续可把共享解析上移：若 polypack-bundle 需要完整 header 只读视图，把本文件抽到上游，
//! polypack-cli 改为消费（当前保持 `rpm_header.rs` 的 verify 自检公开面不变）。

use std::path::Path;

use polypack_core::diagnostics::Diagnostic;

use super::super::repo_error;

const HEADER_MAGIC: [u8; 4] = [0x8e, 0xad, 0xe8, 0x01];
const TYPE_INT32: u32 = 4;
const TYPE_STRING: u32 = 6;
const TYPE_STRING_ARRAY: u32 = 8;
const TYPE_I18NSTRING: u32 = 9;

/// 一个 header 的 index 项（偏移相对数据段起点）。
struct Entry {
    tag: u32,
    typ: u32,
    offset: u32,
    count: u32,
}

/// rpm header 只读视图（签名头 / 主头共用）。
pub(super) struct Header<'a> {
    bytes: &'a [u8],
    data_start: usize,
    /// header 结束位置（= 主头的 payload 起点）。
    pub(super) end: usize,
    entries: Vec<Entry>,
}

impl<'a> Header<'a> {
    pub(super) fn parse(bytes: &'a [u8], start: usize, path: &Path) -> Result<Self, Diagnostic> {
        let bad = |reason: String| {
            repo_error(format!(
                "{} 的 rpm header（偏移 {start}）非法：{reason}",
                path.display()
            ))
        };
        let magic = bytes
            .get(start..start + 4)
            .ok_or_else(|| bad("头部越界".to_string()))?;
        if magic != HEADER_MAGIC.as_slice() {
            return Err(bad("魔数不是 8E AD E8 01".to_string()));
        }
        let nindex = be_u32(bytes, start + 8).ok_or_else(|| bad("缺 nindex".to_string()))? as usize;
        let hsize = be_u32(bytes, start + 12).ok_or_else(|| bad("缺 hsize".to_string()))? as usize;
        let index_start = start
            .checked_add(16)
            .ok_or_else(|| bad("偏移溢出".to_string()))?;
        let data_start = index_start
            .checked_add(
                nindex
                    .checked_mul(16)
                    .ok_or_else(|| bad("index 长度溢出".to_string()))?,
            )
            .ok_or_else(|| bad("偏移溢出".to_string()))?;
        let end = data_start
            .checked_add(hsize)
            .ok_or_else(|| bad("数据段长度溢出".to_string()))?;
        if end > bytes.len() {
            return Err(bad(format!("header 越界（声明到 {end}）")));
        }
        let mut entries = Vec::with_capacity(nindex);
        for index in 0..nindex {
            let base = index_start + index * 16;
            let entry = &bytes[base..base + 16];
            let tag = u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]);
            let typ = u32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]]);
            let offset = u32::from_be_bytes([entry[8], entry[9], entry[10], entry[11]]);
            let count = u32::from_be_bytes([entry[12], entry[13], entry[14], entry[15]]);
            entries.push(Entry {
                tag,
                typ,
                offset,
                count,
            });
        }
        Ok(Self {
            bytes,
            data_start,
            end,
            entries,
        })
    }

    fn entry(&self, tag: u32) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.tag == tag)
    }

    /// 数据段内从 entry 偏移开始的可读切片（截到 header 末尾）。
    fn slice(&self, entry: &Entry) -> Option<&'a [u8]> {
        let start = self.data_start.checked_add(entry.offset as usize)?;
        if start >= self.end {
            return None;
        }
        self.bytes.get(start..self.end)
    }

    pub(super) fn string_or_i18n(&self, tag: u32) -> Option<String> {
        let entry = self.entry(tag)?;
        if entry.typ != TYPE_STRING && entry.typ != TYPE_I18NSTRING {
            return None;
        }
        let data = self.slice(entry)?;
        let len = data.iter().position(|byte| *byte == 0)?;
        std::str::from_utf8(&data[..len]).ok().map(str::to_owned)
    }

    pub(super) fn string(&self, tag: u32) -> Option<String> {
        self.string_or_i18n(tag)
    }

    pub(super) fn i18n(&self, tag: u32) -> Option<String> {
        let entry = self.entry(tag)?;
        if entry.typ != TYPE_I18NSTRING {
            return None;
        }
        self.string_or_i18n(tag)
    }

    pub(super) fn int32(&self, tag: u32) -> Option<u32> {
        let entry = self.entry(tag)?;
        if entry.typ != TYPE_INT32 {
            return None;
        }
        self.int32_at(entry, 0)
    }

    fn int32_at(&self, entry: &Entry, index: usize) -> Option<u32> {
        let start = self
            .data_start
            .checked_add(entry.offset as usize)?
            .checked_add(index.checked_mul(4)?)?;
        be_u32(self.bytes, start)
    }

    pub(super) fn int32_array(&self, tag: u32) -> Vec<u32> {
        let Some(entry) = self.entry(tag) else {
            return Vec::new();
        };
        if entry.typ != TYPE_INT32 {
            return Vec::new();
        }
        (0..entry.count as usize)
            .filter_map(|index| self.int32_at(entry, index))
            .collect()
    }

    pub(super) fn string_array(&self, tag: u32) -> Vec<String> {
        let Some(entry) = self.entry(tag) else {
            return Vec::new();
        };
        if entry.typ != TYPE_STRING_ARRAY {
            return Vec::new();
        }
        let Some(data) = self.slice(entry) else {
            return Vec::new();
        };
        let mut values = Vec::with_capacity(entry.count as usize);
        let mut pos = 0;
        for _ in 0..entry.count {
            let Some(len) = data
                .get(pos..)
                .and_then(|rest| rest.iter().position(|byte| *byte == 0))
            else {
                break;
            };
            match std::str::from_utf8(&data[pos..pos + len]) {
                Ok(value) => values.push(value.to_string()),
                Err(_) => break,
            }
            pos += len + 1;
        }
        values
    }
}

fn be_u32(bytes: &[u8], start: usize) -> Option<u32> {
    let slice = bytes.get(start..start + 4)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}
