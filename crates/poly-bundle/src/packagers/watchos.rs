//! watchOS 伴生包（T-M7-06）：`poly init --target watchos` 的 SwiftUI 工程 →
//! `xcodebuild archive` → `export/Payload/<产品>.app`（签名 / 校验的公共落点，同 iOS 链）。
//!
//! - **工程来源**：`inputs.ios.host_project` 下的 `PolyPackWatch.xcodeproj`（模板产物）。
//!   模板手表端 runtime = none（核心经 iOS 伴生 App 代理），故**不**生成链接核心
//!   XCFramework 的骨架工程；缺工程如实 [`BundleError::MissingInput`]（先跑 `poly init
//!   --target watchos`）。
//! - **宿主门控（R3）**：`xcodebuild` / `ditto` / `plutil` 仅随 Xcode 提供；非 macOS 宿主
//!   在动手前返回 [`BundleError::ToolMissing`]（`poly::E_BND_0001`）——Linux / Windows
//!   发起走远程农场 macOS worker（T-M2-13 协议）。`plan()` 为纯组装，任意宿主可跑。
//! - **签名**：本阶段恒出未签名 `.app`；`apple-codesign`（codesign 硬化运行时 + 时间戳）
//!   由 sign 阶段（poly-sign ios 链）接手，凭据只以 `CredentialRef` 出现。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, BundlePlan, Command, Packager};

/// 目标 / 打包器标识。
pub(crate) const WATCHOS: &str = "watchos";
/// 产物格式（watch 包 = 可签名的 `.app` 目录；不是 iOS 的 `.ipa`）。
const WATCH: &str = "watch";
/// 模板工程名（`poly init --target watchos` 的固定产物）。
pub const PROJECT: &str = "PolyPackWatch.xcodeproj";
/// 模板共享 scheme 名（工程 target / product 同名）。
pub const SCHEME: &str = "PolyPackWatch";
/// 归档后的应用 bundle 名（PRODUCT_NAME = target 名）。
const ARCHIVED_APP: &str = "PolyPackWatch.app";
/// xcodebuild 目标平台（iOS / tvOS 描述符之外的 watchOS 形态）。
pub const DESTINATION: &str = "generic/platform=watchOS";

/// watchOS 伴生包打包器。
pub struct Watchos;

impl Packager for Watchos {
    fn id(&self) -> &'static str {
        WATCHOS
    }

    fn format(&self) -> &'static str {
        WATCH
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, WATCH)?;
        inputs.product.validate(WATCHOS)?;
        let project = project_path(inputs)?;
        let app = app_path(inputs);
        Ok(BundlePlan::new()
            .with_command(archive_command(inputs, &project))
            .with_command(Command::new(
                "ditto",
                [path_arg(&archived_app(inputs)), path_arg(&app)],
            ))
            // 产物校验（与 iOS 链同口径）：Info.plist 语法
            .with_command(Command::new(
                "plutil",
                ["-lint".into(), path_arg(&app.join("Info.plist"))],
            )))
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        // 宿主门控最前置：非 macOS 宿主无从谈起（先于一切输入校验，也先于落盘）。
        require_macos("xcodebuild")?;
        let plan = self.plan(inputs, spec)?;
        let app = app_path(inputs);
        let payload = inputs.out_dir.join("export").join("Payload");
        std::fs::create_dir_all(&payload).map_err(crate::error::io_err(&payload))?;
        fsutil::write_scripts(&plan)?;
        exec::run_all(&plan.commands)?;
        fsutil::dir_artifact(
            WATCHOS,
            WATCH,
            &app,
            vec![
                "未签名 watch 包（`xcodebuild archive` → `export/Payload/<产品>.app`；\
                 签名位由 sign 阶段 poly-sign ios 链的 apple-codesign 接手）"
                    .to_string(),
                "证书 / 描述文件只以凭据引用出现（apple-codesign → CredentialRef）：\
                 打包阶段不接触密钥材料"
                    .to_string(),
                "实验目标：SwiftUI 伴生模板（核心 API 调用桩 + WatchConnectivity 说明），\
                 核心不直接运行"
                    .to_string(),
                "宿主：xcodebuild / ditto / plutil 仅 macOS；Linux / Windows 发起走远程农场 \
                 macOS worker（R3）"
                    .to_string(),
            ],
        )
    }
}

// ---------------------------------------------------------------- 路径

/// 模板工程：`<host_project>/PolyPackWatch.xcodeproj`（或 `watchos/` 子目录下的同名工程）。
fn project_path(inputs: &BundleInputs) -> Result<PathBuf, BundleError> {
    let root = inputs
        .ios
        .as_ref()
        .and_then(|ios| ios.host_project.as_ref())
        .ok_or(BundleError::MissingInput {
            packager: WATCHOS,
            field: "ios.host_project",
        })?;
    for rel in [PROJECT.to_string(), format!("watchos/{PROJECT}")] {
        let candidate = root.join(&rel);
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }
    Err(BundleError::MissingInput {
        packager: WATCHOS,
        field: "watchos.project",
    })
}

/// 归档产物：`<out>/<产品>.xcarchive/Products/Applications/PolyPackWatch.app`。
fn archived_app(inputs: &BundleInputs) -> PathBuf {
    inputs
        .out_dir
        .join(format!("{}.xcarchive", inputs.product.name))
        .join("Products")
        .join("Applications")
        .join(ARCHIVED_APP)
}

/// 签名 / 校验的公共落点：`<out>/export/Payload/<产品>.app`（同 iOS 链）。
fn app_path(inputs: &BundleInputs) -> PathBuf {
    inputs
        .out_dir
        .join("export")
        .join("Payload")
        .join(format!("{}.app", inputs.product.name))
}

// ---------------------------------------------------------------- 命令 / 门控

/// `xcodebuild archive`（未签名归档：签名位由 sign 阶段接入）。
fn archive_command(inputs: &BundleInputs, project: &Path) -> Command {
    Command::new(
        "xcodebuild",
        [
            "archive".into(),
            "-project".into(),
            path_arg(project),
            "-scheme".into(),
            SCHEME.into(),
            "-configuration".into(),
            "Release".into(),
            "-destination".into(),
            DESTINATION.into(),
            "-archivePath".into(),
            path_arg(
                &inputs
                    .out_dir
                    .join(format!("{}.xcarchive", inputs.product.name)),
            ),
            "CODE_SIGNING_ALLOWED=NO".into(),
            "CODE_SIGNING_REQUIRED=NO".into(),
            "CODE_SIGN_IDENTITY=".into(),
        ],
    )
}

/// 宿主门控：watchOS 打包链仅 macOS 宿主可跑（R3：远程农场 macOS worker）。
fn require_macos(tool: &str) -> Result<(), BundleError> {
    if poly_targets::current_host() == Some(poly_targets::Host::Macos) {
        return Ok(());
    }
    Err(BundleError::ToolMissing {
        packager: WATCHOS,
        tool: tool.to_string(),
        hint: "watchOS 打包需 Xcode（xcodebuild / ditto / plutil）→ macOS 宿主；\
               Linux / Windows 发起走远程农场 macOS worker（R3，poly farm submit）"
            .to_string(),
    })
}

/// 打包格式与打包器声明必须一致。
fn check_format(spec: &TargetSpec, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: WATCHOS,
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}

/// 参数形态（路径一律显式传参，避免拼接成 shell 串）。
fn path_arg(path: &Path) -> String {
    path.display().to_string()
}
