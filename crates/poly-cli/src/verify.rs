//! `poly verify`：产物校验。两条通路共用一个命令——
//!
//! - **声明校验器**（缺省）：按 [`TargetSpec::verify`] 注册表逐目标跑已接入的校验器
//!   （`poly build` 的 `verify` 阶段同源，[`VerifyStage`]）。M2 接入 ios 的
//!   `codesign-verify` + `spctl`（同一份 [`poly_sign::ios::plan_verify`]：`codesign --verify
//!   --deep --strict` + `spctl --assess`）；T-M3-04 接入 harmonyos 的 `hap-sign-verify`
//!   （[`poly_sign::harmony::verify`] 的 hap-sign-tool `verify-app`）与 `hap-structure-check`
//!   （[`poly_bundle::check_hap_structure`]，与打包器同一实现）；未接入的 id 如实点名归属，
//!   绝不静默假绿。
//! - **`--reproducible`**：同输入二跑的可复现比对（T-M2-06 验收口径），一个「校验器」的
//!   实现，不进 DAG（比一次要两跑，太贵，只在显式要求时跑）。
//!
//! 口径（最懒且够用）：只比对**本机宿主可达**的目标（`spec.hosts` 含 `current_host()`）；
//! 同一份输入组装两次、落到两个不同的绝对目录
//! （`<root>/.polypack/repro/<target>/{a,b}`），各自 `package()` 后比产物摘要：
//! 文件产物的 `sha384-<base64>`（SRI），目录产物逐文件比（`sri::manifest_of`）；
//! 打包器把一次构建拆成多件落盘时（Android：未签名 APK + AAB + .apks），同目录的其余
//! 顶层文件也逐件比——T-M2-06 的验收字面要求「AAB 与 APK」都字节一致。
//!
//! 跳过语义与 `package` 阶段一致（宿主缺打包工具 / 输入未就绪 → 告警跳过并说明，不硬失败）；
//! 但**全部目标都跳过**时以 `poly::E_VERIFY_0002` 报错——「一个都没比却报绿」是假绿。
//! 已签名的 zip 产物在比对前如实说明并跳过该件（签名会改写 zip 字节，比了也不是「输入决定
//! 产物」的复现性）；`SOURCE_DATE_EPOCH` 未设置时先告警：含时间戳的产物二跑可能不一致。
//!
//! 输入组装与 `poly build` 的 package 阶段共用同一份实现（`crate::package::assemble_inputs`），
//! 「verify 比对的两跑」才和「真实构建的一跑」口径相同；`Ok(None)` = 环境性缺失
//! （该函数已发告警并清掉暂存目录），本命令跳过该目标。

use std::path::{Path, PathBuf};

use poly_bundle::error::BundleError;
use poly_bundle::packager::{Artifact, ArtifactKind};
use poly_core::diagnostics::{Code, Diagnostic};
use poly_core::events::{BuildEvent, EventSink};
use poly_core::job::JobSpec;
use poly_core::plan::{Stage, StageContext, StageError, StageOutcome};
use poly_core::{Profile, Project, codes};
use poly_targets::provider::ProviderError;
use poly_targets::{Host, TargetRequest, TargetSpec, current_host};

use crate::project;

/// 签名块判定：`META-INF/` 下的 `*.SF` / `*.RSA` / `*.DSA` / `*.EC`（zip 签名约定）。
const SIGNATURE_EXTENSIONS: [&str; 4] = [".SF", ".RSA", ".DSA", ".EC"];

/// 小程序族产物目录名（`<packages>/<target>/miniapp/`，与 poly-bundle 同口径）。
const MINIAPP_DIR: &str = "miniapp";
/// 代码包目录名（`<miniapp>/dist/`，同 poly-bundle 口径）。
const DIST_DIR: &str = "dist";

/// `poly verify` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 工程配置路径。
    #[arg(long, default_value = "polypack.toml")]
    pub config: PathBuf,
    /// 可复现比对：同一输入二跑，逐产物比 SRI 摘要（T-M2-06）；缺省跑目标声明的校验器。
    #[arg(long)]
    pub reproducible: bool,
    /// 目标 id（可重复；缺省 = 当前宿主可达的全部已声明目标）。
    #[arg(long = "target", value_name = "ID")]
    pub targets: Vec<String>,
    /// 全部目标都按环境槽跳过时仍以 0 退出（CI 用；跳过原因照常逐条打印，
    /// job summary 据告警如实列出；默认行为不变——全跳过是假绿，报 E_VERIFY_0002）。
    #[arg(long)]
    pub allow_skipped: bool,
}

/// 执行 `poly verify`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let project = project::load(&args.config)?;
    // T-M8-05：目标表与 `poly build` 同源（官方目标 + `[[plugins]]` 插件）。
    let registry = crate::providers::target_registry(&project)?;
    let Some(host) = current_host() else {
        return Err(Diagnostic::error(
            codes::E_VERIFY_0002,
            "无法识别本机宿主：校验只对本机宿主目标成立（交叉目标的校验走远程农场，T-M2-13）",
        ));
    };
    let hosts = vec![host];
    let request = if args.targets.is_empty() || args.targets.iter().any(|id| id == "all") {
        TargetRequest::All
    } else {
        TargetRequest::Explicit(args.targets.clone())
    };
    let job = project
        .config
        .to_job(&registry, &request, Profile::Local, &hosts)
        .map_err(|err| err.diagnostic)?;

    let mut events = project::sink(json);
    if !args.reproducible {
        // 声明校验器通路（`poly build` 的 verify 阶段同源实现）
        let verified = verify_targets(&project, &job.targets, host, events.as_mut())?;
        if verified == 0 {
            if args.allow_skipped {
                warning(
                    events.as_mut(),
                    &format!(
                        "{} 个入选目标都按环境槽 SKIP-with-print（--allow-skipped：不硬红，\
                         跳过原因见上；CI 须在 job summary 如实列出 SKIP）",
                        job.targets.len()
                    ),
                );
                return Ok(0);
            }
            return Err(Diagnostic::error(
                codes::E_VERIFY_0002,
                format!(
                    "{} 个入选目标都没有已接入的校验器可跑（逐目标原因见上方告警）",
                    job.targets.len()
                ),
            )
            .with_hint(
                "校验器按 TargetSpec.verify 声明注册：ios 的 codesign-verify / spctl、harmonyos 的 \
                 hap-sign-verify / hap-structure-check、quickapp 的 hap-toolkit-lint、web 系 TV 的 \
                 wgt-lint / ares-package / sandbox-selfcheck / sideload-structure-check、\
                 OpenBSD 的 portcheck、FreeBSD 的 pkg-check 已接入；\
                 其余 id 的归属见告警，可先用 poly verify --reproducible 做可复现比对",
            ));
        }
        return Ok(0);
    }
    reproducible(&project, &job, host, json, events.as_mut())
}

/// `--reproducible` 通路：同输入二跑比对（T-M2-06 验收口径）。
fn reproducible(
    project: &Project,
    job: &poly_core::JobSpec,
    host: Host,
    json: bool,
    events: &mut dyn EventSink,
) -> Result<i32, Diagnostic> {
    if std::env::var_os("SOURCE_DATE_EPOCH").is_none() {
        warning(
            events,
            "SOURCE_DATE_EPOCH 未设置：含时间戳的产物在二跑间可能不一致，比对结果会如实报出差异（设 `export SOURCE_DATE_EPOCH=<秒>` 后重跑）",
        );
    }

    let mut compared = 0usize;
    for spec in &job.targets {
        if !spec.hosts.contains(&host) {
            warning(
                events,
                &format!(
                    "目标 {} 未做复现比对：非本机宿主（交叉打包的复现比对归 M2 之后）",
                    spec.id
                ),
            );
            continue;
        }
        if compare(project, spec, host, json, events)? {
            compared += 1;
        }
    }
    if compared == 0 {
        return Err(Diagnostic::error(
            codes::E_VERIFY_0002,
            format!(
                "{} 个入选目标都没有可复现比对的产物（逐目标跳过原因见上方告警）",
                job.targets.len()
            ),
        )
        .with_hint(
            "先跑 poly build（本机宿主目标）把打包输入就绪，或补上缺失的打包工具；\
             也可用 --target 只比已就绪的目标",
        ));
    }
    Ok(0)
}

// ---------------------------------------------------------------- 声明校验器（verify 阶段）

/// 已接入的校验器实现。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verifier {
    /// `codesign --verify --deep --strict` + `spctl --assess`（[`poly_sign::ios::plan_verify`]，
    /// 一次执行同时给出 `TargetSpec.verify` 里 `codesign-verify` 与 `spctl` 两个 id 的判决）。
    IosCodesign,
    /// hap-sign-tool `verify-app`（[`poly_sign::harmony::verify`]，T-M3-04）。
    HapSign,
    /// HAP 结构检查（[`poly_bundle::check_hap_structure`]，与 HAP 打包器同一实现）。
    HapStructure,
    /// 快应用 `.rpk` 的 hap-toolkit lint（validate.js 的 `validateJson` + rpk 结构 /
    /// 源清单交叉检查，T-M3-12；注意与上面的 HarmonyOS `Hap*` 两个变体不同链）。
    QuickAppLint,
    /// Tizen `.wgt` 校验（[`poly_bundle::check_wgt_structure`] 的内建结构断言 + 在场时的
    /// 官方 `wgt-lint`，T-M4-08）。
    WgtLint,
    /// webOS `.ipk` 校验（[`poly_bundle::check_ipk_structure`] 的内建结构断言 + 在场时的
    /// 官方 `ares-package` 对暂存载荷真跑一遍，T-M4-09）。
    AresPackage,
    /// VIDAA / Vizio 应用包的沙箱自检（[`poly_bundle::check_web_app_structure`] 三条规则：
    /// 入口文件 / 清单字段 / 体积，T-M4-10；体积上限走 `[budget].size` 同一门禁）。
    SandboxSelfcheck,
    /// OpenBSD ports 骨架校验（[`poly_bundle::check_ports_structure`] 的内建结构断言 +
    /// 在场时的官方 `portcheck -N`，T-M5-07）。
    Portcheck,
    /// FreeBSD pkg 校验（[`poly_bundle::check_pkg_structure`] 的内建结构断言 + 在场时的
    /// 官方 `pkg info -F` / 已安装时的 `pkg check`，T-M5-06）。
    PkgCheck,
    /// NetBSD / DragonFly pkgsrc 骨架校验（[`poly_bundle::check_pkgsrc_structure`] 的内建
    /// 结构断言 + 在场时的 `bmake package`，T-M5-08）。
    PkgsrcCheck,
    /// Roku 频道 zip 的侧载结构自检（[`poly_bundle::check_roku_structure`]，与打包器同一实现；
    /// 真机侧载腿本机不可得，按 T-M4-12 决策 SKIP-with-print，T-M4-15）。
    RokuSideload,
    /// KaiOS 包 zip 结构自检（[`poly_bundle::check_kaios_structure`]：`manifest.webapp`
    /// 必填字段 + 入口存在，与打包器同一实现；设备 / KaiStore 腿本机不可得，SKIP-with-print）。
    KaiosStructure,
    /// TikTok Minis zip 结构自检（[`poly_bundle::check_tiktok_minis_structure`]：
    /// `minis.config.json` / `minis.manifest.json`；平台沙箱腿 SKIP-with-print）。
    TiktokMinisStructure,
    /// Facebook Instant Games zip 结构自检
    /// （[`poly_bundle::check_facebook_instant_games_structure`]：`fbapp-config.json`；
    /// Meta 沙箱腿 SKIP-with-print）。
    FacebookInstantGamesStructure,
    /// Fitbit `.fba` 三层结构自检（[`poly_bundle::check_fitbit_structure`]）+ 在场时的
    /// `fitbit-build --help` 自检；CLI 缺席 SKIP-with-print，T-M7-07。
    FitbitStructure,
    /// Zepp `.zab` 结构自检（[`poly_bundle::check_zepp_structure`]：外层清单 / zpk md5 /
    /// device.zip）+ 在场时的 `zeus --version`；模拟器安装腿不可得，SKIP-with-print，T-M7-08。
    ZeppStructure,
    /// deb 结构检查 + 在场时的官方 lintian（`--fail-on error`），T-M5-01。
    DebLint,
    /// rpm 结构检查 + 在场时的官方 rpmlint，T-M5-01。
    RpmLint,
    /// AppImage / AppDir 结构检查（[`poly_bundle::check_appdir_structure`]），T-M5-04。
    AppimageStructure,
    /// Flatpak manifest + 载荷 + 最小权限集检查，T-M5-02。
    FlatpakLint,
    /// Snap snapcraft.yaml + 接口声明 + 载荷检查，T-M5-03。
    SnapLint,
    /// AppStream 元数据（内建结构检查 + 在场时的 appstreamcli），T-M5-01/04。
    AppstreamValidate,
    /// PWA 三腿审计（manifest / 离线能力 / 安装性；[`poly_bundle::audit_pwa`]），T-M5-10。
    /// 真浏览器腿（Chrome `Page.getInstallabilityErrors` + 断网重载）由
    /// `ui/smoke/pwa/audit.mjs` 另跑，Chrome 缺席时 SKIP-with-print。
    PwaAudit,
    /// QNX 镜像载荷结构检查（[`poly_bundle::check_qnx_structure`]，与打包器同一实现；
    /// 无 SDP 时打包按环境槽跳过，校验侧如实点名无产物），T-M6-03。
    QnxImageStructure,
    /// AUTOSAR ARXML 清单：结构断言 + 服务 / 端口与 C ABI 一一对应 + 在场时的
    /// `xmllint --schema` 真跑（缺席打印 SKIP），T-M6-04。
    ArxmlSchema,
    /// 静态库归档的 `poly_engine_*` 导出符号与 C ABI 清单对照（`nm -g --defined-only`；
    /// nm 缺席时退化为声明侧结构断言并打印 SKIP）。QNX/AUTOSAR 与 VxWorks/Zephyr 共用
    /// 同一 id，按目标分发到对应实现，T-M6-03/04/05/06。
    CAbiSymbols,
    /// VxWorks kit 结构断言 + 在场时 `wr-cc` 真链（T-M6-05；工具链缺席 SKIP-with-print）。
    VxWorksLd,
    /// Zephyr west 模板结构 + 在场时两板 `west build`（T-M6-06；环境缺席 SKIP-with-print）。
    ZephyrWestBuild,
    /// FreeRTOS mps2-an385 板级包结构 + 在场时 QEMU 冒烟（T-M6-07；工具链 / 网络缺席
    /// SKIP-with-print）。
    FreeRtosQemuSmoke,
    /// QNX 镜像 QEMU 冒烟（T-M6-09）：SDP / mkifs / QEMU 齐备时编译自检程序、出 `.ifs`
    /// 并启动；缺席 SKIP-with-print。判定与日志归档与 Zephyr 腿共用 `smoke` 模块。
    QnxQemuSmoke,
    /// Zephyr `west build -t run` 冒烟（T-M6-10）：两板退出码断言 + 具名步骤；
    /// west / ZEPHYR_BASE / SDK 缺席 SKIP-with-print。
    ZephyrQemuSmoke,
}

/// 校验器 id → 已接入实现（`None` = 未接入，归属查 [`verifier_owner`]）。
fn verifier_for(id: &str) -> Option<Verifier> {
    match id {
        "codesign-verify" | "spctl" => Some(Verifier::IosCodesign),
        "hap-sign-verify" => Some(Verifier::HapSign),
        "hap-structure-check" => Some(Verifier::HapStructure),
        "hap-toolkit-lint" => Some(Verifier::QuickAppLint),
        "wgt-lint" => Some(Verifier::WgtLint),
        "ares-package" => Some(Verifier::AresPackage),
        "sandbox-selfcheck" => Some(Verifier::SandboxSelfcheck),
        "portcheck" => Some(Verifier::Portcheck),
        "pkg-check" => Some(Verifier::PkgCheck),
        "pkgsrc-check" => Some(Verifier::PkgsrcCheck),
        "sideload-structure-check" => Some(Verifier::RokuSideload),
        "kaios-structure-check" => Some(Verifier::KaiosStructure),
        "tiktok-minis-structure-check" => Some(Verifier::TiktokMinisStructure),
        "facebook-instant-games-structure-check" => Some(Verifier::FacebookInstantGamesStructure),
        "fitbit-structure-check" => Some(Verifier::FitbitStructure),
        "zepp-structure-check" => Some(Verifier::ZeppStructure),
        "lintian" => Some(Verifier::DebLint),
        "rpmlint" => Some(Verifier::RpmLint),
        "appimage-structure" => Some(Verifier::AppimageStructure),
        "flatpak-lint" => Some(Verifier::FlatpakLint),
        "snap-lint" => Some(Verifier::SnapLint),
        "appstream-validate" => Some(Verifier::AppstreamValidate),
        "pwa-audit" => Some(Verifier::PwaAudit),
        "qnx-image-structure" => Some(Verifier::QnxImageStructure),
        "arxml-schema" => Some(Verifier::ArxmlSchema),
        "c-abi-symbols" => Some(Verifier::CAbiSymbols),
        "vxworks-ld-check" => Some(Verifier::VxWorksLd),
        "zephyr-west-build" => Some(Verifier::ZephyrWestBuild),
        "freertos-qemu-smoke" => Some(Verifier::FreeRtosQemuSmoke),
        "qnx-qemu-smoke" => Some(Verifier::QnxQemuSmoke),
        "zephyr-qemu-smoke" => Some(Verifier::ZephyrQemuSmoke),
        _ => None,
    }
}

/// 未接入校验器的归属与替代路径；`None` = 注册表里出现了没人认领的 id（如新增目标忘了接线）。
fn verifier_owner(id: &str) -> Option<&'static str> {
    match id {
        "apksigner-verify" => Some(
            "poly-sign 的 `AndroidSigner::sign` 已在签名后内联 `apksigner verify --print-certs` \
             证据（T-M2-09）；独立校验器入 DAG 归 M2 之后",
        ),
        "bundletool-validate" => {
            Some("bundletool validate 归 T-M2-07（打包器侧已跑，入 DAG 未接线）")
        }
        "stapler-validate" => {
            Some("poly-sign 有 `plan_staple` 的 validate 半步（T-M2-10）；独立校验器未接线")
        }
        "signtool-verify" => Some("Windows 链（M2 之后）"),
        "html-validate" | "sri-check" => Some("Web 链（M2 之后；SRI 清单已由打包器产出）"),
        "wechat-devtools-cli" | "alipay-devtools-cli" | "douyin-devtools-cli" => {
            Some("小程序开发者工具 CLI 校验（T-M3-11；本机无 devtools 时 SKIP-with-print）")
        }
        "iq-structure-check" => Some(
            "Garmin `.iq` 工程 / 包结构自检（T-M7-09 Node spike 已实现：manifest / jungle / \
             strings / Monkey C 子集；Rust 校验器接线归 T-M7-10）",
        ),
        _ => None,
    }
}

/// `verify` 阶段：按 `TargetSpec.verify` 注册表逐目标跑已接入的校验器。
///
/// 宿主门控与 `package` 阶段同判据：只校验「本机宿主目标」，交叉目标的校验走远程农场
/// （T-M2-13）。全部目标都跳过时如实返回 `Skipped`（原因里点名），绝不假装校验过。
pub struct VerifyStage;

impl Stage for VerifyStage {
    fn name(&self) -> &'static str {
        "verify"
    }

    fn plan(&self, _job: &JobSpec) -> Result<(), StageError> {
        Ok(())
    }

    fn run(&self, ctx: &mut StageContext<'_>) -> Result<StageOutcome, StageError> {
        let Some(project) = ctx.project else {
            return Ok(StageOutcome::Skipped {
                reason: "缺少项目上下文".to_string(),
            });
        };
        let Some(host) = current_host() else {
            return Ok(StageOutcome::Skipped {
                reason: "无法识别宿主（交叉目标的校验归 M2 之后）".to_string(),
            });
        };
        let verified = verify_targets(project, &ctx.job.targets, host, ctx.events)?;
        if verified == 0 {
            return Ok(StageOutcome::Skipped {
                reason: format!(
                    "{} 个入选目标都没有已接入的校验器可跑（见 verify 告警）",
                    ctx.job.targets.len()
                ),
            });
        }
        Ok(StageOutcome::Done)
    }
}

/// 逐目标跑已接入的校验器；返回真的跑过的目标数（跳过已在事件流里说明原因）。
///
/// `poly verify`（声明校验器通路）与 [`VerifyStage`] 共用本函数，保证「显式校验」与
/// 「构建内校验」口径一致。
fn verify_targets(
    project: &Project,
    specs: &[TargetSpec],
    host: Host,
    events: &mut dyn EventSink,
) -> Result<usize, StageError> {
    let mut verified = 0usize;
    for spec in specs {
        if !spec.hosts.contains(&host) {
            warning(
                events,
                &format!(
                    "目标 {} 未校验：非本机宿主（交叉目标的校验归 M2 之后）",
                    spec.id
                ),
            );
            continue;
        }
        if verify_target(project, spec, events)? {
            verified += 1;
        }
    }
    Ok(verified)
}

/// 校验单个目标；返回是否真的校验了（`false` = 告警跳过并已说明原因）。
fn verify_target(
    project: &Project,
    spec: &TargetSpec,
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let mut unserved: Vec<String> = Vec::new();
    let mut ios_ids: Vec<String> = Vec::new();
    let mut hap_sign_ids: Vec<String> = Vec::new();
    let mut hap_structure_ids: Vec<String> = Vec::new();
    let mut quickapp_lint_ids: Vec<String> = Vec::new();
    let mut wgt_lint_ids: Vec<String> = Vec::new();
    let mut ares_ids: Vec<String> = Vec::new();
    let mut sandbox_ids: Vec<String> = Vec::new();
    let mut portcheck_ids: Vec<String> = Vec::new();
    let mut pkg_check_ids: Vec<String> = Vec::new();
    let mut pkgsrc_ids: Vec<String> = Vec::new();
    let mut roku_ids: Vec<String> = Vec::new();
    let mut kaios_ids: Vec<String> = Vec::new();
    let mut tiktok_ids: Vec<String> = Vec::new();
    let mut facebook_ids: Vec<String> = Vec::new();
    let mut fitbit_ids: Vec<String> = Vec::new();
    let mut zepp_ids: Vec<String> = Vec::new();
    let mut deb_ids: Vec<String> = Vec::new();
    let mut rpm_ids: Vec<String> = Vec::new();
    let mut appimage_ids: Vec<String> = Vec::new();
    let mut flatpak_ids: Vec<String> = Vec::new();
    let mut snap_ids: Vec<String> = Vec::new();
    let mut appstream_ids: Vec<String> = Vec::new();
    let mut pwa_ids: Vec<String> = Vec::new();
    let mut qnx_ids: Vec<String> = Vec::new();
    let mut arxml_ids: Vec<String> = Vec::new();
    let mut cabi_ids: Vec<String> = Vec::new();
    let mut vxworks_ld_ids: Vec<String> = Vec::new();
    let mut zephyr_west_ids: Vec<String> = Vec::new();
    let mut freertos_qemu_ids: Vec<String> = Vec::new();
    let mut qnx_qemu_ids: Vec<String> = Vec::new();
    let mut zephyr_qemu_ids: Vec<String> = Vec::new();
    for id in &spec.verify {
        match verifier_for(id) {
            Some(Verifier::IosCodesign) => ios_ids.push(id.clone()),
            Some(Verifier::HapSign) => hap_sign_ids.push(id.clone()),
            Some(Verifier::HapStructure) => hap_structure_ids.push(id.clone()),
            Some(Verifier::QuickAppLint) => quickapp_lint_ids.push(id.clone()),
            Some(Verifier::WgtLint) => wgt_lint_ids.push(id.clone()),
            Some(Verifier::AresPackage) => ares_ids.push(id.clone()),
            Some(Verifier::SandboxSelfcheck) => sandbox_ids.push(id.clone()),
            Some(Verifier::Portcheck) => portcheck_ids.push(id.clone()),
            Some(Verifier::PkgCheck) => pkg_check_ids.push(id.clone()),
            Some(Verifier::PkgsrcCheck) => pkgsrc_ids.push(id.clone()),
            Some(Verifier::RokuSideload) => roku_ids.push(id.clone()),
            Some(Verifier::KaiosStructure) => kaios_ids.push(id.clone()),
            Some(Verifier::TiktokMinisStructure) => tiktok_ids.push(id.clone()),
            Some(Verifier::FacebookInstantGamesStructure) => facebook_ids.push(id.clone()),
            Some(Verifier::FitbitStructure) => fitbit_ids.push(id.clone()),
            Some(Verifier::ZeppStructure) => zepp_ids.push(id.clone()),
            Some(Verifier::DebLint) => deb_ids.push(id.clone()),
            Some(Verifier::RpmLint) => rpm_ids.push(id.clone()),
            Some(Verifier::AppimageStructure) => appimage_ids.push(id.clone()),
            Some(Verifier::FlatpakLint) => flatpak_ids.push(id.clone()),
            Some(Verifier::SnapLint) => snap_ids.push(id.clone()),
            Some(Verifier::AppstreamValidate) => appstream_ids.push(id.clone()),
            Some(Verifier::PwaAudit) => pwa_ids.push(id.clone()),
            Some(Verifier::QnxImageStructure) => qnx_ids.push(id.clone()),
            Some(Verifier::ArxmlSchema) => arxml_ids.push(id.clone()),
            Some(Verifier::CAbiSymbols) => cabi_ids.push(id.clone()),
            Some(Verifier::VxWorksLd) => vxworks_ld_ids.push(id.clone()),
            Some(Verifier::ZephyrWestBuild) => zephyr_west_ids.push(id.clone()),
            Some(Verifier::FreeRtosQemuSmoke) => freertos_qemu_ids.push(id.clone()),
            Some(Verifier::QnxQemuSmoke) => qnx_qemu_ids.push(id.clone()),
            Some(Verifier::ZephyrQemuSmoke) => zephyr_qemu_ids.push(id.clone()),
            None => unserved.push(match verifier_owner(id) {
                Some(owner) => format!("{id}（{owner}）"),
                // 注册表有 id 但没人认领：如实点名到具体 id，测试保证这里在 M2 的注册表上是死路
                None => format!("{id}（归属未登记）"),
            }),
        }
    }
    if !unserved.is_empty() {
        warning(
            events,
            &format!(
                "目标 {} 的这些校验器未接入：{}",
                spec.id,
                unserved.join("；")
            ),
        );
    }

    let mut checked = false;
    if !ios_ids.is_empty() {
        checked |= verify_ios(project, spec, &ios_ids, events)?;
    }
    if !hap_sign_ids.is_empty() {
        checked |= verify_hap_sign(project, spec, &hap_sign_ids, events)?;
    }
    if !hap_structure_ids.is_empty() {
        checked |= verify_hap_structure(project, spec, &hap_structure_ids, events)?;
    }
    if !quickapp_lint_ids.is_empty() {
        checked |= verify_quickapp_lint(project, spec, &quickapp_lint_ids, events)?;
    }
    if !wgt_lint_ids.is_empty() {
        checked |= verify_wgt_lint(project, spec, &wgt_lint_ids, events)?;
    }
    if !ares_ids.is_empty() {
        checked |= verify_ares_package(project, spec, &ares_ids, events)?;
    }
    if !sandbox_ids.is_empty() {
        checked |= verify_sandbox_selfcheck(project, spec, &sandbox_ids, events)?;
    }
    if !portcheck_ids.is_empty() {
        checked |= verify_ports(project, spec, &portcheck_ids, events)?;
    }
    if !pkg_check_ids.is_empty() {
        checked |= verify_pkg(project, spec, &pkg_check_ids, events)?;
    }
    if !pkgsrc_ids.is_empty() {
        checked |= verify_pkgsrc(project, spec, &pkgsrc_ids, events)?;
    }
    if !roku_ids.is_empty() {
        checked |= verify_roku_sideload(project, spec, &roku_ids, events)?;
    }
    if !kaios_ids.is_empty() {
        checked |= verify_kaios_structure(project, spec, &kaios_ids, events)?;
    }
    if !tiktok_ids.is_empty() {
        checked |= verify_tiktok_minis_structure(project, spec, &tiktok_ids, events)?;
    }
    if !facebook_ids.is_empty() {
        checked |= verify_facebook_instant_games_structure(project, spec, &facebook_ids, events)?;
    }
    if !fitbit_ids.is_empty() {
        checked |= verify_fitbit(project, spec, &fitbit_ids, events)?;
    }
    if !zepp_ids.is_empty() {
        checked |= verify_zepp(project, spec, &zepp_ids, events)?;
    }
    if !deb_ids.is_empty() {
        checked |= verify_deb_lint(project, spec, &deb_ids, events)?;
    }
    if !rpm_ids.is_empty() {
        checked |= verify_rpm_lint(project, spec, &rpm_ids, events)?;
    }
    if !appimage_ids.is_empty() {
        checked |= verify_appimage_structure(project, spec, &appimage_ids, events)?;
    }
    if !flatpak_ids.is_empty() {
        checked |= verify_flatpak_lint(project, spec, &flatpak_ids, events)?;
    }
    if !snap_ids.is_empty() {
        checked |= verify_snap_lint(project, spec, &snap_ids, events)?;
    }
    if !appstream_ids.is_empty() {
        checked |= verify_appstream_validate(project, spec, &appstream_ids, events)?;
    }
    if !pwa_ids.is_empty() {
        checked |= verify_pwa_audit(project, spec, &pwa_ids, events)?;
    }
    if !qnx_ids.is_empty() {
        checked |= crate::verify_embedded::verify_qnx_image(project, spec, &qnx_ids, events)?;
    }
    if !arxml_ids.is_empty() {
        checked |= crate::verify_embedded::verify_arxml_schema(project, spec, &arxml_ids, events)?;
    }
    if !cabi_ids.is_empty() {
        // 同一 verify id 覆盖五类嵌入式目标；两套实现按 staged 布局区分，按目标分发。
        checked |= if matches!(spec.id.as_str(), "vxworks" | "zephyr" | "freertos") {
            crate::verify_rtos::verify_c_abi_symbols(project, spec, &cabi_ids, events)?
        } else {
            crate::verify_embedded::verify_c_abi_symbols(project, spec, &cabi_ids, events)?
        };
    }
    if !vxworks_ld_ids.is_empty() {
        checked |= crate::verify_rtos::verify_vxworks_ld(project, spec, &vxworks_ld_ids, events)?;
    }
    if !zephyr_west_ids.is_empty() {
        checked |= crate::verify_rtos::verify_zephyr_west(project, spec, &zephyr_west_ids, events)?;
    }
    if !freertos_qemu_ids.is_empty() {
        checked |= crate::verify_freertos::verify_freertos_qemu(
            project,
            spec,
            &freertos_qemu_ids,
            events,
        )?;
    }
    if !qnx_qemu_ids.is_empty() {
        checked |= crate::verify_smoke::verify_qnx_qemu(project, spec, &qnx_qemu_ids, events)?;
    }
    if !zephyr_qemu_ids.is_empty() {
        checked |=
            crate::verify_smoke::verify_zephyr_qemu(project, spec, &zephyr_qemu_ids, events)?;
    }
    Ok(checked)
}

/// iOS 腿（`codesign-verify` / `spctl`）：`.app` 就位且已签名 → 跑 poly-sign 的同源校验计划。
fn verify_ios(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let app = crate::sign::ios_app(project, spec);
    if !app.is_dir() {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 不存在（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                app.display()
            ),
        );
        return Ok(false);
    }
    // 已签名判定沿用 zip 签名块的同款思路：`_CodeSignature/CodeResources` 是 codesign 的落点
    if !app.join("_CodeSignature").join("CodeResources").is_file() {
        warning(
            events,
            &format!(
                "目标 {} 未校验：产物未签名（{} 缺 `_CodeSignature/CodeResources`）——校验器对未签名产物\
                 无从谈起，先跑签名（sign 阶段 / poly retry）",
                spec.id,
                app.display()
            ),
        );
        return Ok(false);
    }
    // 「命令计划正确生成」是本阶段的运行时前置：命令序由 poly-sign 组装（`codesign --verify
    // --deep --strict` + `spctl --assess`），空计划意味着注册表与 poly-sign 脱节——如实跳过，
    // 绝不假装校验过
    let plan = poly_sign::ios::plan_verify(&app);
    if plan.commands.is_empty() {
        warning(
            events,
            &format!(
                "目标 {} 未校验：poly-sign 未给出校验计划（注册表脱节）",
                spec.id
            ),
        );
        return Ok(false);
    }
    // 执行：非 macOS 宿主在 `require_macos` 处得到 E_SIG_0001（skip-if-missing）
    match poly_sign::ios::verify(&app) {
        Ok(_) => {
            // 一次执行给声明的每个 id 一份判决（计划同源：两个 id 是同一份 plan 的两条命令）
            for id in ids {
                events.emit(&BuildEvent::Verified {
                    checker: id.clone(),
                    ok: true,
                });
            }
            Ok(true)
        }
        Err(err @ poly_sign::SignError::ToolMissing { .. }) => {
            warning(events, &format!("目标 {} 未校验：{err}", spec.id));
            Ok(false)
        }
        Err(other) => Err(crate::sign::sign_error(other).into()),
    }
}

/// HarmonyOS 签名腿（`hap-sign-verify`）：hap-sign-tool `verify-app` 就地校验已签名 HAP。
fn verify_hap_sign(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let Some((hap, signed)) = hap_for_verify(project, spec) else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 HAP（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                project::packages_root(&project.root)
                    .join(&spec.id)
                    .display()
            ),
        );
        return Ok(false);
    };
    if !signed {
        warning(
            events,
            &format!(
                "目标 {} 未校验：产物未签名（{}）——签名校验对未签名件无从谈起，先跑签名（sign 阶段 / poly retry）",
                spec.id,
                hap.display()
            ),
        );
        return Ok(false);
    }
    // 执行：缺 java / hap-sign-tool 按 E_SIG_0001 归入跳过；签名非法以 E_SIG_0003 硬失败
    match poly_sign::harmony::verify(&hap) {
        Ok(evidence) => {
            warning(
                events,
                &format!("目标 {} 的 HAP 签名校验：{}", spec.id, evidence),
            );
            for id in ids {
                events.emit(&BuildEvent::Verified {
                    checker: id.clone(),
                    ok: true,
                });
            }
            Ok(true)
        }
        Err(err @ poly_sign::SignError::ToolMissing { .. }) => {
            warning(events, &format!("目标 {} 未校验：{err}", spec.id));
            Ok(false)
        }
        Err(other) => Err(crate::sign::sign_error(other).into()),
    }
}

/// HarmonyOS 结构腿（`hap-structure-check`）：与 HAP 打包器同一份结构检查（必需条目 + 清单字段）。
fn verify_hap_structure(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let Some((hap, _signed)) = hap_for_verify(project, spec) else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 HAP（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                project::packages_root(&project.root)
                    .join(&spec.id)
                    .display()
            ),
        );
        return Ok(false);
    };
    match poly_bundle::check_hap_structure(&hap) {
        Ok(structure) => {
            warning(
                events,
                &format!(
                    "目标 {} 的 HAP 结构检查：{} 条条目，bundleName {}，versionCode {}（模块 {}）",
                    spec.id,
                    structure.entries.len(),
                    structure.bundle_name,
                    structure.version_code,
                    structure.module_name
                ),
            );
            for id in ids {
                events.emit(&BuildEvent::Verified {
                    checker: id.clone(),
                    ok: true,
                });
            }
            Ok(true)
        }
        // 结构非法是产物缺陷（不是环境缺失）：如实硬失败，绝不降级成跳过
        Err(err) => Err(bundle_diagnostic(spec, &err).into()),
    }
}

/// 快应用 `.rpk` lint 腿（`hap-toolkit-lint`，T-M3-12）：产物 `dist/*.rpk` → rpk 结构（必需
/// 条目）→ rpk 内 `manifest.json`（hap build 的合并产物）与源 `src/manifest.json` 交叉核对 →
/// 在 hap-toolkit 的 `validate.js`（`validateJson`：ajv + manifest-schema，即 hap-toolkit
/// 「lint」的本体）上过 schema 校验。
///
/// 环境槽语义（口径同其它腿）：工具链不在场（hap-toolkit / node）或产物无 `.rpk`（打包时
/// 工具缺席）→ 告警跳过；rpk 结构缺陷 / 清单交叉不符 / schema 命中 → 硬失败（产物缺陷不降级）。
fn verify_quickapp_lint(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root)
        .join(&spec.id)
        .join(MINIAPP_DIR);
    let Some((rpk, count)) = quickapp_rpk(&staged) else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 dist/*.rpk（打包时 hap-toolkit 不在场——装齐工具链后\
                 重跑 poly build 再校验）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份 .rpk（取最新一份 {}）",
                spec.id,
                rpk.display()
            ),
        );
    }
    // 工具链（hap-toolkit 安装根 + node）：缺席按环境槽跳过，绝不假装校验过
    let root = match poly_bundle::quickapp_hap_root() {
        Ok(root) => root,
        Err(reason) => {
            warning(events, &format!("目标 {} 未校验：{reason}", spec.id));
            return Ok(false);
        }
    };
    let Some(node) = poly_bundle::exec::tool_path("node") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：PATH 里没有 node（schema 校验经 hap-toolkit 的 node 实现承载）",
                spec.id
            ),
        );
        return Ok(false);
    };

    // 结构：rpk 必需条目齐备（真跑实证的条目面）
    let entries = poly_bundle::zipwrite::entry_names(&rpk).map_err(|err| {
        quickapp_defect(
            spec,
            format!("{} 不是可读的 zip 容器：{err}", file_label(&rpk)),
        )
    })?;
    for required in ["manifest.json", "app.js", "META-INF/CERT"] {
        if !entries.iter().any(|name| name == required) {
            return Err(quickapp_defect(
                spec,
                format!(
                    "{} 缺必需条目 {required}（hap build 产物面漂移）",
                    file_label(&rpk)
                ),
            )
            .into());
        }
    }
    let manifest_text = match poly_bundle::zipwrite::entry_text(&rpk, "manifest.json")
        .map_err(|err| quickapp_defect(spec, format!("读 manifest.json 失败：{err}")))?
    {
        Some(text) => text,
        None => {
            return Err(
                quickapp_defect(spec, format!("{} 缺 manifest.json", file_label(&rpk))).into(),
            );
        }
    };
    let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
        .map_err(|err| quickapp_defect(spec, format!("rpk 内 manifest.json 非法 JSON：{err}")))?;

    // 源清单交叉核对（打包器组装的 src/manifest.json 是事实来源）
    let source_manifest = staged.join("src").join("manifest.json");
    if source_manifest.is_file() {
        let source = std::fs::read_to_string(&source_manifest)
            .map_err(|err| quickapp_defect(spec, format!("读源清单失败：{err}")))?;
        let source: serde_json::Value = serde_json::from_str(&source)
            .map_err(|err| quickapp_defect(spec, format!("源清单非法 JSON：{err}")))?;
        for key in ["package", "versionName", "versionCode"] {
            if manifest.get(key) != source.get(key) {
                return Err(quickapp_defect(
                    spec,
                    format!(
                        "rpk 内 manifest.{key}（{}）与源清单（{}）不一致",
                        manifest
                            .get(key)
                            .cloned()
                            .unwrap_or(serde_json::Value::Null),
                        source.get(key).cloned().unwrap_or(serde_json::Value::Null)
                    ),
                )
                .into());
            }
        }
    } else {
        warning(
            events,
            &format!(
                "目标 {} 的源清单缺席（{}）——交叉核对跳过，只做 rpk 内 schema 校验",
                spec.id,
                source_manifest.display()
            ),
        );
    }

    // schema 校验：真 hap-toolkit 的 validateJson（stdlib stdin 传 manifest，驱动见常量）
    let schema_errors = match run_hap_lint(&node, &root, &manifest_text) {
        Ok(errors) => errors,
        Err(reason) => {
            warning(events, &format!("目标 {} 未校验：{reason}", spec.id));
            return Ok(false);
        }
    };
    if !schema_errors.is_empty() {
        return Err(quickapp_defect(
            spec,
            format!(
                "hap-toolkit schema 校验命中 {} 项：{}",
                schema_errors.len(),
                schema_errors.join("；")
            ),
        )
        .into());
    }
    warning(
        events,
        &format!(
            "目标 {} 的 .rpk 校验：{} 条目，manifest package={} versionName={}（hap-toolkit validateJson 通过）",
            spec.id,
            entries.len(),
            manifest
                .get("package")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("(缺)"),
            manifest
                .get("versionName")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("(缺)")
        ),
    );
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// 产物里的 `.rpk` 定位：`<staged>/dist/*.rpk`（排序取最新一份）；返回 `(路径, 份数)`。
fn quickapp_rpk(staged: &Path) -> Option<(PathBuf, usize)> {
    let dist = staged.join(DIST_DIR);
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&dist)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "rpk"))
        .collect();
    candidates.sort();
    let count = candidates.len();
    candidates.pop().map(|path| (path, count))
}

/// lint 驱动（`node -e` 嵌入）：在 hap-toolkit 安装根上解析 `validate.js`，从 stdin 读
/// manifest JSON，把 `validateJson` 的判决按一行 JSON 打到 stdout。
///
/// 用 `node -e` + stdin（不落临时文件、不依赖 node_modules 解析 cwd）：`createRequire`
/// 以安装根的 `package.json` 为基准，`validate.js` 的依赖（ajv / ajv-errors / manifest-schema /
/// @hap-toolkit/*）按真安装布局解析（真跑实证：2.1.1 hoisted 与嵌套两种布局都命中）。
const HAP_LINT_DRIVER: &str = r#"const path = require('path');
const [kit] = process.argv.slice(-1);
const createRequire = require('module').createRequire;
const req = createRequire(path.join(kit, 'package.json'));
const validate = req(path.join(kit, 'lib/gen-webpack-conf/validate.js'));
let text = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => { text += chunk; });
process.stdin.on('end', () => {
  const manifest = JSON.parse(text);
  const errors = validate.validateJson(manifest, 'manifest.json');
  console.log(JSON.stringify({ ok: errors.length === 0, errors: errors.map((e) => e.message) }));
});"#;

/// 跑 lint 驱动；`Ok(errors)` = 驱动判决的 schema 错误清单（空 = 通过），
/// `Err(reason)` = 环境侧执行失败（驱动起不来 / 非零退出 / 输出非法——按跳过语义上抛）。
fn run_hap_lint(node: &Path, root: &Path, manifest: &str) -> Result<Vec<String>, String> {
    use std::io::Write as _;

    let mut child = std::process::Command::new(node)
        .arg("-e")
        .arg(HAP_LINT_DRIVER)
        .arg(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("lint 驱动（node）启动失败：{err}"))?;
    {
        let mut stdin = child.stdin.take().expect("已管道化 stdin");
        stdin
            .write_all(manifest.as_bytes())
            .map_err(|err| format!("lint 驱动写入失败：{err}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|err| format!("lint 驱动等待失败：{err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "lint 驱动退出码 {}：{}（hap-toolkit 安装须完整，含其依赖）",
            output.status,
            stderr.trim().chars().take(400).collect::<String>()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let value: serde_json::Value = serde_json::from_str(line.trim())
        .map_err(|err| format!("lint 驱动输出非法 JSON：{err}"))?;
    let errors = value
        .get("errors")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| item.as_str().unwrap_or("(非字符串错误项)").to_string())
                .collect::<Vec<String>>()
        })
        .ok_or_else(|| "lint 驱动输出缺 errors 字段".to_string())?;
    if value.get("ok").and_then(serde_json::Value::as_bool) != Some(true) && errors.is_empty() {
        return Err("lint 驱动判决非法（ok=false 但无错误清单）".to_string());
    }
    Ok(errors)
}

/// `.rpk` 产物缺陷诊断（产物缺陷硬失败，不降级成跳过）。
fn quickapp_defect(spec: &TargetSpec, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(
        codes::E_VERIFY_0003,
        format!("目标 {} 的 .rpk 校验失败：{}", spec.id, message.into()),
    )
    .with_hint("修打包输入后重跑 poly build --target quickapp（hap-toolkit 在场时重出 .rpk）")
}

/// 校验对象 HAP：优先已签名件（`<未签名名>-signed.hap` 约定，否则 `*-signed.hap` 扫描——
/// 签名器产物命名 `poly_sign::harmony::signed_output`）；缺已签名件时退化到未签名 HAP，
/// 供结构检查（对未签件同样成立）；返回 `(路径, 是否已签名)`。
fn hap_for_verify(project: &Project, spec: &TargetSpec) -> Option<(PathBuf, bool)> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let preferred = staged.join(format!(
        "{}-signed.hap",
        crate::sign::unsigned_name_for(project, "hap")
    ));
    if preferred.is_file() {
        return Some((preferred, true));
    }
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&staged)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with("-signed.hap"))
        })
        .collect();
    candidates.sort();
    if let Some(found) = candidates.into_iter().next() {
        return Some((found, true));
    }
    crate::sign::unsigned_hap(project, spec).map(|path| (path, false))
}

/// 打包暂存目录顶层的单件产物定位（`<staged>/*.<ext>`，排序取最新一份）；返回 `(路径, 份数)`。
///
/// 只扫暂存根：打包器的工作子目录（`wgt/`、`ipk/`、`webapp/`）不是产物落点。
fn staged_file(staged: &Path, ext: &str) -> Option<(PathBuf, usize)> {
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(staged)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|e| e == ext))
        .collect();
    candidates.sort();
    let count = candidates.len();
    candidates.pop().map(|path| (path, count))
}

/// web 系 TV 腿的产物缺陷诊断（产物缺陷硬失败，绝不降级成跳过；`hint` 给可执行的下一步）。
fn webtv_defect(spec: &TargetSpec, message: impl Into<String>, hint: &str) -> Diagnostic {
    Diagnostic::error(
        codes::E_VERIFY_0003,
        format!("目标 {} 的产物校验失败：{}", spec.id, message.into()),
    )
    .with_hint(hint.to_string())
}

/// Tizen `.wgt` 校验腿（`wgt-lint`，T-M4-08）：先跑与打包器**同一份**结构检查
/// （[`poly_bundle::check_wgt_structure`]：清单必填项 + 引用存在性，缺陷硬失败），官方
/// `wgt-lint` 在场时对产物再真跑一遍（非零退出 = 产物缺陷硬失败）；SDK 校验器缺席按环境槽
/// 告警跳过并注记（口径同 M3 devtools 腿：不假装校验过，内建结构断言是本机的真跑面）。
fn verify_wgt_lint(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((wgt, count)) = staged_file(&staged, "wgt") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.wgt（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份 .wgt（取最新一份 {}）",
                spec.id,
                wgt.display()
            ),
        );
    }
    let structure = match poly_bundle::check_wgt_structure(&wgt) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的 .wgt 结构检查：{} 条目，tizen:application@id={}（profile {}，入口 {}）",
            spec.id,
            structure.entries.len(),
            structure.app_id,
            structure.profile,
            structure.content_src
        ),
    );
    match poly_bundle::wgt_lint_tool() {
        Ok(tool) => {
            let command =
                poly_bundle::Command::new(tool.display().to_string(), [wgt.display().to_string()]);
            match poly_bundle::exec::run(&command) {
                Ok(_) => warning(
                    events,
                    &format!("目标 {} 的 wgt-lint 真跑通过：{}", spec.id, command.line()),
                ),
                Err(err @ BundleError::ToolMissing { .. }) => {
                    warning(events, &format!("目标 {} 的 wgt-lint 未跑：{err}", spec.id));
                }
                Err(err) => {
                    return Err(webtv_defect(
                        spec,
                        format!("wgt-lint 判决不通过：{err}"),
                        "修 manifest / 载荷后重跑 poly build --target tizen-tv",
                    )
                    .into());
                }
            }
        }
        Err(reason) => warning(
            events,
            &format!(
                "目标 {} 的 wgt-lint 未跑：{reason}（内建结构断言已过；SDK 校验器缺席如实注记）",
                spec.id
            ),
        ),
    }
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// webOS `.ipk` 校验腿（`ares-package`，T-M4-09）：先跑与打包器**同一份**结构检查
/// （[`poly_bundle::check_ipk_structure`]：ar 成员序 + control 字段 + appinfo 交叉 + 载荷条目，
/// 缺陷硬失败），官方 `ares-package` 在场时对暂存载荷目录再真跑一遍（输出进独立 scratch，
/// 不动本打包器的确定性 `.ipk`；非零退出 = 载荷缺陷硬失败）；缺席按环境槽告警跳过并注记。
fn verify_ares_package(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((ipk, count)) = staged_file(&staged, "ipk") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.ipk（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份 .ipk（取最新一份 {}）",
                spec.id,
                ipk.display()
            ),
        );
    }
    let structure = match poly_bundle::check_ipk_structure(&ipk) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的 .ipk 结构检查：{} 条目，control Package={} Version={}（appinfo id={} type={}）",
            spec.id,
            structure.entries.len(),
            structure.package,
            structure.version,
            structure.app_id,
            structure.app_type
        ),
    );
    let app = staged.join("ipk").join("app");
    match poly_bundle::ares_package_tool() {
        Ok(tool) if app.is_dir() => {
            // ares-package 是打包器兼校验器（吃载荷目录）：对暂存载荷真跑，输出进独立 scratch
            let scratch = staged.join("ipk").join("verify-ares");
            let command = poly_bundle::Command::new(
                tool.display().to_string(),
                [
                    app.display().to_string(),
                    "-o".to_string(),
                    scratch.display().to_string(),
                ],
            )
            .in_dir(staged.join("ipk"));
            match poly_bundle::exec::run(&command) {
                Ok(_) => warning(
                    events,
                    &format!(
                        "目标 {} 的 ares-package 真跑通过：{}",
                        spec.id,
                        command.line()
                    ),
                ),
                Err(err @ BundleError::ToolMissing { .. }) => {
                    warning(
                        events,
                        &format!("目标 {} 的 ares-package 未跑：{err}", spec.id),
                    );
                }
                Err(err) => {
                    return Err(webtv_defect(
                        spec,
                        format!("ares-package 判决不通过：{err}"),
                        "修 appinfo / 载荷后重跑 poly build --target webos-tv",
                    )
                    .into());
                }
            }
        }
        Ok(_) => warning(
            events,
            &format!(
                "目标 {} 的 ares-package 未跑：暂存载荷目录 {} 不在场（内建结构断言已过；可重跑 \
                 poly build --target webos-tv 重建暂存面）",
                spec.id,
                app.display()
            ),
        ),
        Err(reason) => warning(
            events,
            &format!(
                "目标 {} 的 ares-package 未跑：{reason}（内建结构断言已过；SDK 校验器缺席如实注记）",
                spec.id
            ),
        ),
    }
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// VIDAA / Vizio 应用包沙箱自检腿（`sandbox-selfcheck`，T-M4-10）：与打包器**同一份**检查
/// （[`poly_bundle::check_web_app_structure`] 三条规则：入口文件 / 清单字段 / 体积；缺陷硬失败），
/// 体积上限按 `[budget].size` 判定——与 `package` 阶段同一门禁（`fail` 策略下硬失败，
/// 判定结果照进事件流）。两平台无本地 SDK 校验器（目标声明只挂本 id），自检即真跑面。
fn verify_sandbox_selfcheck(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let Some(platform) = poly_bundle::WebAppPlatform::from_target(&spec.id) else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：sandbox-selfcheck 只服务 vidaa-tv / smartcast-tv 两个 web TV 目标",
                spec.id
            ),
        );
        return Ok(false);
    };
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((package, count)) = staged_file(&staged, "zip") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.zip（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份包（取最新一份 {}）",
                spec.id,
                package.display()
            ),
        );
    }
    let structure = match poly_bundle::check_web_app_structure(&package, platform) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    // 体积规则：与 package 阶段同一判据（[budget].size / policy；`startup_ms` 尚无采集来源）
    let budget = poly_core::job::Budget {
        size: project.config.budget.size,
        startup_ms: None,
        policy: project.config.budget.policy,
    };
    let mut volume = format!(
        "体积 {} 字节（未设 [budget].size，不设门禁）",
        structure.bytes
    );
    if let Some(verdict) = poly_core::budget::size_verdict(structure.bytes, &budget) {
        events.emit(&verdict.event(&spec.id, &spec.package.format));
        if verdict.exceeded && budget.policy == poly_core::job::SizePolicy::Fail {
            return Err(verdict.failure(&spec.id, &spec.package.format).into());
        }
        volume = format!(
            "体积 {} 字节（预算上限 {}，{}）",
            structure.bytes,
            verdict.limit,
            if verdict.exceeded {
                "超预算（策略 warn）"
            } else {
                "在预算内"
            }
        );
    }
    warning(
        events,
        &format!(
            "目标 {} 的沙箱自检：{} 条目，入口 {}（清单 {} 字段齐备），{}",
            spec.id,
            structure.entries.len(),
            structure.entry,
            structure.manifest,
            volume
        ),
    );
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// OpenBSD ports 校验腿（`portcheck`，T-M5-07）：先跑与打包器**同一份**结构检查
/// （[`poly_bundle::check_ports_structure`]：Makefile 必填项 / DESCR / PLIST 与载荷双向一致，
/// 缺陷硬失败），官方 `portcheck -N` 在场时对端口目录再真跑一遍（非零退出 = 端口缺陷硬失败）；
/// Linux 宿主通常无 portcheck（OpenBSD base 工具），缺席按环境槽告警跳过并注记——内建结构
/// 断言是本机的真跑面，VM 真跑归 T-M5-09。
fn verify_ports(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((port, count)) = staged_ports_dir(&staged) else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 ports/<DISTNAME>/（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 个 ports 目录（取 {}）",
                spec.id,
                port.display()
            ),
        );
    }
    let structure = match poly_bundle::check_ports_structure(&port) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的 ports 结构检查：{} 个 PLIST 条目，DISTNAME={}，MAINTAINER={}",
            spec.id,
            structure.entries.len(),
            structure.distname,
            structure.maintainer
        ),
    );
    if poly_bundle::exec::tool_present("portcheck") {
        let command = poly_bundle::Command::new("portcheck", ["-N".to_string()]).in_dir(&port);
        match poly_bundle::exec::run(&command) {
            Ok(_) => warning(
                events,
                &format!("目标 {} 的 portcheck 真跑通过：{}", spec.id, command.line()),
            ),
            Err(err @ BundleError::ToolMissing { .. }) => {
                warning(
                    events,
                    &format!("目标 {} 的 portcheck 未跑：{err}", spec.id),
                );
            }
            Err(err) => {
                return Err(webtv_defect(
                    spec,
                    format!("portcheck 判决不通过：{err}"),
                    "修 Makefile / pkg/PLIST / pkg/DESCR 后重跑 poly build --target openbsd",
                )
                .into());
            }
        }
    } else {
        warning(
            events,
            &format!(
                "目标 {} 的 portcheck 未跑：PATH 无 portcheck（内建结构断言已过；OpenBSD VM \
                 真跑归 T-M5-09）",
                spec.id
            ),
        );
    }
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// FreeBSD pkg 校验腿（`pkg-check`，T-M5-06）：先跑与打包器**同一份**结构检查
/// （[`poly_bundle::check_pkg_structure`]：+MANIFEST 必填项 / prefix 安全 / 暂存树与
/// manifest 同源，缺陷硬失败）；`pkg` 在场时对 `.pkg` 产物跑 `pkg info -F`，已安装时再跑
/// `pkg check -s`（非零退出 = 包缺陷硬失败）。Linux 宿主通常无 pkg(8)，缺席按环境槽告警
/// 跳过并注记——内建结构断言是本机真跑面，`pkg create` / 安装冒烟归 T-M5-09 FreeBSD VM。
fn verify_pkg(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((stage, manifest, count)) = staged_pkg(&staged) else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.pkgstage（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份 pkgstage（取 {}）",
                spec.id,
                stage.display()
            ),
        );
    }
    let structure = match poly_bundle::check_pkg_structure(&stage, &manifest) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的 pkg 结构检查：{} 个文件，name={}-{}，origin={}，prefix={}",
            spec.id,
            structure.files.len(),
            structure.name,
            structure.version,
            structure.origin,
            structure.prefix
        ),
    );
    let pkg = stage.with_extension("pkg");
    if poly_bundle::exec::tool_present("pkg") {
        if pkg.is_file() {
            let command = poly_bundle::Command::new(
                "pkg",
                [
                    "info".to_string(),
                    "-F".to_string(),
                    pkg.display().to_string(),
                ],
            );
            match poly_bundle::exec::run(&command) {
                Ok(_) => warning(
                    events,
                    &format!(
                        "目标 {} 的 pkg info -F 真跑通过：{}",
                        spec.id,
                        command.line()
                    ),
                ),
                Err(err @ BundleError::ToolMissing { .. }) => {
                    warning(
                        events,
                        &format!("目标 {} 的 pkg info -F 未跑：{err}", spec.id),
                    );
                }
                Err(err) => {
                    return Err(webtv_defect(
                        spec,
                        format!("pkg info -F 判决不通过：{err}"),
                        "在 FreeBSD VM 上重跑 poly build --target freebsd 生成 .pkg 后重试 poly verify",
                    )
                    .into());
                }
            }
        } else {
            warning(
                events,
                &format!(
                    "目标 {} 的 pkg info -F 未跑：{} 下没有 .pkg（Linux 交叉发起只生成暂存树；\
                     pkg create 与安装冒烟归 FreeBSD VM（T-M5-09））",
                    spec.id,
                    staged.display()
                ),
            );
        }
        let installed = poly_bundle::Command::new(
            "pkg",
            [
                "info".to_string(),
                "-e".to_string(),
                format!("{}-{}", structure.name, structure.version),
            ],
        );
        if poly_bundle::exec::run(&installed).is_ok() {
            let command = poly_bundle::Command::new(
                "pkg",
                [
                    "check".to_string(),
                    "-s".to_string(),
                    structure.name.clone(),
                ],
            );
            match poly_bundle::exec::run(&command) {
                Ok(_) => warning(
                    events,
                    &format!(
                        "目标 {} 的 pkg check -s 真跑通过：{}",
                        spec.id,
                        command.line()
                    ),
                ),
                Err(err @ BundleError::ToolMissing { .. }) => {
                    warning(
                        events,
                        &format!("目标 {} 的 pkg check -s 未跑：{err}", spec.id),
                    );
                }
                Err(err) => {
                    return Err(webtv_defect(
                        spec,
                        format!("pkg check -s 判决不通过：{err}"),
                        "重新 pkg install 后重试；持续失败按 pkg check 输出修包内容",
                    )
                    .into());
                }
            }
        } else {
            warning(
                events,
                &format!(
                    "目标 {} 的 pkg check -s 未跑：本机未安装 {}-{}（安装冒烟 pkg install -y \
                     归 FreeBSD VM（T-M5-09））",
                    spec.id, structure.name, structure.version
                ),
            );
        }
    } else {
        warning(
            events,
            &format!(
                "目标 {} 的 pkg check 未跑：PATH 无 pkg（内建结构断言已过；pkg create / \
                 pkg check / 安装冒烟归 FreeBSD VM（T-M5-09））",
                spec.id
            ),
        );
    }
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// NetBSD / DragonFly pkgsrc 校验腿（`pkgsrc-check`，T-M5-08）：先跑与打包器**同一份**结构
/// 检查（[`poly_bundle::check_pkgsrc_structure`]：Makefile 必填项 / DESCR / PLIST 与载荷
/// 双向一致，缺陷硬失败）；骨架位于 pkgsrc 树内且 `bmake` 在场时对包目录跑 `bmake package`
/// （非零退出 = 包缺陷硬失败）。Linux 交叉发起只有树外骨架，`bmake` 缺席或基础设施不在时
/// 按环境槽告警跳过并注记——内建结构断言是本机真跑面，VM 真跑归 NetBSD（T-M5-09）。
fn verify_pkgsrc(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((port, count)) = staged_pkgsrc_dir(&staged) else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 pkgsrc/<DISTNAME>/（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 个 pkgsrc 骨架（取 {}）",
                spec.id,
                port.display()
            ),
        );
    }
    let structure = match poly_bundle::check_pkgsrc_structure(&port) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的 pkgsrc 结构检查：{} 个 PLIST 条目，DISTNAME={}（name={} version={}）",
            spec.id,
            structure.entries.len(),
            structure.distname,
            structure.name,
            structure.version
        ),
    );
    let tree_mk = port.join("../../mk/bsd.pkg.mk");
    if poly_bundle::exec::tool_present("bmake") && tree_mk.is_file() {
        let command = poly_bundle::Command::new("bmake", ["package".to_string()]).in_dir(&port);
        match poly_bundle::exec::run(&command) {
            Ok(_) => warning(
                events,
                &format!(
                    "目标 {} 的 bmake package 真跑通过：{}",
                    spec.id,
                    command.line()
                ),
            ),
            Err(err @ BundleError::ToolMissing { .. }) => {
                warning(
                    events,
                    &format!("目标 {} 的 bmake package 未跑：{err}", spec.id),
                );
            }
            Err(err) => {
                return Err(webtv_defect(
                    spec,
                    format!("bmake package 判决不通过：{err}"),
                    "按 bmake 输出修 Makefile / PLIST / DESCR 后重跑；骨架须位于 pkgsrc 树 \
                     misc/<DISTNAME>（NetBSD VM，T-M5-09）",
                )
                .into());
            }
        }
    } else if poly_bundle::exec::tool_present("bmake") {
        warning(
            events,
            &format!(
                "目标 {} 的 bmake package 未跑：骨架在 pkgsrc 树外（缺 {}；内建结构断言已过，\
                 bmake 真跑归 NetBSD VM（T-M5-09））",
                spec.id,
                tree_mk.display()
            ),
        );
    } else {
        warning(
            events,
            &format!(
                "目标 {} 的 bmake package 未跑：PATH 无 bmake（内建结构断言已过；bmake package / \
                 pkg_add 真跑归 NetBSD VM（T-M5-09））",
                spec.id
            ),
        );
    }
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// Roku 侧载结构自检腿（`sideload-structure-check`，T-M4-15）：与打包器**同一份**检查
/// （[`poly_bundle::check_roku_structure`]：zip 根布局 + manifest 必填字段 + SceneGraph XML
/// 良构 + 焦点表 + BRS 形态；缺陷 E_BND_0006 硬失败）。真机侧载腿（ECP 探测 / `plugin_install`）
/// 本机不可得——T-M4-12 决策写明真机侧载为后续前置，告警里如实打印 SKIP 句，不假装跑过。
fn verify_roku_sideload(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((package, count)) = staged_file(&staged, "zip") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.zip（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份 zip（取最新一份 {}）",
                spec.id,
                package.display()
            ),
        );
    }
    let structure = match poly_bundle::check_roku_structure(&package) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的侧载结构自检：{} 条目，manifest title={} 版本 {}.{}.{}，焦点表 {} 节点\
             （initial={}）",
            spec.id,
            structure.entries.len(),
            structure.title,
            structure.major,
            structure.minor,
            structure.build,
            structure.focus_nodes,
            structure.initial_focus
        ),
    );
    warning(
        events,
        &format!(
            "目标 {} 的真机侧载腿 SKIP-with-print：本机无 Roku SDK / 设备 / 模拟器（T-M4-12 \
             决策写明真机侧载为后续前置）；结构自检是本机真跑面",
            spec.id
        ),
    );
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// KaiOS 包结构自检腿（`kaios-structure-check`）：与打包器**同一份**检查
/// （[`poly_bundle::check_kaios_structure`]：zip 条目安全、`manifest.webapp` 必填字段
/// name / version / launch_path、入口存在；缺陷 E_BND_0006 硬失败）。本机无 KaiOS 设备 /
/// 模拟器与 KaiStore 凭据——设备 / 商店腿 SKIP-with-print，结构自检是本机真跑面。
fn verify_kaios_structure(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((package, count)) = staged_file(&staged, "zip") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.zip（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份 zip（取最新一份 {}）",
                spec.id,
                package.display()
            ),
        );
    }
    let structure = match poly_bundle::check_kaios_structure(&package) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的包结构自检：{} 条目，manifest name={} 版本 {}，launch_path={}",
            spec.id,
            structure.entries.len(),
            structure.name,
            structure.version,
            structure.launch_path
        ),
    );
    warning(
        events,
        &format!(
            "目标 {} 的设备 / 商店腿 SKIP-with-print：本机无 KaiOS 设备 / 模拟器与 KaiStore \
             凭据（平台侧签名与提审在 KaiStore 完成）；结构自检是本机真跑面",
            spec.id
        ),
    );
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// TikTok Minis zip 结构自检腿（`tiktok-minis-structure-check`）：与打包器**同一份**检查
/// （[`poly_bundle::check_tiktok_minis_structure`]：`minis.config.json` build 字段 /
/// `minis.manifest.json` 资源树；缺陷 E_BND_0006 硬失败）。本机无
/// TikTok 开发者平台沙箱 / 上传入口——平台侧审核 SKIP-with-print，结构自检是真跑面。
fn verify_tiktok_minis_structure(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((package, count)) = staged_file(&staged, "zip") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.zip（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份 zip（取最新一份 {}）",
                spec.id,
                package.display()
            ),
        );
    }
    let structure = match poly_bundle::check_tiktok_minis_structure(&package) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的包结构自检：{} 条目，minis.config.json htmlEntry={} outputDir={}，\
             resource_list {} 项",
            spec.id,
            structure.entries.len(),
            structure.html_entry,
            structure.output_dir,
            structure.resources
        ),
    );
    warning(
        events,
        &format!(
            "目标 {} 的平台侧沙箱 / 审核腿 SKIP-with-print：本机无 TikTok 开发者平台沙箱 / \
             上传入口（experimental，不声明商店受理）；结构自检是本机真跑面",
            spec.id
        ),
    );
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// Facebook Instant Games zip 结构自检腿（`facebook-instant-games-structure-check`）：
/// 与打包器**同一份**检查（[`poly_bundle::check_facebook_instant_games_structure`]：
/// `fbapp-config.json` 字段；缺陷 E_BND_0006 硬失败）。本机无 Meta
/// 应用后台 / 沙箱——平台侧审核 SKIP-with-print，结构自检是真跑面。
fn verify_facebook_instant_games_structure(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((package, count)) = staged_file(&staged, "zip") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.zip（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份 zip（取最新一份 {}）",
                spec.id,
                package.display()
            ),
        );
    }
    let structure = match poly_bundle::check_facebook_instant_games_structure(&package) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的包结构自检：{} 条目，fbapp-config.json platform_version={} \
             orientation={} navigation_menu_version={}",
            spec.id,
            structure.entries.len(),
            structure.platform_version,
            structure.orientation,
            structure.navigation_menu_version
        ),
    );
    warning(
        events,
        &format!(
            "目标 {} 的平台侧沙箱 / 审核腿 SKIP-with-print：本机无 Meta 应用后台 / 沙箱\
             （experimental，不声明商店受理）；结构自检是本机真跑面",
            spec.id
        ),
    );
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// Fitbit `.fba` 校验腿（`fitbit-structure-check`，T-M7-07）：与打包器**同一份**检查
/// （[`poly_bundle::check_fitbit_structure`]：外层应用清单、每家族 `device-<family>.zip`、
/// 组件清单三层；缺陷 E_BND_0006 硬失败）。官方 `fitbit-build` 只作工具在场自检
/// （`--help`）：本机缺席 SKIP-with-print，结构断言是真跑面；产物组装不依赖 CLI。
fn verify_fitbit(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((package, count)) = staged_file(&staged, "fba") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.fba（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份 .fba（取最新一份 {}）",
                spec.id,
                package.display()
            ),
        );
    }
    let structure = match poly_bundle::check_fitbit_structure(&package) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的 .fba 结构自检：{} 条目，appId={} buildId={}，设备家族 {}（入口 {}）{}",
            spec.id,
            structure.entries.len(),
            structure.app_id,
            structure.build_id,
            structure.families.join(", "),
            structure.device_main,
            if structure.companion {
                "，含 companion.zip"
            } else {
                ""
            }
        ),
    );
    match poly_bundle::exec::tool_path("fitbit-build") {
        Some(tool) => {
            let command = poly_bundle::Command::new(tool.display().to_string(), ["--help"]);
            match poly_bundle::exec::run(&command) {
                Ok(_) => warning(
                    events,
                    &format!(
                        "目标 {} 的 fitbit-build 自检真跑通过：{}",
                        spec.id,
                        command.line()
                    ),
                ),
                Err(err) => warning(
                    events,
                    &format!(
                        "目标 {} 的 fitbit-build 自检未通过：{err}（内建结构断言已过；官方 CLI 承载的是 \
                         TS / rollup 编译与打包腿，不在本项目产物链）",
                        spec.id
                    ),
                ),
            }
        }
        None => warning(
            events,
            &format!(
                "目标 {} 的 fitbit-build 自检 SKIP-with-print：PATH 里没有 fitbit-build（Fitbit OS \
                 SDK 未安装）；内建结构断言已真跑，产物组装不依赖官方 CLI",
                spec.id
            ),
        ),
    }
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// Zepp `.zab` 校验腿（`zepp-structure-check`，T-M7-08）：与打包器**同一份**检查
/// （[`poly_bundle::check_zepp_structure`]：外层清单 + zpk 的 md5 同源 + `device.zip`
/// 内 `app.json` 交叉；缺陷硬失败）。官方 `zeus` 只作工具在场自检（`--version`），
/// 模拟器安装腿本机不可得——两者缺席均 SKIP-with-print，结构断言是真跑面。
fn verify_zepp(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((package, count)) = staged_file(&staged, "zab") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.zab（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份 .zab（取最新一份 {}）",
                spec.id,
                package.display()
            ),
        );
    }
    let structure = match poly_bundle::check_zepp_structure(&package) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的 .zab 结构自检：{} 条目，configVersion={}，appName={} appId={} 版本 {}({})，\
             zpk={}（md5 同源），device.zip {} 条目{}",
            spec.id,
            structure.entries.len(),
            structure.config_version,
            structure.app_name,
            structure.app_id,
            structure.version_name,
            structure.version_code,
            structure.zpk_name,
            structure.device_entries.len(),
            if structure.has_app_side {
                "，含 app-side.zip"
            } else {
                ""
            }
        ),
    );
    match poly_bundle::exec::tool_path("zeus") {
        Some(tool) => {
            let command = poly_bundle::Command::new(tool.display().to_string(), ["--version"]);
            match poly_bundle::exec::run(&command) {
                Ok(_) => warning(
                    events,
                    &format!("目标 {} 的 zeus 自检真跑通过：{}", spec.id, command.line()),
                ),
                Err(err) => warning(
                    events,
                    &format!(
                        "目标 {} 的 zeus 自检未通过：{err}（内建结构断言已过；官方 CLI 承载的是 \
                         qjsc 编译与 App Store 打包腿，不在本项目产物链）",
                        spec.id
                    ),
                ),
            }
        }
        None => warning(
            events,
            &format!(
                "目标 {} 的 zeus 自检 SKIP-with-print：PATH 里没有 zeus（Zepp 开发者工具未安装）",
                spec.id
            ),
        ),
    }
    warning(
        events,
        &format!(
            "目标 {} 的模拟器安装腿 SKIP-with-print：本机无 Zepp 模拟器 / 开发者工具；\
             内建结构断言与官方 zpm 打包形态是本机真跑面，安装性待有模拟器的环境复核",
            spec.id
        ),
    );
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// 暂存目录下的 ports 骨架（`<staged>/ports/<DISTNAME>/`；多个取字典序最后一份）。
fn staged_ports_dir(staged: &Path) -> Option<(PathBuf, usize)> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(staged.join("ports"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join("Makefile").is_file())
        .collect();
    dirs.sort();
    let count = dirs.len();
    dirs.pop().map(|path| (path, count))
}

/// 暂存目录下的 pkg 暂存树（`<staged>/*.pkgstage/`，须有同名 `.manifest`；多个取字典序最后一份）。
fn staged_pkg(staged: &Path) -> Option<(PathBuf, PathBuf, usize)> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(staged)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path.extension().is_some_and(|ext| ext == "pkgstage")
                && path.with_extension("manifest").is_file()
        })
        .collect();
    dirs.sort();
    let count = dirs.len();
    dirs.pop().map(|stage| {
        let manifest = stage.with_extension("manifest");
        (stage, manifest, count)
    })
}

/// 暂存目录下的 pkgsrc 骨架（`<staged>/pkgsrc/<DISTNAME>/`，须有 Makefile + PLIST；
/// 多个取字典序最后一份）。
fn staged_pkgsrc_dir(staged: &Path) -> Option<(PathBuf, usize)> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(staged.join("pkgsrc"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir() && path.join("Makefile").is_file() && path.join("PLIST").is_file()
        })
        .collect();
    dirs.sort();
    let count = dirs.len();
    dirs.pop().map(|path| (path, count))
}

/// 打包暂存目录里的目录产物定位（`<staged>/*<suffix>`，排序取最新一份）；返回 `(路径, 份数)`。
fn staged_dir(staged: &Path, suffix: &str) -> Option<(PathBuf, usize)> {
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(staged)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(suffix))
        })
        .collect();
    candidates.sort();
    let count = candidates.len();
    candidates.pop().map(|path| (path, count))
}

/// 官方 lint 工具输出里的 error 行（`E:` 前缀 / ` E: ` 中缀；warning 不计）。
fn lint_error_lines(stdout: &[u8], stderr: &[u8]) -> Vec<String> {
    let mut text = String::from_utf8_lossy(stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(stderr));
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("E:") || line.contains(" E: "))
        .take(5)
        .map(str::to_string)
        .collect()
}

/// deb 腿（`lintian`）：与打包器同一份结构检查 + 在场时官方 lintian 的真跑判决。
fn verify_deb_lint(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((deb, count)) = staged_file(&staged, "deb") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.deb（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份 .deb（取最新一份 {}）",
                spec.id,
                deb.display()
            ),
        );
    }
    let structure = match poly_bundle::check_deb_structure(&deb) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的 .deb 结构检查：{}-{} {}，Depends={}，{} 个数据段条目",
            spec.id,
            structure.package,
            structure.version,
            structure.architecture,
            if structure.depends.is_empty() {
                "(无)"
            } else {
                &structure.depends
            },
            structure.entries.len()
        ),
    );
    if poly_bundle::exec::tool_present("lintian") {
        let command = poly_bundle::Command::new(
            "lintian",
            [
                "--fail-on".to_string(),
                "error".to_string(),
                deb.display().to_string(),
            ],
        );
        match poly_bundle::exec::run(&command) {
            Ok(output) => {
                let errors = lint_error_lines(&output.stdout, &output.stderr);
                if !errors.is_empty() {
                    return Err(webtv_defect(
                        spec,
                        format!("lintian 判出 error：{}", errors.join("；")),
                        "修 control / 维护者脚本 / 数据段后重跑 poly build --target ubuntu",
                    )
                    .into());
                }
                warning(
                    events,
                    &format!("目标 {} 的 lintian 真跑通过（--fail-on error）", spec.id),
                );
            }
            Err(err @ BundleError::ToolMissing { .. }) => {
                warning(events, &format!("目标 {} 的 lintian 未跑：{err}", spec.id));
            }
            Err(err) => {
                return Err(webtv_defect(
                    spec,
                    format!("lintian 判决不通过：{err}"),
                    "修产物后重跑 poly build --target ubuntu",
                )
                .into());
            }
        }
    } else {
        warning(
            events,
            &format!(
                "目标 {} 的 lintian 未跑：本机无 lintian（内建结构断言已过）",
                spec.id
            ),
        );
    }
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// rpm 腿（`rpmlint`）：纯 Rust 头部结构检查 + 在场时官方 rpmlint 的真跑判决。
fn verify_rpm_lint(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((rpm, count)) = staged_file(&staged, "rpm") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.rpm（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    if count > 1 {
        warning(
            events,
            &format!(
                "目标 {} 的产物有 {count} 份 .rpm（取最新一份 {}）",
                spec.id,
                rpm.display()
            ),
        );
    }
    let structure = match poly_bundle::check_rpm_structure(&rpm) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的 .rpm 头部检查：{}-{}-{}.{}，License={}，{} 个文件",
            spec.id,
            structure.name,
            structure.version,
            structure.release,
            structure.arch,
            structure.license,
            structure.files.len()
        ),
    );
    if poly_bundle::exec::tool_present("rpmlint") {
        let command = poly_bundle::Command::new("rpmlint", [rpm.display().to_string()]);
        match poly_bundle::exec::run(&command) {
            Ok(output) => {
                let errors = lint_error_lines(&output.stdout, &output.stderr);
                if !errors.is_empty() {
                    return Err(webtv_defect(
                        spec,
                        format!("rpmlint 判出 error：{}", errors.join("；")),
                        "修 spec / 载荷后重跑 poly build --target fedora",
                    )
                    .into());
                }
                warning(events, &format!("目标 {} 的 rpmlint 真跑通过", spec.id));
            }
            Err(err @ BundleError::ToolMissing { .. }) => {
                warning(events, &format!("目标 {} 的 rpmlint 未跑：{err}", spec.id));
            }
            Err(err) => {
                return Err(webtv_defect(
                    spec,
                    format!("rpmlint 判决不通过：{err}"),
                    "修产物后重跑 poly build --target fedora",
                )
                .into());
            }
        }
    } else {
        warning(
            events,
            &format!(
                "目标 {} 的 rpmlint 未跑：本机无 rpmlint（内建头部断言已过）",
                spec.id
            ),
        );
    }
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// AppImage 腿（`appimage-structure`）：AppDir 结构（打包器同一实现）+ 单文件 ELF 头。
fn verify_appimage_structure(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let appdir = staged_dir(&staged, ".AppDir");
    let image = staged_file(&staged, "AppImage");
    if appdir.is_none() && image.is_none() {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.AppDir / *.AppImage（先跑 poly build 把产物流到打包暂存目录）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    }
    if let Some((dir, count)) = &appdir {
        if *count > 1 {
            warning(
                events,
                &format!(
                    "目标 {} 的产物有 {count} 份 AppDir（取最新一份 {}）",
                    spec.id,
                    dir.display()
                ),
            );
        }
        let structure = match poly_bundle::check_appdir_structure(dir) {
            Ok(structure) => structure,
            Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
        };
        warning(
            events,
            &format!(
                "目标 {} 的 AppDir 结构检查：{} 条目，desktop={}，metainfo={}，图标 {} 枚",
                spec.id, structure.entries, structure.desktop, structure.metainfo, structure.icons
            ),
        );
    }
    if let Some((path, _)) = &image {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                return Err(webtv_defect(
                    spec,
                    format!("AppImage 读取失败（{}）：{err}", path.display()),
                    "重跑 poly build --target linux",
                )
                .into());
            }
        };
        if !bytes.starts_with(b"\x7fELF") {
            return Err(webtv_defect(
                spec,
                format!("{} 不是 ELF AppImage", path.display()),
                "重跑 poly build --target linux（appimagetool 产物应为 ELF）",
            )
            .into());
        }
        warning(
            events,
            &format!(
                "目标 {} 的 AppImage 文件头检查：ELF，{} 字节",
                spec.id,
                bytes.len()
            ),
        );
    }
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// Flatpak 腿（`flatpak-lint`）：manifest + 载荷 + 最小 finish-args 集。
fn verify_flatpak_lint(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((dir, _)) = staged_dir(&staged, "-flatpak.stage") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *-flatpak.stage（Flatpak 产物须先把 [targets.{}].format \
                 设为 flatpak 再跑 poly build）",
                spec.id,
                staged.display(),
                spec.id
            ),
        );
        return Ok(false);
    };
    let structure = match poly_bundle::check_flatpak_structure(&dir) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的 Flatpak 结构检查：app-id={}，{}，command={}，权限 {:?}，{} 个载荷文件",
            spec.id,
            structure.app_id,
            structure.runtime,
            structure.command,
            structure.finish_args,
            structure.files.len()
        ),
    );
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// Snap 腿（`snap-lint`）：snapcraft.yaml + 接口声明 + 载荷。
fn verify_snap_lint(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    let Some((dir, _)) = staged_dir(&staged, "-snap.stage") else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *-snap.stage（Snap 产物须先把 [targets.{}].format 设为 \
                 snap 再跑 poly build）",
                spec.id,
                staged.display(),
                spec.id
            ),
        );
        return Ok(false);
    };
    let structure = match poly_bundle::check_snap_structure(&dir) {
        Ok(structure) => structure,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    warning(
        events,
        &format!(
            "目标 {} 的 Snap 结构检查：name={}，{}，confinement={}，plugs={:?}，{} 个载荷文件",
            spec.id,
            structure.name,
            structure.base,
            structure.confinement,
            structure.plugs,
            structure.files.len()
        ),
    );
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// AppStream 腿（`appstream-validate`）：暂存树里的 `<id>.metainfo.xml` 结构检查 +
/// 在场时官方 `appstreamcli validate`（warning 不计，`E:` 行硬失败）。
fn verify_appstream_validate(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let staged = project::packages_root(&project.root).join(&spec.id);
    // 暂存树形态：deb / rpm 原生文件（元数据需解包，见告警）；AppDir =
    // `<名>.AppDir/usr/share/metainfo/`、Flatpak = `<名>-flatpak.stage/files/share/metainfo/`、
    // Snap 同形 → 最多 5 层目录，留一层余量。
    let Some(metainfo) = find_metainfo(&staged, 6) else {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 下没有 *.metainfo.xml（先跑 poly build；deb / rpm 内嵌元数据 \
                 须解包后校验）",
                spec.id,
                staged.display()
            ),
        );
        return Ok(false);
    };
    let internal = match poly_bundle::packagers::linux::check_appstream_metainfo(&metainfo) {
        Ok(meta) => meta,
        Err(err) => return Err(bundle_diagnostic(spec, &err).into()),
    };
    if poly_bundle::exec::tool_present("appstreamcli") {
        let command = poly_bundle::Command::new(
            "appstreamcli",
            [
                "validate".to_string(),
                "--no-net".to_string(),
                metainfo.display().to_string(),
            ],
        );
        match poly_bundle::exec::run(&command) {
            Ok(output) => {
                let errors = lint_error_lines(&output.stdout, &output.stderr);
                if !errors.is_empty() {
                    return Err(webtv_defect(
                        spec,
                        format!("appstreamcli 判出 error：{}", errors.join("；")),
                        "修 AppStream 元数据（id / license / release date）后重跑 poly build",
                    )
                    .into());
                }
                warning(
                    events,
                    &format!(
                        "目标 {} 的 appstreamcli validate 真跑通过（warning 不计）",
                        spec.id
                    ),
                );
            }
            // appstreamcli 1.0.5 对 warning 也以 3 退出（error 为 1）：只有 `E:` 行是硬失败。
            Err(BundleError::ToolFailed { status, stderr, .. })
                if status == "3" && lint_error_lines(b"", stderr.as_bytes()).is_empty() =>
            {
                warning(
                    events,
                    &format!(
                        "目标 {} 的 appstreamcli validate 通过（退出码 3 = warning/info，不计；{}）",
                        spec.id,
                        stderr.lines().next().unwrap_or("").trim()
                    ),
                );
            }
            Err(err @ BundleError::ToolMissing { .. }) => {
                warning(
                    events,
                    &format!("目标 {} 的 appstreamcli 未跑：{err}", spec.id),
                );
            }
            Err(err) => {
                return Err(webtv_defect(
                    spec,
                    format!("appstreamcli 判决不通过：{err}"),
                    "修 AppStream 元数据后重跑 poly build",
                )
                .into());
            }
        }
    } else {
        warning(
            events,
            &format!(
                "目标 {} 的 appstreamcli 未跑：本机无 appstreamcli（内建结构断言已过）",
                spec.id
            ),
        );
    }
    warning(
        events,
        &format!(
            "目标 {} 的 AppStream 结构检查：id={}，name={}，summary={}",
            spec.id, internal.id, internal.name, internal.summary
        ),
    );
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// PWA 三腿审计（`pwa-audit`，T-M5-10）：manifest / 离线能力 / 安装性的结构断言。
///
/// 与打包器同一份 manifest 校验（[`poly_bundle::audit_pwa`] → `validate_manifest`），
/// 产物缺陷硬失败（E_VERIFY_0003），不降级成跳过。真浏览器腿（Chrome
/// `Page.getInstallabilityErrors` + 断网重载）不在 verify 进程内跑：它需要浏览器与静态
/// 托管，由 `ui/smoke/pwa/audit.mjs` 在 Chrome 在场时执行；Chrome 缺席时该脚本
/// SKIP-with-print，verify 的结构断言仍是本机的真跑面。
fn verify_pwa_audit(
    project: &Project,
    spec: &TargetSpec,
    ids: &[String],
    events: &mut dyn EventSink,
) -> Result<bool, StageError> {
    let site = project::packages_root(&project.root)
        .join(&spec.id)
        .join("site");
    if !site.is_dir() {
        warning(
            events,
            &format!(
                "目标 {} 未校验：{} 不存在（先跑 poly build 把静态站点流到打包暂存目录）",
                spec.id,
                site.display()
            ),
        );
        return Ok(false);
    }
    let audit = match poly_bundle::audit_pwa(&site) {
        Ok(audit) => audit,
        Err(err) => {
            return Err(webtv_defect(
                spec,
                err.to_string(),
                "补齐 manifest / service worker / ≥144px 图标后重跑 poly build --target chromeos-pwa",
            )
            .into());
        }
    };
    warning(
        events,
        &format!("目标 {} 的 PWA manifest 审计：{}", spec.id, audit.manifest),
    );
    warning(
        events,
        &format!("目标 {} 的 PWA 离线能力审计：{}", spec.id, audit.offline),
    );
    warning(
        events,
        &format!(
            "目标 {} 的 PWA 安装性审计：{}",
            spec.id, audit.installability
        ),
    );
    warning(
        events,
        &format!(
            "目标 {} 的真浏览器腿未在 verify 内跑：`node ui/smoke/pwa/audit.mjs --site {}` \
             （Chrome 缺席时 SKIP-with-print；manifest 结构断言已由本段真跑）",
            spec.id,
            site.display()
        ),
    );
    for id in ids {
        events.emit(&BuildEvent::Verified {
            checker: id.clone(),
            ok: true,
        });
    }
    Ok(true)
}

/// 暂存树内首个 `*.metainfo.xml`（限深，目录按名排序保证确定性）。
fn find_metainfo(dir: &Path, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for path in &entries {
        if path.is_file()
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".metainfo.xml"))
        {
            return Some(path.clone());
        }
    }
    for path in entries {
        if path.is_dir()
            && let Some(found) = find_metainfo(&path, depth - 1)
        {
            return Some(found);
        }
    }
    None
}

/// 对单个目标做二跑比对；返回是否真的比对了（`false` = 告警跳过并已说明原因）。
fn compare(
    project: &Project,
    spec: &TargetSpec,
    host: Host,
    json: bool,
    events: &mut dyn EventSink,
) -> Result<bool, Diagnostic> {
    let providers = crate::providers::registry(project)?;
    if providers.get(&spec.id, &spec.package.format).is_none() {
        warning(
            events,
            &format!(
                "目标 {} 未做复现比对：无 provider（format = {}）",
                spec.id, spec.package.format
            ),
        );
        return Ok(false);
    }

    // 每一跑一个独立绝对目录；同一目标的旧证据先清掉，比对后的产物留作证据
    let root = project.root.join(".polypack").join("repro").join(&spec.id);
    std::fs::remove_dir_all(&root).ok();

    let mut runs = Vec::new();
    let mut run_dirs = Vec::new();
    for tag in ["a", "b"] {
        let out_dir = root.join(tag);
        run_dirs.push(out_dir.clone());
        // 输入组装与 package 阶段共用同一实现（同一份输入，两次独立落点）；
        // `None` = 环境性缺失，该函数已告警并清掉暂存目录，跳过本目标即可
        let Some(inputs) = crate::package::assemble_inputs(project, spec, host, &out_dir, events)?
        else {
            return Ok(false);
        };
        match crate::providers::package(&providers, spec, &inputs) {
            Ok(artifact) => {
                let Some(path) = artifact.path else {
                    return Err(Diagnostic::error(
                        Code::new(codes::E_TGT_0003.as_str()),
                        format!("目标 {} 的 provider 未落盘产物", spec.id),
                    ));
                };
                runs.push(Artifact {
                    target: artifact.target,
                    format: artifact.format,
                    path: PathBuf::from(path),
                    bytes: artifact.bytes,
                    kind: if artifact.directory {
                        ArtifactKind::Directory
                    } else {
                        ArtifactKind::File
                    },
                    notes: artifact.notes,
                });
            }
            // 宿主缺打包工具 / 输入未就绪：与 package 阶段同语义，告警跳过本目标
            Err(ProviderError::Unavailable(reason)) => {
                warning(events, &format!("目标 {} 未做复现比对：{reason}", spec.id));
                return Ok(false);
            }
            Err(other) => return Err(crate::package::provider_diagnostic(spec, other)),
        }
    }

    let (a, b) = (&runs[0], &runs[1]);
    // 签名会改写 zip 字节：已签名的产物不比字节，如实说明并跳过该件（绝不假装通过）
    if let Some(entry) = signed_entry(a).or_else(|| signed_entry(b)) {
        warning(
            events,
            &format!(
                "目标 {} 未做复现比对：产物已签名（{entry}）——签名块与时间戳会改写 zip 字节，\
                 复现比对只对未签名件成立（签名位在 T-M2-09 已接入，比对请用打包暂存目录里的未签名件）",
                spec.id
            ),
        );
        return Ok(false);
    }

    // hap-toolkit 盖章的 .rpk（zip / build.txt 带时间戳，工具无确定性开关）自身字节二跑必不同：
    // 如实说明并把它排除出逐字节比对（其余文件照比），语义同 signed 件的「绝不假装通过」
    if let Some(rpk) = stamped_rpk(&run_dirs[0]).or_else(|| stamped_rpk(&run_dirs[1])) {
        warning(
            events,
            &format!(
                "目标 {} 的 .rpk（{}）由 hap-toolkit 盖章（zip / build.txt 时间戳）——该件不参与\
                 逐字节复现比对，产物其余文件照比",
                spec.id,
                file_label(&rpk)
            ),
        );
    }

    // 声明产物之外，同目录的其余顶层文件（Android：未签名 APK + .apks）也逐件比，
    // 两者任一不一致都收敛到同一条 E_VERIFY_0001
    let compared = compare_artifacts(a, b).and_then(|digest| {
        compare_sibling_files(&run_dirs[0], &run_dirs[1], &file_label(&a.path))
            .map(|files| (digest, files))
    });
    match compared {
        Ok((digest, files)) => {
            project::report(
                "verify",
                json,
                serde_json::json!({
                    "event": "verify-reproducible",
                    "target": spec.id,
                    "format": a.format,
                    "kind": kind_name(a.kind),
                    "digest": digest,
                    "files": files,
                    "match": true,
                }),
            );
            Ok(true)
        }
        Err(problems) => {
            project::report(
                "verify",
                json,
                serde_json::json!({
                    "event": "verify-reproducible",
                    "target": spec.id,
                    "format": a.format,
                    "kind": if a.kind == b.kind { kind_name(a.kind) } else { "mismatch" },
                    "match": false,
                    "mismatches": problems,
                }),
            );
            Err(Diagnostic::error(
                codes::E_VERIFY_0001,
                format!(
                    "目标 {} 二跑产物不一致（{} 项，输入相同即应字节一致）",
                    spec.id,
                    problems.len()
                ),
            )
            .with_hint(format!(
                "不一致项：{}；证据在 {}",
                problems.join("；"),
                root.display()
            )))
        }
    }
}

/// 比对二跑产物：一致 → `Ok(摘要)`；不一致 → `Err(不一致项清单)`。
///
/// 文件产物比整串 `sha384-<base64>`；目录产物逐文件比 SRI 摘要与文件集
/// （缺失 / 摘要不符 / 清单外新增），汇总摘要取「路径 + 摘要」行的 SRI（稳定序）。
fn compare_artifacts(a: &Artifact, b: &Artifact) -> Result<String, Vec<String>> {
    match (a.kind, b.kind) {
        (ArtifactKind::File, ArtifactKind::File) => {
            let digest_a = match digest_of_file(&a.path) {
                Ok(digest) => digest,
                Err(err) => return Err(vec![err]),
            };
            let digest_b = match digest_of_file(&b.path) {
                Ok(digest) => digest,
                Err(err) => return Err(vec![err]),
            };
            if digest_a == digest_b {
                Ok(digest_a)
            } else {
                Err(vec![format!(
                    "{} 与 {} 内容不同：a {digest_a}，b {digest_b}",
                    a.path.display(),
                    b.path.display()
                )])
            }
        }
        (ArtifactKind::Directory, ArtifactKind::Directory) => {
            let manifest_a = match poly_bundle::sri::manifest_of(&a.path) {
                Ok(manifest) => manifest,
                Err(err) => return Err(vec![format!("读取 a 清单失败：{err}")]),
            };
            let manifest_b = match poly_bundle::sri::manifest_of(&b.path) {
                Ok(manifest) => manifest,
                Err(err) => return Err(vec![format!("读取 b 清单失败：{err}")]),
            };
            // hap-toolkit 盖章件（`*.rpk` 内嵌时间戳）不参与逐字节比对（调用侧已告警说明）
            let stamped = |path: &str| path.ends_with(".rpk");
            let mut problems = Vec::new();
            for (path, entry) in &manifest_a.files {
                if stamped(path) {
                    continue;
                }
                match manifest_b.files.get(path) {
                    None => problems.push(format!("b 缺失: {path}")),
                    Some(found) if found.integrity != entry.integrity => problems.push(format!(
                        "摘要不符: {path}（a {}，b {}）",
                        entry.integrity, found.integrity
                    )),
                    Some(_) => {}
                }
            }
            for path in manifest_b.files.keys() {
                if !stamped(path) && !manifest_a.files.contains_key(path) {
                    problems.push(format!("b 清单外新增: {path}"));
                }
            }
            if !problems.is_empty() {
                return Err(problems);
            }
            let canonical: String = manifest_a
                .files
                .iter()
                .filter(|(path, _)| !stamped(path))
                .map(|(path, entry)| format!("{path} {}\n", entry.integrity))
                .collect();
            Ok(poly_bundle::sri::integrity_of(canonical.as_bytes()))
        }
        (kind_a, kind_b) => Err(vec![format!(
            "产物形态不一致：a {:?}，b {:?}",
            kind_a, kind_b
        )]),
    }
}

/// 二跑目录里的其余产物逐件比（返回已比对的文件名，稳定序）。
///
/// 打包器常把一次构建拆成多件落盘（Android：未签名 APK + AAB + .apks），只比「声明的产物」
/// 会漏掉同目录的其余件。只取顶层**文件**：顶层目录是暂存中间物（`.stage`），里面的绝对路径
/// 随落点变，比了必假红。`declared` 是已单独比过的声明产物名，跳过以免重复。
fn compare_sibling_files(
    dir_a: &Path,
    dir_b: &Path,
    declared: &str,
) -> Result<Vec<String>, Vec<String>> {
    /// 顶层文件（名 + 路径，按名排序）。
    fn top_files(dir: &Path, declared: &str) -> Result<Vec<(String, PathBuf)>, String> {
        let entries =
            std::fs::read_dir(dir).map_err(|err| format!("{} 读取失败：{err}", dir.display()))?;
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| format!("{} 读取失败：{err}", dir.display()))?;
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == declared {
                continue;
            }
            files.push((name, entry.path()));
        }
        files.sort();
        Ok(files)
    }

    let (files_a, files_b) = match (top_files(dir_a, declared), top_files(dir_b, declared)) {
        (Ok(files_a), Ok(files_b)) => (files_a, files_b),
        (Err(err), _) | (_, Err(err)) => return Err(vec![err]),
    };
    let mut compared = Vec::new();
    let mut problems = Vec::new();
    for (name, path_a) in &files_a {
        match files_b.iter().find(|(other, _)| other == name) {
            None => problems.push(format!("b 缺失: {name}")),
            Some((_, path_b)) => match (digest_of_file(path_a), digest_of_file(path_b)) {
                (Ok(digest_a), Ok(digest_b)) if digest_a == digest_b => compared.push(name.clone()),
                (Ok(digest_a), Ok(digest_b)) => {
                    problems.push(format!("摘要不符: {name}（a {digest_a}，b {digest_b}）"));
                }
                (Err(err), _) | (_, Err(err)) => problems.push(err),
            },
        }
    }
    for (name, _) in &files_b {
        if !files_a.iter().any(|(other, _)| other == name) {
            problems.push(format!("b 清单外新增: {name}"));
        }
    }
    if problems.is_empty() {
        Ok(compared)
    } else {
        Err(problems)
    }
}

/// 文件产物的 SRI 摘要（失败返回人读原因，进不一致清单）。
fn digest_of_file(path: &Path) -> Result<String, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(poly_bundle::sri::integrity_of(&bytes)),
        Err(err) => Err(format!("{} 读取失败：{err}", file_label(path))),
    }
}

/// zip 产物里的签名块条目（`META-INF/*.SF|*.RSA|*.DSA|*.EC`）；非 zip / 未签名 → `None`。
fn signed_entry(artifact: &Artifact) -> Option<String> {
    if artifact.kind != ArtifactKind::File {
        return None;
    }
    let names = poly_bundle::zipwrite::entry_names(&artifact.path).ok()?;
    names.into_iter().find(|name| {
        let upper = name.to_ascii_uppercase();
        upper.starts_with("META-INF/")
            && SIGNATURE_EXTENSIONS
                .iter()
                .any(|extension| upper.ends_with(extension))
    })
}

/// 二跑产物目录里的 `.rpk`（hap-toolkit 盖章件；浅扫已知落点 `<dir>/miniapp/dist/`）。
fn stamped_rpk(dir: &Path) -> Option<PathBuf> {
    let dist = dir.join(MINIAPP_DIR).join(DIST_DIR);
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dist)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "rpk"))
        .collect();
    found.sort();
    found.pop()
}

/// 产物形态名（JSON 报告的 `kind` 字段）。
fn kind_name(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::File => "file",
        ArtifactKind::Directory => "directory",
    }
}

/// 文件名（报告里比全路径短，且两跑路径必然不同）。
fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// `BundleError` → 诊断（错误码原样透传，`poly::E_BND_*`）。
pub(crate) fn bundle_diagnostic(spec: &TargetSpec, err: &BundleError) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(
        Code::new(err.code()),
        format!("目标 {} 打包失败：{err}", spec.id),
    );
    if let Some(hint) = err.hint() {
        diagnostic = diagnostic.with_hint(hint);
    }
    diagnostic
}

/// 告警（跳过原因如实进事件流）。
pub(crate) fn warning(events: &mut dyn EventSink, message: &str) {
    events.emit(&BuildEvent::Warning {
        stage: "verify".to_string(),
        message: message.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use poly_targets::Registry;

    /// 建最小工程骨架（含 `polypack.toml` 的 package 段：`.app` 名由它派生）。
    fn fixture(tag: &str) -> (std::path::PathBuf, Project) {
        fixture_with(
            tag,
            "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\nentry = \"index.html\"\n[core]\ncrate = \"core\"\n",
        )
    }

    /// 同 [`fixture`]，但用给定 `polypack.toml`（如带 `[budget]` 的夹具）。
    fn fixture_with(tag: &str, toml: &str) -> (std::path::PathBuf, Project) {
        let root =
            std::env::temp_dir().join(format!("polypack-verify-reg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("core")).expect("工程目录可建");
        let config = poly_core::config::PolypackConfig::parse(toml).expect("fixture 配置应合法");
        let project = Project {
            root: root.clone(),
            config,
        };
        (root, project)
    }

    /// 内建注册表的每个 `verify` id 都必须「已接入」或「有归属」：新目标 / 新校验器漏接线即红。
    #[test]
    fn every_declared_verifier_is_served_or_owned() {
        for spec in Registry::builtin().all() {
            for id in &spec.verify {
                assert!(
                    verifier_for(id).is_some() || verifier_owner(id).is_some(),
                    "目标 {} 声明的校验器 {id} 既未接入也没有归属登记",
                    spec.id
                );
            }
        }
        assert!(verifier_for("no-such-verifier").is_none());
        assert!(verifier_owner("no-such-verifier").is_none());
    }

    /// roku 的 `sideload-structure-check` 已由 owner 占位转为真实现（T-M4-15）。
    #[test]
    fn roku_sideload_structure_check_is_served() {
        let roku = Registry::builtin()
            .get("roku")
            .expect("内建 roku 目标")
            .clone();
        assert_eq!(roku.verify, vec!["sideload-structure-check"]);
        assert_eq!(
            verifier_for("sideload-structure-check"),
            Some(Verifier::RokuSideload)
        );
        assert!(
            verifier_owner("sideload-structure-check").is_none(),
            "已接入的校验器不得再留 owner 占位"
        );
    }

    /// KaiOS 的 `kaios-structure-check` 已接入，与打包器共用结构检查。
    #[test]
    fn kaios_structure_check_is_served() {
        let kaios = Registry::builtin()
            .get("kaios")
            .expect("内建 kaios 目标")
            .clone();
        assert_eq!(kaios.verify, vec!["kaios-structure-check"]);
        assert_eq!(
            verifier_for("kaios-structure-check"),
            Some(Verifier::KaiosStructure)
        );
        assert!(
            verifier_owner("kaios-structure-check").is_none(),
            "已接入的校验器不得再留 owner 占位"
        );
    }

    /// HTML5 L2 平台（TikTok Minis / Facebook Instant Games）的结构自检已接入，
    /// 与打包器共用结构检查。
    #[test]
    fn html5_structure_checks_are_served() {
        let registry = Registry::builtin();
        for (target, id, verifier) in [
            (
                "tiktok-minis",
                "tiktok-minis-structure-check",
                Verifier::TiktokMinisStructure,
            ),
            (
                "facebook-instant-games",
                "facebook-instant-games-structure-check",
                Verifier::FacebookInstantGamesStructure,
            ),
        ] {
            let spec = registry
                .get(target)
                .unwrap_or_else(|| panic!("内建 {target} 目标"))
                .clone();
            assert_eq!(spec.verify, vec![id]);
            assert_eq!(verifier_for(id), Some(verifier));
            assert!(
                verifier_owner(id).is_none(),
                "已接入的校验器不得再留 owner 占位"
            );
        }
    }

    /// ChromeOS PWA 轨的 `pwa-audit` 已接入（T-M5-10）；browser 模式下不可安装的
    /// display 与离线能力缺失都由 `poly_bundle::audit_pwa` 硬失败（见 poly-bundle 测试）。
    #[test]
    fn pwa_audit_is_served_for_chromeos_pwa() {
        let pwa = Registry::builtin()
            .get("chromeos-pwa")
            .expect("内建 chromeos-pwa 目标")
            .clone();
        assert_eq!(pwa.verify, vec!["pwa-audit"]);
        assert_eq!(verifier_for("pwa-audit"), Some(Verifier::PwaAudit));
        assert!(
            verifier_owner("pwa-audit").is_none(),
            "已接入的校验器不得再留 owner 占位"
        );
    }

    /// ios 声明的两个 id（`codesign-verify` / `spctl`）都落在 poly-sign 的同一份校验计划上。
    #[test]
    fn ios_verifiers_share_the_poly_sign_plan() {
        let ios = Registry::builtin()
            .get("ios")
            .expect("内建 ios 目标")
            .clone();
        assert_eq!(ios.verify, vec!["codesign-verify", "spctl"]);
        for id in &ios.verify {
            assert_eq!(
                verifier_for(id),
                Some(Verifier::IosCodesign),
                "{id} 应由 poly_sign::ios::plan_verify 覆盖"
            );
        }
        // 计划本体：`.app` 形态 → codesign --verify --deep --strict + spctl --assess
        let root =
            std::env::temp_dir().join(format!("polypack-verify-plan-{}", std::process::id()));
        let app = root.join("Payload").join("fixture-app.app");
        std::fs::create_dir_all(&app).expect("伪造 .app 目录");
        let lines = poly_sign::ios::plan_verify(&app).lines();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].starts_with("codesign --verify --deep --strict"),
            "{lines:?}"
        );
        assert!(
            lines[1].starts_with("spctl --assess --type execute"),
            "{lines:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 未接入的校验器（android 的两个）如实点名归属并跳过，不静默也不硬失败。
    #[test]
    fn unserved_verifiers_are_named_with_their_owner() {
        let (root, project) = fixture("unserved");
        let android = Registry::builtin()
            .get("android")
            .expect("内建 android 目标")
            .clone();
        let mut sink = poly_core::events::HumanSink {
            out: Vec::<u8>::new(),
        };
        assert!(
            !verify_target(&project, &android, &mut sink).expect("未接入不该失败"),
            "android 的校验器都未接入 → 未校验"
        );
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("apksigner-verify"), "{text}");
        assert!(text.contains("bundletool-validate"), "{text}");
        assert!(text.contains("T-M2-09"), "归属要具体到任务：{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// harmonyos 声明的两个 id 都落在已接入实现上（签名校验 + 结构检查）。
    #[test]
    fn harmonyos_verifiers_are_served() {
        let harmonyos = Registry::builtin()
            .get("harmonyos")
            .expect("内建 harmonyos 目标")
            .clone();
        assert_eq!(
            harmonyos.verify,
            vec!["hap-sign-verify", "hap-structure-check"]
        );
        assert_eq!(verifier_for("hap-sign-verify"), Some(Verifier::HapSign));
        assert_eq!(
            verifier_for("hap-structure-check"),
            Some(Verifier::HapStructure)
        );
    }

    /// 校验对象 HAP：约定名已签件优先，只有未签件时退化（结构检查对未签件成立）。
    #[test]
    fn hap_for_verify_prefers_the_signed_convention() {
        let (root, project) = fixture("hap-pick");
        let spec = Registry::builtin()
            .get("harmonyos")
            .expect("内建 harmonyos 目标")
            .clone();
        let staged = root.join(".polypack/packages/harmonyos");
        std::fs::create_dir_all(&staged).expect("暂存目录可建");
        assert_eq!(
            hap_for_verify(&project, &spec),
            None,
            "空目录没有可校验的输入"
        );

        let unsigned = staged.join("fixture-app-0.0.1-unsigned.hap");
        std::fs::write(&unsigned, b"u").expect("写未签件");
        assert_eq!(
            hap_for_verify(&project, &spec),
            Some((unsigned.clone(), false)),
            "只有未签件时退化"
        );

        let signed = staged.join("fixture-app-0.0.1-unsigned-signed.hap");
        std::fs::write(&signed, b"s").expect("写已签件");
        assert_eq!(
            hap_for_verify(&project, &spec),
            Some((signed, true)),
            "约定名已签件优先"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 结构腿：合成 HAP 过检并给 `Verified` 判决；缺必需条目如实硬失败（不是跳过）。
    #[test]
    fn hap_structure_leg_verifies_synthetic_hap_and_fails_hard_on_defects() {
        let (root, project) = fixture("hap-struct");
        let spec = Registry::builtin()
            .get("harmonyos")
            .expect("内建 harmonyos 目标")
            .clone();
        let staged = root.join(".polypack/packages/harmonyos");
        let src = staged.join("src");
        std::fs::create_dir_all(&src).expect("暂存目录可建");
        let module_json = src.join("module.json");
        std::fs::write(
            &module_json,
            "{\"app\":{\"bundleName\":\"com.polypack.fixture\",\"versionCode\":100,\
             \"versionName\":\"0.0.1\"},\"module\":{\"name\":\"entry\",\"type\":\"entry\"}}",
        )
        .expect("写清单");
        let pack_info = src.join("pack.info");
        std::fs::write(&pack_info, "pack-info").expect("写 pack.info");
        let index = src.join("resources.index");
        std::fs::write(&index, "index").expect("写索引");
        let good = staged.join("fixture-app-0.0.1-unsigned.hap");
        poly_bundle::zipwrite::write_zip(
            &[
                ("module.json".to_string(), module_json.clone()),
                ("pack.info".to_string(), pack_info.clone()),
                ("resources.index".to_string(), index),
            ],
            &good,
        )
        .expect("合成 HAP");

        let ids = vec!["hap-structure-check".to_string()];
        let mut sink = poly_core::events::HumanSink {
            out: Vec::<u8>::new(),
        };
        assert!(
            verify_hap_structure(&project, &spec, &ids, &mut sink).expect("结构检查应通过"),
            "合成 HAP 应过检"
        );
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("com.polypack.fixture"), "{text}");
        assert!(text.contains("hap-structure-check"), "{text}");
        assert!(text.contains("→ true"), "{text}");

        // 缺 resources.index → E_BND_0006 硬失败（产物缺陷不得降级成跳过）
        std::fs::remove_file(&good).expect("清掉合格件");
        let bad = staged.join("other-unsigned.hap");
        poly_bundle::zipwrite::write_zip(
            &[
                ("module.json".to_string(), module_json),
                ("pack.info".to_string(), pack_info),
            ],
            &bad,
        )
        .expect("合成缺条目 HAP");
        let err = verify_hap_structure(&project, &spec, &ids, &mut sink)
            .expect_err("缺必需条目必须硬失败");
        assert_eq!(err.diagnostic.code.to_string(), "poly::E_BND_0006");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `.app` 缺位 / 未签名时跳过并说明原因（校验器对未签名产物无从谈起）。
    #[test]
    fn missing_or_unsigned_app_is_skipped_with_a_reason() {
        let (root, project) = fixture("unsigned");
        let ios = Registry::builtin()
            .get("ios")
            .expect("内建 ios 目标")
            .clone();
        let mut sink = poly_core::events::HumanSink {
            out: Vec::<u8>::new(),
        };
        assert!(!verify_target(&project, &ios, &mut sink).expect("缺 .app 不该失败"));
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        // 路径按宿主分隔符打印（Windows 为 `\`）；断言先归一，跨宿主同判。
        let text_unix = text.replace('\\', "/");
        assert!(text.contains("fixture-app.app"), "{text}");
        assert!(text_unix.contains("packages/ios"), "{text}");

        let app = crate::sign::ios_app(&project, &ios);
        std::fs::create_dir_all(&app).expect("伪造 .app 目录");
        sink.out.clear();
        assert!(!verify_target(&project, &ios, &mut sink).expect("未签名不该失败"));
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("未签名"), "{text}");
        assert!(
            text.replace('\\', "/")
                .contains("_CodeSignature/CodeResources"),
            "{text}"
        );

        // 已签名（布局就位）→ 走到计划校验；执行在非 macOS 宿主按 E_SIG_0001 跳过
        std::fs::create_dir_all(app.join("_CodeSignature")).expect("伪造签名目录");
        std::fs::write(app.join("_CodeSignature/CodeResources"), b"<xml/>").expect("写签名件");
        sink.out.clear();
        // 非 macOS 宿主：E_SIG_0001（缺 codesign）归入跳过；macOS 上会真跑 codesign，
        // 对伪造的 .app 必然以 E_SIG_0003 硬失败（该签没签成 ≠ 校验通过）
        let macos = current_host() == Some(Host::Macos);
        match verify_target(&project, &ios, &mut sink) {
            Ok(checked) => {
                assert!(
                    !checked,
                    "非 macOS 宿主应把 E_SIG_0001 归入跳过：{}",
                    String::from_utf8_lossy(&sink.out)
                );
                assert!(!macos, "macOS 上伪造 .app 不该校验通过");
            }
            Err(err) => assert!(macos, "只有 macOS 宿主才可能真的执行 codesign：{err}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 目录产物比对的三类不一致都要点名（缺失 / 摘要不符 / 清单外新增），一致时给汇总摘要。
    #[test]
    fn directory_comparison_names_every_kind_of_difference() {
        let root = std::env::temp_dir().join(format!("polypack-verify-cmp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (dir_a, dir_b) = (root.join("a"), root.join("b"));
        for dir in [&dir_a, &dir_b] {
            std::fs::create_dir_all(dir).expect("暂存目录可建");
        }
        std::fs::write(dir_a.join("index.html"), b"one").expect("写 a");
        std::fs::write(dir_b.join("index.html"), b"one").expect("写 b");
        let artifact = |path: &Path| {
            poly_bundle::fsutil::dir_artifact("web", "static-site", path, Vec::new())
                .expect("构造目录产物")
        };
        let (a, b) = (artifact(&dir_a), artifact(&dir_b));

        let digest = compare_artifacts(&a, &b).expect("同输入二跑应一致");
        assert!(digest.starts_with("sha384-"), "{digest}");

        std::fs::write(dir_b.join("index.html"), b"two").expect("改写 b");
        let problems = compare_artifacts(&a, &b).expect_err("内容不同应报不一致");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("摘要不符: index.html"), "{problems:?}");

        std::fs::write(dir_b.join("index.html"), b"one").expect("还原 b");
        std::fs::write(dir_b.join("extra.txt"), b"x").expect("b 多写一件");
        let problems = compare_artifacts(&a, &b).expect_err("清单外新增应报不一致");
        assert!(
            problems[0].contains("b 清单外新增: extra.txt"),
            "{problems:?}"
        );

        std::fs::remove_file(dir_b.join("extra.txt")).expect("清掉多余件");
        std::fs::write(dir_a.join("only-a.txt"), b"y").expect("a 多写一件");
        let problems = compare_artifacts(&a, &b).expect_err("b 缺件应报不一致");
        assert!(problems[0].contains("b 缺失: only-a.txt"), "{problems:?}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 文件产物（apk / ipa / 安装包）比整串 SRI：一致给摘要，不一致两名摘要都要报出。
    #[test]
    fn file_comparison_reports_both_digests() {
        let root =
            std::env::temp_dir().join(format!("polypack-verify-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("暂存目录可建");
        let (path_a, path_b) = (root.join("a.apk"), root.join("b.apk"));
        std::fs::write(&path_a, b"same").expect("写 a");
        std::fs::write(&path_b, b"same").expect("写 b");
        let artifact = |path: &Path| Artifact {
            target: "android".to_string(),
            format: "apk".to_string(),
            path: path.to_path_buf(),
            bytes: 0,
            kind: ArtifactKind::File,
            notes: Vec::new(),
        };

        assert!(
            compare_artifacts(&artifact(&path_a), &artifact(&path_b))
                .expect("同内容应一致")
                .starts_with("sha384-")
        );
        std::fs::write(&path_b, b"other").expect("改写 b");
        let problems = compare_artifacts(&artifact(&path_a), &artifact(&path_b))
            .expect_err("内容不同应报不一致");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("a.apk"), "{problems:?}");
        assert!(problems[0].contains("b.apk"), "{problems:?}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 同目录的其余产物逐件比（Android 三件套），声明的产物排除、顶层暂存目录排除。
    #[test]
    fn sibling_files_compare_covers_the_rest_of_the_run_dir() {
        let root = std::env::temp_dir().join(format!("polypack-verify-sib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (dir_a, dir_b) = (root.join("a"), root.join("b"));
        for dir in [&dir_a, &dir_b] {
            std::fs::create_dir_all(dir.join("app.stage")).expect("暂存目录可建");
            std::fs::write(dir.join("app-0.1.0.aab"), b"declared").expect("写声明产物");
            std::fs::write(dir.join("app-0.1.0-unsigned.apk"), b"apk").expect("写 APK");
            std::fs::write(dir.join("app-0.1.0.apks"), b"apks").expect("写 APKS");
            // 暂存中间物里是落点的绝对路径（两跑必然不同）：不进比对
            std::fs::write(dir.join("app.stage/abs.txt"), dir.display().to_string())
                .expect("写暂存件");
        }
        // 声明产物两跑不同：已被排除，不影响结论（它由 compare_artifacts 单独比）
        std::fs::write(dir_b.join("app-0.1.0.aab"), b"declared-other").expect("改写声明产物");

        let files = compare_sibling_files(&dir_a, &dir_b, "app-0.1.0.aab").expect("二跑应一致");
        assert_eq!(
            files,
            vec!["app-0.1.0-unsigned.apk", "app-0.1.0.apks"],
            "{files:?}"
        );

        std::fs::write(dir_b.join("app-0.1.0.apks"), b"other").expect("改写 b 的 APKS");
        let problems = compare_sibling_files(&dir_a, &dir_b, "app-0.1.0.aab")
            .expect_err("APKS 不同应报不一致");
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].contains("摘要不符: app-0.1.0.apks"),
            "{problems:?}"
        );

        std::fs::write(dir_b.join("app-0.1.0.apks"), b"apks").expect("还原 b");
        std::fs::remove_file(dir_b.join("app-0.1.0-unsigned.apk")).expect("b 缺件");
        let problems =
            compare_sibling_files(&dir_a, &dir_b, "app-0.1.0.aab").expect_err("b 缺件应报不一致");
        assert!(
            problems[0].contains("b 缺失: app-0.1.0-unsigned.apk"),
            "{problems:?}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    // ------------------------------------------------------------ 快应用 lint 腿（T-M3-12）

    /// 子进程标记（env 注入走子进程：Rust 2024 进程内 `set_var` 是 unsafe，本仓 forbid）。
    const LINT_CHILD: &str = "POLYPACK_LINT_CHILD_TEST";

    /// 以注入环境变量精确重跑本测试（子进程腿；口径同 poly-publish/tests/credentials.rs）。
    fn rerun_with_env(test_name: &str, envs: &[(&str, &str)]) {
        let exe = std::env::current_exe().expect("测试可执行路径");
        let full = format!("verify::tests::{test_name}");
        let mut command = std::process::Command::new(exe);
        command.args([full.as_str(), "--exact", "--nocapture"]);
        for (key, value) in envs {
            command.env(key, value);
        }
        let status = command.status().expect("子进程可执行");
        assert!(status.success(), "子进程断言失败（见上方输出）");
    }

    /// 合成 stub hap-toolkit 安装根（真工具缺席时测接线）：`bin/hap` + `package.json` +
    /// `lib/gen-webpack-conf/validate.js`（`validateJson` 只认 `package == "com.polypack.good"`）。
    fn stub_toolkit(root: &Path) {
        let bin = root.join("bin");
        let conf = root.join("lib/gen-webpack-conf");
        std::fs::create_dir_all(&bin).expect("stub bin 可建");
        std::fs::create_dir_all(&conf).expect("stub conf 可建");
        std::fs::write(bin.join("hap"), b"#!/bin/sh\nexit 0\n").expect("写假 hap");
        std::fs::write(root.join("package.json"), "{}\n").expect("写假 package.json");
        std::fs::write(
            conf.join("validate.js"),
            "exports.validateJson = function (manifest, label) {\n  \
             if (manifest.package === 'com.polypack.good') return [];\n  \
             return [new Error('校验 ' + label + ' 错误（stub toolkit）：package 须为 com.polypack.good')];\n};\n",
        )
        .expect("写 stub validate.js");
    }

    /// 合成 `.rpk`（zip：条目名 → 内容文本），落 `<dir>/<name>`。
    fn synth_rpk(dir: &Path, name: &str, entries: &[(&str, &str)]) -> PathBuf {
        let parts = dir.join("parts");
        std::fs::create_dir_all(&parts).expect("条目源目录可建");
        let mut listed = Vec::new();
        for (entry, body) in entries {
            let path = parts.join(entry.replace('/', "_"));
            std::fs::write(&path, body).expect("写条目源文件");
            listed.push(((*entry).to_string(), path));
        }
        std::fs::create_dir_all(dir).expect("dist 可建");
        let out = dir.join(name);
        poly_bundle::zipwrite::write_zip(&listed, &out).expect("合成 rpk");
        out
    }

    /// quickapp 声明的 `hap-toolkit-lint` 已接入（服务归属登记同步移除，测试防回退）。
    #[test]
    fn quickapp_lint_verifier_is_served() {
        let quickapp = Registry::builtin()
            .get("quickapp")
            .expect("内建 quickapp 目标")
            .clone();
        assert_eq!(quickapp.verify, vec!["hap-toolkit-lint"]);
        assert_eq!(
            verifier_for("hap-toolkit-lint"),
            Some(Verifier::QuickAppLint)
        );
        assert!(
            verifier_owner("hap-toolkit-lint").is_none(),
            "已接入的校验器不得再挂归属登记"
        );
    }

    /// 产物无 `.rpk`（打包时工具缺席）→ 告警跳过；该判定先于工具链探测，任意环境可跑。
    #[test]
    fn quickapp_lint_skips_without_rpk_artifact() {
        let (root, project) = fixture("qa-no-rpk");
        let spec = Registry::builtin()
            .get("quickapp")
            .expect("内建 quickapp 目标")
            .clone();
        let mut sink = poly_core::events::HumanSink {
            out: Vec::<u8>::new(),
        };
        assert!(
            !verify_quickapp_lint(
                &project,
                &spec,
                &["hap-toolkit-lint".to_string()],
                &mut sink
            )
            .expect("无产物不该失败"),
            "无 .rpk → 未校验"
        );
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("dist/*.rpk"), "{text}");
        assert!(text.contains("hap-toolkit"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// lint 腿（stub toolkit 代真工具）：接线（node 驱动 + 判决解析）与三态——
    /// schema 命中 / 结构缺条目 = E_VERIFY_0003 硬失败，合规件给 `Verified`。
    ///
    /// 真 hap-toolkit 的 schema 语义（ajv + manifest-schema）在 real 契约腿（工具在场时）断言。
    #[test]
    fn quickapp_lint_leg_checks_rpk_and_runs_schema_validation() {
        if std::env::var_os(LINT_CHILD).is_some() {
            let (root, project) = fixture("qa-lint-child");
            let spec = Registry::builtin()
                .get("quickapp")
                .expect("内建 quickapp 目标")
                .clone();
            let staged = root.join(".polypack/packages/quickapp/miniapp");
            let src = staged.join("src");
            let dist = staged.join("dist");
            std::fs::create_dir_all(&src).expect("src 可建");
            let ids = vec!["hap-toolkit-lint".to_string()];
            let mut sink = poly_core::events::HumanSink {
                out: Vec::<u8>::new(),
            };
            let good_manifest = "{\"package\":\"com.polypack.good\",\"versionName\":\"0.1.0\",\
                                 \"versionCode\":100,\"router\":{\"entry\":\"pages/index\"}}";
            let rpk_entries = [
                ("manifest.json", good_manifest),
                ("app.js", "App({})"),
                ("META-INF/CERT", "cert"),
            ];

            // 例 1：合规件 → 通过并给 `hap-toolkit-lint` 的 Verified 判决
            std::fs::write(src.join("manifest.json"), good_manifest).expect("写源清单");
            synth_rpk(&dist, "com.good.debug.0.1.0.rpk", &rpk_entries);
            let ok = verify_quickapp_lint(&project, &spec, &ids, &mut sink).expect("合规件应通过");
            let text = String::from_utf8_lossy(&sink.out).into_owned();
            assert!(ok, "合规件应校验过；告警流：{text}");
            assert!(text.contains("hap-toolkit-lint"), "{text}");
            assert!(text.contains("com.polypack.good"), "{text}");
            assert!(text.contains("→ true"), "{text}");

            // 例 2：schema 命中（stub 拒绝 package）→ E_VERIFY_0003 硬失败
            let bad_manifest = "{\"package\":\"com.polypack.bad\",\"versionName\":\"0.1.0\",\
                                \"versionCode\":100}";
            std::fs::write(src.join("manifest.json"), bad_manifest).expect("改写源清单");
            let _ = std::fs::remove_dir_all(&dist);
            synth_rpk(
                &dist,
                "com.bad.debug.0.1.0.rpk",
                &[
                    ("manifest.json", bad_manifest),
                    ("app.js", "App({})"),
                    ("META-INF/CERT", "cert"),
                ],
            );
            let err = verify_quickapp_lint(&project, &spec, &ids, &mut sink)
                .expect_err("schema 命中必须硬失败");
            assert_eq!(err.diagnostic.code.to_string(), "poly::E_VERIFY_0003");
            assert!(err.to_string().contains("schema 校验命中"), "{err}");

            // 例 3：rpk 缺必需条目（app.js）→ E_VERIFY_0003 硬失败（结构缺陷不降级）
            std::fs::write(src.join("manifest.json"), good_manifest).expect("源清单回合规");
            let _ = std::fs::remove_dir_all(&dist);
            synth_rpk(
                &dist,
                "com.good2.debug.0.1.0.rpk",
                &[("manifest.json", good_manifest), ("META-INF/CERT", "cert")],
            );
            let err = verify_quickapp_lint(&project, &spec, &ids, &mut sink)
                .expect_err("缺必需条目必须硬失败");
            assert_eq!(err.diagnostic.code.to_string(), "poly::E_VERIFY_0003");
            assert!(err.to_string().contains("缺必需条目"), "{err}");
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        // 父进程：造 stub toolkit 后以 POLYPACK_HAP 注入重跑
        let kit = std::env::temp_dir().join(format!("polypack-lint-kit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&kit);
        stub_toolkit(&kit);
        rerun_with_env(
            "quickapp_lint_leg_checks_rpk_and_runs_schema_validation",
            &[
                (LINT_CHILD, "1"),
                (
                    "POLYPACK_HAP",
                    kit.join("bin/hap").to_str().expect("stub 路径"),
                ),
            ],
        );
        let _ = std::fs::remove_dir_all(&kit);
    }

    /// 工具链（hap-toolkit）缺席腿：`.rpk` 在场但工具不在 → 告警跳过，不假绿也不硬失败。
    #[test]
    fn quickapp_lint_skips_when_toolkit_is_absent() {
        if std::env::var_os(LINT_CHILD).is_some() {
            let (root, project) = fixture("qa-lint-nokit");
            let spec = Registry::builtin()
                .get("quickapp")
                .expect("内建 quickapp 目标")
                .clone();
            let dist = root.join(".polypack/packages/quickapp/miniapp/dist");
            let manifest = "{\"package\":\"com.polypack.good\",\"versionName\":\"0.1.0\",\
                            \"versionCode\":100}";
            synth_rpk(
                &dist,
                "com.good.debug.0.1.0.rpk",
                &[
                    ("manifest.json", manifest),
                    ("app.js", "App({})"),
                    ("META-INF/CERT", "cert"),
                ],
            );
            let mut sink = poly_core::events::HumanSink {
                out: Vec::<u8>::new(),
            };
            assert!(
                !verify_quickapp_lint(
                    &project,
                    &spec,
                    &["hap-toolkit-lint".to_string()],
                    &mut sink
                )
                .expect("缺工具不该失败"),
                ".rpk 在场但工具缺席 → 未校验"
            );
            let text = String::from_utf8_lossy(&sink.out).into_owned();
            assert!(text.contains("未校验"), "{text}");
            assert!(text.contains("POLYPACK_HAP"), "{text}");
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let missing = std::env::temp_dir()
            .join(format!("polypack-lint-nokit-{}", std::process::id()))
            .join("hap");
        rerun_with_env(
            "quickapp_lint_skips_when_toolkit_is_absent",
            &[
                (LINT_CHILD, "1"),
                ("POLYPACK_HAP", missing.to_str().expect("路径")),
            ],
        );
    }

    /// 复现比对把 hap-toolkit 盖章的 `.rpk` 排除在逐字节比对之外（其余文件照比、照报差异）。
    #[test]
    fn stamped_rpk_is_carved_out_of_reproducible_compare() {
        let root = std::env::temp_dir().join(format!("polypack-repro-rpk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let make = |tag: &str, rpk: &[u8], extra: &str| -> Artifact {
            let miniapp = root.join(tag).join("miniapp");
            std::fs::create_dir_all(miniapp.join("src")).expect("src 可建");
            std::fs::create_dir_all(miniapp.join("dist")).expect("dist 可建");
            std::fs::write(miniapp.join("src/manifest.json"), "{}").expect("写源清单");
            std::fs::write(miniapp.join("src/extra.js"), extra).expect("写对照件");
            std::fs::write(miniapp.join("dist/x.rpk"), rpk).expect("写 rpk");
            Artifact {
                target: "quickapp".to_string(),
                format: "quickapp".to_string(),
                path: miniapp,
                bytes: 0,
                kind: ArtifactKind::Directory,
                notes: Vec::new(),
            }
        };
        let a = make("a", b"rpk-run-a", "same");
        let b = make("b", b"rpk-run-b", "same");
        assert!(
            compare_artifacts(&a, &b).is_ok(),
            ".rpk 差异（盖章时间戳）应被排除出比对"
        );
        assert_eq!(
            stamped_rpk(&root.join("a")),
            Some(root.join("a/miniapp/dist/x.rpk")),
            "盖章件定位"
        );

        // 对照：非 .rpk 文件差异仍如实报出
        std::fs::write(root.join("b/miniapp/src/extra.js"), "changed").expect("改写对照件");
        let problems = compare_artifacts(&a, &b).expect_err("非 rpk 差异必须报出");
        assert!(
            problems.iter().any(|item| item.contains("src/extra.js")),
            "{problems:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// web 系 TV（T-M4-08/09/10）三个校验器 id 都落在已接入实现上。
    #[test]
    fn web_tv_verifiers_are_served() {
        assert_eq!(verifier_for("wgt-lint"), Some(Verifier::WgtLint));
        assert_eq!(verifier_for("ares-package"), Some(Verifier::AresPackage));
        assert_eq!(
            verifier_for("sandbox-selfcheck"),
            Some(Verifier::SandboxSelfcheck)
        );
        for id in ["tizen-tv", "webos-tv", "vidaa-tv", "smartcast-tv"] {
            let spec = Registry::builtin()
                .get(id)
                .cloned()
                .unwrap_or_else(|| panic!("内建注册表缺少 {id}"));
            assert_eq!(spec.tier, 3, "{id} 应为 L3（tier 3）");
        }
    }

    /// T-M5-07：portcheck 已接入；合成 ports 骨架过结构检查并发出 `Verified`，
    /// PLIST 与载荷漂移的坏件硬失败（portcheck 缺席时内建结构断言即真跑面）。
    #[test]
    fn portcheck_leg_verifies_synthetic_ports_and_fails_hard_on_defects() {
        assert_eq!(verifier_for("portcheck"), Some(Verifier::Portcheck));
        let (root, project) = fixture("ports-leg");
        let spec = Registry::builtin()
            .get("openbsd")
            .expect("内建 openbsd 目标")
            .clone();
        let port = root.join(".polypack/packages/openbsd/ports/fixture-app-0.0.1");
        std::fs::create_dir_all(port.join("pkg")).expect("建 ports 目录");
        std::fs::write(
            port.join("Makefile"),
            "COMMENT =\tFixture app\nDISTNAME =\tfixture-app-0.0.1\nCATEGORIES =\tx11\n\
             HOMEPAGE =\thttps://example.com\nMAINTAINER =\tFixture <ports@example.com>\n\
             PERMIT_PACKAGE =\tYes\n\nNO_BUILD =\tYes\nNO_TEST =\tYes\n\ndo-install:\n\
             \tcp -R ${FILESDIR}/payload/. ${PREFIX}/\n\n.include <bsd.port.mk>\n",
        )
        .expect("写 Makefile");
        std::fs::write(port.join("pkg/DESCR"), "Fixture app.\n").expect("写 DESCR");
        std::fs::write(
            port.join("pkg/PLIST"),
            "@bin bin/fixture-app\nshare/applications/fixture-app.desktop\n",
        )
        .expect("写 PLIST");
        std::fs::create_dir_all(port.join("files/payload/bin")).expect("建载荷目录");
        std::fs::create_dir_all(port.join("files/payload/share/applications")).expect("建载荷目录");
        std::fs::write(port.join("files/payload/bin/fixture-app"), "#!/bin/sh\n").expect("写载荷");
        std::fs::write(
            port.join("files/payload/share/applications/fixture-app.desktop"),
            "[Desktop Entry]\n",
        )
        .expect("写 desktop");

        let mut sink = poly_core::events::HumanSink {
            out: Vec::<u8>::new(),
        };
        assert!(
            verify_target(&project, &spec, &mut sink).expect("ports 结构检查应通过"),
            "合成 ports 骨架应过检"
        );
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("ports 结构检查"), "{text}");
        assert!(text.contains("fixture-app-0.0.1"), "{text}");
        assert!(text.contains("portcheck"), "{text}");

        // 载荷缺 PLIST 声明的文件 → 产物缺陷硬失败，不降级成跳过
        std::fs::remove_file(port.join("files/payload/bin/fixture-app")).expect("移除载荷");
        let err = verify_target(&project, &spec, &mut sink).expect_err("PLIST 漂移必须硬失败");
        assert_eq!(err.diagnostic.code.to_string(), "poly::E_BND_0006");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// T-M5-06：pkg-check 已接入；合成 pkg 暂存树 + `+MANIFEST` 过结构检查并发出 `Verified`，
    /// manifest 与载荷漂移的坏件硬失败（pkg(8) 缺席时内建结构断言即真跑面）。
    #[test]
    fn pkg_check_leg_verifies_synthetic_stage_and_fails_hard_on_defects() {
        assert_eq!(verifier_for("pkg-check"), Some(Verifier::PkgCheck));
        let (root, project) = fixture("pkg-leg");
        let spec = Registry::builtin()
            .get("freebsd")
            .expect("内建 freebsd 目标")
            .clone();
        let staged = root.join(".polypack/packages/freebsd");
        let stage = staged.join("fixture-app-0.0.1.pkgstage");
        std::fs::create_dir_all(stage.join("usr/local/bin")).expect("建 pkg 前缀目录");
        std::fs::create_dir_all(stage.join("usr/local/share/applications")).expect("建 pkg 目录");
        std::fs::write(
            stage.join("usr/local/bin/fixture-app"),
            "#!/bin/sh\nexit 0\n",
        )
        .expect("写载荷");
        std::fs::write(
            stage.join("usr/local/share/applications/fixture-app.desktop"),
            "[Desktop Entry]\nName=Fixture\n",
        )
        .expect("写 desktop");
        std::fs::write(
            staged.join("fixture-app-0.0.1.manifest"),
            "name: \"fixture-app\"\nversion: \"0.0.1\"\norigin: \"misc/fixture-app\"\n\
             comment: \"Fixture app\"\ndesc: \"Fixture app\"\nwww: \"https://example.com\"\n\
             maintainer: \"Fixture <ports@example.com>\"\nprefix: \"/usr/local\"\n",
        )
        .expect("写 +MANIFEST");

        let mut sink = poly_core::events::HumanSink {
            out: Vec::<u8>::new(),
        };
        assert!(
            verify_target(&project, &spec, &mut sink).expect("pkg 结构检查应通过"),
            "合成 pkg 暂存树应过检"
        );
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("pkg 结构检查"), "{text}");
        assert!(text.contains("fixture-app-0.0.1"), "{text}");
        assert!(text.contains("pkg check"), "{text}");

        // 载荷缺 manifest name 对应的可执行 → 产物缺陷硬失败，不降级成跳过
        std::fs::remove_file(stage.join("usr/local/bin/fixture-app")).expect("移除载荷");
        let err = verify_target(&project, &spec, &mut sink).expect_err("载荷漂移必须硬失败");
        assert_eq!(err.diagnostic.code.to_string(), "poly::E_BND_0006");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// T-M5-08：pkgsrc-check 已接入；合成 pkgsrc 骨架过结构检查并发出 `Verified`，
    /// PLIST 与载荷漂移的坏件硬失败（bmake 缺席或树外骨架时内建结构断言即真跑面）。
    #[test]
    fn pkgsrc_check_leg_verifies_synthetic_skeleton_and_fails_hard_on_defects() {
        assert_eq!(verifier_for("pkgsrc-check"), Some(Verifier::PkgsrcCheck));
        let (root, project) = fixture("pkgsrc-leg");
        let spec = Registry::builtin()
            .get("netbsd")
            .expect("内建 netbsd 目标")
            .clone();
        let port = root.join(".polypack/packages/netbsd/pkgsrc/fixture-app-0.0.1");
        std::fs::create_dir_all(port.join("files/payload/bin")).expect("建载荷目录");
        std::fs::create_dir_all(port.join("files/payload/share/applications")).expect("建载荷目录");
        std::fs::write(port.join("files/payload/bin/fixture-app"), "#!/bin/sh\n").expect("写载荷");
        std::fs::write(
            port.join("files/payload/share/applications/fixture-app.desktop"),
            "[Desktop Entry]\n",
        )
        .expect("写 desktop");
        std::fs::write(
            port.join("Makefile"),
            "DISTNAME=\tfixture-app-0.0.1\nCATEGORIES=\tmisc\n\
             MAINTAINER=\tFixture <ports@example.com>\nCOMMENT=\tFixture app\n\
             LICENSE=\tapache-2.0\nNO_CHECKSUM=\tyes\nNO_CONFIGURE=\tyes\nNO_BUILD=\tyes\n\n\
             do-extract:\n\t${MKDIR} ${WRKSRC}\n\
             \t${CP} -R ${FILESDIR}/payload/. ${WRKSRC}/\n\ndo-install:\n\
             \t${INSTALL_PROGRAM} ${WRKSRC}/bin/fixture-app ${DESTDIR}${PREFIX}/bin/fixture-app\n\n\
             .include \"../../mk/bsd.pkg.mk\"\n",
        )
        .expect("写 Makefile");
        std::fs::write(port.join("DESCR"), "Fixture app.\n").expect("写 DESCR");
        std::fs::write(
            port.join("PLIST"),
            "bin/fixture-app\nshare/applications/fixture-app.desktop\n",
        )
        .expect("写 PLIST");

        let mut sink = poly_core::events::HumanSink {
            out: Vec::<u8>::new(),
        };
        assert!(
            verify_target(&project, &spec, &mut sink).expect("pkgsrc 结构检查应通过"),
            "合成 pkgsrc 骨架应过检"
        );
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("pkgsrc 结构检查"), "{text}");
        assert!(text.contains("fixture-app-0.0.1"), "{text}");
        assert!(text.contains("bmake"), "{text}");

        // 载荷缺 PLIST 声明的文件 → 产物缺陷硬失败，不降级成跳过
        std::fs::remove_file(port.join("files/payload/bin/fixture-app")).expect("移除载荷");
        let err = verify_target(&project, &spec, &mut sink).expect_err("PLIST 漂移必须硬失败");
        assert_eq!(err.diagnostic.code.to_string(), "poly::E_BND_0006");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Tizen 腿：真造 `.wgt` → 结构检查过检并发出 `Verified`；缺清单的坏件硬失败（不降级）。
    #[test]
    fn wgt_leg_verifies_synthetic_package_and_fails_hard_on_defects() {
        let (root, project) = fixture("wgt-leg");
        let spec = Registry::builtin()
            .get("tizen-tv")
            .expect("内建 tizen-tv 目标")
            .clone();
        let staged = root.join(".polypack/packages/tizen-tv");
        std::fs::create_dir_all(&staged).expect("暂存目录可建");
        let index = staged.join("index.html");
        std::fs::write(&index, "<!doctype html><html></html>").expect("写入口");
        let icon = staged.join("icon.png");
        std::fs::write(&icon, b"\x89PNG").expect("写图标");
        let config = staged.join("config.xml");
        std::fs::write(
            &config,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <widget xmlns=\"http://www.w3.org/ns/widgets\" xmlns:tizen=\"http://tizen.org/ns/widgets\" \
             id=\"com.polypack.fixture.fixtureapp\" version=\"0.0.1\" width=\"1920\" height=\"1080\" \
             viewmodes=\"maximized\">\n\
             \x20 <icon src=\"icon.png\"/>\n\
             \x20 <content src=\"index.html\"/>\n\
             \x20 <tizen:application id=\"com.polypack.fixture.fixtureapp\" \
             package=\"com.polypack.fixture\" required_version=\"2.3\"/>\n\
             \x20 <tizen:profile name=\"tv-samsung\"/>\n\
             </widget>\n",
        )
        .expect("写清单");
        let good = staged.join("fixture-app-0.0.1.wgt");
        poly_bundle::zipwrite::write_zip(
            &[
                ("config.xml".to_string(), config.clone()),
                ("icon.png".to_string(), icon),
                ("index.html".to_string(), index),
            ],
            &good,
        )
        .expect("合成 wgt");

        let mut sink = poly_core::events::HumanSink {
            out: Vec::<u8>::new(),
        };
        assert!(
            verify_target(&project, &spec, &mut sink).expect("结构检查应通过"),
            "合成 .wgt 应过检"
        );
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains(".wgt 结构检查"), "{text}");
        assert!(text.contains("com.polypack.fixture.fixtureapp"), "{text}");
        assert!(text.contains("wgt-lint"), "{text}");

        // 坏件（缺 config.xml）→ 硬失败，不降级成跳过
        std::fs::remove_file(&good).expect("清掉合格件");
        let bad_index = staged.join("index.html");
        let bad = staged.join("other-0.0.1.wgt");
        poly_bundle::zipwrite::write_zip(&[("index.html".to_string(), bad_index)], &bad)
            .expect("合成缺清单 wgt");
        let err = verify_target(&project, &spec, &mut sink).expect_err("缺清单必须硬失败");
        assert_eq!(err.diagnostic.code.to_string(), "poly::E_BND_0006");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// VIDAA / Vizio 沙箱自检腿：过检发 `Verified`；体积超 `[budget].size` 且 `fail` 策略硬失败。
    #[test]
    fn sandbox_leg_enforces_entry_manifest_and_budget() {
        let (root, project) = fixture("sandbox-leg");
        let spec = Registry::builtin()
            .get("vidaa-tv")
            .expect("内建 vidaa-tv 目标")
            .clone();
        let staged = root.join(".polypack/packages/vidaa-tv");
        std::fs::create_dir_all(&staged).expect("暂存目录可建");
        let index = staged.join("index.html");
        std::fs::write(&index, "<!doctype html><html></html>").expect("写入口");
        let manifest = staged.join("manifest.json");
        std::fs::write(
            &manifest,
            "{\"pkgname\":\"com.polypack.fixture\",\"appname\":\"Fixture\",\
             \"version\":\"0.0.1\",\"page\":{\"index\":\"index.html\"}}",
        )
        .expect("写清单");
        let package = staged.join("fixture-app-0.0.1.zip");
        poly_bundle::zipwrite::write_zip(
            &[
                ("index.html".to_string(), index.clone()),
                ("manifest.json".to_string(), manifest.clone()),
            ],
            &package,
        )
        .expect("合成应用包");

        let mut sink = poly_core::events::HumanSink {
            out: Vec::<u8>::new(),
        };
        assert!(
            verify_target(&project, &spec, &mut sink).expect("沙箱自检应通过"),
            "合成应用包应过检"
        );
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("沙箱自检"), "{text}");
        assert!(text.contains("不设门禁"), "未设预算须如实说明：{text}");

        // 预算 fail：size 调到 1 字节 → E_BUDGET_0001 硬失败（体积规则接进 verify）
        let (root2, project2) = fixture_with(
            "sandbox-budget",
            "[package]\nname = \"fixture-app\"\nversion = \"0.0.1\"\n[ui]\nframework = \"react\"\n\
             entry = \"index.html\"\n[core]\ncrate = \"core\"\n[budget]\nsize = 1\npolicy = \"fail\"\n",
        );
        let staged2 = root2.join(".polypack/packages/vidaa-tv");
        std::fs::create_dir_all(&staged2).expect("暂存目录可建");
        std::fs::write(staged2.join("index.html"), "<html></html>").expect("写入口");
        std::fs::write(
            staged2.join("manifest.json"),
            "{\"pkgname\":\"com.polypack.fixture\",\"appname\":\"Fixture\",\
             \"version\":\"0.0.1\",\"page\":{\"index\":\"index.html\"}}",
        )
        .expect("写清单");
        let package2 = staged2.join("fixture-app-0.0.1.zip");
        poly_bundle::zipwrite::write_zip(
            &[
                ("index.html".to_string(), staged2.join("index.html")),
                ("manifest.json".to_string(), staged2.join("manifest.json")),
            ],
            &package2,
        )
        .expect("合成应用包");
        let err = verify_target(&project2, &spec, &mut sink).expect_err("超预算必须硬失败");
        assert_eq!(err.diagnostic.code.to_string(), "poly::E_BUDGET_0001");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    /// Roku 侧载结构自检三态：合规过检发 `Verified`（含设备腿 SKIP-with-print）/
    /// 缺条目 E_BND_0006 硬失败 / 缺产物告警跳过并说明原因。
    #[test]
    fn roku_sideload_leg_covers_pass_fail_and_skip() {
        let (root, project) = fixture("roku-sideload");
        let spec = Registry::builtin()
            .get("roku")
            .expect("内建 roku 目标")
            .clone();
        let staged = root.join(".polypack/packages/roku");
        let mut sink = poly_core::events::HumanSink {
            out: Vec::<u8>::new(),
        };

        // 三态 ①：缺产物 → 跳过并点名落点，不硬失败
        assert!(!verify_target(&project, &spec, &mut sink).expect("缺产物不该失败"));
        let text = String::from_utf8_lossy(&sink.out).replace('\\', "/");
        assert!(text.contains("没有 *.zip"), "{text}");
        assert!(text.contains(".polypack/packages/roku"), "{text}");

        // 合规五件（与 poly-bundle 结构检查的口径一致）
        let files = staged.join("files");
        std::fs::create_dir_all(&files).expect("暂存目录可建");
        let write = |name: &str, body: &str| {
            let path = files.join(name);
            std::fs::write(&path, body).expect("写频道件");
            path
        };
        let manifest = write(
            "manifest",
            "title=Fixture Channel\nmajor_version=1\nminor_version=2\nbuild_version=3\n\
             ui_resolutions=fhd\n",
        );
        let main = write(
            "main.brs",
            "sub Main()\n    screen = CreateObject(\"roSGScreen\")\n    \
             scene = screen.CreateScene(\"PolypackScene\")\nend sub\n",
        );
        let focus = write(
            "focusgraph.brs",
            "function ppFocusGraph() as Object\n    return \
             {\"order\":[\"counter\",\"reset\"],\"edges\":{\"counter\":{\"up\":\"counter\"},\
             \"reset\":{\"down\":\"reset\"}},\"initial\":\"counter\"}\nend function\n",
        );
        let xml = write(
            "scene.xml",
            "<?xml version=\"1.0\" encoding=\"utf-8\" ?>\n\
             <component name=\"PolypackScene\" extends=\"Scene\" initialFocus=\"node_counter\">\n  \
             <children>\n    <Group id=\"node_counter\" translation=\"[0,0]\" focusable=\"true\" />\
             \n  </children>\n</component>\n",
        );
        let scene = write(
            "scene.brs",
            "sub init()\nend sub\nfunction onKeyEvent(key as String, press as Boolean) as \
             Boolean\n    m.top.findNode(\"node_counter\").setFocus(true)\n    return true\n\
             end function\n",
        );
        let good = staged.join("fixture-app-0.0.1.zip");
        poly_bundle::zipwrite::write_zip(
            &[
                ("manifest".to_string(), manifest),
                ("source/main.brs".to_string(), main),
                ("source/focusgraph.brs".to_string(), focus),
                ("components/PolypackScene.xml".to_string(), xml),
                ("components/PolypackScene.brs".to_string(), scene),
            ],
            &good,
        )
        .expect("合成合规频道包");

        // 三态 ②：合规件 → 过检，结构摘要 + 设备腿 SKIP 句都进事件流
        sink.out.clear();
        assert!(
            verify_target(&project, &spec, &mut sink).expect("合规频道包应过检"),
            "合规件必须给出 Verified 判决"
        );
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("侧载结构自检"), "{text}");
        assert!(text.contains("Fixture Channel"), "{text}");
        assert!(text.contains("焦点表 2 节点"), "{text}");
        assert!(text.contains("SKIP-with-print"), "{text}");
        assert!(text.contains("T-M4-12"), "{text}");
        assert!(text.contains("sideload-structure-check"), "{text}");

        // 三态 ③：坏件（缺 source/focusgraph.brs）→ E_BND_0006 硬失败，不降级成跳过
        std::fs::remove_file(&good).expect("清掉合格件");
        let bad = staged.join("broken-0.0.1.zip");
        poly_bundle::zipwrite::write_zip(&[("manifest".to_string(), files.join("manifest"))], &bad)
            .expect("合成坏频道包");
        let err = verify_target(&project, &spec, &mut sink).expect_err("缺必需条目必须硬失败");
        assert_eq!(err.diagnostic.code.to_string(), "poly::E_BND_0006");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// KaiOS 包结构自检三态：合规过检发 `Verified`（含设备 / 商店腿 SKIP-with-print）/
    /// 清单缺字段 E_BND_0006 硬失败 / 缺产物告警跳过并说明原因。
    #[test]
    fn kaios_structure_leg_covers_pass_fail_and_skip() {
        let (root, project) = fixture("kaios-structure");
        let spec = Registry::builtin()
            .get("kaios")
            .expect("内建 kaios 目标")
            .clone();
        let staged = root.join(".polypack/packages/kaios");
        let mut sink = poly_core::events::HumanSink {
            out: Vec::<u8>::new(),
        };

        // 三态 ①：缺产物 → 跳过并点名落点，不硬失败
        assert!(!verify_target(&project, &spec, &mut sink).expect("缺产物不该失败"));
        let text = String::from_utf8_lossy(&sink.out).replace('\\', "/");
        assert!(text.contains("没有 *.zip"), "{text}");
        assert!(text.contains(".polypack/packages/kaios"), "{text}");

        std::fs::create_dir_all(&staged).expect("暂存目录可建");
        let manifest = staged.join("manifest.webapp");
        std::fs::write(
            &manifest,
            r#"{"name":"Fixture App","version":"1.0.0","launch_path":"/index.html"}"#,
        )
        .expect("写清单");
        let index = staged.join("index.html");
        std::fs::write(&index, "<!doctype html><title>fixture</title>").expect("写入口");
        let good = staged.join("fixture-app-0.0.1.zip");
        poly_bundle::zipwrite::write_zip(
            &[
                ("manifest.webapp".to_string(), manifest),
                ("index.html".to_string(), index.clone()),
            ],
            &good,
        )
        .expect("合成合规包");

        // 三态 ②：合规件 → 过检，结构摘要 + 设备 / 商店腿 SKIP 句都进事件流
        sink.out.clear();
        assert!(
            verify_target(&project, &spec, &mut sink).expect("合规包应过检"),
            "合规件必须给出 Verified 判决"
        );
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("包结构自检"), "{text}");
        assert!(text.contains("Fixture App"), "{text}");
        assert!(text.contains("launch_path=/index.html"), "{text}");
        assert!(text.contains("SKIP-with-print"), "{text}");
        assert!(text.contains("KaiStore"), "{text}");
        assert!(text.contains("kaios-structure-check"), "{text}");

        // 三态 ③：坏件（清单缺 launch_path）→ E_BND_0006 硬失败，不降级成跳过
        std::fs::remove_file(&good).expect("清掉合格件");
        let bad_manifest = staged.join("bad.webapp");
        std::fs::write(&bad_manifest, r#"{"name":"Broken","version":"1.0.0"}"#).expect("写坏清单");
        let bad = staged.join("broken-0.0.1.zip");
        poly_bundle::zipwrite::write_zip(
            &[
                ("manifest.webapp".to_string(), bad_manifest),
                ("index.html".to_string(), index),
            ],
            &bad,
        )
        .expect("合成坏包");
        let err = verify_target(&project, &spec, &mut sink).expect_err("缺必填字段必须硬失败");
        assert_eq!(err.diagnostic.code.to_string(), "poly::E_BND_0006");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// HTML5 L2 平台结构自检三态：合规过检发 `Verified`（含平台侧 SKIP-with-print）/
    /// 平台配置坏件 E_BND_0006 硬失败 / 缺产物告警跳过并说明原因。
    #[test]
    fn html5_structure_legs_cover_pass_fail_and_skip() {
        let cases = [
            (
                "tiktok-minis",
                "tiktok-minis-structure-check",
                "<!doctype html><html><body><script>TTMinis.init({});</script></body></html>",
                "minis.config.json",
                r#"{"build":{"htmlEntry":"index.html","outputDir":"."}}"#,
                Some((
                    "minis.manifest.json",
                    r#"{"name":"minis.manifest.json","resource_list":[{"type":"file","name":"index.html"}]}"#,
                )),
                r#"{"dev":{"clientKey":"x"},"build":{"htmlEntry":"index.html","outputDir":"."}}"#,
            ),
            (
                "facebook-instant-games",
                "facebook-instant-games-structure-check",
                "<!doctype html><html><body><script src=\"https://connect.facebook.net/en_US/fbinstant.latest.js\"></script></body></html>",
                "fbapp-config.json",
                r#"{"instant_games":{"platform_version":"RICH_GAMEPLAY","orientation":"LANDSCAPE","navigation_menu_version":"NAV_FLOATING"}}"#,
                None,
                r#"{"instant_games":{"platform_version":"RICH_GAMEPLAY","orientation":"SIDEWAYS","navigation_menu_version":"NAV_FLOATING"}}"#,
            ),
        ];
        for (target, checker, index_text, config_name, config_text, manifest, bad_config) in cases {
            let (root, project) = fixture(&format!("html5-{target}"));
            let spec = Registry::builtin()
                .get(target)
                .unwrap_or_else(|| panic!("内建 {target} 目标"))
                .clone();
            let staged = root.join(format!(".polypack/packages/{target}"));
            let mut sink = poly_core::events::HumanSink {
                out: Vec::<u8>::new(),
            };

            // 三态 ①：缺产物 → 跳过并点名落点，不硬失败。
            assert!(!verify_target(&project, &spec, &mut sink).expect("缺产物不该失败"));
            let text = String::from_utf8_lossy(&sink.out).replace('\\', "/");
            assert!(text.contains("没有 *.zip"), "{target}: {text}");
            assert!(
                text.contains(&format!(".polypack/packages/{target}")),
                "{target}: {text}"
            );

            std::fs::create_dir_all(&staged).expect("暂存目录可建");
            let index = staged.join("index.html");
            std::fs::write(&index, index_text).expect("写入口");
            let config = staged.join(config_name);
            std::fs::write(&config, config_text).expect("写平台配置");
            let mut entries = vec![
                ("index.html".to_string(), index.clone()),
                (config_name.to_string(), config),
            ];
            if let Some((name, text)) = manifest {
                let path = staged.join(name);
                std::fs::write(&path, text).expect("写资源清单");
                entries.push((name.to_string(), path));
            }
            let good = staged.join("fixture-0.0.1.zip");
            poly_bundle::zipwrite::write_zip(&entries, &good).expect("合成合规包");

            // 三态 ②：合规件 → 过检，结构摘要 + 平台侧 SKIP 句都进事件流。
            sink.out.clear();
            assert!(
                verify_target(&project, &spec, &mut sink).expect("合规包应过检"),
                "{target}: 合规件必须给出 Verified 判决"
            );
            let text = String::from_utf8_lossy(&sink.out).into_owned();
            assert!(text.contains("包结构自检"), "{target}: {text}");
            assert!(text.contains("SKIP-with-print"), "{target}: {text}");
            assert!(text.contains(checker), "{target}: {text}");
            if target == "tiktok-minis" {
                assert!(text.contains("htmlEntry=index.html"), "{target}: {text}");
                assert!(text.contains("resource_list 1 项"), "{target}: {text}");
            } else {
                assert!(text.contains("RICH_GAMEPLAY"), "{target}: {text}");
                assert!(text.contains("Meta"), "{target}: {text}");
            }

            // 三态 ③：平台配置坏件 → E_BND_0006 硬失败，不降级成跳过。
            std::fs::remove_file(&good).expect("清掉合格件");
            let bad_config_path = staged.join("bad-config.json");
            std::fs::write(&bad_config_path, bad_config).expect("写坏配置");
            let bad = staged.join("broken-0.0.1.zip");
            let bad_entries = vec![
                ("index.html".to_string(), index),
                (config_name.to_string(), bad_config_path),
            ];
            poly_bundle::zipwrite::write_zip(&bad_entries, &bad).expect("合成坏包");
            let err = verify_target(&project, &spec, &mut sink).expect_err("坏配置必须硬失败");
            assert_eq!(err.diagnostic.code.to_string(), "poly::E_BND_0006");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    /// webOS 腿：暂存目录没有 `.ipk`（打包未就绪）→ 告警跳过，不硬失败。
    #[test]
    fn ares_leg_skips_when_no_ipk_is_staged() {
        let (root, project) = fixture("ares-leg");
        let spec = Registry::builtin()
            .get("webos-tv")
            .expect("内建 webos-tv 目标")
            .clone();
        let mut sink = poly_core::events::HumanSink {
            out: Vec::<u8>::new(),
        };
        assert!(
            !verify_target(&project, &spec, &mut sink).expect("缺产物不该失败"),
            "无 .ipk 应如实跳过"
        );
        let text = String::from_utf8_lossy(&sink.out).into_owned();
        assert!(text.contains("没有 *.ipk"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
