//! `Packager` 契约（ROADMAP §4.1）与产物 / 计划类型。
//!
//! 两段式：`plan()` 只生成脚本文本与外部命令（只读输入，不写盘、不执行工具），
//! 供契约测试断言与 `poly plan` 预演；`package()` 落盘脚本、执行命令并汇总 [`Artifact`]。
//! 宿主工具缺失时 `package()` 返回 [`BundleError::ToolMissing`]，绝不静默降级。

use std::path::PathBuf;

use poly_targets::TargetSpec;

use crate::error::BundleError;
use crate::inputs::BundleInputs;

/// 产物形态（AppDir / 静态站点为目录）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    /// 单文件产物（msi / nsis / dmg / deb / AppImage）。
    File,
    /// 目录产物（AppDir / 静态站点）。
    Directory,
}

/// 一次打包的产物。
///
/// 可被 `poly-core` package 阶段直接消费：`format` → `BuildEvent::Packaged.format`，
/// `bytes` → `BuildEvent::Packaged.bytes`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// 目标 id（`TargetSpec.id`）。
    pub target: String,
    /// 实际产物格式（`msi` / `nsis` / `dmg` / `deb` / `appimage` / `appdir` / `static-site`）。
    pub format: String,
    /// 产物路径（文件或目录）。
    pub path: PathBuf,
    /// 产物体积（文件大小；目录为树内文件字节总和）。
    pub bytes: u64,
    /// 产物形态。
    pub kind: ArtifactKind,
    /// 宿主运行时依赖 / 阻塞说明（写入构建报告与产物说明）。
    pub notes: Vec<String>,
}

/// 一条外部命令。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// 可执行名（按 PATH 解析）。
    pub program: String,
    /// 参数列表。
    pub args: Vec<String>,
    /// 工作目录。
    pub cwd: Option<PathBuf>,
    /// 附加环境变量（凭据一律以 `env:NAME` 形式引用，值不进 [`Command::line`]）。
    pub env: Vec<(String, String)>,
}

impl Command {
    /// 构造命令。
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            cwd: None,
            env: Vec::new(),
        }
    }

    /// 指定工作目录。
    pub fn in_dir(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// 附加一个环境变量。
    ///
    /// 值**不得**是凭据明文（凭据经 `CredentialRef` 在签名阶段以内存态注入，见 poly-sign）；
    /// 此处的常规用途是 `SOURCE_DATE_EPOCH` / `ANDROID_HOME` 等构建参数。
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// 展示形态（报告 / 断言用；参数含空格时加引号）。
    pub fn line(&self) -> String {
        let mut out = String::new();
        if let Some(cwd) = &self.cwd {
            out.push_str(&format!("(cd {} && ", cwd.display()));
        }
        out.push_str(&self.program);
        for arg in &self.args {
            out.push(' ');
            if arg.contains(' ') {
                out.push('"');
                out.push_str(arg);
                out.push('"');
            } else {
                out.push_str(arg);
            }
        }
        if self.cwd.is_some() {
            out.push(')');
        }
        out
    }
}

/// 计划中待落盘的脚本 / 配置文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptFile {
    /// 目标路径。
    pub path: PathBuf,
    /// 文本内容。
    pub contents: String,
}

/// 因工具缺失被跳过的可选步骤（如实记录，不伪装完成）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// 缺失的工具。
    pub tool: String,
    /// 跳过原因 / 建议。
    pub reason: String,
}

/// 打包计划：脚本 + 外部命令 + 跳过项。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundlePlan {
    /// 待落盘的脚本（.wxs / .nsi / control / desktop …）。
    pub scripts: Vec<ScriptFile>,
    /// 待执行的外部命令（按序）。
    pub commands: Vec<Command>,
    /// 被跳过的可选步骤。
    pub skipped: Vec<Skipped>,
}

impl BundlePlan {
    /// 空计划。
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一个脚本。
    #[must_use]
    pub fn with_script(mut self, path: impl Into<PathBuf>, contents: impl Into<String>) -> Self {
        self.scripts.push(ScriptFile {
            path: path.into(),
            contents: contents.into(),
        });
        self
    }

    /// 追加一条命令。
    #[must_use]
    pub fn with_command(mut self, command: Command) -> Self {
        self.commands.push(command);
        self
    }

    /// 记录一个跳过项。
    #[must_use]
    pub fn skipping(mut self, tool: impl Into<String>, reason: impl Into<String>) -> Self {
        self.skipped.push(Skipped {
            tool: tool.into(),
            reason: reason.into(),
        });
        self
    }
}

/// 打包器契约（ROADMAP §4.1）。
pub trait Packager: Send + Sync {
    /// 打包器 id（缺省与服务目标同名；共链打包器可另取，见 [`Packager::serves`]）。
    fn id(&self) -> &'static str;
    /// 产物格式（如 `msi` / `deb`；同一目标可有多个打包器）。
    fn format(&self) -> &'static str;
    /// 本打包器是否服务该目标（缺省 = 与 `id` 同名）。
    ///
    /// T-M4-07：Apple 链一条实现服务 `ios` / `tvos` 两目标——[`find`] 以本方法判定
    /// 服务范围，不再要求 `id` 与目标全等（产物格式仍须逐字一致）。
    fn serves(&self, target_id: &str) -> bool {
        target_id == self.id()
    }
    /// 生成打包计划：只读取输入元数据，不写文件系统、不执行外部工具。
    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError>;
    /// 执行打包：落盘脚本 → 执行命令 → 汇总产物。
    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError>;
}

/// 内建打包器集合（M1：msi / nsis / dmg / deb / appimage / static-site；M2：aab / ipa
/// （Apple 链 ios / tvos 共用一条实现，T-M4-07）；M3：小程序族 weapp / alipay / tt / rpk + hap；
/// M4：TV 家族 wgt / ipk / web-app + TV 变体 android-tv / fire-tv（复用 aab 链）；
/// M5：Linux 发行版 rpm / Flatpak / Snap（ubuntu / fedora / mint，T-M5-01/02/03）+
/// BSD 家族 ports / pkg / pkgsrc（openbsd T-M5-07 / freebsd T-M5-06 / netbsd T-M5-08）+
/// Roku 频道 zip（T-M4-15）；M6：AAOS 变体 android-automotive（复用 aab 链，T-M6-11）+
/// VxWorks 库 / 镜像（T-M6-05，两 format 共用同一 kit 构建）+ Zephyr west 模板（T-M6-06）+
/// FreeRTOS QEMU 示例板（T-M6-07）；M7：watchOS 伴生包（T-M7-06，xcodebuild archive → 可签
/// `.app`，复用 M2 Apple 宿主门控 / 远程农场 R3；签名 / 校验复用 ios 链）+ Fitbit `.fba`
/// （T-M7-07）+ Zepp `.zab`（T-M7-08）+ Wear 变体 wearos（复用 aab 链，T-M7-02/03）。
pub fn builtin() -> Vec<Box<dyn Packager>> {
    vec![
        Box::new(crate::packagers::windows::WixMsi),
        Box::new(crate::packagers::windows::Nsis),
        Box::new(crate::packagers::dmg::Dmg),
        Box::new(crate::packagers::ios::Ios),
        Box::new(crate::packagers::linux::Deb),
        Box::new(crate::packagers::linux::AppImage),
        Box::new(crate::packagers::linux::Rpm),
        Box::new(crate::packagers::linux::Flatpak),
        Box::new(crate::packagers::linux::Snap),
        Box::new(crate::packagers::web::StaticSite),
        Box::new(crate::packagers::android::AndroidAab),
        Box::new(crate::packagers::android::AndroidTv("android-tv")),
        Box::new(crate::packagers::android::AndroidTv("fire-tv")),
        Box::new(crate::packagers::android::AndroidAutomotive),
        Box::new(crate::packagers::android::AndroidWear),
        Box::new(crate::packagers::miniapp::WechatMini),
        Box::new(crate::packagers::miniapp::AlipayMini),
        Box::new(crate::packagers::miniapp::DouyinMini),
        Box::new(crate::packagers::miniapp::QuickApp),
        Box::new(crate::packagers::harmony::HarmonyHap),
        Box::new(crate::packagers::tizen::TizenWgt),
        Box::new(crate::packagers::webos::WebOsIpk),
        Box::new(crate::packagers::webtv::VidaaApp),
        Box::new(crate::packagers::webtv::SmartcastApp),
        Box::new(crate::packagers::openbsd::OpenbsdPorts),
        Box::new(crate::packagers::freebsd::FreeBsdPkg),
        Box::new(crate::packagers::pkgsrc::Pkgsrc),
        Box::new(crate::packagers::roku::Roku),
        Box::new(crate::packagers::qnx::QnxImage),
        Box::new(crate::packagers::qnx::QnxLibrary),
        Box::new(crate::packagers::autosar::AutosarAdaptive),
        Box::new(crate::packagers::vxworks::VxWorksLibrary),
        Box::new(crate::packagers::vxworks::VxWorksImage),
        Box::new(crate::packagers::zephyr::ZephyrWest),
        Box::new(crate::packagers::freertos::FreeRtosBoard),
        Box::new(crate::packagers::watchos::Watchos),
        Box::new(crate::packagers::fitbit::Fitbit),
        Box::new(crate::packagers::zepp::Zepp),
    ]
}

/// 按（目标 id，产物格式）查找内建打包器（服务范围经 [`Packager::serves`] 判定）。
pub fn find(target_id: &str, format: &str) -> Option<Box<dyn Packager>> {
    builtin()
        .into_iter()
        .find(|p| p.serves(target_id) && p.format() == format)
}

/// 目标声明的默认打包器（按 `TargetSpec.package.format` 查找）。
pub fn for_spec(spec: &TargetSpec) -> Option<Box<dyn Packager>> {
    find(&spec.id, &spec.package.format)
}
