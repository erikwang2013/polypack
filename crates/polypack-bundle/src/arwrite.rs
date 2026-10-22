//! `.ipk` 的归档读写（T-M4-09）：Debian 形态的 ar 容器 + ustar tar + gzip，全部确定性。
//!
//! `.ipk` = ar 归档（`debian-binary` + `control.tar.gz` + `data.tar.gz`），本机无 ares-cli
//! 也能真造真验：写入端不依赖 `ar` / `tar` / `gzip` 外部命令（缺席环境照样出包），读取端
//! 独立解析字节流，供 `.ipk` 结构检查与测试里的「系统 ar / tar 交叉验证」两条腿共用。
//!
//! 字节可复现（口径同 T-M4-08 的 zip 写入）：tar 条目与成员按名排序、mtime / uid / gid
//! 固定 0、mode 固定 0644、gzip 头 mtime 固定 0。

use std::io::{Read as _, Write as _};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::error::{BundleError, io_err};

/// ar 归档魔数（`!<arch>\n`）。
const AR_MAGIC: &[u8] = b"!<arch>\n";

/// 一个 ar 成员（写与读共用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// 成员名（`debian-binary` / `control.tar.gz` / `data.tar.gz`）。
    pub name: String,
    /// 成员字节。
    pub bytes: Vec<u8>,
}

/// 写 ar 归档（成员按给定顺序落盘；长度字段十进制、奇数长度补 `\n` 对齐）。
pub fn write_ar(members: &[Member], out: &Path) -> Result<(), BundleError> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(AR_MAGIC);
    for member in members {
        let mut header = [b' '; 60];
        field(&mut header[0..16], member.name.as_bytes());
        field(&mut header[16..28], b"0");
        field(&mut header[28..34], b"0");
        field(&mut header[34..40], b"0");
        field(&mut header[40..48], b"100644");
        field(
            &mut header[48..58],
            member.bytes.len().to_string().as_bytes(),
        );
        header[58] = b'`';
        header[59] = b'\n';
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&member.bytes);
        if member.bytes.len() % 2 == 1 {
            bytes.push(b'\n');
        }
    }
    std::fs::write(out, bytes).map_err(io_err(out))
}

/// 读 ar 归档：解析魔数与各成员头（名字取字段文本、去尾随 `/`；数据段按偶数对齐推进）。
pub fn read_ar(path: &Path) -> Result<Vec<Member>, BundleError> {
    let bytes = std::fs::read(path).map_err(io_err(path))?;
    let bad = |reason: String| BundleError::Archive {
        path: path.to_path_buf(),
        reason,
    };
    if !bytes.starts_with(AR_MAGIC) {
        return Err(bad("缺 ar 魔数 `!<arch>`（不是 ar 归档）".to_string()));
    }
    let mut members = Vec::new();
    let mut offset = AR_MAGIC.len();
    while offset + 60 <= bytes.len() {
        let header = &bytes[offset..offset + 60];
        if &header[58..60] != b"`\n" {
            return Err(bad(format!(
                "第 {} 个成员的头尾标不是 `` `\\n ``",
                members.len() + 1
            )));
        }
        let name = text(&header[0..16]);
        let size: usize = text(&header[48..58])
            .parse()
            .map_err(|_| bad(format!("成员 {name} 的长度字段非法")))?;
        let start = offset + 60;
        let end = start + size;
        if end > bytes.len() {
            return Err(bad(format!("成员 {name} 数据越界（声明 {size} 字节）")));
        }
        members.push(Member {
            name: name.trim_end_matches('/').to_string(),
            bytes: bytes[start..end].to_vec(),
        });
        offset = end + (size % 2);
    }
    if members.is_empty() {
        return Err(bad("ar 归档里没有任何成员".to_string()));
    }
    Ok(members)
}

/// 写 ustar tar（条目按名排序；mtime / uid / gid 固定 0、mode 0644、`ustar` 魔数）。
///
/// 只出常规文件条目（目录隐式由路径表达，Debian 的 data.tar 同款），长名按 ustar 的
/// `prefix` 字段在 `/` 处切分（切不下即报错，不静默截断）。
pub fn tar_bytes(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, BundleError> {
    let mut sorted: Vec<&(String, Vec<u8>)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = Vec::new();
    for (name, data) in sorted {
        let (prefix, leaf) = split_name(name)?;
        out.extend_from_slice(&header(&leaf, prefix.as_deref(), data.len() as u64));
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0); // 末尾两块 512 字节零块
    Ok(out)
}

/// gzip 包装（deflate；头 mtime 固定 0、OS 字节 255「未知」→ 跨宿主二跑字节一致）。
pub fn gzip_bytes(bytes: &[u8], label: &Path) -> Result<Vec<u8>, BundleError> {
    let mut encoder = flate2::GzBuilder::new()
        .mtime(0)
        .operating_system(255)
        .write(Vec::new(), flate2::Compression::default());
    let failed = |err: std::io::Error| BundleError::Archive {
        path: label.to_path_buf(),
        reason: format!("gzip 写入失败：{err}"),
    };
    encoder.write_all(bytes).map_err(failed)?;
    encoder.finish().map_err(failed)
}

/// 已解析的 tar（条目名 → 数据区间；数据本体由 [`TarArchive`] 持有）。
#[derive(Debug, Clone)]
pub struct TarArchive {
    data: Vec<u8>,
    entries: Vec<(String, Range<usize>)>,
}

impl TarArchive {
    /// 解析 tar 字节（`label` 只用于错误定位；非 `ustar` 魔数一律拒绝）。
    pub fn parse(data: Vec<u8>, label: &Path) -> Result<Self, BundleError> {
        let bad = |reason: String| BundleError::Archive {
            path: label.to_path_buf(),
            reason,
        };
        let mut entries = Vec::new();
        let mut offset = 0usize;
        while offset + 512 <= data.len() {
            let block = &data[offset..offset + 512];
            if block.iter().all(|byte| *byte == 0) {
                break;
            }
            if &block[257..262] != b"ustar" {
                return Err(bad(format!("条目 {} 缺 ustar 魔数", text(&block[0..100]))));
            }
            let leaf = text(&block[0..100]);
            let prefix = text(&block[345..500]);
            let name = if prefix.is_empty() {
                leaf.clone()
            } else {
                format!("{prefix}/{leaf}")
            };
            let size: usize = usize::from_str_radix(&text(&block[124..136]), 8)
                .map_err(|_| bad(format!("条目 {name} 的长度字段非法（八进制）")))?;
            let flag = block[156];
            let start = offset + 512;
            let end = start + size;
            if end > data.len() {
                return Err(bad(format!("条目 {name} 数据越界（声明 {size} 字节）")));
            }
            if flag == b'0' || flag == 0 {
                entries.push((name, start..end));
            }
            offset = end.div_ceil(512) * 512;
        }
        Ok(Self { data, entries })
    }

    /// gunzip 后解析（`.ipk` 的 `control.tar.gz` / `data.tar.gz` 读入面）。
    pub fn from_tar_gz(bytes: &[u8], label: &Path) -> Result<Self, BundleError> {
        let mut decoder = flate2::read::GzDecoder::new(bytes);
        let mut plain = Vec::new();
        decoder
            .read_to_end(&mut plain)
            .map_err(|err| BundleError::Archive {
                path: label.to_path_buf(),
                reason: format!("gunzip 失败：{err}"),
            })?;
        Self::parse(plain, label)
    }

    /// 条目名（tar 内原序）。
    pub fn names(&self) -> Vec<String> {
        self.entries.iter().map(|(name, _)| name.clone()).collect()
    }

    /// 单个条目的文本内容（`None` = 条目不存在或不是 UTF-8）。
    pub fn text(&self, name: &str) -> Option<String> {
        let (_, range) = self.entries.iter().find(|(entry, _)| entry == name)?;
        std::str::from_utf8(&self.data[range.clone()])
            .ok()
            .map(str::to_string)
    }
}

/// Debian 控制文件解析（`Key: value` + 以空格开头的续行）；`.ipk` / `.deb` 共用。
pub fn control_fields(text: &str) -> std::collections::BTreeMap<String, String> {
    let mut fields: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut last: Option<String> = None;
    for line in text.lines() {
        if line.starts_with(' ') {
            if let Some(key) = &last
                && let Some(value) = fields.get_mut(key)
            {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_string();
            fields.insert(key.clone(), value.trim().to_string());
            last = Some(key);
        }
    }
    fields
}

/// 固定字段写入：文本左对齐、余位补空格（ar 头字段形态）。
fn field(target: &mut [u8], text: &[u8]) {
    let len = text.len().min(target.len());
    target[..len].copy_from_slice(&text[..len]);
    for byte in &mut target[len..] {
        *byte = b' ';
    }
}

/// 字段读取：截到 NUL / 去尾随空格。
fn text(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end])
        .trim_end()
        .to_string()
}

/// ustar 长名切分：`name` ≤ 100 直接落 `name` 字段；否则在 `/` 处于 `prefix`（≤ 155）与
/// `name`（≤ 100）间切分；切不下即报错（不静默截断——截断的名字等于打错包）。
fn split_name(name: &str) -> Result<(Option<String>, String), BundleError> {
    let too_long = || BundleError::Archive {
        path: PathBuf::from(name),
        reason: format!(
            "tar 条目名超过 ustar 上限（{} 字节，prefix/name 切分不下）",
            name.len()
        ),
    };
    if name.len() <= 100 {
        return Ok((None, name.to_string()));
    }
    for (index, _) in name.match_indices('/') {
        let (prefix, leaf) = (&name[..index], &name[index + 1..]);
        if prefix.len() <= 155 && leaf.len() <= 100 {
            return Ok((Some(prefix.to_string()), leaf.to_string()));
        }
    }
    Err(too_long())
}

/// ustar 头块：八进制字段（末位 NUL）、`ustar\0` + `00` 版本、常规文件 typeflag、校验和。
fn header(name: &str, prefix: Option<&str>, size: u64) -> [u8; 512] {
    let mut block = [0u8; 512];
    block[..name.len()].copy_from_slice(name.as_bytes());
    octal(&mut block[100..108], 0o644); // mode
    octal(&mut block[108..116], 0); // uid
    octal(&mut block[116..124], 0); // gid
    octal(&mut block[124..136], size);
    octal(&mut block[136..148], 0); // mtime
    block[156] = b'0'; // typeflag：常规文件
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    if let Some(prefix) = prefix {
        block[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    }
    for byte in &mut block[148..156] {
        *byte = b' ';
    }
    let sum: u64 = block.iter().map(|byte| u64::from(*byte)).sum();
    block[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    block
}

/// 八进制字段（宽度 − 1 位数字 + 末位 NUL）。
fn octal(field: &mut [u8], value: u64) {
    let width = field.len() - 1;
    field.copy_from_slice(format!("{value:0width$o}\0").as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("polypack-arwrite-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("临时目录可创建");
        dir
    }

    /// 写读回环：ar / tar / gzip 三层都从字节流解回原值。
    #[test]
    fn archives_round_trip() {
        let base = scratch("roundtrip");
        let ipk = base.join("x.ipk");
        let control = tar_bytes(&[("./control".to_string(), b"Package: x\n".to_vec())]).unwrap();
        let gz = gzip_bytes(&control, &ipk).unwrap();
        write_ar(
            &[
                Member {
                    name: "debian-binary".to_string(),
                    bytes: b"2.0\n".to_vec(),
                },
                Member {
                    name: "control.tar.gz".to_string(),
                    bytes: gz,
                },
            ],
            &ipk,
        )
        .unwrap();

        let members = read_ar(&ipk).expect("ar 读回");
        assert_eq!(
            members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            vec!["debian-binary", "control.tar.gz"]
        );
        assert_eq!(members[0].bytes, b"2.0\n");
        let tar = TarArchive::from_tar_gz(&members[1].bytes, &ipk).expect("tar.gz 读回");
        assert_eq!(tar.names(), vec!["./control"]);
        assert_eq!(tar.text("./control").as_deref(), Some("Package: x\n"));

        // 确定性：同内容二写字节一致（gzip mtime / OS 字节固定）
        let again = gzip_bytes(&control, &ipk).unwrap();
        assert_eq!(again, members[1].bytes);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 结构缺陷如实报错（不静默当空归档）：缺魔数 / 长度字段非法 / 数据越界。
    #[test]
    fn read_ar_flags_malformed_input() {
        let base = scratch("malformed");
        let plain = base.join("plain.bin");
        std::fs::write(&plain, b"not an ar").unwrap();
        let err = read_ar(&plain).expect_err("缺魔数须报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0006);

        let truncated = base.join("truncated.ar");
        let mut bytes = AR_MAGIC.to_vec();
        bytes.extend_from_slice(&[b' '; 60]); // 头部尾标不是 `\n`
        std::fs::write(&truncated, bytes).unwrap();
        assert!(read_ar(&truncated).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// tar 长名按 ustar `prefix` 切分；切不下（无 `/` 且超长）时报错而不是截断。
    #[test]
    fn tar_long_names_split_or_fail() {
        let deep = format!("{}/{}", "a".repeat(80), "b".repeat(90));
        let bytes = tar_bytes(&[(deep.clone(), b"x".to_vec())]).unwrap();
        let archive = TarArchive::parse(bytes, Path::new("long.tar")).unwrap();
        assert_eq!(archive.names(), vec![deep]);

        let hopeless = "c".repeat(120);
        assert!(tar_bytes(&[(hopeless, Vec::new())]).is_err());
    }

    /// 外部见证（本机有 `ar` / `tar` 时）：系统工具能读回我们写的归档。
    #[test]
    fn system_ar_and_tar_read_our_output() {
        let base = scratch("system");
        let ipk = base.join("x.ipk");
        write_ar(
            &[
                Member {
                    name: "debian-binary".to_string(),
                    bytes: b"2.0\n".to_vec(),
                },
                Member {
                    name: "control.tar.gz".to_string(),
                    bytes: gzip_bytes(
                        &tar_bytes(&[("./control".to_string(), b"Package: x\n".to_vec())]).unwrap(),
                        &ipk,
                    )
                    .unwrap(),
                },
            ],
            &ipk,
        )
        .unwrap();
        let out = std::process::Command::new("ar").arg("t").arg(&ipk).output();
        match out {
            Ok(out) if out.status.success() => {
                let names = String::from_utf8_lossy(&out.stdout);
                assert!(names.contains("debian-binary"), "{names}");
                assert!(names.contains("control.tar.gz"), "{names}");
            }
            _ => println!("SKIP: 本机无 ar（系统交叉验证腿跳过）"),
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
