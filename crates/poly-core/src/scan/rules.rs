//! secret-scan 规则集：文本命中器（PEM / AWS / GitHub / Google / Slack / 平台 AppSecret
//! 32 位十六进制 / 高熵启发式）与二进制魔数规则；白名单标记与脱敏由引擎（`super`）统一施加。

use super::ALLOW_MARKERS;

/// 高熵启发式的最短值长。
const MIN_SECRET_LEN: usize = 32;

/// 修复提示：撤换密钥 + 改走凭据引用（只引用不内联，见 `poly_core::job::CredentialRef`）。
const HINT_KEY: &str =
    "撤销并轮换该密钥，改用 CredentialRef 引用（keyring / Vault / CI 注入），不要内联明文";
/// 修复提示：密钥材料文件（私钥 / 密钥库）不入库。
const HINT_STORE: &str =
    "密钥材料不入库：把文件加入 .gitignore，凭据只经 CredentialRef 引用（keyring / Vault）";

/// 文本规则：`id` / 命中器（返回行内 `(起点, 长度)`）/ 修复提示。
pub(super) struct TextRule {
    pub(super) id: &'static str,
    pub(super) detect: fn(&[u8]) -> Option<(usize, usize)>,
    pub(super) hint: &'static str,
}

/// 二进制规则：文件头魔数。
pub(super) struct BinaryRule {
    pub(super) id: &'static str,
    pub(super) magic: &'static [u8],
    pub(super) hint: &'static str,
}

/// 插件制品（`.wasm`）规则：按字节搜索标记，命中报告只给脱敏描述与偏移（T-M8-03）。
pub(super) struct ArtifactRule {
    pub(super) id: &'static str,
    pub(super) needle: &'static [u8],
    /// 可读命中描述（不含命中路径本身，§7 脱敏）。
    pub(super) label: &'static str,
    pub(super) hint: &'static str,
}

/// 文本规则表（顺序即优先级：同一行只报首个命中的规则，避免同一位点重复刷屏）。
pub(super) const TEXT_RULES: &[TextRule] = &[
    TextRule {
        id: "pem-private-key",
        detect: detect_pem_private_key,
        hint: HINT_STORE,
    },
    TextRule {
        id: "github-token",
        detect: detect_github_token,
        hint: HINT_KEY,
    },
    TextRule {
        id: "aws-access-key",
        detect: detect_aws_access_key,
        hint: HINT_KEY,
    },
    TextRule {
        id: "google-api-key",
        detect: detect_google_api_key,
        hint: HINT_KEY,
    },
    TextRule {
        id: "slack-token",
        detect: detect_slack_token,
        hint: HINT_KEY,
    },
    TextRule {
        id: "appsecret-hex32",
        detect: detect_appsecret_hex32,
        hint: HINT_KEY,
    },
    TextRule {
        id: "high-entropy-secret",
        detect: detect_high_entropy_secret,
        hint: HINT_KEY,
    },
];

/// 二进制规则表（JKS 密钥库魔数：`java.io` 序列化头 `FE ED FE ED`）。
pub(super) const BINARY_RULES: &[BinaryRule] = &[BinaryRule {
    id: "jks-keystore",
    magic: b"\xFE\xED\xFE\xED",
    hint: HINT_STORE,
}];

/// 修复提示：插件制品不得携带构建机信息。
const HINT_ARTIFACT: &str = "重新构建插件并剥离宿主路径 / 环境变量痕迹（release 构建、strip 掉 name / debug 段）；\
     制品不得携带构建机信息";
/// 修复提示：调试符号段。
const HINT_DEBUG: &str =
    "发布前去掉调试段：wasm 用 `strip` / `wasm-opt -g`，或在 release profile 关 debug";

/// 插件制品规则表（只在 `.wasm` 上施加）：宿主绝对路径 / 环境变量痕迹 / 调试符号段。
pub(super) const ARTIFACT_RULES: &[ArtifactRule] = &[
    ArtifactRule {
        id: "wasm-host-path",
        needle: b"/home/",
        label: "宿主绝对路径（Linux 用户主目录）",
        hint: HINT_ARTIFACT,
    },
    ArtifactRule {
        id: "wasm-host-path",
        needle: b"C:\\Users\\",
        label: "宿主绝对路径（Windows 用户目录）",
        hint: HINT_ARTIFACT,
    },
    ArtifactRule {
        id: "wasm-host-path",
        needle: b"C:/Users/",
        label: "宿主绝对路径（Windows 用户目录）",
        hint: HINT_ARTIFACT,
    },
    ArtifactRule {
        id: "wasm-host-path",
        needle: b"/var/folders/",
        label: "宿主临时目录（macOS）",
        hint: HINT_ARTIFACT,
    },
    ArtifactRule {
        id: "wasm-host-path",
        needle: b"/tmp/",
        label: "宿主临时目录（Unix）",
        hint: HINT_ARTIFACT,
    },
    ArtifactRule {
        id: "wasm-env-trace",
        needle: b"CARGO_MANIFEST_DIR",
        label: "环境变量痕迹 CARGO_MANIFEST_DIR",
        hint: HINT_ARTIFACT,
    },
    ArtifactRule {
        id: "wasm-env-trace",
        needle: b"USERPROFILE=",
        label: "环境变量痕迹 USERPROFILE",
        hint: HINT_ARTIFACT,
    },
    ArtifactRule {
        id: "wasm-env-trace",
        needle: b"TMPDIR=",
        label: "环境变量痕迹 TMPDIR",
        hint: HINT_ARTIFACT,
    },
    ArtifactRule {
        id: "wasm-env-trace",
        needle: b"$HOME",
        label: "环境变量痕迹 $HOME",
        hint: HINT_ARTIFACT,
    },
    ArtifactRule {
        id: "wasm-debug-symbols",
        needle: b".debug_info",
        label: "调试符号段 .debug_info",
        hint: HINT_DEBUG,
    },
    ArtifactRule {
        id: "wasm-debug-symbols",
        needle: b".debug_abbrev",
        label: "调试符号段 .debug_abbrev",
        hint: HINT_DEBUG,
    },
];

/// PEM 私钥头：`-----BEGIN <标签>` 且标签以 `PRIVATE KEY` 收尾（RSA / EC / DSA / OPENSSH /
/// ENCRYPTED / PGP 变体全覆盖；CERTIFICATE 不是私钥，不报）。
fn detect_pem_private_key(line: &[u8]) -> Option<(usize, usize)> {
    const BEGIN: &[u8] = b"-----BEGIN ";
    const LABELS: &[&str] = &[
        "PRIVATE KEY-----",
        "RSA PRIVATE KEY-----",
        "EC PRIVATE KEY-----",
        "DSA PRIVATE KEY-----",
        "OPENSSH PRIVATE KEY-----",
        "ENCRYPTED PRIVATE KEY-----",
        "PGP PRIVATE KEY BLOCK-----",
    ];
    let start = find_sub(line, BEGIN)?;
    let rest = &line[start + BEGIN.len()..];
    let label = LABELS
        .iter()
        .find(|label| rest.starts_with(label.as_bytes()))?;
    Some((start, BEGIN.len() + label.len()))
}

/// GitHub 令牌：`ghp_` / `gho_` / `ghu_` / `ghs_` / `ghr_` + 36 位字母数字，
/// 或细粒度令牌 `github_pat_` + ≥22 位 `[A-Za-z0-9_]`。
fn detect_github_token(line: &[u8]) -> Option<(usize, usize)> {
    let classic = detect_run(
        line,
        &["ghp_", "gho_", "ghu_", "ghs_", "ghr_"],
        is_alnum,
        36,
        36,
    );
    let pat = detect_run(line, &["github_pat_"], is_word_byte, 22, 255);
    match (classic, pat) {
        (Some(a), Some(b)) if b.0 < a.0 => Some(b),
        (classic, pat) => classic.or(pat),
    }
}

/// AWS 访问密钥：`AKIA` / `ASIA` + 16 位大写字母数字（恰好 20 字节的完整词）。
fn detect_aws_access_key(line: &[u8]) -> Option<(usize, usize)> {
    detect_run(line, &["AKIA", "ASIA"], is_upper_alnum, 16, 16)
}

/// Google API key：`AIza` + 35 位 `[A-Za-z0-9_\-]`。
fn detect_google_api_key(line: &[u8]) -> Option<(usize, usize)> {
    detect_run(line, &["AIza"], is_b64_token, 35, 35)
}

/// Slack 令牌：`xoxb-` / `xoxa-` / `xoxp-` / `xoxr-` / `xoxs-` + ≥10 位 `[A-Za-z0-9-]`。
fn detect_slack_token(line: &[u8]) -> Option<(usize, usize)> {
    detect_run(
        line,
        &["xoxb-", "xoxa-", "xoxp-", "xoxr-", "xoxs-"],
        is_slack_token,
        10,
        255,
    )
}

/// 高熵启发式：关键字后跟（可带空白 / 引号的）`=` 或 `:` 分隔符，再接 ≥32 字节高熵串
/// （判定见 [`is_high_entropy`]）。
fn detect_high_entropy_secret(line: &[u8]) -> Option<(usize, usize)> {
    const KEYWORDS: &[&str] = &[
        "secret",
        "token",
        "password",
        "passwd",
        "passphrase",
        "api_key",
        "apikey",
        "access_key",
        "private_key",
        "credential",
        // 平台 AppSecret 也收（非 32 位十六进制的形态由本条启发式兜底，如混写值）。
        "appsecret",
        "app_secret",
    ];
    detect_keyword_value(line, KEYWORDS, |value| {
        let run = value.iter().take_while(|&&b| is_b64_token(b)).count();
        (run >= MIN_SECRET_LEN && is_high_entropy(&value[..run])).then_some(run)
    })
}

/// 平台 AppSecret 形态（微信 / 抖音口径）：关键字门控 + 恰好 32 位十六进制。
///
/// 高熵启发式**排除**纯小写十六进制（md5 / 校验和形态），32 位 AppSecret 在它那里不会
/// 命中——故单列此规则；关键字门控避免把裸校验和（无 `secret=` 上下文的 md5 / blake3 前缀）
/// 误报，多一位少一位都不报。
fn detect_appsecret_hex32(line: &[u8]) -> Option<(usize, usize)> {
    const KEYWORDS: &[&str] = &[
        "appsecret",
        "app_secret",
        "appkey",
        "app_key",
        "secret",
        "secret_key",
    ];
    detect_keyword_value(line, KEYWORDS, |value| {
        let run = value.iter().take_while(|&&b| b.is_ascii_hexdigit()).count();
        (run == 32).then_some(run)
    })
}

/// 关键字门控的值命中器：找「关键字（词边界，大小写不敏感）+（空白 / 引号后的）`=` 或 `:`
/// （含 `=>` / `:=`）+ 空白 / 引号」之后的值起点，交 `check` 判定长度；返回首个命中。
fn detect_keyword_value(
    line: &[u8],
    keywords: &[&str],
    check: fn(&[u8]) -> Option<usize>,
) -> Option<(usize, usize)> {
    let lowered = line.to_ascii_lowercase();
    for keyword in keywords {
        let mut from = 0;
        while let Some(rel) = find_sub(&lowered[from..], keyword.as_bytes()) {
            let pos = from + rel;
            from = pos + 1;
            // 关键字词边界：左不得接字母数字（避免 matching / secrets 之误），但可接 `_`——
            // `WECHAT_APPSECRET=` / `MY_SECRET=` 这种 snake_case / 环境变量前缀是最常见的
            // 泄漏形态，右须非词内字节。
            let left_word = pos > 0 && is_word_byte(lowered[pos - 1]) && lowered[pos - 1] != b'_';
            let right_word = lowered
                .get(pos + keyword.len())
                .is_some_and(|&b| is_word_byte(b));
            if left_word || right_word {
                continue;
            }
            // 分隔符：跳过空白与引号后必须是 `=` 或 `:`（含 `=>` / `:=` 形态）
            let after = &line[pos + keyword.len()..];
            let mut index = 0;
            let skippable = |b: u8| matches!(b, b' ' | b'\t' | b'"' | b'\'');
            while after.get(index).is_some_and(|&b| skippable(b)) {
                index += 1;
            }
            if !matches!(after.get(index), Some(b'=') | Some(b':')) {
                continue;
            }
            index += 1;
            while after
                .get(index)
                .is_some_and(|&b| skippable(b) || matches!(b, b'=' | b'>'))
            {
                index += 1;
            }
            let value_start = pos + keyword.len() + index;
            if let Some(len) = check(&line[value_start..]) {
                return Some((value_start, len));
            }
        }
    }
    None
}

/// 高熵判定：已含数字，且「小写 / 大写 / 符号」至少两类（排除纯小写十六进制——sha256
/// 校验和形态），Shannon 熵 ≥ 3.2 bit/字节，且不含 `://`（URL 不报）。
fn is_high_entropy(value: &[u8]) -> bool {
    let (mut lower, mut upper, mut digit, mut symbol) = (false, false, false, false);
    for &byte in value {
        match byte {
            b'a'..=b'z' => lower = true,
            b'A'..=b'Z' => upper = true,
            b'0'..=b'9' => digit = true,
            _ => symbol = true,
        }
    }
    if !digit || [lower, upper, symbol].iter().filter(|flag| **flag).count() < 2 {
        return false;
    }
    find_sub(value, b"://").is_none() && entropy(value) >= 3.2
}

/// 逐字节 Shannon 熵（bit/字节）。
fn entropy(bytes: &[u8]) -> f64 {
    let mut counts = [0u32; 256];
    for &byte in bytes {
        counts[byte as usize] += 1;
    }
    let len = bytes.len() as f64;
    counts
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let p = count as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// 「前缀 + 字符类串」命中器：前缀须在词边界起，串长落在 `min_len..=max_len`，
/// 且串后不得紧跟词内字节。
fn detect_run(
    line: &[u8],
    prefixes: &[&str],
    class: fn(u8) -> bool,
    min_len: usize,
    max_len: usize,
) -> Option<(usize, usize)> {
    for start in 0..line.len() {
        if start > 0 && is_word_byte(line[start - 1]) {
            continue;
        }
        for prefix in prefixes {
            let head = prefix.as_bytes();
            if !line[start..].starts_with(head) {
                continue;
            }
            let run = line[start + head.len()..]
                .iter()
                .take_while(|&&b| class(b))
                .count();
            let tail_ok = line
                .get(start + head.len() + run)
                .is_none_or(|&b| !is_word_byte(b));
            if run >= min_len && run <= max_len && tail_ok {
                return Some((start, head.len() + run));
            }
        }
    }
    None
}

/// 词内字节（字母 / 数字 / 下划线）：词边界与 `[A-Za-z0-9_]` 字符类共用。
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// `[A-Z0-9]`（AWS 密钥字符类）。
fn is_upper_alnum(byte: u8) -> bool {
    byte.is_ascii_uppercase() || byte.is_ascii_digit()
}

/// `[A-Za-z0-9]`。
fn is_alnum(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}

/// `[A-Za-z0-9_+=\-]`（base64 / URL-safe base64 的无 `/` 子集）。
fn is_b64_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'=' | b'-')
}

/// `[A-Za-z0-9-]`（Slack 令牌字符类）。
fn is_slack_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}

/// 行内是否含白名单标记（字节级搜索，兼容非 UTF-8 行）。
pub(super) fn line_has_allow_marker(line: &[u8]) -> bool {
    ALLOW_MARKERS
        .iter()
        .any(|marker| find_sub(line, marker.as_bytes()).is_some())
}

/// 子串首次出现位置。
pub(super) fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
