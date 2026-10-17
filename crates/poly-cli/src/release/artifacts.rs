//! 产物定位：按仓库扩展名递归收集，并给跨平台稳定的相对名。

use std::path::{Path, PathBuf};

/// 递归收集指定扩展名的产物（相对名 + 绝对路径）。
pub(super) fn collect_artifacts(
    dir: &Path,
    extension: &str,
    base: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_artifacts(&path, extension, base, out)?;
        } else if path.extension().is_some_and(|ext| ext == extension) {
            out.push((relative_name(base, &path), path));
        }
    }
    Ok(())
}

/// 相对产物目录的 `/` 分隔名（跨平台摘要稳定）。
fn relative_name(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
