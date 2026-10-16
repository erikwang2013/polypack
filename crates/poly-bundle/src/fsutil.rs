//! 产物文件系统操作：目录树拷贝、体积统计、脚本落盘与 Artifact 汇总。

use std::path::{Path, PathBuf};

use crate::error::{BundleError, io_err};
use crate::packager::{Artifact, ArtifactKind, BundlePlan};

/// 写文本文件（自动建父目录，权限 0644）。
pub fn write_text(path: &Path, contents: &str) -> Result<(), BundleError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    std::fs::write(path, contents).map_err(io_err(path))?;
    normalize_mode(path, 0o644)
}

/// 写文本文件，内容与现文件逐字节相同时不动（保住 mtime——hvigor 增量构建按工程文件
/// 状态判缓存，重写同内容会白白击穿「二跑缓存命中」）。
pub fn write_text_if_changed(path: &Path, contents: &str) -> Result<(), BundleError> {
    if std::fs::read(path).is_ok_and(|current| current == contents.as_bytes()) {
        return Ok(());
    }
    write_text(path, contents)
}

/// 拷贝单个文件（自动建父目录；`exec` 为真时置 0755）。
///
/// 权限按发行包惯例归一：保留可执行位与读位、去掉组 / 其他可写位（构建树常带
/// umask 002 的 664 / 775，deb 与 AppImage 里属非常规权限）。
pub fn copy_file(src: &Path, dst: &Path, exec: bool) -> Result<(), BundleError> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    std::fs::copy(src, dst).map_err(io_err(src))?;
    if exec {
        make_executable(dst)
    } else {
        strip_group_write(dst)
    }
}

/// 递归拷贝目录树（符号链接按链接复制，目录权限 0755）。
pub fn copy_tree(src: &Path, dst: &Path) -> Result<(), BundleError> {
    std::fs::create_dir_all(dst).map_err(io_err(dst))?;
    normalize_mode(dst, 0o755)?;
    for entry in std::fs::read_dir(src).map_err(io_err(src))? {
        let entry = entry.map_err(io_err(src))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ty = entry.file_type().map_err(io_err(&from))?;
        if ty.is_dir() {
            copy_tree(&from, &to)?;
        } else if ty.is_symlink() {
            let target = std::fs::read_link(&from).map_err(io_err(&from))?;
            symlink(&target, &to)?;
        } else {
            copy_file(&from, &to, false)?;
        }
    }
    Ok(())
}

/// 置可执行位（Windows 上为空操作）。
pub fn make_executable(path: &Path) -> Result<(), BundleError> {
    normalize_mode(path, 0o755)
}

/// 去掉组 / 其他可写位（Windows 上为空操作）。
fn strip_group_write(path: &Path) -> Result<(), BundleError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).map_err(io_err(path))?.permissions();
        perms.set_mode(perms.mode() & !0o022);
        std::fs::set_permissions(path, perms).map_err(io_err(path))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// 统一权限位（Windows 上为空操作）。
fn normalize_mode(path: &Path, mode: u32) -> Result<(), BundleError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(io_err(path))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

/// 建符号链接（Windows 上缺权限时降级为拷贝）。
pub fn symlink(target: &Path, link: &Path) -> Result<(), BundleError> {
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).map_err(io_err(link))
    }
    #[cfg(not(unix))]
    {
        let _ = (target, link);
        Err(BundleError::ToolMissing {
            packager: "fsutil",
            tool: "symlink".to_string(),
            hint: "非 Unix 宿主无法创建符号链接（DMG 拖拽布局需 macOS 宿主）".to_string(),
        })
    }
}

/// 目录树内文件字节总和（目录型产物的 `bytes`）。
pub fn tree_bytes(path: &Path) -> Result<u64, BundleError> {
    let mut total = 0u64;
    for entry in std::fs::read_dir(path).map_err(io_err(path))? {
        let entry = entry.map_err(io_err(path))?;
        let meta = entry.metadata().map_err(io_err(&entry.path()))?;
        if meta.is_dir() {
            total += tree_bytes(&entry.path())?;
        } else {
            total += meta.len();
        }
    }
    Ok(total)
}

/// 落盘计划中的全部脚本。
pub fn write_scripts(plan: &BundlePlan) -> Result<(), BundleError> {
    for script in &plan.scripts {
        write_text(&script.path, &script.contents)?;
    }
    Ok(())
}

/// 汇总文件型产物。
pub fn file_artifact(
    target: &str,
    format: &str,
    path: &Path,
    notes: Vec<String>,
) -> Result<Artifact, BundleError> {
    let bytes = std::fs::metadata(path).map_err(io_err(path))?.len();
    Ok(Artifact {
        target: target.to_string(),
        format: format.to_string(),
        path: path.to_path_buf(),
        bytes,
        kind: ArtifactKind::File,
        notes,
    })
}

/// 汇总目录型产物。
pub fn dir_artifact(
    target: &str,
    format: &str,
    path: &Path,
    notes: Vec<String>,
) -> Result<Artifact, BundleError> {
    let bytes = tree_bytes(path)?;
    Ok(Artifact {
        target: target.to_string(),
        format: format.to_string(),
        path: PathBuf::from(path),
        bytes,
        kind: ArtifactKind::Directory,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_tree_and_size() {
        let base = std::env::temp_dir().join(format!("poly-bundle-fs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("src");
        write_text(&src.join("a.txt"), "12345").unwrap();
        write_text(&src.join("nested/b.txt"), "12345").unwrap();
        let dst = base.join("dst");
        copy_tree(&src, &dst).unwrap();
        assert_eq!(tree_bytes(&dst).unwrap(), 10);
        assert!(dst.join("nested/b.txt").is_file());

        write_text(&dst.join("c.bin"), "abc").unwrap();
        make_executable(&dst.join("c.bin")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dst.join("c.bin"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 构建树的 umask 002 权限（664 / 775）不得带进产物。
    #[test]
    fn copy_normalizes_group_writable_modes() {
        let base = std::env::temp_dir().join(format!("poly-bundle-mode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("src");
        write_text(&src.join("data.txt"), "x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(src.join("data.txt"), std::fs::Permissions::from_mode(0o664))
                .unwrap();
            std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o775)).unwrap();
        }
        let dst = base.join("dst");
        copy_tree(&src, &dst).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file = std::fs::metadata(dst.join("data.txt"))
                .unwrap()
                .permissions()
                .mode();
            let dir = std::fs::metadata(&dst).unwrap().permissions().mode();
            assert_eq!(file & 0o777, 0o644, "文件不得组/其他可写");
            assert_eq!(dir & 0o777, 0o755, "目录不得组/其他可写");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn artifacts_report_bytes() {
        let base = std::env::temp_dir().join(format!("poly-bundle-art-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        write_text(&base.join("f"), "1234").unwrap();
        let file = file_artifact("linux", "deb", &base.join("f"), vec![]).unwrap();
        assert_eq!((file.bytes, file.kind), (4, ArtifactKind::File));
        let dir = dir_artifact("linux", "appdir", &base, vec![]).unwrap();
        assert_eq!((dir.bytes, dir.kind), (4, ArtifactKind::Directory));
        let _ = std::fs::remove_dir_all(&base);
    }
}
