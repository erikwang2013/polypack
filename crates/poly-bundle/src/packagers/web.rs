//! Web 静态站点打包器（T-M1-08）：UI dist + PWA manifest → 可直接托管的静态包。
//!
//! 产物为目录 `<out_dir>/site/`：dist 整树 + `manifest.webmanifest`（T-M1-04 生成，
//! 原样嵌入）+ `integrity.json`（SRI 清单，见 [`crate::sri`]）。
//! html-validate 缺失时不失败：改由内建 HTML 结构断言守底，跳过项如实写进产物 `notes`。

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::{BundleError, io_err};
use crate::exec;
use crate::fsutil;
use crate::inputs::BundleInputs;
use crate::packager::{Artifact, BundlePlan, Command, Packager};
use crate::sri;

/// Web 静态站点打包器（`static-site`）。
pub struct StaticSite;

/// 产物格式（须与 `[targets.web].package.format` 一致）。
const STATIC_SITE: &str = "static-site";

/// 服务静态站点的目标：L1 `web` 与 T-M5-10 的 ChromeOS PWA 轨（同一条 Web 链）。
const STATIC_SITE_TARGETS: &[&str] = &["web", "chromeos-pwa"];

/// 内建 HTML 结构断言的最小标记集（大小写不敏感）。
const HTML_REQUIRED: &[&str] = &["<!doctype html", "<html", "</html>", "<body", "</body>"];

/// PWA manifest 的必填非空字符串字段（JSON 键 → 错误 field 名）。
const MANIFEST_REQUIRED_TEXT: &[(&str, &str)] = &[
    ("name", "web_manifest(name)"),
    ("start_url", "web_manifest(start_url)"),
    ("display", "web_manifest(display)"),
];

impl Packager for StaticSite {
    fn id(&self) -> &'static str {
        "web"
    }

    fn format(&self) -> &'static str {
        STATIC_SITE
    }

    fn serves(&self, target_id: &str) -> bool {
        STATIC_SITE_TARGETS.contains(&target_id)
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, STATIC_SITE)?;
        let dist = inputs.require_ui_dist(STATIC_SITE)?;
        if !exec::tool_present("html-validate") {
            return Ok(BundlePlan::new().skipping(
                "html-validate",
                "未安装：跳过外部 HTML 校验，仅做内建结构断言（doctype / html 标签配对）",
            ));
        }
        let mut plan = BundlePlan::new();
        for html in html_files(dist)? {
            plan = plan.with_command(Command::new(
                "html-validate",
                ["--formatter", "stylish", path_arg(&html).as_str()],
            ));
        }
        Ok(plan)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        let plan = self.plan(inputs, spec)?;
        let dist = inputs.require_ui_dist(STATIC_SITE)?;
        let site = site_dir(inputs);
        let _ = std::fs::remove_dir_all(&site);
        fsutil::copy_tree(dist, &site)?;

        // PWA 资产是验收项：manifest 缺失即失败，不静默跳过。
        let source = inputs
            .web_manifest
            .as_deref()
            .ok_or(BundleError::MissingInput {
                packager: STATIC_SITE,
                field: "web_manifest",
            })?;
        let text = std::fs::read_to_string(source).map_err(io_err(source))?;
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|parse| BundleError::InvalidInput {
                packager: STATIC_SITE,
                field: "web_manifest",
                value: source.display().to_string(),
                reason: format!("JSON 解析失败：{parse}"),
            })?;
        validate_manifest(&value, &site, STATIC_SITE)?;
        // 原样写入：manifest 的生成（含图标尺寸）由 T-M1-04 负责，此处不做无谓改写。
        fsutil::write_text(&site.join("manifest.webmanifest"), &text)?;

        assert_html_structure(&site)?;

        let manifest = sri::manifest_of(&site)?;
        let integrity =
            serde_json::to_string_pretty(&manifest).map_err(|error| BundleError::Io {
                path: site.join("integrity.json"),
                source: std::io::Error::other(error),
            })?;
        fsutil::write_text(&site.join("integrity.json"), &format!("{integrity}\n"))?;
        let problems = sri::verify(&site, &manifest)?;
        if !problems.is_empty() {
            return Err(BundleError::InvalidInput {
                packager: STATIC_SITE,
                field: "integrity.json",
                value: site.display().to_string(),
                reason: format!("SRI 清单与产物不一致：{}", problems.join("；")),
            });
        }

        exec::run_all(&plan.commands)?;

        let mut notes = vec![
            format!(
                "SRI: {}，{} 个文件（integrity.json）",
                manifest.algorithm,
                manifest.files.len()
            ),
            format!(
                "PWA manifest 来源：{}（原样写为 manifest.webmanifest）",
                source.display()
            ),
        ];
        notes.extend(
            plan.skipped
                .iter()
                .map(|s| format!("跳过：{}（{}）", s.tool, s.reason)),
        );
        notes.push(format!(
            "静态托管：python3 -m http.server 8000 --directory {}",
            site.display()
        ));
        fsutil::dir_artifact(self.id(), STATIC_SITE, &site, notes)
    }
}

/// 打包格式与打包器声明必须一致（防「按 static-site 目标产出 appimage」）。
fn check_format(spec: &TargetSpec, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: "web",
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}

/// 站点目录（静态托管的根）。
fn site_dir(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join("site")
}

/// 递归收集 `*.html`（排序稳定：命令顺序与断言顺序都可复现）。
fn html_files(root: &Path) -> Result<Vec<PathBuf>, BundleError> {
    let mut out = Vec::new();
    collect_html(root, &mut out)?;
    out.sort();
    Ok(out)
}

/// 递归收集实现（目录优先深入，扩展名大小写不敏感）。
fn collect_html(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), BundleError> {
    for entry in std::fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        let path = entry.path();
        if entry.file_type().map_err(io_err(&path))?.is_dir() {
            collect_html(&path, out)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("html"))
        {
            out.push(path);
        }
    }
    Ok(())
}

/// 内建 HTML 结构断言：每个页面须含 doctype / html / body 标签（大小写不敏感）。
///
/// html-validate 缺失时的守底检查；失败退化为「产物不可托管」的显式错误。
fn assert_html_structure(site: &Path) -> Result<(), BundleError> {
    for html in html_files(site)? {
        let text = std::fs::read_to_string(&html).map_err(io_err(&html))?;
        let lower = text.to_lowercase();
        if let Some(missing) = HTML_REQUIRED.iter().find(|m| !lower.contains(**m)) {
            let rel = html.strip_prefix(site).unwrap_or(&html);
            return Err(BundleError::InvalidInput {
                packager: STATIC_SITE,
                field: "ui_dist(html)",
                value: rel.to_string_lossy().to_string(),
                reason: format!(
                    "缺少 `{missing}`（内建结构断言；安装 html-validate 可获完整校验）"
                ),
            });
        }
    }
    Ok(())
}

/// PWA manifest 结构与引用校验（错误一律 [`BundleError::InvalidInput`]）。
///
/// shortcut: M1 只做结构与引用存在性校验，不做 W3C manifest 全量 schema 校验。
pub(crate) fn validate_manifest(
    value: &serde_json::Value,
    site: &Path,
    packager: &'static str,
) -> Result<(), BundleError> {
    for (key, field) in MANIFEST_REQUIRED_TEXT {
        if !value
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
        {
            return Err(invalid(
                packager,
                field,
                value.get(*key),
                "必填，且须为非空字符串",
            ));
        }
    }

    let icons = value
        .get("icons")
        .and_then(serde_json::Value::as_array)
        .filter(|icons| !icons.is_empty())
        .ok_or_else(|| {
            invalid(
                packager,
                "web_manifest(icons)",
                value.get("icons"),
                "必填，且须为非空数组",
            )
        })?;
    for icon in icons {
        let src = icon
            .get("src")
            .and_then(serde_json::Value::as_str)
            .filter(|src| !src.trim().is_empty())
            .ok_or_else(|| {
                invalid(
                    packager,
                    "web_manifest(icons.src)",
                    icon.get("src"),
                    "必填，且须为非空字符串",
                )
            })?;
        if !icon
            .get("sizes")
            .and_then(serde_json::Value::as_str)
            .is_some_and(sizes_is_valid)
        {
            return Err(invalid(
                packager,
                "web_manifest(icons.sizes)",
                icon.get("sizes"),
                "须为 `192x192` 形态或 `any`",
            ));
        }
        // 本地引用必须真在站点里（托管即 404 的图标等于没有 PWA 资产）。
        let remote =
            src.starts_with("http://") || src.starts_with("https://") || src.starts_with('/');
        if !remote && !site.join(src).is_file() {
            return Err(invalid(
                packager,
                "web_manifest(icons.src)",
                icon.get("src"),
                &format!("图标文件缺失：站点内不存在 {src}"),
            ));
        }
    }

    for (key, field) in [
        ("theme_color", "web_manifest(theme_color)"),
        ("background_color", "web_manifest(background_color)"),
    ] {
        if let Some(color) = value.get(key)
            && !color.as_str().is_some_and(color_is_valid)
        {
            return Err(invalid(
                packager,
                field,
                Some(color),
                "须为 `#rgb` 或 `#rrggbb` 形态字符串",
            ));
        }
    }
    Ok(())
}

/// 图标尺寸形态：`any` 或 `数字x数字`。
fn sizes_is_valid(sizes: &str) -> bool {
    if sizes == "any" {
        return true;
    }
    let Some((w, h)) = sizes.split_once(['x', 'X']) else {
        return false;
    };
    !w.is_empty()
        && !h.is_empty()
        && w.chars().all(|c| c.is_ascii_digit())
        && h.chars().all(|c| c.is_ascii_digit())
}

/// 颜色形态：`#rgb` 或 `#rrggbb`。
fn color_is_valid(color: &str) -> bool {
    let Some(hex) = color.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

/// 构造 manifest 校验错误（`value` 为缺失字段时按 `null` 呈现）。
fn invalid(
    packager: &'static str,
    field: &'static str,
    value: Option<&serde_json::Value>,
    reason: &str,
) -> BundleError {
    BundleError::InvalidInput {
        packager,
        field,
        value: value.map_or_else(|| "null".to_string(), serde_json::Value::to_string),
        reason: reason.to_string(),
    }
}

/// 参数形态（路径一律显式传参，避免拼接成 shell 串）。
fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_sizes_and_color_forms() {
        for ok in ["any", "192x192", "48x48", "1024x1024"] {
            assert!(sizes_is_valid(ok), "应接受 {ok:?}");
        }
        for bad in ["", "192", "192x", "x192", "192*192", "big"] {
            assert!(!sizes_is_valid(bad), "应拒绝 {bad:?}");
        }
        for ok in ["#fff", "#0f172a", "#ABCDEF"] {
            assert!(color_is_valid(ok), "应接受 {ok:?}");
        }
        for bad in ["", "fff", "#ff", "blue", "#12345", "rgb(0,0,0)"] {
            assert!(!color_is_valid(bad), "应拒绝 {bad:?}");
        }
    }

    #[test]
    fn format_guard_rejects_other_formats() {
        let mut spec = TargetSpec::new_stub("web", 1, &[poly_targets::Host::Linux]);
        spec.package.format = "appimage".to_string();
        let err = check_format(&spec, STATIC_SITE).expect_err("格式不符必须报错");
        assert_eq!(err.code(), crate::error::codes::E_BND_0003);
        spec.package.format = STATIC_SITE.to_string();
        assert!(check_format(&spec, STATIC_SITE).is_ok());
    }
}
