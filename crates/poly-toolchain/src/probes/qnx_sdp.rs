//! QNX SDP 探测：`$POLYPACK_QNX_SDP`（显式覆盖，可指 SDP 根 / host / target）或
//! `qnxsdp-env.sh` 导出的 `$QNX_HOST` + `$QNX_TARGET` 对。
//!
//! 命中判据与 HarmonyOS 探测同口径（只认能编译的那部分，不认空壳）：
//! `$QNX_HOST/usr/bin` 下存在 `qcc` 或 `nto*-gcc`，且 `$QNX_TARGET` 有 `usr/` 树。
//! 探测只读；`poly doctor` 缺失时原样渲染 [`MISSING_NOTE`]。

use std::path::{Path, PathBuf};

use crate::probe::{Probe, ProbeResult};

use super::{found_result, missing_result};

/// 缺失时给用户的安装 / 配置提示（doctor 原样渲染）。
pub const MISSING_NOTE: &str = "未找到 QNX SDP：安装 QNX SDP 7.1 后 `source qnxsdp-env.sh`\
    （导出 $QNX_HOST 与 $QNX_TARGET），或设 $POLYPACK_QNX_SDP 指向 SDP 根\
    （形如 /opt/qnx710，含 host/ 与 target/）";

/// QNX SDP 探测器。
pub struct QnxSdp;

impl Probe for QnxSdp {
    fn id(&self) -> &'static str {
        "qnx-sdp"
    }

    fn required_for(&self) -> &'static [&'static str] {
        &["qnx"]
    }

    fn locate(&self) -> ProbeResult {
        match sdp_root() {
            Some(root) => {
                let version = sdp_version(&root);
                found_result(self.id(), self.required_for(), root, version)
            }
            None => missing_result(self.id(), self.required_for(), MISSING_NOTE),
        }
    }
}

/// QNX SDP 的 host / target 对（打包器交给 cargo / QNX 工具链的完整构建面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdpEnv {
    /// SDP 根（含 `host/` 与 `target/`）。
    pub root: PathBuf,
    /// `$QNX_HOST`（`<root>/host/<os>/<arch>`）。
    pub host: PathBuf,
    /// `$QNX_TARGET`（`<root>/target/<ver>`）。
    pub target: PathBuf,
}

/// 探测 QNX SDP 根（= 含 `host/` 与 `target/` 的安装目录）。
///
/// 候选来源顺序：显式覆盖 → `QNX_HOST` + `QNX_TARGET` 对。打包器（poly-cli）与
/// doctor 共用本函数，不另写第二套探测。
pub fn sdp_root() -> Option<PathBuf> {
    sdp_env().map(|env| env.root)
}

/// 探测 QNX SDP 的 host / target 对（构建需要两者，不只根路径）。
///
/// `None` = 无 SDP：调用方按环境槽 SKIP-with-print，绝不伪造交叉产物。
pub fn sdp_env() -> Option<SdpEnv> {
    let explicit: Vec<PathBuf> = ["POLYPACK_QNX_SDP", "QNX_SDP_ROOT", "QNX_SDP"]
        .iter()
        .filter_map(|key| {
            std::env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .collect();
    let host = env_path("QNX_HOST");
    let target = env_path("QNX_TARGET");
    let (host, target) = detect_pair(&explicit, host.as_deref(), target.as_deref())?;
    Some(SdpEnv {
        root: sdp_root_of(&host, &target),
        host,
        target,
    })
}

/// 纯探测核心（不读环境；测试直接喂路径），返回校验过的 host / target 对。
///
/// 接受三种形态：SDP 根（含 `host/*/*` 与 `target/*`）、`host/<os>/<arch>`、`target/<ver>`；
/// 后两者用对侧环境变量或同根候选补齐。
fn detect_pair(
    explicit: &[PathBuf],
    env_host: Option<&Path>,
    env_target: Option<&Path>,
) -> Option<(PathBuf, PathBuf)> {
    let mut pairs: Vec<(PathBuf, PathBuf)> = Vec::new();
    if let (Some(host), Some(target)) = (env_host, env_target) {
        pairs.push((host.to_path_buf(), target.to_path_buf()));
    }
    for root in explicit {
        if root.join("host").is_dir() && root.join("target").is_dir() {
            for host in child_dirs(&root.join("host")) {
                for arch in child_dirs(&host) {
                    for target in child_dirs(&root.join("target")) {
                        pairs.push((arch.clone(), target));
                    }
                }
            }
        } else if is_host(root) {
            let target = env_target
                .map(Path::to_path_buf)
                .or_else(|| root.ancestors().nth(3).map(|sdp| sdp.join("target")))
                .and_then(|dir| {
                    if is_target(&dir) {
                        Some(dir)
                    } else {
                        child_dirs(&dir).into_iter().find(|child| is_target(child))
                    }
                });
            if let Some(target) = target {
                pairs.push((root.to_path_buf(), target));
            }
        } else if is_target(root) {
            let host = env_host.map(Path::to_path_buf).or_else(|| {
                root.ancestors()
                    .nth(2)
                    .map(|sdp| sdp.join("host"))
                    .and_then(|dir| {
                        child_dirs(&dir)
                            .into_iter()
                            .flat_map(|os| child_dirs(&os))
                            .find(|arch| is_host(arch))
                    })
            });
            if let Some(host) = host {
                pairs.push((host, root.to_path_buf()));
            }
        }
    }
    pairs
        .into_iter()
        .find(|(host, target)| is_host(host) && is_target(target))
}

/// 测试用的根视图（由 host / target 对反推）。
#[cfg(test)]
fn detect(
    explicit: &[PathBuf],
    env_host: Option<&Path>,
    env_target: Option<&Path>,
) -> Option<PathBuf> {
    detect_pair(explicit, env_host, env_target).map(|(host, target)| sdp_root_of(&host, &target))
}

/// host 命中判据：`usr/bin` 有 QNX 编译器（`qcc` 或 `nto*-gcc`）。
fn is_host(dir: &Path) -> bool {
    let bin = dir.join("usr/bin");
    let Ok(entries) = std::fs::read_dir(&bin) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name == "qcc" || (name.starts_with("nto") && name.ends_with("-gcc"))
    })
}

/// target 命中判据：`usr/` 树在场（QNX_TARGET 指向 `.../target/qnx7`）。
fn is_target(dir: &Path) -> bool {
    dir.join("usr").is_dir()
}

/// 由 host / target 反推 SDP 根：`<root>/host/<os>/<arch>` 上溯三级；
/// 回退 `<root>/target/<ver>` 上溯两级。
fn sdp_root_of(host: &Path, target: &Path) -> PathBuf {
    if host.join("usr/bin").is_dir()
        && let Some(root) = host.ancestors().nth(3)
    {
        return root.to_path_buf();
    }
    target
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| target.to_path_buf())
}

/// 版本：`$QNX_VERSION` 优先；否则取 SDP 根目录名（如 `qnx710`）。
fn sdp_version(root: &Path) -> Option<String> {
    std::env::var("QNX_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            root.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
}

/// 目录下的子目录（排序保证探测结果确定）。
fn child_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// 非空环境变量 → 路径。
fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// 临时 SDP 骨架：返回 (根, host, target)。
    fn fixture(compiler: Option<&str>) -> (PathBuf, PathBuf, PathBuf) {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("poly-qnx-probe-{}-{seq}", std::process::id()));
        let host = root.join("host/linux/x86_64");
        let target = root.join("target/qnx7");
        std::fs::create_dir_all(host.join("usr/bin")).unwrap();
        std::fs::create_dir_all(target.join("usr/include")).unwrap();
        if let Some(name) = compiler {
            std::fs::write(host.join("usr/bin").join(name), b"").unwrap();
        }
        (root, host, target)
    }

    #[test]
    fn detects_standard_sdp_layout_from_root() {
        let (root, _host, _target) = fixture(Some("qcc"));
        assert_eq!(
            detect(std::slice::from_ref(&root), None, None),
            Some(root.clone())
        );
        // host / target 单独给也能反推根
        assert_eq!(
            detect(
                &[],
                Some(&root.join("host/linux/x86_64")),
                Some(&root.join("target/qnx7"))
            ),
            Some(root.clone())
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn rejects_host_without_compiler() {
        let (root, host, target) = fixture(None);
        assert_eq!(detect(&[], Some(&host), Some(&target)), None);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn accepts_nto_prefixed_gcc() {
        let (root, host, target) = fixture(Some("ntoaarch64-gcc"));
        assert_eq!(detect(&[], Some(&host), Some(&target)), Some(root.clone()));
        std::fs::remove_dir_all(root).ok();
    }
}
