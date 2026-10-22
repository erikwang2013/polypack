//! 商店 metadata 管线（M9-E5 / T-M9-13..14）：`store/metadata/<platform>/<locale>.toml`
//! 的版本化 schema、校验器与确定性材料打包。
//!
//! 目录：`store/metadata/<platform>/<locale>.toml`（`default.toml` 必须存在）；
//! 截图只存相对 platform 目录的路径（如 `screenshots/home.png`），不读图片内容。
//! 字段：title、short_description、full_description、keywords[]、screenshots[]、
//! privacy_url、support_url；`schema = 1` 必填，未知版本报违规，不静默降级。
//!
//! ## 平台规则表（检索日期 2026-10-20；规则漂移时升 `schema` 并更新本表）
//!
//! 长度按平台口径折算：CJK / 全角字符计 2，其余计 1（[`length_units`]）。
//!
//! | 字段 | 微信（wechat） | 支付宝（alipay） |
//! | --- | --- | --- |
//! | title | 必填，4–30 | 必填，3–20 |
//! | short_description | 必填，4–120 | 必填，10–32 |
//! | full_description | 必填，4–2000（微信无独立长描述字段，内部编辑器上限） | 必填，20–500 |
//! | keywords | 必填，1–10 项，每项 1–20 | 必填，1–10 项，每项 1–20 |
//! | screenshots | 0–10 个路径，png/jpg/jpeg/gif | 必填，2–5 个路径，png/jpg/jpeg |
//! | privacy_url / support_url | 必填，http(s) 绝对 URL | 必填，http(s) 绝对 URL |
//!
//! 来源：微信名称 / 简介（4–30 / 4–120，中文计 2）与提审预览图（png/jpeg/jpg/gif，
//! 单图 ≤ 2 MB；上限按 85093 口径取保守 10）见
//! <https://developers.weixin.qq.com/miniprogram/dev/platform-capabilities/business-capabilities/ministore/minishopopencomponent/API/register/submit_basicinfo.html>
//! 与 <https://developers.weixin.qq.com/doc/oplatform/openApi/miniprogram-management/code-management/api_submitaudit.html>；
//! 支付宝名称 / 简介（3–20 / 10–32）与审核指引见 <https://opendocs.alipay.com/mini/0csvpl>；
//! 提审截图 2–5 张、jpg/png、单图 ≤ 4 MB 为次要来源
//! <https://www.bookstack.cn/read/alipay-mini-3rd/22.md>（提交前以控制台为准）。
//!
//! 校验返回**全部**违规（不短路）；违规码为稳定 ASCII 标识（[`codes`]）。材料对象按 key
//! 排序；摘要 = blake3(canonical JSON)，跨进程 / 跨平台稳定。新文件；遵守 `schema` 版本化。

#![allow(missing_docs)]

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path};

use serde::Serialize;
use serde_json::{Map, Value};

/// metadata schema 版本（`schema = 1`）。
pub const SCHEMA_VERSION: u32 = 1;
/// 默认 locale 文件名（不含 `.toml`）；缺失即违规。
pub const DEFAULT_LOCALE: &str = "default";

pub mod codes {
    pub const DEFAULT_MISSING: &str = "metadata.default_missing";
    pub const LOCALE_DUPLICATE: &str = "metadata.locale_duplicate";
    pub const FILE_READ: &str = "metadata.file_read";
    pub const PARSE: &str = "metadata.parse";
    pub const SCHEMA_MISSING: &str = "metadata.schema_missing";
    pub const SCHEMA_UNSUPPORTED: &str = "metadata.schema_unsupported";
    pub const FIELD_MISSING: &str = "metadata.field_missing";
    pub const FIELD_EMPTY: &str = "metadata.field_empty";
    pub const FIELD_TOO_SHORT: &str = "metadata.field_too_short";
    pub const FIELD_TOO_LONG: &str = "metadata.field_too_long";
    pub const KEYWORDS_TOO_FEW: &str = "metadata.keywords_too_few";
    pub const KEYWORDS_TOO_MANY: &str = "metadata.keywords_too_many";
    pub const KEYWORD_EMPTY: &str = "metadata.keyword_empty";
    pub const KEYWORD_TOO_LONG: &str = "metadata.keyword_too_long";
    pub const SCREENSHOTS_REQUIRED: &str = "metadata.screenshots_required";
    pub const SCREENSHOTS_TOO_FEW: &str = "metadata.screenshots_too_few";
    pub const SCREENSHOTS_TOO_MANY: &str = "metadata.screenshots_too_many";
    pub const SCREENSHOT_PATH_INVALID: &str = "metadata.screenshot_path_invalid";
    pub const SCREENSHOT_PATH_MISSING: &str = "metadata.screenshot_path_missing";
    pub const SCREENSHOT_EXTENSION: &str = "metadata.screenshot_extension";
    pub const URL_INVALID: &str = "metadata.url_invalid";
}

/// metadata 支持的两个已发布平台（目录名与 `Channel` 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// 微信开放平台（小程序）。
    Wechat,
    /// 支付宝开放平台（小程序）。
    Alipay,
}

impl Platform {
    /// platform 目录名（`store/metadata/<platform>/`）。
    pub fn id(self) -> &'static str {
        match self {
            Self::Wechat => "wechat",
            Self::Alipay => "alipay",
        }
    }

    /// 由 `polypack-publish` 渠道 id 反解（CLI 目标 / `--channel` 口径）。
    pub fn from_channel(channel: &str) -> Option<Self> {
        match channel {
            "wechat-open-platform" => Some(Self::Wechat),
            "alipay-open-platform" => Some(Self::Alipay),
            _ => None,
        }
    }

    fn rules(self) -> &'static Rules {
        match self {
            Self::Wechat => &WECHAT_RULES,
            Self::Alipay => &ALIPAY_RULES,
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

type TextRule = (&'static str, usize, usize);

struct Rules {
    fields: &'static [TextRule],
    keywords: (usize, usize, usize, usize),
    screenshots: (bool, usize, usize, &'static [&'static str]),
}

static WECHAT_RULES: Rules = Rules {
    fields: &[
        ("title", 4, 30),
        ("short_description", 4, 120),
        ("full_description", 4, 2000),
    ],
    keywords: (1, 10, 1, 20),
    screenshots: (false, 0, 10, &["png", "jpg", "jpeg", "gif"]),
};
static ALIPAY_RULES: Rules = Rules {
    fields: &[
        ("title", 3, 20),
        ("short_description", 10, 32),
        ("full_description", 20, 500),
    ],
    keywords: (1, 10, 1, 20),
    screenshots: (true, 2, 5, &["png", "jpg", "jpeg"]),
};

/// 一条违规（`code` 稳定；`message` 可读）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Violation {
    /// 稳定违规码（[`codes`]）。
    pub code: &'static str,
    /// 归一后的 locale；目录级违规为空串。
    pub locale: String,
    /// 字段名（`title` / `screenshots[0]`）；文件级违规为空串。
    pub field: String,
    /// 可读消息。
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.field.is_empty() {
            write!(f, "{} [{}]: {}", self.code, self.locale, self.message)
        } else {
            write!(
                f,
                "{} [{}.{}]: {}",
                self.code, self.locale, self.field, self.message
            )
        }
    }
}

/// 通过校验的一个 locale。
#[derive(Debug, Clone)]
pub struct LocaleMetadata {
    /// 归一后的 locale 名。
    pub locale: String,
    title: String,
    short_description: String,
    full_description: String,
    keywords: Vec<String>,
    screenshots: Vec<String>,
    privacy_url: String,
    support_url: String,
}

/// 确定性材料：`value` 为 key 排序的 JSON 对象，`digest` = blake3(canonical JSON)。
#[derive(Debug, Clone)]
pub struct Materials {
    /// 实际使用的 locale（请求缺失并回退时为 `default`）。
    pub locale: String,
    /// 材料对象（key 有序）。
    pub value: Value,
    /// 材料摘要（64 hex；与 [`materials_digest`] 同口径）。
    pub digest: String,
}

/// 一次目录加载 + 校验结果。
#[derive(Debug)]
pub struct MetadataSet {
    platform: Platform,
    locale_names: Vec<String>,
    locales: BTreeMap<String, LocaleMetadata>,
    violations: Vec<Violation>,
}

#[rustfmt::skip]
impl MetadataSet {
    /// 扫描到的 locale（归一、升序；含解析失败者，便于完整性报告）。
    pub fn locale_names(&self) -> &[String] { &self.locale_names }

    /// 全部违规（确定性顺序：locale → 字段 → 码）。
    pub fn violations(&self) -> &[Violation] { &self.violations }

    /// 是否通过全部校验。
    pub fn is_ok(&self) -> bool { self.violations.is_empty() }

    /// 按请求 locale 构建材料；请求缺失 → 回退 `default`（`locale` 字段如实回报）。
    pub fn materials(&self, requested: &str) -> Option<Materials> {
        self.locales.get(&normalize_locale(requested)).or_else(|| self.locales.get(DEFAULT_LOCALE)).map(|meta| build_materials(self.platform, meta))
    }

    /// 请求 locale 缺失且存在 default 可回退。
    pub fn falls_back(&self, requested: &str) -> bool {
        !self.locales.contains_key(&normalize_locale(requested)) && self.locales.contains_key(DEFAULT_LOCALE)
    }
}

macro_rules! bad {
    ($list:expr, $locale:expr, $code:expr, $field:expr, $($arg:tt)*) => {
        $list.push(Violation { code: $code, locale: $locale.to_string(), field: $field.to_string(), message: format!($($arg)*) })
    };
}

/// 加载并校验 `<dir>`（platform 目录）下全部 `*.toml`；单文件读 / 解析失败记为违规并继续。
#[rustfmt::skip]
pub fn load(platform: Platform, dir: impl AsRef<Path>) -> Result<MetadataSet, std::io::Error> {
    let dir = dir.as_ref().to_path_buf();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("toml") && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            files.push((stem.to_string(), normalize_locale(stem), path));
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut set = MetadataSet { platform, locale_names: Vec::new(), locales: BTreeMap::new(), violations: Vec::new() };
    let mut seen = BTreeMap::new();
    for (raw_name, key, path) in files {
        if let Some(prev) = seen.get(&key) {
            bad!(set.violations, key, codes::LOCALE_DUPLICATE, "", "重复 locale：{prev}.toml / {raw_name}.toml");
            continue;
        }
        seen.insert(key.clone(), raw_name);
        set.locale_names.push(key.clone());
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => { bad!(set.violations, key, codes::FILE_READ, "", "{}：{err}", path.display()); continue; }
        };
        let raw: toml::Value = match toml::from_str(&text) {
            Ok(raw) => raw,
            Err(err) => { bad!(set.violations, key, codes::PARSE, "", "{}：{err}", path.display()); continue; }
        };
        let (meta, mut found) = validate(platform, &dir, &key, &raw);
        let ok = found.is_empty();
        set.violations.append(&mut found);
        if ok { set.locales.insert(key, meta); }
    }
    if !seen.contains_key(DEFAULT_LOCALE) {
        bad!(set.violations, "", codes::DEFAULT_MISSING, "", "缺少 default.toml");
    }
    set.violations.sort_by(|a, b| (&a.locale, &a.field, a.code, &a.message).cmp(&(&b.locale, &b.field, b.code, &b.message)));
    Ok(set)
}

#[rustfmt::skip]
fn validate(platform: Platform, dir: &Path, locale: &str, raw: &toml::Value) -> (LocaleMetadata, Vec<Violation>) {
    let rules = platform.rules();
    let mut v = Vec::new();
    match raw.get("schema").and_then(toml::Value::as_integer) {
        Some(schema) if schema == SCHEMA_VERSION as i64 => {}
        Some(other) => bad!(v, locale, codes::SCHEMA_UNSUPPORTED, "schema", "schema {other} 不受支持（支持 {SCHEMA_VERSION}）"),
        None => bad!(v, locale, codes::SCHEMA_MISSING, "schema", "缺少 schema（支持 {SCHEMA_VERSION}）"),
    }
    let rule = |key: &str| *rules.fields.iter().find(|rule| rule.0 == key).expect("字段规则");
    let text = |key: &str| raw.get(key).and_then(toml::Value::as_str);
    let title = text_field(&mut v, locale, text("title"), rule("title"));
    let short_description = text_field(&mut v, locale, text("short_description"), rule("short_description"));
    let full_description = text_field(&mut v, locale, text("full_description"), rule("full_description"));
    let keywords = list_field(&mut v, locale, strings(raw.get("keywords")), rules.keywords);
    let screenshots = screenshots_field(platform, dir, locale, strings(raw.get("screenshots")), &mut v);
    let privacy_url = url_field(&mut v, locale, "privacy_url", text("privacy_url"));
    let support_url = url_field(&mut v, locale, "support_url", text("support_url"));
    (LocaleMetadata { locale: locale.to_string(), title, short_description, full_description, keywords, screenshots, privacy_url, support_url }, v)
}

fn strings(value: Option<&toml::Value>) -> Option<Vec<String>> {
    value?
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_string))
        .collect()
}

#[rustfmt::skip]
fn text_field(v: &mut Vec<Violation>, locale: &str, value: Option<&str>, rule: TextRule) -> String {
    let (key, min, max) = rule;
    let Some(value) = value else {
        bad!(v, locale, codes::FIELD_MISSING, key, "缺少必填字段 {key}");
        return String::new();
    };
    let value = value.trim().to_string();
    if value.is_empty() {
        bad!(v, locale, codes::FIELD_EMPTY, key, "字段 {key} 为空");
        return value;
    }
    let units = length_units(&value);
    if units < min { bad!(v, locale, codes::FIELD_TOO_SHORT, key, "字段 {key} 过短：{units} < {min}"); }
    if units > max { bad!(v, locale, codes::FIELD_TOO_LONG, key, "字段 {key} 超长：{units} > {max}"); }
    value
}

#[rustfmt::skip]
fn url_field(v: &mut Vec<Violation>, locale: &str, key: &str, value: Option<&str>) -> String {
    let Some(value) = value else {
        bad!(v, locale, codes::FIELD_MISSING, key, "缺少必填字段 {key}");
        return String::new();
    };
    let value = value.trim().to_string();
    if value.is_empty() {
        bad!(v, locale, codes::FIELD_EMPTY, key, "字段 {key} 为空");
    } else if !valid_url(&value) {
        bad!(v, locale, codes::URL_INVALID, key, "非法 URL：{value}");
    }
    value
}

#[rustfmt::skip]
fn list_field(v: &mut Vec<Violation>, locale: &str, value: Option<Vec<String>>, rule: (usize, usize, usize, usize)) -> Vec<String> {
    let (min_items, max_items, min_len, max_len) = rule;
    let items = value.unwrap_or_default();
    if items.len() < min_items { bad!(v, locale, codes::KEYWORDS_TOO_FEW, "keywords", "keywords 少于 {min_items} 项"); }
    if items.len() > max_items { bad!(v, locale, codes::KEYWORDS_TOO_MANY, "keywords", "keywords 多于 {max_items} 项"); }
    items.iter().enumerate().filter_map(|(index, item)| {
        let field = format!("keywords[{index}]");
        let item = item.trim().to_string();
        if item.is_empty() {
            bad!(v, locale, codes::KEYWORD_EMPTY, field, "{field} 为空");
            return None;
        }
        if !(min_len..=max_len).contains(&length_units(&item)) {
            bad!(v, locale, codes::KEYWORD_TOO_LONG, field, "{field} 长度越界");
        }
        Some(item)
    }).collect()
}

#[rustfmt::skip]
fn screenshots_field(platform: Platform, dir: &Path, locale: &str, value: Option<Vec<String>>, v: &mut Vec<Violation>) -> Vec<String> {
    let (required, min_items, max_items, extensions) = platform.rules().screenshots;
    let items = value.unwrap_or_default();
    if items.len() < min_items {
        let code = if items.is_empty() && required { codes::SCREENSHOTS_REQUIRED } else { codes::SCREENSHOTS_TOO_FEW };
        bad!(v, locale, code, "screenshots", "screenshots 至少需要 {min_items} 个");
    }
    if items.len() > max_items { bad!(v, locale, codes::SCREENSHOTS_TOO_MANY, "screenshots", "screenshots 多于 {max_items} 个"); }
    items.iter().enumerate().filter_map(|(index, item)| {
        let field = format!("screenshots[{index}]");
        let path = item.trim().to_string();
        if !relative_posix_path(&path) {
            bad!(v, locale, codes::SCREENSHOT_PATH_INVALID, field, "{field} 不是相对 POSIX 路径：{path}");
            return None;
        }
        let ext = Path::new(&path).extension().and_then(|ext| ext.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        if !extensions.contains(&ext.as_str()) {
            bad!(v, locale, codes::SCREENSHOT_EXTENSION, field, "{field} 扩展名 .{ext} 不支持");
        }
        if !dir.join(&path).is_file() {
            bad!(v, locale, codes::SCREENSHOT_PATH_MISSING, field, "{field} 文件不存在：{path}");
        }
        Some(path)
    }).collect()
}

/// 平台长度口径：CJK / 全角字符计 2，其余计 1。
pub fn length_units(text: &str) -> usize {
    text.chars().map(|ch| if is_wide(ch) { 2 } else { 1 }).sum()
}

fn is_wide(ch: char) -> bool {
    matches!(ch as u32, 0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x20000..=0x3FFFD)
}

fn relative_posix_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn valid_url(value: &str) -> bool {
    let Some(rest) = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = authority.rsplit_once(':').unwrap_or((authority, ""));
    !host.is_empty()
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host.contains('.')
        && host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
        && !authority.contains('@')
        && (port.is_empty() || port.parse::<u16>().is_ok_and(|port| port > 0))
        && !value
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
}

fn normalize_locale(locale: &str) -> String {
    locale.trim().replace('_', "-").to_ascii_lowercase()
}

#[rustfmt::skip]
fn build_materials(platform: Platform, meta: &LocaleMetadata) -> Materials {
    let mut map = Map::new();
    let entries = [
        ("schema", Value::from(SCHEMA_VERSION)),
        ("platform", Value::from(platform.id())),
        ("locale", Value::from(meta.locale.clone())),
        ("title", Value::from(meta.title.clone())),
        ("short_description", Value::from(meta.short_description.clone())),
        ("full_description", Value::from(meta.full_description.clone())),
        ("keywords", Value::from(meta.keywords.clone())),
        ("screenshots", Value::from(meta.screenshots.clone())),
        ("privacy_url", Value::from(meta.privacy_url.clone())),
        ("support_url", Value::from(meta.support_url.clone())),
    ];
    for (key, value) in entries { map.insert(key.to_string(), value); }
    let value = canonical_json(&Value::Object(map));
    let digest = materials_digest(&value);
    Materials { locale: meta.locale.clone(), value, digest }
}

/// 递归 key 排序的 canonical JSON（数组顺序保留）。
#[rustfmt::skip]
pub fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let sorted: BTreeMap<&str, Value> = object.iter().map(|(key, value)| (key.as_str(), canonical_json(value))).collect();
            Value::Object(sorted.into_iter().map(|(key, value)| (key.to_string(), value)).collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

/// 材料摘要：blake3(canonical JSON UTF-8)，64 hex；跨进程 / 跨平台稳定。
pub fn materials_digest(value: &Value) -> String {
    let bytes = serde_json::to_vec(&canonical_json(value)).expect("canonical JSON 序列化");
    blake3::hash(&bytes).to_hex().to_string()
}
