//! 确定性 zip 组装（T-M2-06 / T-M2-07）。
//!
//! Android 产物（APK / AAB 模块）由「aapt2 的 zip + 自补条目」拼成，二跑字节一致取决于
//! 这里：条目按名称排序写入，时间戳固定 1980-01-01（MS-DOS 纪元起点，aapt2 同款）、
//! 权限固定 0644、压缩方式固定 Stored（不引入 zlib 版本差异）。搬运 aapt2 条目走
//! `raw_copy_file`（不解压不重压，保留其时间戳与压缩方式）。
//!
//! shortcut: 时间戳固定为 1980-01-01 而非 `SOURCE_DATE_EPOCH`——zip 表示不了 1980 之前，
//! 且固定值比环境变量更可复现；`SOURCE_DATE_EPOCH` 只注入 aapt2 / java（见 android.rs）。

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::error::{BundleError, io_err};

/// 产物 zip 的固定条目时间戳（1980-01-01 00:00，DOS 时间可表示的最小值）。
const FIXED_TIME: DateTime = DateTime::DEFAULT;

/// 产物 zip 的固定权限位（普通文件；产物不解包执行）。
const FIXED_MODE: u32 = 0o644;

/// 写一个确定性 zip：条目按名称排序，固定时间戳 / 权限，内容取自磁盘文件。
pub fn write_zip(entries: &[(String, PathBuf)], out: &Path) -> Result<(), BundleError> {
    let mut writer = start(out)?;
    add_files(&mut writer, entries)?;
    finish(writer, out)
}

/// 既有 zip 搬运 + 磁盘文件追加：`base` 的条目原样拷贝（保留 aapt2 的时间戳与压缩方式），
/// 命中 `drops` 前缀的条目丢弃、`renames` 里的条目改名，`added` 追加写入。
pub fn extend_zip(
    base: &Path,
    renames: &[(&str, &str)],
    drops: &[&str],
    added: &[(String, PathBuf)],
    out: &Path,
) -> Result<(), BundleError> {
    let mut writer = start(out)?;
    copy_base(&mut writer, base, renames, drops)?;
    add_files(&mut writer, added)?;
    finish(writer, out)
}

/// 列出 zip 内的条目名（结构断言与产物说明用；不上 shell，跨宿主一致）。
pub fn entry_names(path: &Path) -> Result<Vec<String>, BundleError> {
    let file = File::open(path).map_err(io_err(path))?;
    let mut archive = ZipArchive::new(BufReader::new(file)).map_err(|err| zip_err(path, err))?;
    let mut names = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|err| zip_err(path, err))?;
        if !entry.is_dir() {
            names.push(entry.name().map_err(|err| zip_err(path, err))?.into_owned());
        }
    }
    Ok(names)
}

/// 读 zip 内单个条目的文本内容（`None` = 条目不存在；HAP 结构检查的清单解析用）。
pub fn entry_text(path: &Path, name: &str) -> Result<Option<String>, BundleError> {
    let file = File::open(path).map_err(io_err(path))?;
    let mut archive = ZipArchive::new(BufReader::new(file)).map_err(|err| zip_err(path, err))?;
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(zip_err(path, err)),
    };
    let mut text = String::new();
    entry.read_to_string(&mut text).map_err(io_err(path))?;
    Ok(Some(text))
}

/// 建产物文件与 zip 写入器（父目录自动建）。
fn start(out: &Path) -> Result<ZipWriter<BufWriter<File>>, BundleError> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    let file = File::create(out).map_err(io_err(out))?;
    Ok(ZipWriter::new(BufWriter::new(file)))
}

/// 收尾（zip 中央目录 + 落盘）。
fn finish(writer: ZipWriter<BufWriter<File>>, out: &Path) -> Result<(), BundleError> {
    writer
        .finish()
        .map_err(|err| zip_err(out, err))?
        .flush()
        .map_err(io_err(out))
}

/// 追加磁盘文件（按名称排序，保证与调用方枚举顺序无关）。
fn add_files(
    writer: &mut ZipWriter<BufWriter<File>>,
    entries: &[(String, PathBuf)],
) -> Result<(), BundleError> {
    let mut sorted: Vec<&(String, PathBuf)> = entries.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, source) in sorted {
        let mut file = File::open(source).map_err(io_err(source))?;
        writer
            .start_file(name.as_str(), options())
            .map_err(|err| zip_err(source, err))?;
        std::io::copy(&mut file, writer).map_err(io_err(source))?;
    }
    Ok(())
}

/// 搬运既有 zip 的条目（`raw_copy_file_rename`：不解压不重压）。
fn copy_base(
    writer: &mut ZipWriter<BufWriter<File>>,
    base: &Path,
    renames: &[(&str, &str)],
    drops: &[&str],
) -> Result<(), BundleError> {
    let file = File::open(base).map_err(io_err(base))?;
    let mut archive = ZipArchive::new(BufReader::new(file)).map_err(|err| zip_err(base, err))?;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|err| zip_err(base, err))?;
        let name = entry.name().map_err(|err| zip_err(base, err))?.into_owned();
        if entry.is_dir() || drops.iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        let target = renames
            .iter()
            .find(|(from, _)| *from == name)
            .map_or(name, |(_, to)| (*to).to_string());
        writer
            .raw_copy_file_rename(entry, target)
            .map_err(|err| zip_err(base, err))?;
    }
    Ok(())
}

/// 自写条目的固定属性（时间戳 / 权限 / 压缩方式三固定 → 字节可复现）。
fn options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(FIXED_TIME)
        .unix_permissions(FIXED_MODE)
}

/// zip crate 错误 → [`BundleError`]（IO 归 `Io`，归档结构 / 压缩方式归 `Archive`）。
fn zip_err(path: &Path, err: zip::result::ZipError) -> BundleError {
    match err {
        zip::result::ZipError::Io(source) => BundleError::Io {
            path: path.to_path_buf(),
            source,
        },
        other => BundleError::Archive {
            path: path.to_path_buf(),
            reason: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poly-zip-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    fn entries(base: &Path) -> Vec<(String, PathBuf)> {
        let a = base.join("a.bin");
        let b = base.join("b.bin");
        std::fs::write(&a, "AAAA").expect("写夹具");
        std::fs::write(&b, "BBBBBBBBBBBBBBBBBBBB").expect("写夹具");
        vec![("z/b.bin".to_string(), b), ("a.bin".to_string(), a)]
    }

    #[test]
    fn zip_is_byte_identical_across_runs() {
        let base = scratch("repro");
        let list = entries(&base);
        let first = base.join("first.zip");
        let second = base.join("second.zip");
        write_zip(&list, &first).expect("首跑");
        write_zip(&list, &second).expect("二跑");
        assert_eq!(
            std::fs::read(&first).expect("读首跑"),
            std::fs::read(&second).expect("读二跑"),
            "同输入二跑必须字节一致"
        );
    }

    #[test]
    fn entries_are_sorted_with_fixed_timestamp() {
        let base = scratch("sorted");
        let out = base.join("out.zip");
        write_zip(&entries(&base), &out).expect("写 zip");

        let file = File::open(&out).expect("开 zip");
        let mut archive = ZipArchive::new(BufReader::new(file)).expect("读 zip");
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
        assert_eq!(names, ["a.bin", "z/b.bin"], "条目须按名称排序");
        for index in 0..archive.len() {
            let entry = archive.by_index(index).expect("条目");
            assert_eq!(entry.last_modified(), Some(FIXED_TIME), "时间戳须固定");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn extend_copies_renames_and_drops() {
        let base = scratch("extend");
        let list = entries(&base);
        let seed = base.join("seed.zip");
        write_zip(&list, &seed).expect("造底包");

        let extra = base.join("c.bin");
        std::fs::write(&extra, "CCCC").expect("写夹具");
        let out = base.join("extended.zip");
        extend_zip(
            &seed,
            &[("a.bin", "manifest/a.bin")],
            &["z/"],
            &[("c.bin".to_string(), extra)],
            &out,
        )
        .expect("扩展");

        let names = entry_names(&out).expect("条目名");
        assert_eq!(names, ["manifest/a.bin", "c.bin"], "改名 / 丢弃 / 追加");
        let _ = std::fs::remove_dir_all(&base);
    }
}
