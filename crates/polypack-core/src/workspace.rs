//! M9-E2：`[workspace]` 根配置、成员发现与校验。
//!
//! 工作区根 `polypack.toml` 只声明成员目录（每个成员目录内再放自己的 `polypack.toml`）：
//!
//! ```toml
//! schema = 1
//! [workspace]
//! members = ["apps/a", "apps/b"]
//! ```
//!
//! 成员 id 取成员目录名（如 `apps/a` → `a`）；声明顺序即 `--workspace` 的构建顺序。
//! 无 `[workspace]` 的单应用配置完全不受影响（本模块只在 CLI 传 `--workspace` / `--app`
//! 时装载）。

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::{CONFIG_SCHEMA, PolypackConfig};
use crate::diagnostics::{Diagnostic, codes};
use crate::plan::Project;

/// `[workspace]` 段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSection {
    /// 成员目录列表（相对工作区根；每个目录内必须有 `polypack.toml`）。
    pub members: Vec<String>,
}

/// 工作区根配置：只有 `schema` 与 `[workspace]`，成员各自持有完整应用配置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceConfig {
    /// 配置 schema 版本（缺省为 1）。
    #[serde(default = "default_schema")]
    pub schema: u32,
    /// `[workspace]`。
    pub workspace: WorkspaceSection,
}

fn default_schema() -> u32 {
    CONFIG_SCHEMA
}

impl WorkspaceConfig {
    /// 从 TOML 文本解析并校验 schema 版本。
    pub fn parse(text: &str) -> Result<Self, Diagnostic> {
        let config: Self = toml::from_str(text).map_err(|err| {
            Diagnostic::error(
                codes::E_CFG_0002,
                format!("工作区根 polypack.toml 解析失败: {err}"),
            )
            .with_hint(
                "工作区根配置只写 schema 与 [workspace] members；应用段放在成员目录的 polypack.toml",
            )
        })?;
        if config.schema != CONFIG_SCHEMA {
            return Err(Diagnostic::error(
                codes::E_CFG_0001,
                format!(
                    "不支持的工作区 schema 版本 {}（当前 polypack 支持 {CONFIG_SCHEMA}）",
                    config.schema
                ),
            )
            .with_hint("升级 polypack，或把根配置的 schema 改为受支持版本"));
        }
        Ok(config)
    }

    /// 从文件装载。
    pub fn load(path: &Path) -> Result<Self, Diagnostic> {
        let text = std::fs::read_to_string(path).map_err(|err| {
            Diagnostic::error(
                codes::E_IO_0001,
                format!("无法读取工作区配置 {}: {err}", path.display()),
            )
            .with_hint("确认文件存在且当前用户可读")
        })?;
        Self::parse(&text).map_err(|mut diagnostic| {
            if let Some(hint) = &mut diagnostic.hint {
                *hint = format!("{hint}（当前配置路径: {}）", path.display());
            }
            diagnostic
        })
    }
}

/// 一个已装载的工作区成员。
pub struct WorkspaceApp {
    /// 成员 id（成员目录名）。
    pub id: String,
    /// 成员项目上下文（`root` = 成员目录；状态目录 = 工作区根）。
    pub project: Project,
}

/// 已发现并校验的工作区。
pub struct Workspace {
    /// 工作区根目录（根 `polypack.toml` 所在目录，规范化绝对路径）。
    pub root: PathBuf,
    /// 成员（声明顺序）。
    pub apps: Vec<WorkspaceApp>,
}

impl Workspace {
    /// 按 `--app` 选择成员：`None` = 全部（声明顺序）；未知 id → `polypack::E_WS_0004`。
    pub fn select(&self, app: Option<&str>) -> Result<Vec<&WorkspaceApp>, Diagnostic> {
        match app {
            None => Ok(self.apps.iter().collect()),
            Some(id) => self
                .apps
                .iter()
                .find(|member| member.id == id)
                .map(|member| vec![member])
                .ok_or_else(|| {
                    let known: Vec<&str> =
                        self.apps.iter().map(|member| member.id.as_str()).collect();
                    Diagnostic::error(codes::E_WS_0004, format!("工作区没有成员应用 {id}"))
                        .with_hint(format!("可用成员: {}（id 取成员目录名）", known.join(", ")))
                }),
        }
    }
}

/// 发现工作区：装载根配置、逐个校验成员目录并装载成员配置。
///
/// 校验失败一律给稳定诊断：
/// - `polypack::E_WS_0001` 声明非法（空成员列表 / 绝对路径 / 成员 id 重复）；
/// - `polypack::E_WS_0002` 成员目录或成员 `polypack.toml` 缺失；
/// - `polypack::E_WS_0003` 成员路径越出工作区根（含符号链接逃逸）；
/// - 成员配置自身错误透传 `polypack::E_CFG_0001` / `E_CFG_0002` / `E_IO_0001`。
pub fn discover(config: &Path) -> Result<Workspace, Diagnostic> {
    let absolute = absolute_path(config);
    let root = absolute
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let workspace = WorkspaceConfig::load(&absolute)?;
    if workspace.workspace.members.is_empty() {
        return Err(
            Diagnostic::error(codes::E_WS_0001, "工作区 [workspace].members 不能为空")
                .with_hint("至少列出一个成员目录，如 members = [\"apps/a\"]"),
        );
    }

    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    let mut apps = Vec::with_capacity(workspace.workspace.members.len());
    for member in &workspace.workspace.members {
        let relative = member_relative(member)?;
        let id = relative
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                Diagnostic::error(
                    codes::E_WS_0001,
                    format!("工作区成员路径无法推导应用 id: {member}"),
                )
                .with_hint("成员路径必须指向目录（id 取目录名）")
            })?
            .to_string();
        if let Some(previous) = ids.insert(id.clone(), member.clone()) {
            return Err(Diagnostic::error(
                codes::E_WS_0001,
                format!("工作区成员 id 重复: {id}（{previous} 与 {member}）"),
            )
            .with_hint("成员 id 取目录名：让两个成员目录名不同，或合并为一个成员"));
        }

        let dir = root.join(&relative);
        let canonical = std::fs::canonicalize(&dir).map_err(|_| {
            Diagnostic::error(
                codes::E_WS_0002,
                format!("工作区成员目录不存在: {member}（{}）", dir.display()),
            )
            .with_hint("核对 [workspace].members 中的相对路径")
        })?;
        if !canonical.starts_with(&root) {
            return Err(Diagnostic::error(
                codes::E_WS_0003,
                format!(
                    "工作区成员越出工作区根: {member}（{}）",
                    canonical.display()
                ),
            )
            .with_hint("成员路径必须是工作区根内的相对路径；符号链接也不能指向根外"));
        }
        if !canonical.is_dir() {
            return Err(Diagnostic::error(
                codes::E_WS_0002,
                format!("工作区成员不是目录: {member}"),
            )
            .with_hint("成员路径必须指向含 polypack.toml 的目录"));
        }

        let member_config = canonical.join("polypack.toml");
        if !member_config.is_file() {
            return Err(Diagnostic::error(
                codes::E_WS_0002,
                format!("工作区成员缺少 polypack.toml: {member}"),
            )
            .with_hint("在每个成员目录内提供完整的应用配置（package / ui / core 等）"));
        }
        let config = PolypackConfig::load(&member_config).map_err(|err| {
            let mut diagnostic = err.diagnostic();
            if let Some(hint) = &mut diagnostic.hint {
                *hint = format!("{hint}（成员配置: {}）", member_config.display());
            }
            diagnostic
        })?;

        apps.push(WorkspaceApp {
            id: id.clone(),
            project: Project {
                root: canonical,
                config,
                workspace_root: Some(root.clone()),
                app_id: Some(id),
            },
        });
    }

    Ok(Workspace { root, apps })
}

/// 成员路径规范化：只接受工作区根内的相对路径（`.` 忽略、`..` 不得越顶）。
fn member_relative(member: &str) -> Result<PathBuf, Diagnostic> {
    let raw = Path::new(member);
    if raw.is_absolute() {
        return Err(Diagnostic::error(
            codes::E_WS_0001,
            format!("工作区成员路径必须是相对路径: {member}"),
        )
        .with_hint("写成相对工作区根的路径，如 apps/a"));
    }
    let mut relative = PathBuf::new();
    for component in raw.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !relative.pop() {
                    return Err(Diagnostic::error(
                        codes::E_WS_0003,
                        format!("工作区成员路径越出工作区根: {member}"),
                    )
                    .with_hint("成员路径不得以 .. 越出工作区根"));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(Diagnostic::error(
                    codes::E_WS_0001,
                    format!("工作区成员路径必须是相对路径: {member}"),
                )
                .with_hint("写成相对工作区根的路径，如 apps/a"));
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return Err(
            Diagnostic::error(codes::E_WS_0001, "工作区成员路径不能为空")
                .with_hint("成员必须是工作区根下的子目录"),
        );
    }
    Ok(relative)
}

/// 与 `polypack-cli` 的项目装载同口径：优先规范化，失败时退回「绝对化」。
fn absolute_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| match path.is_absolute() {
        true => path.to_path_buf(),
        false => std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path),
    })
}
