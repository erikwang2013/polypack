//! 版本解析与选择：exact / 最新可用 SemVer、吊销与宿主兼容过滤。

use polypack_core::{Diagnostic, codes};
use semver::Version;

use super::rules::matching_revocation;
use super::schema::{IndexDocument, PluginEntry, RevocationsDocument, SUPPORTED_WIT, index_error};

pub(super) fn parse_spec(spec: &str) -> Result<(&str, Option<&str>), Diagnostic> {
    match spec.rsplit_once('@') {
        Some((id, version)) if !version.is_empty() => Ok((id, Some(version))),
        Some(_) => Err(index_error(format!("插件版本为空：{spec:?}"))),
        None => Ok((spec, None)),
    }
}

pub(super) fn select_entry<'a>(
    index: &'a IndexDocument,
    revocations: &RevocationsDocument,
    id: &str,
    wanted: Option<&str>,
    host: &Version,
) -> Result<&'a PluginEntry, Diagnostic> {
    let mut candidates: Vec<&PluginEntry> = index
        .plugins
        .iter()
        .filter(|entry| entry.id == id)
        .collect();
    if candidates.is_empty() {
        return Err(index_error(format!("索引中没有插件：{id}")));
    }
    if let Some(version) = wanted {
        let entry = candidates
            .iter()
            .copied()
            .find(|entry| entry.version == version)
            .ok_or_else(|| index_error(format!("索引中没有版本：{id}@{version}")))?;
        ensure_installable(entry, revocations, host)?;
        return Ok(entry);
    }
    candidates.sort_by_key(|entry| std::cmp::Reverse(entry_version(entry)));
    for entry in &candidates {
        if ensure_installable(entry, revocations, host).is_ok() {
            return Ok(entry);
        }
    }
    ensure_installable(candidates[0], revocations, host)?;
    unreachable!("候选集非空且循环已确认全部不可安装")
}

fn ensure_installable(
    entry: &PluginEntry,
    revocations: &RevocationsDocument,
    host: &Version,
) -> Result<(), Diagnostic> {
    if let Some(revocation) = matching_revocation(entry, revocations) {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0013,
            format!(
                "插件 {}@{} 已吊销（{}）",
                entry.id, entry.version, revocation.reason
            ),
        )
        .with_hint("安装 replacement 指向的修复版本，或选择其他插件"));
    }
    if entry.status != "active" {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0013,
            format!(
                "插件 {}@{} 已 yanked{}",
                entry.id,
                entry.version,
                entry
                    .yank_reason
                    .as_deref()
                    .map(|reason| format!("（{reason}）"))
                    .unwrap_or_default()
            ),
        )
        .with_hint("选择该插件的其他 active 版本"));
    }
    if entry.wit_version != SUPPORTED_WIT {
        return Err(Diagnostic::error(
            codes::E_PLUGIN_0005,
            format!(
                "插件 {}@{} 接口版本不受支持：{}（宿主要求 {SUPPORTED_WIT}）",
                entry.id, entry.version, entry.wit_version
            ),
        )
        .with_hint("升级插件，或升级 polypack 以支持新的接口版本"));
    }
    if !host_supports(entry, host) {
        return Err(index_error(format!(
            "插件 {}@{} 要求 PolyPack >= {}（当前 {}）",
            entry.id, entry.version, entry.min_host_version, host
        ))
        .with_hint("升级 polypack 后重试"));
    }
    Ok(())
}

pub(super) fn host_supports(entry: &PluginEntry, host: &Version) -> bool {
    &entry_version_of(&entry.min_host_version) <= host
}

/// 索引条目的 SemVer（schema 校验已保证可解析）。
pub(super) fn entry_version(entry: &PluginEntry) -> Version {
    entry_version_of(&entry.version)
}

fn entry_version_of(value: &str) -> Version {
    Version::parse(value).expect("索引校验已保证 SemVer")
}

pub(super) fn host_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("包版本是 SemVer")
}

pub(super) fn matches_query(entry: &PluginEntry, query: &str) -> bool {
    query.is_empty()
        || entry.id.to_lowercase().contains(query)
        || entry.name.to_lowercase().contains(query)
        || entry.summary.to_lowercase().contains(query)
        || entry
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.keywords.as_ref())
            .is_some_and(|keywords| {
                keywords
                    .iter()
                    .any(|keyword| keyword.to_lowercase().contains(query))
            })
}
