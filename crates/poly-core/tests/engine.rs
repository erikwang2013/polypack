//! T-M6-01 验收：引擎层在 std 与 no_std 两种构建下行为一致。
//!
//! 同一份测试分别以默认 features 与 `--no-default-features`（本 crate 走 `no_std + alloc`）
//! 运行，两边都必须给出同一个 golden 缓存键、同一份解析/校验结果：
//!
//! ```text
//! cargo test -p poly-core --test engine
//! cargo test -p poly-core --no-default-features --test engine
//! ```

use poly_core::engine::{CacheInputs, Digest, cache_key, parse_job_spec, parse_target_spec};

/// 固定输入的缓存键：任何一侧（std / no_std）漂移即红。
const GOLDEN_KEY: &str = "263bcee0a06fbd80a257e7194b63af7ee5f9e7265f00ceb20a72d66c721754fb";

const TARGET_JSON: &str = r#"{
  "id": "linux",
  "tier": 1,
  "hosts": ["linux"],
  "runtime": "native",
  "bridge": "uniffi",
  "ui": [],
  "package": { "format": "deb", "builder": "linux-deb" }
}"#;

const JOB_JSON: &str = r#"{
  "id": "engine-layer",
  "profile": "ci",
  "targets": [{
    "id": "linux",
    "tier": 1,
    "hosts": ["linux"],
    "runtime": "native",
    "bridge": "uniffi",
    "ui": [],
    "package": { "format": "deb", "builder": "linux-deb" }
  }]
}"#;

#[test]
fn cache_key_is_stable_across_build_modes() {
    let inputs = CacheInputs {
        sources: Digest::parse(&"aa".repeat(32)).expect("64 位十六进制"),
        lockfile: Digest::parse(&"bb".repeat(32)).expect("64 位十六进制"),
        target_spec: "linux-1".to_string(),
        toolchain: "poly-test/1".to_string(),
        profile: "ci".to_string(),
    };
    assert_eq!(cache_key(&inputs).as_str(), GOLDEN_KEY);
}

#[test]
fn target_spec_parses_and_validates_in_engine_layer() {
    let spec = parse_target_spec(TARGET_JSON).expect("TargetSpec JSON 可解析");
    assert_eq!(spec.id, "linux");
    assert_eq!(spec.validate(), Ok(()));
}

#[test]
fn job_spec_parses_and_validates_in_engine_layer() {
    let job = parse_job_spec(JOB_JSON).expect("JobSpec JSON 可解析");
    assert_eq!(job.id, "engine-layer");
    assert_eq!(job.validate(), Ok(()));

    let bad = JOB_JSON.replace("\"tier\": 1", "\"tier\": 9");
    let bad = parse_job_spec(&bad).expect("JSON 仍可解析");
    assert!(bad.validate().is_err(), "越界 tier 必须校验失败");
}
