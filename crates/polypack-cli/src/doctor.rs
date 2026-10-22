//! `polypack doctor`：工具链探测报告 + L1 目标覆盖矩阵（T-M0-08）。

use polypack_core::Diagnostic;
use polypack_targets::Registry;
use polypack_toolchain::{ProbeResult, default_probes};

/// `polypack doctor` 的参数。
#[derive(Debug, clap::Args)]
pub struct Args {}

/// 运行探测并输出报告；退出码恒 0（报告不门控，`--strict` 门控留待 M1）。
pub fn run(_args: &Args, json: bool) -> Result<i32, Diagnostic> {
    let results: Vec<ProbeResult> = default_probes()
        .iter()
        .map(|probe| probe.locate())
        .collect();
    let gaps = target_gaps(&Registry::builtin(), &results);
    if json {
        // 首行：doctor 报告对象；随后逐行：每探测器一条 ProbeResult（JSON Lines）
        let found = results.iter().filter(|r| r.found.is_some()).count();
        let report = serde_json::json!({
            "event": "doctor",
            "summary": {
                "probes": results.len(),
                "found": found,
                "missing": results.len() - found,
                "targets_ready": gaps.iter().filter(|g| g.missing.is_empty()).count(),
            },
            "targets": gaps.iter().map(|gap| serde_json::json!({
                "id": gap.id,
                "ready": gap.missing.is_empty(),
                "missing": gap.missing,
            })).collect::<Vec<_>>(),
        });
        println!(
            "{}",
            serde_json::to_string(&report).expect("doctor 报告可序列化")
        );
        for result in &results {
            println!(
                "{}",
                serde_json::to_string(result).expect("ProbeResult 可序列化")
            );
        }
    } else {
        println!("polypack doctor — 工具链探测报告");
        for result in &results {
            match &result.found {
                Some(toolchain) => {
                    let version = toolchain.version.as_deref().unwrap_or("未知版本");
                    println!("✓ {}（{}，{version}）", result.id, toolchain.path.display());
                }
                None => {
                    let note = result.note.as_deref().unwrap_or("未找到");
                    println!("✗ {}：{note}", result.id);
                }
            }
        }
        println!("L1 目标覆盖矩阵：");
        if gaps.is_empty() {
            println!("  （内建目标表为空）");
        }
        for gap in &gaps {
            if gap.missing.is_empty() {
                println!("  {}：就绪", gap.id);
            } else {
                println!("  {}：缺 {}", gap.id, gap.missing.join("、"));
            }
        }
    }
    Ok(0)
}

/// 单个 L1 目标相对探测结果的就绪状态。
struct TargetGap {
    /// 目标 id。
    id: String,
    /// 该目标必需但缺失的探测器 id。
    missing: Vec<&'static str>,
}

/// 逐目标汇总缺口：`required_for` 含该目标且探测未找到的探测器。
fn target_gaps(registry: &Registry, results: &[ProbeResult]) -> Vec<TargetGap> {
    registry
        .all()
        .map(|spec| TargetGap {
            missing: results
                .iter()
                .filter(|r| r.found.is_none() && r.required_for.contains(&spec.id.as_str()))
                .map(|r| r.id)
                .collect(),
            id: spec.id.clone(),
        })
        .collect()
}
