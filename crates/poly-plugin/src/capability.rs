//! 能力模型：默认无文件系统、无网络；按 [`PluginGrant`] 显式放行（ROADMAP §4.5）。
//!
//! 文件路径以工程根解析，canonicalize 后再做目录包含判定——路径穿越（`..`）与符号
//! 链接逃逸都在这一步被拒。出网按 host / host:port 精确匹配，`http(s)` 之外一律拒绝。

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use poly_core::{PluginGrant, codes};

use crate::error::PluginError;

/// 单个出网响应正文上限（防止插件把宿主内存当下载盘）。
pub const MAX_RESPONSE_BYTES: u64 = 1 << 20;

/// 一次解析后的能力策略（加载插件时冻结）。
#[derive(Debug, Clone)]
pub struct CapabilityPolicy {
    /// 工程根（相对路径的基准）。
    root: PathBuf,
    /// 只读授权目录（已 canonicalize）。
    fs_read: Vec<PathBuf>,
    /// 可写授权目录（已 canonicalize）。
    fs_write: Vec<PathBuf>,
    /// 出网域名白名单（小写 `host` 或 `host:port`）。
    net: Vec<String>,
}

/// 通过白名单校验的 URL。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedUrl {
    /// 原始 URL。
    pub url: String,
    /// 小写主机名。
    pub host: String,
    /// 端口（缺省按 scheme 取 80/443）。
    pub port: u16,
}

impl CapabilityPolicy {
    /// 冻结授权清单；授权目录必须存在且为目录（非法清单是配置错误，不是插件越权）。
    pub fn new(root: &Path, grant: &PluginGrant) -> Result<Self, PluginError> {
        let root = root.canonicalize().map_err(|err| {
            PluginError::new(codes::E_PLUGIN_0008, format!("工程根不可用: {err}"))
        })?;
        let read = canonical_dirs(&root, &grant.fs_read)?;
        let write = canonical_dirs(&root, &grant.fs_write)?;
        let net = grant
            .net
            .iter()
            .map(|domain| normalize_domain(domain))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            root,
            fs_read: read,
            fs_write: write,
            net,
        })
    }

    /// 读取授权目录内的文件。
    pub fn read(&self, requested: &str) -> Result<Vec<u8>, PluginError> {
        let canonical = self.resolve_existing(requested, "读取")?;
        check_contained(&canonical, &self.fs_read, "读取")?;
        std::fs::read(&canonical)
            .map_err(|err| PluginError::new(codes::E_PLUGIN_0007, format!("读取失败: {err}")))
    }

    /// 写入授权目录内的文件（父路径 canonicalize，最终组件拒绝符号链接）。
    pub fn write(&self, requested: &str, contents: &[u8]) -> Result<u32, PluginError> {
        let requested_path = Path::new(requested);
        if requested_path.as_os_str().is_empty() {
            return Err(denied("写入", "路径为空"));
        }
        let joined = if requested_path.is_absolute() {
            requested_path.to_path_buf()
        } else {
            self.root.join(requested_path)
        };
        let normalized =
            normalize_lexically(&joined).ok_or_else(|| denied("写入", "路径越出工程根"))?;
        let parent = normalized
            .parent()
            .ok_or_else(|| denied("写入", "路径缺少父目录"))?;
        let name = normalized
            .file_name()
            .ok_or_else(|| denied("写入", "路径缺少文件名"))?;
        let canonical_parent = parent
            .canonicalize()
            .map_err(|_| denied("写入", "父目录不可达"))?;
        let target = canonical_parent.join(name);
        check_contained(&target, &self.fs_write, "写入")?;
        // 已存在的目标（含符号链接）先解析再判定：指向授权目录之外的链接直接拒绝。
        if target.symlink_metadata().is_ok() {
            let canonical = target
                .canonicalize()
                .map_err(|_| denied("写入", "目标不可解析（悬空链接？）"))?;
            check_contained(&canonical, &self.fs_write, "写入")?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        // 最后一跳拒绝符号链接（父路径已 canonicalize；父组件换链的竞态留待 cap-std 化）。
        // shortcut: 父目录组件的 TOCTOU 未封闭，插件制品发布前升级 cap-std/openat。
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options
            .open(&target)
            .map_err(|err| denied("写入", &format!("打开失败: {err}")))?;
        file.write_all(contents)
            .map_err(|err| PluginError::new(codes::E_PLUGIN_0007, format!("写入失败: {err}")))?;
        u32::try_from(contents.len())
            .map_err(|_| PluginError::new(codes::E_PLUGIN_0008, "单次写入超过 4 GiB"))
    }

    /// 校验出网 URL：只接受授权域名上的 http(s)，用户信息 / 其他 scheme 一律拒绝。
    pub fn authorize_url(&self, url: &str) -> Result<AuthorizedUrl, PluginError> {
        let uri: ureq::http::Uri = url.parse().map_err(|_| denied("出网", "URL 非法"))?;
        let scheme = uri.scheme_str().unwrap_or_default();
        if !matches!(scheme, "http" | "https") {
            return Err(denied("出网", "只允许 http/https"));
        }
        let authority = uri
            .authority()
            .ok_or_else(|| denied("出网", "URL 缺少主机"))?;
        if authority.as_str().contains('@') {
            return Err(denied("出网", "URL 不允许携带用户信息"));
        }
        let host = uri
            .host()
            .ok_or_else(|| denied("出网", "URL 缺少主机"))?
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let port = uri
            .port_u16()
            .unwrap_or(if scheme == "https" { 443 } else { 80 });
        if !self
            .net
            .iter()
            .any(|grant| grant_matches(grant, &host, port))
        {
            return Err(denied("出网", &format!("域名 {host}:{port} 未授权"))
                .with_hint("在 polypack.toml 的 [[plugins]] net 列表显式加入该域名"));
        }
        Ok(AuthorizedUrl {
            url: url.to_string(),
            host,
            port,
        })
    }

    fn resolve_existing(&self, requested: &str, operation: &str) -> Result<PathBuf, PluginError> {
        let requested_path = Path::new(requested);
        if requested_path.as_os_str().is_empty() {
            return Err(denied(operation, "路径为空"));
        }
        let joined = if requested_path.is_absolute() {
            requested_path.to_path_buf()
        } else {
            self.root.join(requested_path)
        };
        joined
            .canonicalize()
            .map_err(|_| denied(operation, "路径不可达"))
    }
}

/// 稳定拒绝码（进 diagnostics codes 的 `E_PLUGIN_0001` / `E_PLUGIN_0002`）。
fn denied(operation: &str, reason: &str) -> PluginError {
    let code = if operation == "出网" {
        codes::E_PLUGIN_0002
    } else {
        codes::E_PLUGIN_0001
    };
    PluginError::new(code, format!("{operation}拒绝：{reason}"))
}

fn canonical_dirs(root: &Path, dirs: &[String]) -> Result<Vec<PathBuf>, PluginError> {
    dirs.iter()
        .map(|dir| {
            let joined = if Path::new(dir).is_absolute() {
                PathBuf::from(dir)
            } else {
                root.join(dir)
            };
            let canonical = joined.canonicalize().map_err(|err| {
                PluginError::new(
                    codes::E_PLUGIN_0008,
                    format!("授权目录 {dir} 不可用: {err}"),
                )
            })?;
            if !canonical.is_dir() {
                return Err(PluginError::new(
                    codes::E_PLUGIN_0008,
                    format!("授权路径 {dir} 不是目录"),
                ));
            }
            Ok(canonical)
        })
        .collect()
}

fn check_contained(canonical: &Path, dirs: &[PathBuf], operation: &str) -> Result<(), PluginError> {
    if dirs.iter().any(|dir| canonical.starts_with(dir)) {
        return Ok(());
    }
    Err(denied(operation, "目标不在授权目录内"))
}

/// 词法归一化：消掉 `.` / `..`，`..` 越界返回 `None`。
fn normalize_lexically(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => out.push(component),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(part) => out.push(part),
        }
    }
    Some(out)
}

fn normalize_domain(domain: &str) -> Result<String, PluginError> {
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    let (host, port) = match domain.rsplit_once(':') {
        Some((host, port)) => {
            if port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
                return Err(PluginError::new(
                    codes::E_PLUGIN_0008,
                    format!("授权域名端口非法: {domain}"),
                ));
            }
            (host, Some(port))
        }
        None => (domain.as_str(), None),
    };
    let host_ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'));
    if !host_ok || host.starts_with('.') || host.ends_with('.') {
        return Err(PluginError::new(
            codes::E_PLUGIN_0008,
            format!("授权域名非法: {domain}"),
        ));
    }
    let Some(port) = port else {
        return Ok(host.to_string());
    };
    let port = port.parse::<u16>().map_err(|_| {
        PluginError::new(codes::E_PLUGIN_0008, format!("授权域名端口非法: {domain}"))
    })?;
    Ok(format!("{host}:{port}"))
}

fn grant_matches(grant: &str, host: &str, port: u16) -> bool {
    match grant.rsplit_once(':') {
        Some((grant_host, grant_port)) => {
            grant_host == host && grant_port.parse::<u16>() == Ok(port)
        }
        None => grant == host,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(fs_read: &[&str], fs_write: &[&str], net: &[&str]) -> PluginGrant {
        PluginGrant {
            id: "probe".into(),
            path: "probe.wasm".into(),
            fs_read: fs_read.iter().map(|s| (*s).into()).collect(),
            fs_write: fs_write.iter().map(|s| (*s).into()).collect(),
            net: net.iter().map(|s| (*s).into()).collect(),
            sri: None,
            signature: None,
            trust_root: None,
        }
    }

    #[test]
    fn empty_grants_deny_everything() {
        let root = std::env::temp_dir();
        let policy = CapabilityPolicy::new(&root, &grant(&[], &[], &[])).expect("策略可建");
        assert_eq!(
            policy.read("any-file.txt").unwrap_err().code,
            codes::E_PLUGIN_0001
        );
        assert_eq!(
            policy
                .authorize_url("https://example.com/")
                .unwrap_err()
                .code,
            codes::E_PLUGIN_0002
        );
    }

    #[test]
    fn domain_allowlist_is_exact_and_port_aware() {
        let root = std::env::temp_dir();
        let policy = CapabilityPolicy::new(
            &root,
            &grant(&[], &[], &["API.Example.com", "127.0.0.1:8080"]),
        )
        .expect("策略可建");
        assert_eq!(
            policy
                .authorize_url("https://api.example.com/x")
                .unwrap()
                .host,
            "api.example.com"
        );
        assert_eq!(
            policy
                .authorize_url("https://api.example.com.evil/")
                .unwrap_err()
                .code,
            codes::E_PLUGIN_0002
        );
        assert_eq!(
            policy
                .authorize_url("http://127.0.0.1:8081/")
                .unwrap_err()
                .code,
            codes::E_PLUGIN_0002
        );
        assert!(policy.authorize_url("http://127.0.0.1:8080/").is_ok());
        assert_eq!(
            policy
                .authorize_url("ftp://api.example.com/")
                .unwrap_err()
                .code,
            codes::E_PLUGIN_0002
        );
        assert_eq!(
            policy
                .authorize_url("https://user@api.example.com/")
                .unwrap_err()
                .code,
            codes::E_PLUGIN_0002
        );
    }

    #[test]
    fn invalid_grant_is_config_error() {
        let root = std::env::temp_dir();
        assert_eq!(
            CapabilityPolicy::new(&root, &grant(&["definitely-not-a-dir-xyz"], &[], &[]))
                .unwrap_err()
                .code,
            codes::E_PLUGIN_0008
        );
        assert_eq!(
            CapabilityPolicy::new(&root, &grant(&[], &[], &["bad host"]))
                .unwrap_err()
                .code,
            codes::E_PLUGIN_0008
        );
    }
}
