//! M9-E5 / T-M9-13 验收：商店 metadata schema 校验与确定性材料。
//!
//! 目录口径：`<tmp>/store/metadata/<platform>/<locale>.toml`，截图路径相对 platform 目录。
//! 单元测试用空文件充当截图（不要求真实图片二进制）；校验只读路径与扩展名。

use std::path::{Path, PathBuf};

use polypack_publish::metadata::{self, Platform, codes, materials_digest};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "polypack-metadata-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("时钟")
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可建");
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

const WECHAT_DEFAULT: &str = r#"schema = 1
title = "PolyPack Tool"
short_description = "Build and ship mini apps with one command."
full_description = "PolyPack builds, packages, and submits mini programs from one configuration file."
keywords = ["build", "publish"]
screenshots = ["screenshots/home.png"]
privacy_url = "https://example.com/privacy"
support_url = "https://example.com/support"
"#;

const ALIPAY_DEFAULT: &str = r#"schema = 1
title = "PolyPack"
short_description = "Ship mini apps in one command."
full_description = "PolyPack builds and submits mini programs from one configuration file."
keywords = ["build", "publish"]
screenshots = ["screenshots/home.png", "screenshots/detail.png"]
privacy_url = "https://example.com/privacy"
support_url = "https://example.com/support"
"#;

/// 写入 platform 目录、locale 文件与（空）截图占位。
fn fixture(tag: &str, platform: Platform, toml: &str, screenshots: &[&str]) -> TempDir {
    let temp = TempDir::new(tag);
    let dir = temp
        .path()
        .join("store")
        .join("metadata")
        .join(platform.id());
    std::fs::create_dir_all(&dir).expect("platform 目录可建");
    std::fs::write(dir.join("default.toml"), toml).expect("写 metadata");
    for screenshot in screenshots {
        let path = dir.join(screenshot);
        std::fs::create_dir_all(path.parent().expect("截图有父目录")).expect("截图目录可建");
        std::fs::write(path, b"").expect("写截图占位（空文件即可，不要求真实图片）");
    }
    temp
}

fn platform_dir(temp: &TempDir, platform: Platform) -> PathBuf {
    temp.path()
        .join("store")
        .join("metadata")
        .join(platform.id())
}

fn codes_of(set: &metadata::MetadataSet) -> Vec<&'static str> {
    set.violations()
        .iter()
        .map(|violation| violation.code)
        .collect()
}

fn find<'a>(set: &'a metadata::MetadataSet, code: &str) -> &'a metadata::Violation {
    set.violations()
        .iter()
        .find(|violation| violation.code == code)
        .unwrap_or_else(|| {
            panic!(
                "缺少违规 {code}；实际：{:?}",
                set.violations()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            )
        })
}

// ---------------------------------------------------------------- 正向

#[test]
fn valid_metadata_loads_for_both_platforms() {
    let wechat = fixture(
        "valid-wechat",
        Platform::Wechat,
        WECHAT_DEFAULT,
        &["screenshots/home.png"],
    );
    let set = metadata::load(Platform::Wechat, platform_dir(&wechat, Platform::Wechat))
        .expect("目录可读");
    assert!(set.is_ok(), "微信 metadata 应通过：{:?}", set.violations());
    assert_eq!(set.locale_names(), &["default".to_string()]);
    let built = set.materials("default").expect("材料可构建");
    assert_eq!(built.value["schema"], 1);
    assert_eq!(built.value["platform"], "wechat");
    assert_eq!(built.value["locale"], "default");
    assert_eq!(built.value["title"], "PolyPack Tool");
    assert_eq!(built.value["screenshots"][0], "screenshots/home.png");
    assert_eq!(built.digest.len(), 64, "blake3 hex");

    let alipay = fixture(
        "valid-alipay",
        Platform::Alipay,
        ALIPAY_DEFAULT,
        &["screenshots/home.png", "screenshots/detail.png"],
    );
    let set = metadata::load(Platform::Alipay, platform_dir(&alipay, Platform::Alipay))
        .expect("目录可读");
    assert!(
        set.is_ok(),
        "支付宝 metadata 应通过：{:?}",
        set.violations()
    );
    let built = set.materials("default").expect("材料可构建");
    assert_eq!(built.value["platform"], "alipay");
    assert_eq!(built.value["keywords"][1], "publish");

    // 请求 locale 缺失 → 回退 default 并在材料里如实回报实际 locale。
    let fallback = set.materials("zh-CN").expect("回退 default");
    assert_eq!(fallback.locale, "default");
    assert!(set.falls_back("zh-CN"));
}

// ---------------------------------------------------------------- 反向（稳定码）

#[test]
fn missing_default_locale_is_reported() {
    let temp = TempDir::new("no-default");
    let dir = temp.path().join("store/metadata/wechat");
    std::fs::create_dir_all(&dir).expect("目录可建");
    std::fs::write(dir.join("en-US.toml"), WECHAT_DEFAULT).expect("写 en-US");
    std::fs::create_dir_all(dir.join("screenshots")).expect("截图目录");
    std::fs::write(dir.join("screenshots/home.png"), b"").expect("截图占位");

    let set = metadata::load(Platform::Wechat, &dir).expect("目录可读");
    assert!(!set.is_ok());
    let violation = find(&set, codes::DEFAULT_MISSING);
    assert!(
        violation.message.contains("default.toml"),
        "{}",
        violation.message
    );
    assert!(
        set.materials("default").is_none(),
        "无 default 不得构建材料"
    );
}

#[test]
fn overlong_and_missing_required_fields_report_all_violations() {
    let long_title = "a".repeat(31);
    let toml = format!(
        r#"schema = 1
title = "{long_title}"
short_description = "Build and ship mini apps with one command."
full_description = "PolyPack builds, packages, and submits mini programs from one configuration file."
keywords = ["build"]
screenshots = []
support_url = "https://example.com/support"
"#
    );
    let temp = fixture("fields", Platform::Wechat, &toml, &[]);
    let set =
        metadata::load(Platform::Wechat, platform_dir(&temp, Platform::Wechat)).expect("目录可读");

    let overlong = find(&set, codes::FIELD_TOO_LONG);
    assert_eq!(overlong.field, "title");
    assert!(overlong.message.contains("31 > 30"), "{}", overlong.message);
    let missing = find(&set, codes::FIELD_MISSING);
    assert_eq!(missing.field, "privacy_url");
    assert!(
        missing.message.contains("缺少必填字段 privacy_url"),
        "{}",
        missing.message
    );
    // 一次报告全部：不是只报第一个。
    assert!(codes_of(&set).len() >= 2, "{:?}", codes_of(&set));
}

#[test]
fn duplicate_locales_fail_stably() {
    let temp = fixture(
        "duplicate",
        Platform::Wechat,
        WECHAT_DEFAULT,
        &["screenshots/home.png"],
    );
    let dir = platform_dir(&temp, Platform::Wechat);
    std::fs::write(dir.join("en-US.toml"), WECHAT_DEFAULT).expect("写 en-US");
    std::fs::write(dir.join("en_us.toml"), WECHAT_DEFAULT).expect("写 en_us");

    let set = metadata::load(Platform::Wechat, &dir).expect("目录可读");
    let violation = find(&set, codes::LOCALE_DUPLICATE);
    assert!(
        violation.message.contains("en-US.toml") && violation.message.contains("en_us.toml"),
        "重复须点名两个文件：{}",
        violation.message
    );
    assert_eq!(violation.locale, "en-us");
}

#[test]
fn invalid_urls_fail_stably() {
    let toml = WECHAT_DEFAULT.replace("https://example.com/privacy", "example.com/privacy");
    let temp = fixture("url", Platform::Wechat, &toml, &["screenshots/home.png"]);
    let set =
        metadata::load(Platform::Wechat, platform_dir(&temp, Platform::Wechat)).expect("目录可读");
    let violation = find(&set, codes::URL_INVALID);
    assert_eq!(violation.field, "privacy_url");
    assert!(
        violation.message.contains("example.com/privacy"),
        "{}",
        violation.message
    );
}

#[test]
fn missing_screenshot_path_and_count_are_reported() {
    let toml = ALIPAY_DEFAULT.replace(
        "screenshots = [\"screenshots/home.png\", \"screenshots/detail.png\"]",
        "screenshots = [\"screenshots/home.png\", \"screenshots/missing.png\"]",
    );
    let temp = fixture(
        "screenshot-path",
        Platform::Alipay,
        &toml,
        &["screenshots/home.png"],
    );
    let set =
        metadata::load(Platform::Alipay, platform_dir(&temp, Platform::Alipay)).expect("目录可读");
    let violation = find(&set, codes::SCREENSHOT_PATH_MISSING);
    assert_eq!(violation.field, "screenshots[1]");
    assert!(
        violation.message.contains("screenshots/missing.png"),
        "{}",
        violation.message
    );

    let empty = ALIPAY_DEFAULT.replace(
        "screenshots = [\"screenshots/home.png\", \"screenshots/detail.png\"]",
        "screenshots = []",
    );
    let temp = fixture("screenshot-count", Platform::Alipay, &empty, &[]);
    let set =
        metadata::load(Platform::Alipay, platform_dir(&temp, Platform::Alipay)).expect("目录可读");
    let violation = find(&set, codes::SCREENSHOTS_REQUIRED);
    assert!(
        violation.message.contains("至少需要 2"),
        "{}",
        violation.message
    );
}

#[test]
fn unsupported_schema_version_is_reported() {
    let toml = WECHAT_DEFAULT.replace("schema = 1", "schema = 2");
    let temp = fixture("schema", Platform::Wechat, &toml, &["screenshots/home.png"]);
    let set =
        metadata::load(Platform::Wechat, platform_dir(&temp, Platform::Wechat)).expect("目录可读");
    let violation = find(&set, codes::SCHEMA_UNSUPPORTED);
    assert!(violation.message.contains("2"), "{}", violation.message);
}

// ---------------------------------------------------------------- 确定性

#[test]
fn materials_are_byte_identical_across_runs() {
    let temp = fixture(
        "deterministic",
        Platform::Alipay,
        ALIPAY_DEFAULT,
        &["screenshots/home.png", "screenshots/detail.png"],
    );
    let dir = platform_dir(&temp, Platform::Alipay);
    let first = metadata::load(Platform::Alipay, &dir)
        .expect("目录可读")
        .materials("default")
        .expect("材料可构建");
    let second = metadata::load(Platform::Alipay, &dir)
        .expect("目录可读")
        .materials("default")
        .expect("材料可构建");

    let first_bytes = serde_json::to_vec(&first.value).expect("序列化");
    let second_bytes = serde_json::to_vec(&second.value).expect("序列化");
    assert_eq!(first_bytes, second_bytes, "两次打包必须字节一致");
    assert_eq!(first.digest, second.digest);
    assert_eq!(first.digest, materials_digest(&first.value));
    // key 排序（default serde_json Map = BTreeMap；canonical 口径同源）。
    let text = String::from_utf8(first_bytes).expect("UTF-8");
    let schema = text.find("\"schema\"").expect("有 schema");
    let title = text.find("\"title\"").expect("有 title");
    assert!(schema < title, "key 须排序：{text}");
}

#[test]
fn legacy_materials_digest_is_order_independent() {
    let left = serde_json::json!({"b": 1, "a": {"d": 2, "c": [3, 1]}});
    let right = serde_json::json!({"a": {"c": [3, 1], "d": 2}, "b": 1});
    assert_eq!(materials_digest(&left), materials_digest(&right));
}
