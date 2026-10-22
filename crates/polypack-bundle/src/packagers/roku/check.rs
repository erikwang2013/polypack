//! Roku 频道 zip 的侧载结构自检（打包器自检与 verify 腿同一实现）。
//!
//! 规则面对齐 T-M4-11 spike 自检 19 项中与产物有关的全部条目：zip 条目/根布局（manifest +
//! source/ + components/，路径无绝对/回溯/反斜杠）、manifest 四个必填字段（版本三段为数字）、
//! SceneGraph XML 良构 + 根组件形态 + initialFocus、焦点表可解析且 order/edges 对齐、
//! main.brs 入口形态、PolypackScene.brs 交互逻辑形态。缺陷一律 [`BundleError::Archive`]
//! 硬失败（E_BND_0006），绝不降级成跳过。

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::BundleError;
use crate::zipwrite;

use super::MANIFEST;

/// zip 根布局允许的条目（对齐 spike 自检：manifest 文件 + 五个可选目录）。
const ALLOWED_ROOTS: [&str; 6] = [
    "manifest",
    "source",
    "components",
    "images",
    "fonts",
    "locale",
];

/// zip 必需条目（channel 骨架三件 + 发射器两件）。
const REQUIRED_ENTRIES: [&str; 5] = [
    "manifest",
    "source/main.brs",
    "source/focusgraph.brs",
    "components/PolypackScene.xml",
    "components/PolypackScene.brs",
];

const SCENE_XML: &str = "components/PolypackScene.xml";
const SCENE_BRS: &str = "components/PolypackScene.brs";
const MAIN_BRS: &str = "source/main.brs";
const FOCUS_BRS: &str = "source/focusgraph.brs";

/// 结构自检入口（`package()` 落包后自检 + verify 腿 `sideload-structure-check`）。
pub fn check_roku_structure(package: &Path) -> Result<RokuStructure, BundleError> {
    let bad = |reason: String| BundleError::Archive {
        path: package.to_path_buf(),
        reason,
    };
    let mut entries = zipwrite::entry_names(package)?;
    entries.sort();
    if entries.is_empty() {
        return Err(bad("包内没有任何条目".to_string()));
    }
    for required in REQUIRED_ENTRIES {
        if !entries.iter().any(|entry| entry == required) {
            return Err(bad(format!("缺必需条目：{required}")));
        }
    }
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.starts_with('/') || entry.contains("..") || entry.contains('\\'))
    {
        return Err(bad(format!("条目路径形态非法：{entry}")));
    }
    if let Some(root) = entries
        .iter()
        .map(|entry| entry.split('/').next().unwrap_or(entry))
        .find(|root| !ALLOWED_ROOTS.contains(root))
    {
        return Err(bad(format!(
            "zip 根多出未知条目：{root}（只允许 manifest + source/ + components/ + \
             images/ + fonts/ + locale/）"
        )));
    }

    let manifest = zipwrite::entry_text(package, MANIFEST)?
        .ok_or_else(|| bad(format!("缺 {MANIFEST}（频道清单必须落在包根）")))?;
    let fields = manifest_fields(&manifest);
    let title = fields
        .get("title")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| bad("manifest 缺必填字段：title".to_string()))?
        .to_string();
    let mut numbers = [0u64; 3];
    for (slot, key) in ["major_version", "minor_version", "build_version"]
        .into_iter()
        .enumerate()
    {
        let value = fields
            .get(key)
            .ok_or_else(|| bad(format!("manifest 缺必填字段：{key}")))?;
        if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_digit()) {
            return Err(bad(format!("manifest.{key} 非数字：{value}")));
        }
        numbers[slot] = value
            .parse()
            .map_err(|_| bad(format!("manifest.{key} 非数字：{value}")))?;
    }

    let xml =
        zipwrite::entry_text(package, SCENE_XML)?.ok_or_else(|| bad(format!("缺 {SCENE_XML}")))?;
    xml_well_formed(&xml).map_err(|err| bad(format!("{SCENE_XML} 不良构：{err}")))?;
    if !xml.contains("<component name=\"PolypackScene\" extends=\"Scene\"") {
        return Err(bad(
            "根组件形态不符（须 <component name=\"PolypackScene\" extends=\"Scene\">）".to_string(),
        ));
    }
    if !xml.contains("initialFocus=\"node_") {
        return Err(bad("initialFocus 缺失（须指向 node_<id>）".to_string()));
    }

    let brs =
        zipwrite::entry_text(package, FOCUS_BRS)?.ok_or_else(|| bad(format!("缺 {FOCUS_BRS}")))?;
    let line = brs
        .lines()
        .find(|line| line.trim_start().starts_with("return "))
        .ok_or_else(|| bad("焦点表缺 return 行".to_string()))?;
    let json = line
        .trim_start()
        .strip_prefix("return ")
        .unwrap_or_default()
        .trim();
    let table: serde_json::Value =
        serde_json::from_str(json).map_err(|err| bad(format!("焦点表非法 JSON：{err}")))?;
    let order = table
        .get("order")
        .and_then(serde_json::Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or_else(|| bad("焦点表 order 缺失或为空".to_string()))?;
    let edges = table
        .get("edges")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| bad("焦点表 edges 缺失".to_string()))?;
    if edges.len() != order.len() {
        return Err(bad(format!(
            "焦点表 edges 条目数 {} ≠ order 节点数 {}",
            edges.len(),
            order.len()
        )));
    }
    let initial = table
        .get("initial")
        .and_then(serde_json::Value::as_str)
        .filter(|id| order.iter().any(|item| item.as_str() == Some(*id)))
        .ok_or_else(|| bad("焦点表 initial 缺失或不在 order 内".to_string()))?
        .to_string();

    let main = zipwrite::entry_text(package, MAIN_BRS)?.unwrap_or_default();
    if !main.contains("roSGScreen") || !main.contains("CreateScene(\"PolypackScene\")") {
        return Err(bad(
            "main.brs 入口形态不符（须含 roSGScreen + CreateScene(\"PolypackScene\")）".to_string(),
        ));
    }
    let scene = zipwrite::entry_text(package, SCENE_BRS)?.unwrap_or_default();
    if !scene.contains("function onKeyEvent(") || !scene.contains(".setFocus(") {
        return Err(bad(
            "PolypackScene.brs 交互逻辑形态不符（须含 onKeyEvent + setFocus）".to_string(),
        ));
    }

    Ok(RokuStructure {
        entries,
        title,
        major: numbers[0],
        minor: numbers[1],
        build: numbers[2],
        initial_focus: initial,
        focus_nodes: order.len(),
    })
}

/// 结构自检结果（verify 腿的判决面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RokuStructure {
    /// zip 条目名（排序）。
    pub entries: Vec<String>,
    /// `manifest.title`。
    pub title: String,
    /// `manifest.major_version`。
    pub major: u64,
    /// `manifest.minor_version`。
    pub minor: u64,
    /// `manifest.build_version`。
    pub build: u64,
    /// 焦点表 `initial`（BRS 侧入口节点）。
    pub initial_focus: String,
    /// 焦点表节点数（`order` 长度）。
    pub focus_nodes: usize,
}

/// `manifest` 的 `key=value` 行（空行与 `#` 注释跳过；重复键后者覆盖——spike 同义）。
fn manifest_fields(text: &str) -> BTreeMap<&str, &str> {
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            fields.insert(key.trim(), value.trim());
        }
    }
    fields
}

/// 最小 XML 良构扫描器（对齐 spike `xmlWellFormed`：标签配对 + 文本段无未转义字符）。
/// 只覆盖发射器产出的语法子集（元素 / 属性 / 自闭合 / 注释 / XML 声明），够用即止。
fn xml_well_formed(xml: &str) -> Result<(), String> {
    let mut body = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find("<!--") {
        body.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        let Some(end) = after.find("-->") else {
            return Err("注释未闭合".to_string());
        };
        rest = &after[end + 3..];
    }
    body.push_str(rest);
    let body = match body.strip_prefix("<?xml") {
        Some(after) => match after.find("?>") {
            Some(end) => &after[end + 2..],
            None => return Err("XML 声明未闭合".to_string()),
        },
        None => body.as_str(),
    };

    let bytes = body.as_bytes();
    let mut stack: Vec<&str> = Vec::new();
    let mut text_start = 0usize;
    let mut index = 0usize;
    while index < body.len() {
        if bytes[index] != b'<' {
            index += 1;
            continue;
        }
        text_segment(&body[text_start..index])?;
        let Some(end) = find_tag_end(body, index + 1) else {
            return Err(format!(
                "标签未闭合：{}",
                body[index..].chars().take(40).collect::<String>()
            ));
        };
        let content = &body[index + 1..end];
        if let Some(name) = content.strip_prefix('/') {
            let name = name.trim();
            match stack.pop() {
                Some(open) if open == name => {}
                Some(open) => return Err(format!("闭合标签不匹配：{name}（期望 {open}）")),
                None => return Err(format!("多余的闭合标签：{name}")),
            }
        } else if content.starts_with('?') {
            if !content.ends_with('?') {
                return Err("处理指令形态非法".to_string());
            }
        } else if content.starts_with('!') {
            return Err("不支持 DOCTYPE / 声明（发射器产出不应含）".to_string());
        } else {
            let (name, self_close) = open_tag(content)?;
            if !self_close {
                stack.push(name);
            }
        }
        index = end + 1;
        text_start = index;
    }
    // 尾段只允许空白（对齐 spike：根元素之后不得有残留字符）
    let tail = &body[text_start..];
    if !tail.trim().is_empty() {
        return Err(format!(
            "尾段残留字符：{}",
            tail.trim().chars().take(40).collect::<String>()
        ));
    }
    if let Some(open) = stack.last() {
        return Err(format!("未闭合标签：{open}"));
    }
    Ok(())
}

/// 起始标签：标签名 + 属性形态，返回 `(name, 自闭合)`。
fn open_tag(content: &str) -> Result<(&str, bool), String> {
    let trimmed = content.trim_end();
    let (rest, self_close) = match trimmed.strip_suffix('/') {
        Some(rest) => (rest.trim_end(), true),
        None => (trimmed, false),
    };
    let name_end = rest
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-')))
        .unwrap_or(rest.len());
    let name = &rest[..name_end];
    if name.is_empty() || !name.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_') {
        return Err(format!(
            "非法标签名：{}",
            name.chars().take(20).collect::<String>()
        ));
    }
    attributes_valid(&rest[name_end..])?;
    Ok((name, self_close))
}

/// 属性段：仅允许 `name="value"`（或单引号）以空白分隔。
fn attributes_valid(mut rest: &str) -> Result<(), String> {
    loop {
        let trimmed = rest.trim_start();
        if trimmed.len() == rest.len() {
            return if rest.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "标签属性形态非法：{}",
                    rest.chars().take(40).collect::<String>()
                ))
            };
        }
        rest = trimmed;
        let name_end = rest
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-')))
            .unwrap_or(rest.len());
        if name_end == 0 {
            return Err("属性名缺失".to_string());
        }
        rest = &rest[name_end..];
        let Some(after_eq) = rest.strip_prefix('=') else {
            return Err("属性缺 `=`".to_string());
        };
        let Some(quote) = after_eq
            .chars()
            .next()
            .filter(|ch| *ch == '"' || *ch == '\'')
        else {
            return Err("属性值须以引号包裹".to_string());
        };
        let value = &after_eq[1..];
        let Some(end) = value.find(quote) else {
            return Err("属性值未闭合".to_string());
        };
        rest = &value[end + 1..];
    }
}

/// 文本段：不得含 `>`，`&` 只能是五个 XML 实体之一。
fn text_segment(text: &str) -> Result<(), String> {
    if text.contains('>') {
        return Err(format!(
            "文本段含未转义字符：{}",
            text.chars().take(40).collect::<String>()
        ));
    }
    let mut rest = text;
    while let Some(index) = rest.find('&') {
        let after = &rest[index + 1..];
        let named = ["amp;", "lt;", "gt;", "quot;", "apos;"]
            .iter()
            .any(|entity| after.starts_with(entity));
        if !named {
            return Err(format!(
                "文本段含未转义 &：{}",
                rest.chars().take(40).collect::<String>()
            ));
        }
        rest = after;
    }
    Ok(())
}

/// 从 `start` 起找标签结束的 `>`（引号内的 `>` 不算；遇到 `<` 即失败）。
fn find_tag_end(body: &str, start: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (offset, byte) in body.as_bytes()[start..].iter().enumerate() {
        match (quote, *byte) {
            (Some(open), byte) if byte == open => quote = None,
            (None, b'"' | b'\'') => quote = Some(*byte),
            (None, b'>') => return Some(start + offset),
            (None, b'<') => return None,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests;
