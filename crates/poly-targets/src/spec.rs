//! `TargetSpec`：一个目标平台的完整能力声明（ROADMAP §4.3）。

use serde::{Deserialize, Serialize};

/// 可发起构建的宿主平台。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Host {
    /// Windows。
    Win,
    /// Linux。
    Linux,
    /// macOS。
    Macos,
    /// BSD（FreeBSD / OpenBSD / NetBSD / DragonFly）。
    Bsd,
}

/// 核心运行方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    /// 编译为原生库（.so / .a / .dylib）。
    Native,
    /// 编译为 wasm，进宿主沙箱。
    Wasm,
    /// 编译为静态库（嵌入式 / 车机）。
    Staticlib,
    /// 无核心代码（纯平台模板）。
    None,
}

/// Rust 核心与宿主语言之间的绑定桥。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Bridge {
    /// uniffi（Kotlin / Swift）。
    Uniffi,
    /// napi-rs（Node / Taro）。
    Napi,
    /// wasm-bindgen（Web / WebView）。
    WasmBindgen,
    /// cffi（QNX / RTOS 等）。
    Cffi,
    /// Tauri IPC（桌面，核心即宿主）。
    TauriIpc,
    /// 无跨语言桥。
    None,
}

/// 打包器与产物格式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PackageSpec {
    /// 产物格式（如 `apk` / `ipa` / `hap` / `wgt`）。
    pub format: String,
    /// 打包器标识（如 `android-aapt2`）。
    pub builder: String,
}

/// 一个目标平台的完整声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TargetSpec {
    /// 目标标识（= `[targets.*]` 表名）。
    pub id: String,
    /// 支持等级（L1–L7 对应 1–7）。
    pub tier: u8,
    /// 可发起构建的宿主。
    pub hosts: Vec<Host>,
    /// 核心运行方式。
    pub runtime: Runtime,
    /// 绑定桥。
    pub bridge: Bridge,
    /// 渲染适配器列表（如 `react-dom` / `react-native`）。
    pub ui: Vec<String>,
    /// 打包器与产物格式。
    pub package: PackageSpec,
    /// 凭据档案引用（密钥库条目名，绝不存明文）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing: Option<String>,
    /// 产物校验器列表。
    #[serde(default)]
    pub verify: Vec<String>,
    /// 发布渠道客户端列表。
    #[serde(default)]
    pub publish: Vec<String>,
    /// 是否受限 / 实验性（进入文档与 CI 标注）。
    #[serde(default)]
    pub experimental: bool,
    /// 边界与降级说明。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// 目标声明校验的失败原因。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpecError {
    /// 目标 id 不符合 `^[a-z][a-z0-9-]*$`。
    #[error("非法的目标 id: {0}")]
    InvalidId(String),
    /// tier 不在 1–7。
    #[error("tier 越界: {0}（应为 1–7）")]
    TierOutOfRange(u8),
    /// 缺少打包格式或构建器。
    #[error("目标缺少打包格式或构建器: {0}")]
    MissingPackage(String),
    /// 未声明任何宿主。
    #[error("目标未声明宿主: {0}")]
    MissingHost(String),
    /// 目标不存在。
    #[error("目标不存在: {0}")]
    UnknownTarget(String),
    /// 目标在当前环境不可达（宿主 / 授权 / 凭据）。
    #[error("目标不可达: {0}（{1}）")]
    Unreachable(String, String),
    /// 目标 id 重复。
    #[error("目标重复: {0}")]
    Duplicate(String),
}

/// 目标 id 的合法形态：小写字母开头，仅含小写字母 / 数字 / 连字符。
pub fn id_is_valid(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

impl TargetSpec {
    /// 最小合法目标（native / uniffi、空 UI、占位打包器）：测试与插件占位用。
    ///
    /// 只固化 id / tier / hosts 三项，其余字段留给调用方按需覆盖。
    pub fn new_stub(id: &str, tier: u8, hosts: &[Host]) -> Self {
        Self {
            id: id.to_string(),
            tier,
            hosts: hosts.to_vec(),
            runtime: Runtime::Native,
            bridge: Bridge::Uniffi,
            ui: Vec::new(),
            package: PackageSpec {
                format: "stub".to_string(),
                builder: "stub".to_string(),
            },
            signing: None,
            verify: Vec::new(),
            publish: Vec::new(),
            experimental: false,
            notes: None,
        }
    }

    /// 基础校验：id 形态、tier 范围、宿主与打包字段非空。
    pub fn validate(&self) -> Result<(), SpecError> {
        if !id_is_valid(&self.id) {
            return Err(SpecError::InvalidId(self.id.clone()));
        }
        if !(1..=7).contains(&self.tier) {
            return Err(SpecError::TierOutOfRange(self.tier));
        }
        if self.hosts.is_empty() {
            return Err(SpecError::MissingHost(self.id.clone()));
        }
        if self.package.format.trim().is_empty() || self.package.builder.trim().is_empty() {
            return Err(SpecError::MissingPackage(self.id.clone()));
        }
        Ok(())
    }
}
