//! `polypack publish`：商店提审薄层（T-M3-12 起接快应用联盟商店；骨架 = polypack-publish 的零分叉驱动）。
//!
//! 语义（自动化边界 R4：止步提审，绝不自动发布；回执永久保留）：
//! - **渠道来源**：`--channel` 或目标声明 `TargetSpec.publish[0]`（快应用 = `quickapp-union-store`）；
//! - **产物定位**：`--package` 显式给；缺省按目标产物落点找
//!   `<packages>/<target>/miniapp/dist/*.rpk`（hap-toolkit 的 `.rpk`；多个按名序取尾并告警，
//!   零个 = `E_PUBLISH_0001`——先 `polypack build --target quickapp` 出包）；
//! - **凭据引用模型**（同 T-M2-11 / T-M3-09）：`--credential env:VAR`，变量值 = base64(JSON)
//!   载荷；本层只解析**形态**（字段名），值零落屏、零落盘；
//! - **幂等重跑**：回执（`<root>/.polypack/publish/<channel>/<key>.json`）由 polypack-publish 驱动
//!   续走——终态重跑 = no-op 不触网，非终态从断点续走（上传 / 提审不重复）；
//! - **端点显式注入**：快应用联盟商店无公开 CI 端点（2026-10 核：quickapp.cn / 各厂商文档
//!   只有 web 控制台与 IDE 上传，无 CI API 面），`--api-base` 必填、无缺省域名；协议面为
//!   **最小模型，待真账号联调**（R4 记录在案）；
//! - **载具 pre-flight**：代码包经 `Package::load`（polypack-publish 公开入口，自动判别目录 /
//!   单文件）读到**驱动同源的摘要**——dry-run 报 `package_carrier` / `package_digest` /
//!   `receipt_key`（回执键 = appid-版本-摘要前 16，幂等重跑的落点锚），rpk 腿与四步驱动
//!   零分叉同口径；
//! - **渠道客户端缺席**（publish-client 的 T-M3-10 尚缺 builtin() 内快应用客户端）→
//!   `E_PUBLISH_0002` 如实点名归属，绝不假装提审过；`--dry-run` 面不受影响，可先行验证
//!   输入 / 凭据 / 定位 / 载具；
//! - **metadata 管线**（M9-E5）：`--metadata-dir <store/metadata>` 读取
//!   `<platform>/<locale>.toml`（wechat / alipay），校验后生成确定性材料对象（key 排序，
//!   摘要 = canonical JSON 的 blake3，写入回执 `materials_digest`）；`--check-metadata`
//!   只校验、不发布、不落回执。`--metadata-dir` 与旧的 `--materials <file>` **互斥**
//!   （clap 拒绝同时给出；一处材料事实源）。违规一次报告全部（稳定码 + 可读消息）。
//!
//! 接口缺口（T-M3-12 报 'main'；main@cd1fcdb 后状态）：①`Channel::QuickApp` 备位已入主干
//! （`from_id("quickapp-union-store")` 可反解）；②`builtin()` 仍为微信 / 支付宝 / 抖音三平台，
//! 快应用客户端与③端点注入面（`--api-base` → 客户端，现无端点形参）归 publish-client；
//! ④单文件载具 `Package::load_file` 已入主干，本层 pre-flight 即经 `Package::load` 接上。
//! 本层已做到调用面（[`polypack_publish::publish`] 四步驱动 + 公开入口），客户端进 `builtin()`
//! 后零改动激活（`find_publisher` 自动返回；端点一行接线点在真腿处标注）。
//!
//! 模块拆分（M9 收口）：参数在 [`args`]，输入解析在 [`prepare`]，`--check-metadata` 在
//! [`metadata`]；本文件只保留命令入口与提审驱动。

use std::sync::Arc;

use polypack_core::BuildEvent;
use polypack_core::codes;
use polypack_core::diagnostics::{Code, Diagnostic};
use polypack_publish::secret::{CredentialProvider, EnvProvider};
use polypack_publish::{
    Archive, AuditState, PublishError, PublishFailure, PublishRequest, Publisher, Transport,
    UreqTransport,
};
use polypack_targets::Registry;

use crate::project;

mod args;
mod metadata;
mod prepare;
#[cfg(test)]
mod tests;

pub use args::Args;

/// 快应用联盟商店渠道 id（registry `TargetSpec.publish` 名，三处同口径）。
const UNION_STORE_CHANNEL: &str = "quickapp-union-store";

/// 小程序族产物目录名（`<packages>/<target>/miniapp/`，与 polypack-bundle / verify 同口径）。
const MINIAPP_DIR: &str = "miniapp";
/// 代码包目录名（`<miniapp>/dist/`，快应用的 `.rpk` 落在这里；同上）。
const DIST_DIR: &str = "dist";

/// 执行 `polypack publish`；返回进程退出码。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let project = project::load(&args.config)?;
    run_project(&project, args, json)
}

/// 已装载项目上的提审通路（`polypack retry --from publish` 的工作区成员复用此入口）。
pub(crate) fn run_project(
    project: &polypack_core::Project,
    args: &Args,
    json: bool,
) -> Result<i32, Diagnostic> {
    let registry = Registry::builtin();
    let spec = registry.get(&args.target).cloned().ok_or_else(|| {
        Diagnostic::error(
            codes::E_TGT_0001,
            format!("目标 {} 不在内建注册表", args.target),
        )
        .with_hint("用 polypack doctor 核对目标矩阵")
    })?;
    // 只校验 metadata：在解析产物 / 凭据之前返回——不生成发布请求、不触网、不落回执。
    if args.check_metadata {
        return metadata::check_metadata(args, &spec, json);
    }

    let providers: Vec<Box<dyn CredentialProvider>> = vec![Box::new(EnvProvider)];
    let prepared = prepare::prepare(args, project, &spec, &providers)?;
    for warning in &prepared.metadata_warnings {
        eprintln!("publish 告警：{warning}");
    }
    let appid = prepare::required_flag(args.appid.as_deref(), "--appid")?;
    let version = prepare::required_flag(args.version.as_deref(), "--version")?;

    if args.dry_run {
        project::report(
            "publish",
            json,
            serde_json::json!({
                "event": "publish-dry-run",
                "channel": prepared.channel,
                "target": spec.id,
                "appid": appid,
                "version": version,
                "package": prepared.package.display().to_string(),
                "package_bytes": prepared.package_bytes,
                "package_carrier": prepared.carrier,
                "package_digest": prepared.digest,
                "receipt_key": prepared.receipt_key,
                "credential_reference": prepared.reference.name,
                "credential_provider": prepared.provider,
                "credential_fields": prepared.payload_fields,
                "api_base": prepared.api_base,
                "materials": prepared.materials.is_some(),
                "materials_locale": prepared.materials_locale,
                "materials_digest": prepared.materials_digest,
                "metadata_warnings": prepared.metadata_warnings,
                "note": "dry-run：只解析输入，未触网、未落回执（R4：自动化止步提审）",
            }),
        );
        return Ok(0);
    }

    // 真腿：渠道客户端（builtin() 尚无快应用客户端 → E_PUBLISH_0002 缺口诊断）+ 四步驱动。
    let transport = UreqTransport::new().map_err(publish_error)?;
    let publisher = find_publisher(&prepared.channel, Arc::new(transport))?;
    // 端点注入面（T-M3-12 缺口②，归 publish-client）：现 `builtin(transport)` 无端点形参；
    // `prepared.api_base`（快应用联盟必填）已解析校验，客户端接口落位后在此一行接线——
    // 本层不预改 polypack-publish 核心。四步驱动的载具入口 `Package::load` 已自动判别单文件
    // （pre-flight 摘要 / 回执键与其同源），rpk leg 至此除客户端外全通。
    let archive = Archive::new(&project.root);
    let mut request = PublishRequest::new(
        appid,
        version,
        args.description.clone(),
        prepared.package.clone(),
        prepared.reference.clone(),
    );
    if let Some(materials) = prepared.materials.clone() {
        request = request.with_materials(materials);
    }
    let outcome =
        match polypack_publish::publish_checked(publisher.as_ref(), &request, &providers, &archive)
        {
            Ok(outcome) => outcome,
            Err(failure) => {
                // 机器可读：`{"event":"failed", stage, code, hint}`（T-M8-10 验收口径）。
                project::report("publish", json, failure_event(&failure));
                return Err(failure_diagnostic(&failure));
            }
        };
    let receipt = &outcome.receipt;
    // 终态通过 / 已上架 = 发布完成：事件流落 `Published{channel, submission}`（人读同一事件）。
    if matches!(receipt.state, AuditState::Approved | AuditState::Released) {
        project::sink(json).emit(&BuildEvent::Published {
            channel: receipt.channel.clone(),
            submission: published_submission(receipt),
        });
    }
    project::report(
        "publish",
        json,
        serde_json::json!({
            "event": "publish",
            "channel": receipt.channel,
            "appid": receipt.appid,
            "version": receipt.version,
            "digest": receipt.digest,
            "materials_digest": receipt.materials_digest,
            "state": receipt.state.to_string(),
            "runs": receipt.runs,
            "resumed_from": match outcome.resumed_from {
                Some(state) => state.to_string(),
                None => "新执行".to_string(),
            },
            "receipt": outcome.path.display().to_string(),
            "note": "R4：自动化止步提审——审核结果以平台为准，重跑同键命令只续轮询 / no-op",
        }),
    );
    Ok(0)
}

/// `Published.submission`：平台提交号；平台未给则用本地幂等键（同一提交的稳定身份，可追溯）。
fn published_submission(receipt: &polypack_publish::Receipt) -> String {
    receipt.submission_id.clone().unwrap_or_else(|| {
        polypack_publish::submission_key(
            &receipt.channel,
            &receipt.appid,
            &receipt.version,
            &receipt.digest,
        )
    })
}

/// 失败事件（与构建事件流的 `Failed` 同形）：`{"event":"failed","stage","code","hint"}`。
fn failure_event(failure: &PublishFailure) -> serde_json::Value {
    serde_json::to_value(polypack_core::BuildEvent::Failed {
        stage: failure.stage.to_string(),
        code: failure.error.code().to_string(),
        hint: failure.hint(),
    })
    .expect("失败事件可序列化")
}

/// 失败诊断：消息保留 `Failed{stage, code, hint}`，hint 同时进入诊断的修复提示。
fn failure_diagnostic(failure: &PublishFailure) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(
        Code::new(failure.error.code()),
        format!("发布失败：{}（{}）", failure.render(), failure.error),
    );
    if let Some(hint) = failure.hint() {
        diagnostic = diagnostic.with_hint(hint);
    }
    diagnostic
}

/// 渠道客户端查找（真腿）：注册表 `publish` 名 → polypack-publish `Channel` 备位 → `builtin()` 内客户端。
///
/// 任一层缺席 → `E_PUBLISH_0002` 缺口诊断（点名渠道与归属），绝不假装提审过。
/// 自动激活：备位已入主干（main@cd1fcdb 起 `from_id` 可反解 QuickApp）——客户端进
/// `builtin()` 后本函数原样返回，测试 Ok 臂同日转绿。
fn find_publisher(
    channel: &str,
    transport: Arc<dyn Transport>,
) -> Result<Box<dyn Publisher>, Diagnostic> {
    let gap = |reason: String| {
        Diagnostic::error(
            codes::E_PUBLISH_0002,
            format!("渠道 {channel} 的提审客户端未落位：{reason}"),
        )
        .with_hint(
            "快应用联盟商店：Channel 备位已入主干（main@cd1fcdb）；尚缺 builtin() 内客户端 \
             与端点注入面，归 publish-client（T-M3-10 后续）——本层已做到调用面，\
             落位后零改动激活（--dry-run 可先行验证输入 / 载具）",
        )
    };
    let Some(wanted) = polypack_publish::Channel::from_id(channel) else {
        return Err(gap(
            "polypack-publish 无该渠道备位（Channel::from_id 未识别）".to_string(),
        ));
    };
    polypack_publish::builtin(transport)
        .into_iter()
        .find(|publisher| publisher.channel() == wanted)
        .ok_or_else(|| gap("builtin() 里没有该渠道的客户端".to_string()))
}

/// `PublishError` → 诊断：错误码原样透传（`polypack::E_PUB_*`，绝不重新编号），提示取 `hint()`。
fn publish_error(err: PublishError) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(Code::new(err.code()), err.to_string());
    if let Some(hint) = err.hint() {
        diagnostic = diagnostic.with_hint(hint);
    }
    diagnostic
}
