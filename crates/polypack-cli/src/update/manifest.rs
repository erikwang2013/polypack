//! 更新清单的纯函数面：确定性渲染、schema 校验、产物选择、时刻与凭据引用。
//!
//! 这里不打印、不签写；`polypack update manifest` / `check` 负责 IO 与签名编排。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use polypack_core::CredentialRef;
use polypack_core::codes;
use polypack_core::diagnostics::Diagnostic;
use polypack_sign::SignError;
use serde::Serialize;

/// 渠道名长度上限。
const MAX_CHANNEL: usize = 64;
/// Tauri v2 平台键的 OS 段（官方静态 JSON 口径）。
const OS: [&str; 3] = ["linux", "darwin", "windows"];
/// Tauri v2 平台键的 ARCH 段。
const ARCH: [&str; 4] = ["x86_64", "aarch64", "i686", "armv7"];

/// Tauri 静态 JSON 的确定性渲染：BTreeMap 保证键字典序，固定两空格缩进与末尾换行。
#[derive(Serialize)]
struct PlatformEntry {
    signature: String,
    url: String,
}

#[derive(Serialize)]
struct Manifest<'a> {
    notes: &'a str,
    platforms: BTreeMap<String, PlatformEntry>,
    pub_date: &'a str,
    version: &'a str,
}

pub(super) fn render_manifest(
    version: &str,
    notes: &str,
    pub_date: &str,
    platform_key: &str,
    signature: &[u8],
    url: &str,
) -> String {
    let signature = String::from_utf8_lossy(signature).into_owned();
    let mut platforms = BTreeMap::new();
    platforms.insert(
        platform_key.to_string(),
        PlatformEntry {
            signature,
            url: url.to_string(),
        },
    );
    let manifest = Manifest {
        notes,
        platforms,
        pub_date,
        version,
    };
    let mut text = serde_json::to_string_pretty(&manifest).expect("清单结构可序列化");
    text.push('\n');
    text
}

/// 读取清单并校验必需字段；返回版本号。
pub(super) fn validate_manifest(bytes: &[u8]) -> Result<String, Diagnostic> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|err| schema_error(format!("清单不是合法 JSON：{err}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| schema_error("清单顶层必须是 JSON 对象"))?;
    let version = object
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| schema_error("缺少字符串字段 version"))?;
    validate_version(version)?;
    if let Some(value) = object.get("pub_date") {
        let date = value
            .as_str()
            .ok_or_else(|| schema_error("pub_date 必须是字符串"))?;
        if !valid_rfc3339(date) {
            return Err(schema_error(format!("pub_date 不是 RFC 3339：{date}")));
        }
    }
    let platforms = object
        .get("platforms")
        .and_then(serde_json::Value::as_object)
        .filter(|platforms| !platforms.is_empty())
        .ok_or_else(|| schema_error("platforms 必须是非空对象"))?;
    for (key, entry) in platforms {
        if key.is_empty() || key.chars().any(char::is_whitespace) || !key.contains('-') {
            return Err(schema_error(format!("平台键非法：{key:?}（应为 OS-ARCH）")));
        }
        let url = entry
            .get("url")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| schema_error(format!("platforms.{key}.url 缺失")))?;
        if !valid_url(url) {
            return Err(schema_error(format!(
                "platforms.{key}.url 不是 HTTP(S) URL"
            )));
        }
        let signature = entry
            .get("signature")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| schema_error(format!("platforms.{key}.signature 缺失")))?;
        if signature.trim().is_empty() {
            return Err(schema_error(format!("platforms.{key}.signature 为空")));
        }
    }
    Ok(version.to_string())
}

pub(super) fn validate_channel(channel: &str) -> Result<(), Diagnostic> {
    let valid = !channel.is_empty()
        && channel.len() <= MAX_CHANNEL
        && !channel.starts_with('.')
        && !channel.contains("..")
        && channel.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        });
    if valid {
        Ok(())
    } else {
        Err(input_error(format!("渠道名非法：{channel:?}")).with_hint(
            "渠道用 1–64 个小写字母 / 数字 / `.` / `_` / `-`，不得以 `.` 开头或包含 `..`",
        ))
    }
}

pub(super) fn validate_version(version: &str) -> Result<(), Diagnostic> {
    let stripped = version.strip_prefix('v').unwrap_or(version);
    let core = stripped.split(['-', '+']).next().unwrap_or_default();
    let parts: Vec<&str> = core.split('.').collect();
    let valid = parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && !version.contains(['/', '\\'])
        && !version.chars().any(char::is_whitespace);
    if valid {
        Ok(())
    } else {
        Err(input_error(format!("版本号不是 SemVer：{version:?}"))
            .with_hint("写主.次.补丁（如 1.2.3）；Tauri 允许前导 v 与 -pre / +build 后缀"))
    }
}

pub(super) fn validate_platform(platform: &str, arch: &str) -> Result<(), Diagnostic> {
    if OS.contains(&platform) && ARCH.contains(&arch) {
        Ok(())
    } else {
        Err(
            input_error(format!("平台键非法：{platform}-{arch}")).with_hint(format!(
                "OS 取 {:?}，ARCH 取 {:?}（Tauri v2 标准平台键）",
                OS, ARCH
            )),
        )
    }
}

pub(super) fn validated_base_url(value: &str) -> Result<String, Diagnostic> {
    let trimmed = value.trim_end_matches('/');
    if valid_url(trimmed) && !value.chars().any(char::is_whitespace) {
        Ok(trimmed.to_string())
    } else {
        Err(
            input_error(format!("--base-url 不是 HTTP(S) URL：{value:?}"))
                .with_hint("形如 https://updates.example.com/<channel>；生产环境必须 HTTPS"),
        )
    }
}

fn valid_url(value: &str) -> bool {
    (value.starts_with("https://") || value.starts_with("http://")) && value.len() > 8
}

/// 输入时刻：显式 `--pub-date` → `SOURCE_DATE_EPOCH` → 当前时间。
pub(super) fn pub_date(explicit: Option<&str>) -> Result<String, Diagnostic> {
    if let Some(value) = explicit {
        if valid_rfc3339(value) {
            return Ok(value.to_string());
        }
        return Err(input_error(format!("--pub-date 不是 RFC 3339：{value:?}"))
            .with_hint("形如 2026-10-20T00:00:00Z"));
    }
    if let Ok(value) = std::env::var("SOURCE_DATE_EPOCH") {
        let secs = value.parse::<u64>().map_err(|_| {
            input_error(format!("SOURCE_DATE_EPOCH 不是整数秒：{value:?}"))
                .with_hint("unset 或设为 `date +%s` 的整数输出")
        })?;
        return Ok(rfc3339(secs));
    }
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    Ok(rfc3339(secs))
}

/// 最小的 RFC 3339 形状检查（日期 / 时间分隔符与 Z / 偏移）。
pub(super) fn valid_rfc3339(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && (value.ends_with('Z') || matches!(bytes.get(19), Some(b'+' | b'-')))
}

/// unix 秒 → RFC 3339 UTC（不引日历库；与 release 仓库元数据的日期算法同源）。
pub(super) fn rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    let sod = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        sod / 3600,
        sod % 3600 / 60,
        sod % 60
    )
}

/// 自 1970-01-01 起的天数 → (年, 月, 日)（Howard Hinnant `civil_from_days`）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// 产物选择：显式 `--artifact`，或目录内唯一非签名 / 非清单文件。
pub(super) fn find_artifact(dir: &Path, explicit: Option<&str>) -> Result<PathBuf, Diagnostic> {
    if !dir.is_dir() {
        return Err(Diagnostic::error(
            codes::E_UPDATE_0003,
            format!("产物目录不存在：{}", dir.display()),
        )
        .with_hint("先跑 `polypack build` / 打包，或用 --package-dir 指向发布产物目录"));
    }
    if let Some(name) = explicit {
        if name.is_empty() || name.contains(['/', '\\']) || name == ".." {
            return Err(input_error(format!(
                "--artifact 只接受目录内文件名：{name:?}"
            )));
        }
        let path = dir.join(name);
        if !path.is_file() {
            return Err(Diagnostic::error(
                codes::E_UPDATE_0003,
                format!("产物不存在：{}", path.display()),
            ));
        }
        return Ok(path);
    }
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|err| io_error("读取产物目录", dir, err))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file())
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            !name.starts_with('.') && name != "latest.json" && !name.ends_with(".sig")
        })
        .collect();
    candidates.sort();
    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        0 => Err(Diagnostic::error(
            codes::E_UPDATE_0003,
            format!("产物目录无可用产物：{}", dir.display()),
        )
        .with_hint("目录里放一个发布产物（及同名 `.sig`），或用 --artifact 指定")),
        count => Err(Diagnostic::error(
            codes::E_UPDATE_0003,
            format!("产物目录有 {count} 个候选产物，无法唯一确定"),
        )
        .with_hint("用 --artifact <文件名> 指定本次清单对应的产物")),
    }
}

/// `<path>.sig`（不覆盖原文件名，避免 AppImage 等点号扩展名歧义）。
pub(super) fn sig_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".sig");
    path.with_file_name(name)
}

/// URL 路径段的百分号编码（产物名可能含空格 / 非 ASCII）。
pub(super) fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// 写文件（先建父目录）。
pub(super) fn write(path: &Path, bytes: &[u8]) -> Result<(), Diagnostic> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| io_error("创建目录", parent, err))?;
    }
    std::fs::write(path, bytes).map_err(|err| io_error("写入文件", path, err))
}

/// 凭据引用：`--credential` / `--trust-root` 只接受 env / keyring / vault 引用。
pub(super) fn credential_ref(value: Option<&str>, flag: &str) -> Result<CredentialRef, Diagnostic> {
    let Some(value) = value else {
        return Err(Diagnostic::error(
            codes::E_UPDATE_0002,
            format!("缺少 {flag}（签名 / 验签需要凭据引用）"),
        )
        .with_hint(
            "形如 env:POLYPACK_UPDATE_KEY / keyring:update-sign / vault:update-sign；\
             明文凭据绝不进命令行",
        ));
    };
    let (source, alias) = crate::retry::parse_credential(value).map_err(|err| {
        Diagnostic::error(
            codes::E_UPDATE_0002,
            format!("{flag} 引用非法：{}", err.message),
        )
        .with_hint("可写：env:变量名 / keyring[:条目名] / vault[:条目名]")
    })?;
    Ok(CredentialRef {
        name: alias.unwrap_or_else(|| "update-manifest".to_string()),
        source,
    })
}

/// `polypack-sign` 错误 → 稳定更新错误码（凭据类归 E_UPDATE_0002，其余归验签 / 工具失败）。
pub(super) fn sign_diagnostic(err: SignError) -> Diagnostic {
    let code = match &err {
        SignError::CredentialUnavailable { .. } | SignError::CredentialInvalid { .. } => {
            codes::E_UPDATE_0002
        }
        _ => codes::E_UPDATE_0006,
    };
    let mut diagnostic = Diagnostic::error(code, err.to_string());
    if let Some(hint) = err.hint() {
        diagnostic = diagnostic.with_hint(hint);
    }
    diagnostic
}

pub(super) fn input_error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(codes::E_UPDATE_0001, message)
}

fn schema_error(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(codes::E_UPDATE_0005, message)
        .with_hint("Tauri v2 静态清单必需 version / platforms.<target>.url / signature")
}

pub(super) fn io_error(action: &str, path: &Path, err: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        codes::E_UPDATE_0004,
        format!("{action} 失败（{}）：{err}", path.display()),
    )
    .with_hint("检查路径、权限与磁盘空间")
}
