//! `poly publish --check-metadata`：只校验商店 metadata，不发布、不触网、不落回执。

use poly_core::codes;
use poly_core::diagnostics::Diagnostic;
use poly_publish::metadata::{self, Platform};
use poly_targets::TargetSpec;

use super::args::Args;
use super::prepare::{metadata_diagnostic, resolve_channel};
use crate::project;

/// `--check-metadata`：校验全部 locale 并输出完整性报告；违规 → 非零退出（不发布）。
pub(super) fn check_metadata(
    args: &Args,
    spec: &TargetSpec,
    json: bool,
) -> Result<i32, Diagnostic> {
    let channel = resolve_channel(args, spec)?;
    let platform = Platform::from_channel(&channel).ok_or_else(|| {
        Diagnostic::error(
            codes::E_PUBLISH_0001,
            format!("渠道 {channel} 没有 metadata 规则（当前支持 wechat / alipay）"),
        )
        .with_hint("用 --channel wechat-open-platform / alipay-open-platform 校验对应规则")
    })?;
    let root = args.metadata_dir.as_ref().ok_or_else(|| {
        Diagnostic::error(
            codes::E_PUBLISH_0001,
            "--check-metadata 需要 --metadata-dir <store/metadata>",
        )
        .with_hint("示例：poly publish --check-metadata --metadata-dir store/metadata --channel wechat-open-platform")
    })?;
    let dir = root.join(platform.id());
    let set = metadata::load(platform, &dir).map_err(|err| {
        Diagnostic::error(codes::E_PUBLISH_0001, format!("metadata 不可读：{err}")).with_hint(
            "--metadata-dir 应指向 store/metadata 根目录（内含 <platform>/<locale>.toml）",
        )
    })?;
    let violations = set.violations();
    project::report(
        "publish",
        json,
        serde_json::json!({
            "event": "publish-metadata-check",
            "platform": platform.id(),
            "channel": channel,
            "metadata_dir": dir.display().to_string(),
            "locales": set.locale_names(),
            "ok": violations.is_empty(),
            "violations": violations,
            "note": "--check-metadata 只校验：不解析产物 / 凭据，不触网、不落回执、不发布",
        }),
    );
    if violations.is_empty() {
        Ok(0)
    } else {
        Err(metadata_diagnostic(&set))
    }
}
