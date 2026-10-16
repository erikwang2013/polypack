//! UDL 读取（T-M2-01 的「生成产物 ↔ 接口定义」一致性断言基础）。
//!
//! 只解析**断言用得上的那一层**：namespace 名与块内的顶层函数签名（函数名 / 参数名 /
//! 返回类型）。dictionary / enum / interface（对象）/ callback 留给 M3 的完整模型。
//!
//! shortcut: 不建完整 UDL 语法树，只按语句切分 + 深度感知的逗号切分参数；
//! 遇到 `interface` / `dictionary` / `enum` / `callback` / `typedef` 开头的语句整体跳过。
//! 需要按类型生成代码（而非断言签名）时升级为完整解析器（M3 出口）。

use std::path::Path;

use crate::diagnostics::{Diagnostic, codes};
use crate::plan::StageError;

/// 一个顶层函数签名。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// 函数名。
    pub name: String,
    /// 参数名（有序）。
    pub args: Vec<String>,
    /// 参数类型文本（与 [`Self::args`] 同序，原样保留，如 `string` / `u32`）。
    pub arg_types: Vec<String>,
    /// 返回类型文本（原样保留，如 `string` / `sequence<u32>`）。
    pub ret: String,
}

/// 解析结果：namespace 与函数清单。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interface {
    /// UDL namespace（= 生成绑定的包名）。
    pub namespace: String,
    /// 顶层函数（声明序）。
    pub functions: Vec<Function>,
}

/// 读取并解析一份 UDL。
pub fn parse_source(path: &Path) -> Result<Interface, StageError> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        Diagnostic::error(
            codes::E_BRIDGE_0002,
            format!("无法读取接口定义 {}: {err}", path.display()),
        )
        .with_hint("确认 UDL 文件存在且当前用户可读")
    })?;
    parse(&text).map_err(|reason| {
        Diagnostic::error(
            codes::E_BRIDGE_0002,
            format!("接口定义解析失败（{}）: {reason}", path.display()),
        )
        .with_hint("UDL 需要一份 `namespace <名字> { … };` 块；核对括号与分号")
        .into()
    })
}

/// 解析 UDL 文本。
pub fn parse(text: &str) -> Result<Interface, String> {
    let stripped = strip_comments(text);
    let (namespace, body) = split_namespace(&stripped)?;
    let mut functions = Vec::new();
    for statement in split_statements(&body) {
        if let Some(function) = parse_function(&statement) {
            functions.push(function);
        }
    }
    Ok(Interface {
        namespace,
        functions,
    })
}

/// 去掉 `//` 行注释与 `/* */` 块注释。
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '/' if chars.peek() == Some(&'/') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for next in chars.by_ref() {
                    if prev == '*' && next == '/' {
                        break;
                    }
                    prev = next;
                }
            }
            _ => out.push(ch),
        }
    }
    out
}

/// 切出 `namespace <名> { … }` 的名字与块体（块内括号计数，支持块中嵌套的 `{}`）。
fn split_namespace(text: &str) -> Result<(String, String), String> {
    let start = text
        .find("namespace")
        .ok_or_else(|| "缺少 `namespace` 声明".to_string())?;
    let rest = &text[start + "namespace".len()..];
    let brace = rest
        .find('{')
        .ok_or_else(|| "namespace 缺少 `{`".to_string())?;
    let namespace: String = rest[..brace]
        .trim()
        .split(|ch: char| ch.is_whitespace())
        .next()
        .unwrap_or_default()
        .to_string();
    if namespace.is_empty() {
        return Err("namespace 缺少名字".to_string());
    }
    let body = &rest[brace + 1..];
    let mut depth = 1usize;
    let mut end = None;
    for (index, ch) in body.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(index);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end.ok_or_else(|| "namespace 块未闭合（缺 `}`）".to_string())?;
    Ok((namespace, body[..end].to_string()))
}

/// 按顶层 `;` 切语句（`{}` 内不分；`dictionary { … };` 也会被整体切成一句）。
fn split_statements(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in body.chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ';' if depth == 0 => {
                out.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    out.push(current);
    out.into_iter()
        .map(|statement| statement.trim().to_string())
        .filter(|statement| !statement.is_empty())
        .collect()
}

/// 解析一条语句为函数签名；非函数语句返回 `None`。
fn parse_function(statement: &str) -> Option<Function> {
    // 形如 `[Throws=DemoError] string hello(string name);` —— 属性前缀整段剥掉
    let statement = match statement.strip_prefix('[') {
        Some(rest) => rest.split_once(']')?.1.trim_start(),
        None => statement,
    };
    let head = statement.split('{').next()?.trim();
    let open = head.find('(')?;
    let close = head.rfind(')')?;
    if close < open {
        return None;
    }
    let before = head[..open].trim();
    // 首关键字是声明者的语句不是函数：`callback OnDone = void (string);` 的首段即 `callback`
    // （按「名字前的类型段」判会漏掉带 `=` 的 callback，故看首段）
    if matches!(
        before.split_whitespace().next()?,
        "dictionary" | "enum" | "interface" | "callback" | "typedef"
    ) {
        return None;
    }
    let (ret, name) = before.rsplit_once(char::is_whitespace)?;
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|ch| ch.is_alphanumeric() || ch == '_') {
        return None;
    }
    let mut args = Vec::new();
    let mut arg_types = Vec::new();
    for arg in split_args(&head[open + 1..close]) {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }
        // `string name` / `string? name` / `sequence<u32> values`：末段即参数名，其前是类型
        let Some((ty, raw_name)) = arg.rsplit_once(char::is_whitespace) else {
            continue;
        };
        let name = raw_name.trim_end_matches(['?', '=']).trim();
        if name.is_empty() {
            continue;
        }
        args.push(name.to_string());
        // 类型文本原样保留（`string?` 的 `?` 是语义，不许抹掉——可选与空串在 FFI 层不同义）
        arg_types.push(ty.trim().to_string());
    }
    Some(Function {
        name: name.to_string(),
        args,
        arg_types,
        ret: ret.trim().to_string(),
    })
}

/// 按顶层逗号切参数（`record<string, string>` / `sequence<map<string, u32>>` 的内层逗号不算）。
fn split_args(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for ch in text.chars() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    out.push(current);
    out
}
