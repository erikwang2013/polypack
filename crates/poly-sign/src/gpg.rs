//! gpg 可执行文件定位与 Windows MSYS 路径转换。
//!
//! Git for Windows 自带 MSYS 版 gpg，它只接受 POSIX 路径；原生 Rust 进程传入的
//! `C:\...` 会被当成相对路径。这里按 PATH 顺序定位 gpg：命中 MSYS 时把路径参数
//! 转成 `/c/...`，命中原生 Gpg4win 或非 Windows 宿主时原样传递。

use std::ffi::{OsStr, OsString};

/// 返回 `(gpg 程序, 是否需要 POSIX 路径)`。
pub(crate) fn program() -> (OsString, bool) {
    #[cfg(windows)]
    {
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                let candidate = dir.join("gpg.exe");
                if candidate.is_file() {
                    let lower = candidate.to_string_lossy().to_ascii_lowercase();
                    let msys = lower.contains("\\git\\") || lower.contains("\\msys");
                    return (candidate.into_os_string(), msys);
                }
            }
        }
        ("gpg".into(), false)
    }
    #[cfg(not(windows))]
    {
        ("gpg".into(), false)
    }
}

/// 把 Windows 绝对路径转为 MSYS POSIX 形式；其他参数原样返回。
pub(crate) fn path_arg(arg: &OsStr, msys: bool) -> OsString {
    if !msys {
        return arg.to_os_string();
    }
    let text = arg.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\") {
        return OsString::from(format!("//{}", rest.replace('\\', "/")));
    }
    let bytes = text.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        let rest = text[2..].trim_start_matches(['\\', '/']).replace('\\', "/");
        return OsString::from(format!("/{drive}/{rest}"));
    }
    arg.to_os_string()
}

/// 批量转换命令行参数。
pub(crate) fn args(args: &[OsString], msys: bool) -> Vec<OsString> {
    args.iter().map(|arg| path_arg(arg, msys)).collect()
}

#[cfg(test)]
mod tests {
    use super::path_arg;
    use std::ffi::{OsStr, OsString};

    #[test]
    fn converts_drive_and_unc_paths_only_when_msys() {
        assert_eq!(
            path_arg(OsStr::new(r"C:\Users\runner\AppData\Temp\key"), true),
            OsString::from("/c/Users/runner/AppData/Temp/key")
        );
        assert_eq!(
            path_arg(OsStr::new(r"\\server\share\key"), true),
            OsString::from("//server/share/key")
        );
        assert_eq!(
            path_arg(OsStr::new("--homedir"), true),
            OsString::from("--homedir")
        );
        assert_eq!(
            path_arg(OsStr::new(r"C:\key"), false),
            OsString::from(r"C:\key")
        );
    }
}
