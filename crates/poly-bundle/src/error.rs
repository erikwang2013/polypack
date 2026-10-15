//! 打包阶段的稳定错误码与错误类型。
//!
//! 码值与 [`crate::error::codes`] 常量表绑定，格式与 `poly-core::diagnostics::Code`
//! 一致（`poly::E_<模块>_<四位序号>`）；本 crate 不依赖 `poly-core`（保持 ROADMAP §4.1
//! 依赖方向），因此以 `&'static str` 暴露，由 package 阶段转成 `Diagnostic`。

use std::path::PathBuf;

/// 错误码常量表（打包域，新增码在此登记）。
pub mod codes {
    /// 缺少外部打包工具。
    pub const E_BND_0001: &str = "poly::E_BND_0001";
    /// 打包输入缺失。
    pub const E_BND_0002: &str = "poly::E_BND_0002";
    /// 打包输入非法。
    pub const E_BND_0003: &str = "poly::E_BND_0003";
    /// 外部工具执行失败。
    pub const E_BND_0004: &str = "poly::E_BND_0004";
    /// 产物 IO 失败。
    pub const E_BND_0005: &str = "poly::E_BND_0005";
    /// 产物 zip 归档读写失败。
    pub const E_BND_0006: &str = "poly::E_BND_0006";
}

/// 打包失败。
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    /// 宿主缺少必需的外部工具（绝不静默降级）。
    #[error("{packager} 缺少外部工具 {tool}：{hint}")]
    ToolMissing {
        /// 打包器 id。
        packager: &'static str,
        /// 缺失的工具（可执行名）。
        tool: String,
        /// 安装 / 替代建议。
        hint: String,
    },
    /// 必需输入未提供。
    #[error("{packager} 缺少输入：{field}")]
    MissingInput {
        /// 打包器 id。
        packager: &'static str,
        /// 缺失的输入字段名。
        field: &'static str,
    },
    /// 输入值非法（版本号形态、产物名可注入字符等）。
    #[error("{packager} 输入非法：{field} = {value:?}（{reason}）")]
    InvalidInput {
        /// 打包器 id。
        packager: &'static str,
        /// 非法字段名。
        field: &'static str,
        /// 原始值。
        value: String,
        /// 非法原因。
        reason: String,
    },
    /// 外部命令非零退出。
    #[error("外部命令失败（退出码 {status}）：{command}")]
    ToolFailed {
        /// 命令展示形态。
        command: String,
        /// 退出状态。
        status: String,
        /// 诊断所用的 stderr 尾部。
        stderr: String,
    },
    /// 文件系统操作失败。
    #[error("产物 IO 失败：{path}")]
    Io {
        /// 出错路径。
        path: PathBuf,
        /// 底层错误。
        #[source]
        source: std::io::Error,
    },
    /// 产物 zip 归档读写失败（结构非法 / 压缩方式不支持）。
    #[error("产物归档失败：{path}（{reason}）")]
    Archive {
        /// 出错的归档路径。
        path: PathBuf,
        /// 归档器给出的原因。
        reason: String,
    },
}

impl BundleError {
    /// 稳定错误码（见 [`codes`]）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::ToolMissing { .. } => codes::E_BND_0001,
            Self::MissingInput { .. } => codes::E_BND_0002,
            Self::InvalidInput { .. } => codes::E_BND_0003,
            Self::ToolFailed { .. } => codes::E_BND_0004,
            Self::Io { .. } => codes::E_BND_0005,
            Self::Archive { .. } => codes::E_BND_0006,
        }
    }

    /// 修复提示（供 `Diagnostic::with_hint` 使用）。
    pub fn hint(&self) -> Option<&str> {
        match self {
            Self::ToolMissing { hint, .. } => Some(hint),
            Self::MissingInput { field, .. } => Some(match *field {
                "bare" => "先执行 bundle 阶段（T-M1-03）产出裸产物，再进入 package",
                "ui_dist" => "先执行 bundle(ui) 支线（T-M1-04）产出前端 dist",
                "icon_svg" => {
                    "提供品牌 SVG（模板项目为 ui/public/icon.svg，与 docs/assets/pax-mark.svg 同源）"
                }
                "icons(PNG)" | "icons(PNG 256)" => {
                    "先执行图标派生管线（T-M1-14 icons::derive，rsvg-convert 由 SVG 产出 PNG 集）"
                }
                "icons.icns" => {
                    "macOS .app 需要 icns 图标：在 macOS 打包机用 iconutil 从 PNG 派生后作为输入\
                     （T-M1-14 图标派生管线）"
                }
                _ => "核对 polypack.toml 与 JobSpec 的输入声明",
            }),
            Self::InvalidInput { .. } => {
                Some("产品名 / 版本 / 标识只允许字母数字与有限标点（见 ProductInfo::validate）")
            }
            Self::ToolFailed { .. } => Some("查看上方 stderr；打包脚本与命令已落盘，可手动重跑"),
            Self::Io { .. } => Some("检查输出目录权限与磁盘空间"),
            Self::Archive { .. } => Some(
                "归档由本工具链产出（aapt2 / bundletool / zip 组装），结构非法多为工具版本不匹配",
            ),
        }
    }
}

/// 构造 [`BundleError::Io`] 的便捷函数（路径由调用方补齐）。
pub(crate) fn io_err(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> BundleError {
    let path = path.into();
    move |source| BundleError::Io { path, source }
}
