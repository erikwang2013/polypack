//! `poly targets`：内建目标清单（T-M6-08 的 CI 矩阵派生面）。
//!
//! 矩阵条目只从 [`Registry`] 派生，不在 workflow 里手写目标 id：新增一条
//! `TargetSpec` 声明后，`poly targets --tier 6 --headless` 会多出一行，CI 矩阵
//! 自动出现该条目。`--json` 走全局 JSON Lines 约定（首行摘要，随后每目标一行）。
//!
//! `--headless` 是「无 UI 目标」口径（`spec.ui` 为空），与 ROADMAP §3.7 的
//! 嵌入式编译矩阵一致；带 UI 的 L6 目标（如 android-automotive）不进该矩阵。

use poly_core::Diagnostic;
use poly_targets::{Registry, TargetSpec};
use poly_toolchain::default_probes;

/// `poly targets` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {
    /// 只看指定 tier（L1–L7 对应 1–7）。
    #[arg(long)]
    pub tier: Option<u8>,
    /// 只要无 UI 目标（`ui` 为空）；嵌入式 CI 矩阵口径。
    #[arg(long)]
    pub headless: bool,
}

/// 输出目标清单；退出码恒 0（纯信息面，不做门控）。
pub fn run(args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let registry = Registry::builtin();
    let specs: Vec<&TargetSpec> = registry
        .all()
        .filter(|spec| args.tier.is_none_or(|tier| spec.tier == tier))
        .filter(|spec| !args.headless || spec.ui.is_empty())
        .collect();
    if json {
        let summary = serde_json::json!({
            "event": "targets",
            "tier": args.tier,
            "headless": args.headless,
            "count": specs.len(),
        });
        println!(
            "{}",
            serde_json::to_string(&summary).expect("targets 摘要可序列化")
        );
        for spec in &specs {
            println!(
                "{}",
                serde_json::to_string(&target_json(spec)).expect("TargetSpec 可序列化")
            );
        }
    } else {
        println!("poly targets — 内建目标清单（{} 个）", specs.len());
        for spec in &specs {
            println!(
                "  {}（tier {}）→ {} via {}；verify: {}",
                spec.id,
                spec.tier,
                spec.package.format,
                spec.package.builder,
                if spec.verify.is_empty() {
                    "（无）".to_string()
                } else {
                    spec.verify.join(", ")
                }
            );
        }
    }
    Ok(0)
}

/// 单个目标的 JSON 行；字段覆盖 CI 矩阵所需（id / tier / builder / verify / 探测）。
fn target_json(spec: &TargetSpec) -> serde_json::Value {
    let probes: Vec<&str> = default_probes()
        .iter()
        .filter(|probe| probe.required_for().contains(&spec.id.as_str()))
        .map(|probe| probe.id())
        .collect();
    serde_json::json!({
        "event": "target",
        "id": spec.id,
        "tier": spec.tier,
        "hosts": spec.hosts,
        "runtime": spec.runtime,
        "bridge": spec.bridge,
        "ui": spec.ui,
        "format": spec.package.format,
        "builder": spec.package.builder,
        "verify": spec.verify,
        "publish": spec.publish,
        "experimental": spec.experimental,
        "required_probes": probes,
    })
}
