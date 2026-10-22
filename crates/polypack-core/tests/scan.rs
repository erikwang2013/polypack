//! T-M2-12 验收（core 侧）：规则命中 / 白名单双断言 / 脱敏 / 目录扫描。
//!
//! 构造样本一律**运行时拼出**（`format!` / `repeat`），源码行里不出现完整形态；需要长期留存的
//! 样本走 `tests/fixtures/scan/`（每行带同行白名单标记）。这两条合起来保证「仓库自扫为绿」与
//! 「真实形态照常命中」互不牺牲。全历史面（git 对象库批量扫描）见同目录 `scan_history.rs`。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use polypack_core::scan::{ALLOW_MARKERS, Finding, scan_bytes, scan_dir};

// ---------------------------------------------------------------- 构造样本

/// GitHub 令牌形态：`ghp_` + 36 位字母数字（样本由片段拼出）。
fn github_token() -> String {
    format!("ghp_{}", "aB3dE7".repeat(6))
}

/// AWS 访问密钥形态：`AKIA` + 16 位大写字母数字。
fn aws_key() -> String {
    format!("AKIA{}", "A1B2C3D4E5F6G7H8")
}

/// Google API key 形态：`AIza` + 35 位 `[A-Za-z0-9_-]`。
fn google_key() -> String {
    format!("AIza{}", "SyD9fQ2xL7mV4pR8tW1yZ6bN3cH5jK0qS2u")
}

/// Slack 令牌形态：`xoxb-` + ≥10 位 `[A-Za-z0-9-]`。
fn slack_token() -> String {
    format!("xoxb-{}-{}", "123456789012", "abcdefghijkl")
}

/// 高熵值（34 位、含数字与小写/大写、熵 ≈5.1 bit/字节）。
fn entropy_value() -> String {
    "Aq3bZx9Qw7Er2Tyu4Iop6Df8Gh1Jk5Lm0N".to_string()
}

/// 平台 AppSecret 形态：32 位十六进制（运行时分段拼出，源码行内不出现完整形态）。
fn appsecret_hex32() -> String {
    "0123456789abcdef".repeat(2)
}

/// PEM 私钥头（运行时拼出：源码行内不构成完整头部）。
fn pem_header() -> String {
    format!("-----BEGIN {} PRIVATE KEY-----", "RSA")
}

/// JKS 密钥库魔数（`java.io` 序列化头）。
fn jks_magic() -> Vec<u8> {
    vec![0xFE, 0xED, 0xFE, 0xED]
}

// ---------------------------------------------------------------- 测试支架

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 临时目录；drop 即递归删除。
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-scan-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可创建");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// 夹具目录（构造样本的入库位置）。
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("scan")
}

/// 去掉全部白名单标记（逐行处理：标记与样本同行）。
fn without_markers(bytes: &[u8]) -> Vec<u8> {
    bytes
        .split(|&byte| byte == b'\n')
        .map(|line| {
            let mut line = line.to_vec();
            for marker in ALLOW_MARKERS {
                line = remove_all(&line, marker.as_bytes());
            }
            line
        })
        .collect::<Vec<_>>()
        .join(&b"\n"[..])
}

/// 逐次移除字节子串 `needle`。
fn remove_all(haystack: &[u8], needle: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut rest = haystack;
    while let Some(pos) = rest
        .windows(needle.len())
        .position(|window| window == needle)
    {
        out.extend_from_slice(&rest[..pos]);
        rest = &rest[pos + needle.len()..];
    }
    out.extend_from_slice(rest);
    out
}

// ---------------------------------------------------------------- 规则面

/// **白名单双断言**：每条规则的真实形态必须命中；同一行加上标记后必须不命中。
#[test]
fn every_rule_hits_real_shape_and_respects_marker() {
    let cases: &[(&str, String)] = &[
        (
            "pem-private-key",
            format!("let key = \"{}\";", pem_header()),
        ),
        ("github-token", format!("GITHUB_TOKEN={}", github_token())),
        ("aws-access-key", format!("AWS_ACCESS_KEY_ID={}", aws_key())),
        ("google-api-key", format!("GOOGLE_API_KEY={}", google_key())),
        ("slack-token", format!("SLACK_BOT_TOKEN={}", slack_token())),
        (
            "appsecret-hex32",
            format!("APPSECRET={}", appsecret_hex32()),
        ),
        (
            "high-entropy-secret",
            format!("secret = \"{}\"", entropy_value()),
        ),
    ];
    for (rule, real) in cases {
        let hit = scan_bytes("sample.txt", real.as_bytes());
        assert_eq!(hit.findings.len(), 1, "[{rule}] 真实形态必须命中: {hit:?}");
        assert_eq!(hit.findings[0].rule, *rule);
        assert_eq!(hit.findings[0].line, 1);
        assert!(!hit.findings[0].hint.is_empty(), "命中必须带修复提示");

        let marked = format!("{real}  // POLYPACK_TEST_FIXTURE");
        let clean = scan_bytes("sample.txt", marked.as_bytes());
        assert!(
            clean.findings.is_empty(),
            "[{rule}] 同行标记后不得命中: {clean:?}"
        );
    }
}

/// 二进制规则（JKS 魔数）同样双断言：真实魔数命中，带标记的样本不命中。
#[test]
fn jks_magic_hits_and_marked_binary_sample_is_clean() {
    let mut real = jks_magic();
    real.extend_from_slice(b"\x00\x00\x00\x02keystore-body");
    let hit = scan_bytes("real.jks", &real);
    assert_eq!(hit.findings.len(), 1, "{hit:?}");
    assert_eq!(hit.findings[0].rule, "jks-keystore");
    assert_eq!(hit.findings[0].line, 1);

    let mut marked = jks_magic();
    marked.extend_from_slice(b" POLYPACK_TEST_FIXTURE \x00\x00\x00\x02keystore-body");
    let clean = scan_bytes("marked.jks", &marked);
    assert!(clean.findings.is_empty(), "{clean:?}");
}

/// 入库夹具（构造样本 + 同行标记）扫出来必须是绿的。
#[test]
fn committed_fixtures_are_whitelisted() {
    let report = scan_dir(&fixtures()).expect("夹具目录可扫描");
    assert!(
        report.findings.is_empty(),
        "夹具样本必须带标记（自扫为绿）: {:?}",
        report.findings
    );
    assert_eq!(report.scanned, 2, "夹具 = 文本样本 + 二进制样本");
}

/// 夹具的绿**只因**标记：去掉标记后每条规则都要命中（否则白名单会掩盖规则失效）。
#[test]
fn stripping_markers_from_fixtures_hits_every_rule() {
    let stripped = without_markers(
        &std::fs::read(fixtures().join("whitelisted-samples.txt")).expect("夹具存在"),
    );
    let report = scan_bytes("whitelisted-samples.txt", &stripped);
    let mut rules: Vec<&str> = report.findings.iter().map(|finding| finding.rule).collect();
    rules.sort_unstable();
    assert_eq!(
        rules,
        vec![
            "appsecret-hex32",
            "aws-access-key",
            "github-token",
            "google-api-key",
            "high-entropy-secret",
            "pem-private-key",
            "slack-token",
        ],
        "去标记后 7 条规则逐条命中（且不触发别的规则）: {:?}",
        report.findings
    );

    let stripped =
        without_markers(&std::fs::read(fixtures().join("sample-keystore.jks")).expect("夹具存在"));
    let report = scan_bytes("sample-keystore.jks", &stripped);
    assert_eq!(
        report.findings.len(),
        1,
        "去标记后魔数规则命中: {:?}",
        report.findings
    );
    assert_eq!(report.findings[0].rule, "jks-keystore");
}

/// 脱敏：报告绝不回显命中内容本身，只给上下文与修复提示。
#[test]
fn excerpt_never_echoes_the_secret() {
    let sample = github_token();
    let report = scan_bytes("x.env", format!("TOKEN={sample}").as_bytes());
    let finding = &report.findings[0];
    assert!(
        !finding.excerpt.contains(&sample),
        "摘要不得回显命中内容: {}",
        finding.excerpt
    );
    assert!(
        finding.excerpt.contains("⟨已隐去 40 字节⟩"),
        "应给出隐去长度: {}",
        finding.excerpt
    );
    assert!(
        finding.hint.contains("CredentialRef"),
        "修复提示应指向凭据引用: {}",
        finding.hint
    );
}

/// 含 NUL 的载荷按二进制处理：文本规则不适用（避免二进制误报），魔数规则照常。
#[test]
fn nul_bytes_disable_text_rules_but_not_magic_rules() {
    let mut blob = vec![0u8, 0x01, 0x02];
    blob.extend_from_slice(format!("TOKEN={}\n", github_token()).as_bytes());
    let report = scan_bytes("blob.bin", &blob);
    assert!(
        report.findings.is_empty(),
        "二进制载荷不该报文本规则: {:?}",
        report.findings
    );

    let mut keystore = jks_magic();
    keystore.extend_from_slice(b" keystore-with-nul \x00 body");
    assert_eq!(
        scan_bytes("k.jks", &keystore).findings.len(),
        1,
        "魔数不受 NUL 影响"
    );
}

/// T-M8-03：插件制品（`.wasm`）扫描宿主绝对路径 / 环境变量痕迹 / 调试符号段；
/// 报告只给脱敏描述与偏移（不回显宿主路径），非 `.wasm` 的二进制不施加制品规则。
#[test]
fn wasm_artifact_catches_host_paths_env_traces_and_debug_symbols() {
    let mut wasm = b"\0asm\x01\0\0\0".to_vec();
    wasm.extend_from_slice(b"\x10/home/alice/work/plugin/src/lib.rs");
    wasm.extend_from_slice(b"\x08CARGO_MANIFEST_DIR");
    wasm.extend_from_slice(b"\x0b.debug_info");
    wasm.extend_from_slice(b"\x0d.debug_abbrev");

    let report = scan_bytes("plugin.wasm", &wasm);
    let rules: Vec<&str> = report.findings.iter().map(|finding| finding.rule).collect();
    assert_eq!(
        rules,
        ["wasm-debug-symbols", "wasm-env-trace", "wasm-host-path"],
        "三类制品规则各报一次（同规则去重）: {:?}",
        report.findings
    );
    for finding in &report.findings {
        assert_eq!(finding.line, 1);
        assert!(!finding.hint.is_empty(), "命中必须带修复提示");
        assert!(
            !finding.excerpt.contains("/home/") && !finding.excerpt.contains("alice"),
            "摘要不得回显宿主路径: {}",
            finding.excerpt
        );
    }

    // 同一字节流放在非 `.wasm` 路径上：制品规则不适用（既有二进制语义不变）。
    let report = scan_bytes("plugin.bin", &wasm);
    assert!(report.findings.is_empty(), "{:?}", report.findings);
}

/// 熵启发式的负例：校验和形态（纯小写十六进制）、短值、URL 都不报；对照正例命中。
#[test]
fn entropy_heuristic_ignores_checksums_short_values_and_urls() {
    let hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let report = scan_bytes("x.env", format!("secret = \"{hex}\"").as_bytes());
    assert!(
        report.findings.is_empty(),
        "纯小写十六进制（sha256 形态）不报: {:?}",
        report.findings
    );

    let value = entropy_value();
    let short = &value[..31];
    let report = scan_bytes("x.env", format!("secret = \"{short}\"").as_bytes());
    assert!(
        report.findings.is_empty(),
        "31 位短值不报: {:?}",
        report.findings
    );

    let url = format!("secret = \"https://example.invalid/{}\"", entropy_value());
    let report = scan_bytes("x.env", url.as_bytes());
    assert!(
        report.findings.is_empty(),
        "URL 形态不报: {:?}",
        report.findings
    );

    let report = scan_bytes(
        "x.env",
        format!("secret = \"{}\"", entropy_value()).as_bytes(),
    );
    assert_eq!(report.findings.len(), 1, "对照正例必须命中: {report:?}");
}

/// AppSecret 规则（32 位十六进制）：关键字门控命中（含 snake_case / 环境变量前缀与 JSON 形态），
/// 裸校验和、位数不对、词内出现都不报。
#[test]
fn appsecret_hex32_is_keyword_gated_and_exact_length() {
    let value = appsecret_hex32();
    for sample in [
        format!("APPSECRET={value}"),
        format!("WECHAT_APPSECRET={value}"),
        format!("TARO_APP_SECRET=\"{value}\""),
        format!("\"appsecret\": \"{value}\""),
    ] {
        let report = scan_bytes("x.env", sample.as_bytes());
        assert_eq!(report.findings.len(), 1, "应命中: {sample} → {report:?}");
        assert_eq!(report.findings[0].rule, "appsecret-hex32");
    }

    // 裸校验和（无 secret 关键字上下文）不报；位数不对（31 / 33）不报。
    let short = &value[..31];
    let long = format!("{value}0");
    for sample in [
        format!("md5={value}"),
        format!("secret={short}"),
        format!("appsecret={long}"),
    ] {
        let report = scan_bytes("x.env", sample.as_bytes());
        assert!(
            report.findings.is_empty(),
            "不该命中: {sample} → {report:?}"
        );
    }

    // 词内出现：`secrets` / `mysecretstore` 形态不报（左右词边界）。
    let report = scan_bytes("x.env", format!("secrets={value}").as_bytes());
    assert!(report.findings.is_empty(), "secrets 复数不报: {report:?}");
    let report = scan_bytes("x.env", format!("let mysecretstore = {value}").as_bytes());
    assert!(report.findings.is_empty(), "词内 secret 不报: {report:?}");
}

// ---------------------------------------------------------------- 目录扫描

/// 写一个命中文件：`line` 行为样本（1 起），其余行无关内容。
fn write_hit(root: &Path, rel: &str, line: usize) {
    let mut text = String::new();
    for index in 1..line {
        text.push_str(&format!("BENIGN{index}=harmless\n"));
    }
    text.push_str(&format!("GITHUB_TOKEN={}\n", github_token()));
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("有父目录")).expect("建目录");
    std::fs::write(path, text).expect("写样本");
}

/// 目录扫描：相对路径 / 行号 / 排序确定；`SKIP_DIRS` 里的命中不扫也不计数。
#[test]
fn dir_scan_reports_relative_paths_skips_build_dirs_and_sorts() {
    let temp = TempDir::new("dir");
    let root = temp.path();
    write_hit(root, "z/second.txt", 2);
    write_hit(root, "a/first.txt", 1);
    write_hit(root, "node_modules/pkg/ignored.txt", 1);
    write_hit(root, "target/ignored.txt", 1);
    write_hit(root, "dist/ignored.txt", 1);
    write_hit(root, ".git/ignored.txt", 1);
    std::fs::write(
        root.join("b-marked.txt"),
        format!("GITHUB_TOKEN={}  # POLYPACK_TEST_FIXTURE\n", github_token()),
    )
    .expect("写带标记样本");

    let report = scan_dir(root).expect("目录可扫描");
    let keys: Vec<(&str, usize)> = report
        .findings
        .iter()
        .map(|finding| (finding.path.as_str(), finding.line))
        .collect();
    assert_eq!(
        keys,
        vec![("a/first.txt", 1), ("z/second.txt", 2)],
        "路径 → 行号 升序且只扫非跳过目录"
    );
    assert_eq!(
        report.scanned, 3,
        "跳过目录里的文件不计数（a / z / 标记样本）"
    );
}

/// 报告排序键在跨文件时也确定（含历史合并的 `merge` 语义由 CLI 侧复用同一 [`Finding`] 排序）。
#[test]
fn findings_sort_is_deterministic() {
    let temp = TempDir::new("sort");
    let root = temp.path();
    write_hit(root, "b.txt", 1);
    write_hit(root, "a.txt", 1);
    let first = scan_dir(root).expect("可扫描");
    let second = scan_dir(root).expect("可扫描");
    assert_eq!(first.findings, second.findings, "同一目录两次扫描结果一致");
    let paths: Vec<&str> = first
        .findings
        .iter()
        .map(|finding: &Finding| finding.path.as_str())
        .collect();
    assert_eq!(paths, vec!["a.txt", "b.txt"], "跨文件按路径升序");
}
