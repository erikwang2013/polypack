//! CLI 侧的项目装载与目录约定（`.polypack/` 下的缓存与产物快照）。

use std::path::{Path, PathBuf};

use poly_core::{Diagnostic, JsonSink, PolypackConfig, Project, codes};

/// 从 `polypack.toml` 装载项目上下文；根目录 = 配置文件所在目录。
pub fn load(config: &Path) -> Result<Project, Diagnostic> {
    let config_model = PolypackConfig::load(config).map_err(|err| {
        let mut diagnostic = err.diagnostic();
        if let Some(hint) = &mut diagnostic.hint {
            *hint = format!("{hint}（当前配置路径: {}）", config.display());
        }
        diagnostic
    })?;
    // 根目录必须绝对：阶段会以「根目录下的子目录」为 cwd 起子进程（cargo），
    // 相对路径在子进程里会相对子进程 cwd 解析，导致产物落到 core/core/target。
    let absolute = std::fs::canonicalize(config).unwrap_or_else(|_| match config.is_absolute() {
        true => config.to_path_buf(),
        false => std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(config),
    });
    let root = absolute
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    Ok(Project {
        root,
        config: config_model,
    })
}

/// 构建缓存根目录：`<root>/.polypack/cache`。
pub fn cache_root(root: &Path) -> PathBuf {
    root.join(".polypack").join("cache")
}

/// 产物快照根目录：`<root>/.polypack/artifacts`。
pub fn artifacts_root(root: &Path) -> PathBuf {
    root.join(".polypack").join("artifacts")
}

/// 打包暂存根目录：`<root>/.polypack/packages`（每次构建重写，构建后被快照收走）。
pub fn packages_root(root: &Path) -> PathBuf {
    root.join(".polypack").join("packages")
}

/// 事件接收端：`--json` 走 JSON Lines，否则人读输出（均写 stdout）；
/// 人读通道额外累计告警数（T-M1-14 结束表情的「待确认」判据）。
pub fn sink(json: bool) -> Box<dyn poly_core::EventSink> {
    if json {
        Box::new(JsonSink {
            out: std::io::stdout(),
        })
    } else {
        Box::new(crate::mood::MoodSink::new(std::io::stdout()))
    }
}

/// IO 失败 → 稳定诊断。
pub fn io_error(action: &str, path: &Path, err: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        codes::E_IO_0001,
        format!("{action} 失败（{}）: {err}", path.display()),
    )
    .with_hint("检查路径是否存在、权限是否足够")
}

/// 递归统计目录字节数与文件数（不存在按 0 计）。
pub fn dir_size(dir: &Path) -> std::io::Result<(u64, u64)> {
    let mut bytes = 0;
    let mut files = 0;
    if !dir.exists() {
        return Ok((0, 0));
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                bytes += entry.metadata()?.len();
                files += 1;
            }
        }
    }
    Ok((bytes, files))
}

/// 人类可读的字节数。
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
