//! T-M8-05：内建目标经统一 provider 注册表与直接打包器矩阵同源同字节。
//!
//! 这里锁住两件事：
//! 1. 官方 provider 的 `describe` 面与 `polypack_targets::builtin()` **逐字段一致**（目标表零漂移）；
//! 2. 统一入口（`PackageInput` 通用字节输入 → 物化）与直接 `Packager::package` 的 `.wgt`
//!    产物字节一致，且等于基线 SHA-256（确定性 zip：固定时间戳、排序条目）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use polypack_bundle::packagers::tizen::TizenWgt;
use polypack_bundle::provider::{official_providers, official_registry};
use polypack_bundle::{BundleInputs, Packager, ProductInfo};
use polypack_targets::provider::{PackageInput, ProviderError, TargetProvider};
use polypack_targets::{Bridge, Host, PackageSpec, Runtime, TargetSpec};
use sha2::{Digest, Sha256};

/// Tizen WGT 的确定性字节基线（T-M8-05 改造前打包器矩阵的产物摘要）。
///
/// zip 中央目录会记录条目属性（外部属性 / 平台），因此摘要按宿主平台分基线；
/// 「统一入口 == 直接打包器」的字节一致断言在全部平台恒成立（见下方测试）。
#[cfg(windows)]
const WGT_BASELINE: &str = "c4a0e4338e0691fba0cc0cf0d7ec6b98f843862d7c96f3d067c88a564809e45d";
#[cfg(not(windows))]
const WGT_BASELINE: &str = "626f4a5988183c434aeeb32b1e28cbb9a300d5fe49eccf0b4bae07c1a14f1532";

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "polypack-provider-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("临时目录可建");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tizen_spec() -> TargetSpec {
    let mut spec = TargetSpec::new_stub("tizen-tv", 4, &[Host::Linux, Host::Win, Host::Macos]);
    spec.runtime = Runtime::Wasm;
    spec.bridge = Bridge::WasmBindgen;
    spec.package = PackageSpec {
        format: "wgt".to_string(),
        builder: "tizen-wgt".to_string(),
    };
    spec
}

fn files() -> Vec<(String, Vec<u8>)> {
    vec![
        (
            "index.html".to_string(),
            b"<!doctype html><html><body>hi</body></html>".to_vec(),
        ),
        ("app.js".to_string(), b"console.log(1);".to_vec()),
    ]
}

fn direct_wgt(dir: &Path, spec: &TargetSpec) -> (BundleInputs, Vec<u8>) {
    let dist = dir.join("dist");
    std::fs::create_dir_all(&dist).expect("dist 可建");
    for (name, contents) in files() {
        std::fs::write(dist.join(name), contents).expect("UI 文件可写");
    }
    let product = ProductInfo {
        name: "Pax Notes".to_string(),
        version: "1.2.3".to_string(),
        identifier: "com.polypack.pax-notes".to_string(),
        manufacturer: "PolyPack".to_string(),
        description: "Pax Notes".to_string(),
        homepage: None,
    };
    let mut inputs = BundleInputs::new(product, dir.join("out"));
    inputs.ui_dist = Some(dist);
    let artifact = TizenWgt.package(&inputs, spec).expect("直接打包");
    let bytes = std::fs::read(&artifact.path).expect("产物可读");
    (inputs, bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn official_registry_describes_every_builtin_spec_verbatim() {
    let registry = official_registry();
    let builtin = polypack_targets::builtin::builtin();
    assert_eq!(
        registry.specs(),
        builtin,
        "统一注册表的目标声明必须与内建目标逐字段一致（零漂移）"
    );
    for spec in &builtin {
        let provider = registry
            .get(&spec.id, &spec.package.format)
            .unwrap_or_else(|| panic!("目标 {} 的默认格式缺少 provider", spec.id));
        assert_eq!(provider.describe().expect("describe"), *spec);
        assert!(
            polypack_bundle::find(&spec.id, &spec.package.format).is_some(),
            "provider 必须委托既有打包器矩阵（{}，{}）",
            spec.id,
            spec.package.format
        );
    }
}

#[test]
fn every_provider_key_resolves_to_an_existing_packager() {
    for provider in official_providers() {
        let spec = provider.describe().expect("describe");
        let packager = polypack_bundle::find(&spec.id, &spec.package.format)
            .unwrap_or_else(|| panic!("{} 的 {} 无打包器", spec.id, spec.package.format));
        assert_eq!(provider.packager_id(), packager.id());
    }
}

#[test]
fn reused_targets_route_to_their_shared_packagers() {
    // HarmonyOS TV 复用 hap 链，但打包器 id 仍是 harmonyos（产物 target 由 spec.id 给出）。
    let tv = polypack_bundle::find("harmonyos-tv", "hap").expect("harmonyos-tv/hap");
    assert_eq!(tv.id(), "harmonyos");
    assert_eq!(tv.format(), "hap");
    // GrapheneOS 走普通 APK 口径，产物 target 必须是 grapheneos 本身。
    let graphene = polypack_bundle::find("grapheneos", "apk").expect("grapheneos/apk");
    assert_eq!(graphene.id(), "grapheneos");
    assert_eq!(graphene.format(), "apk");
    assert!(
        polypack_bundle::find("grapheneos", "aab").is_none(),
        "GrapheneOS 只声明 apk，不接 AAB"
    );
}

/// 通用字节输入没有工程根：打包器如实报 `MissingInput`，provider 统一映射为可跳过
/// `Unavailable`（调用侧按环境性缺失 SKIP-with-print），不硬失败。
#[test]
fn materialized_bundle_inputs_without_project_skip_as_unavailable() {
    let registry = official_registry();
    let garmin = polypack_targets::builtin::builtin()
        .into_iter()
        .find(|spec| spec.id == "garmin")
        .expect("内建 garmin 声明");
    let provider = registry
        .get(&garmin.id, &garmin.package.format)
        .expect("garmin 仍在统一注册表");
    assert_eq!(provider.describe().expect("describe"), garmin);

    let empty: Vec<(String, Vec<u8>)> = Vec::new();
    let input = PackageInput {
        name: "Pax Notes",
        version: "1.2.3",
        target: "garmin",
        entry: "",
        files: &empty,
        triple: "native",
        lib: &[],
        host: None,
    };
    match provider.package(&input) {
        Err(ProviderError::Unavailable(reason)) => {
            assert!(
                reason.contains("garmin") && reason.contains("miniapp_host"),
                "跳过原因需如实点名目标与缺失输入：{reason}"
            );
        }
        other => panic!("缺工程根应返回 Unavailable，实际 {other:?}"),
    }
}

#[test]
fn unified_tizen_entry_matches_direct_packager_baseline_bytes() {
    let spec = tizen_spec();
    let direct_dir = Scratch::new("direct");
    let (direct_inputs, direct) = direct_wgt(&direct_dir.0, &spec);

    let provider = official_registry();
    let provider = provider
        .get("tizen-tv", "wgt")
        .expect("tizen-tv 的 wgt provider 已注册");
    let files = files();
    let input = PackageInput {
        name: "Pax Notes",
        version: "1.2.3",
        target: "tizen-tv",
        entry: "index.html",
        files: &files,
        triple: "wasm32-unknown-unknown",
        lib: b"CORE",
        host: None,
    };
    let artifact = provider.package(&input).expect("统一入口打包");
    assert_eq!(artifact.format, "wgt");
    assert_eq!(artifact.target, "tizen-tv");
    assert!(artifact.path.is_none(), "物化模式返回内联字节");
    assert_eq!(
        artifact.content, direct,
        "统一入口与直接打包器必须字节一致（除签名 / 时间戳外无差异）"
    );

    let digest = sha256_hex(&artifact.content);
    println!("WGT_BASELINE={digest}");
    assert_eq!(digest, WGT_BASELINE, "统一入口产物偏离基线哈希");

    // 生产路径（宿主已组装 BundleInputs）：同一打包器、同一输入 → 同一字节。
    let hosted = provider
        .package(&PackageInput {
            host: Some(&direct_inputs as &dyn std::any::Any),
            ..input
        })
        .expect("宿主上下文打包");
    assert!(
        hosted.content.is_empty(),
        "宿主路径产物落盘，不回传内联字节"
    );
    let hosted_path = hosted.path.expect("宿主路径回填产物路径");
    assert_eq!(
        std::fs::read(hosted_path).expect("宿主路径产物可读"),
        direct
    );
}
