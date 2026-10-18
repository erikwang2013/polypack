//! T-M6-02 验收：C ABI + 单处生成的头文件 + 编译器真跑。
//!
//! - 头文件与生成器逐字节一致（仓库内唯一一份，生成器即来源）；
//! - `gcc -std=c11 -Wall -Werror` 与 `clang -std=c11` 真编译（缺工具则 SKIP-with-print）；
//! - C ABI 可从 Rust 侧调用（返回码 / 版本 / 缓存键）；
//! - 缓存键在引擎层与宿主 `poly-cache` 两处取值一致（同一实现，golden 钉死）。

use std::path::{Path, PathBuf};
use std::process::Command;

use poly_core::engine::{CacheInputs, Digest, cache_key, ffi, header, parse_job_spec};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn header_path() -> PathBuf {
    manifest_dir().join("include").join(header::HEADER_FILE)
}

#[test]
fn checked_in_header_is_generated_not_handwritten() {
    let generated = header::c_header();
    let actual = std::fs::read_to_string(header_path())
        .expect("include/polypack_engine.h 应随库入库（由生成器产出）");
    assert_eq!(
        actual, generated,
        "头文件漂移：cargo run -p poly-core --example gen_engine_header 重新生成"
    );
}

#[test]
fn repository_has_no_second_header() {
    let root = manifest_dir()
        .ancestors()
        .nth(2)
        .expect("crates/poly-core 上溯两级为仓库根")
        .to_path_buf();
    let mut headers = Vec::new();
    collect_headers(&root, &mut headers);
    headers.sort();
    assert_eq!(
        headers,
        vec![header_path()],
        "仓库内只允许生成器产出的那一份头文件；发现第二份: {headers:?}"
    );
}

fn collect_headers(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if matches!(
            name.to_str(),
            Some("target" | ".git" | "node_modules" | ".claude" | "build")
        ) {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_headers(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "h")
            && std::fs::read_to_string(&path)
                .is_ok_and(|text| text.contains("poly_engine_abi_version"))
        {
            // 只把「声明了引擎 ABI 入口」的头算作候选：RTOS 工程模板的
            // FreeRTOSConfig.h 只声明应用侧 panic 辅助函数（poly_engine_rt_panic），
            // 不是第二份 ABI 头。
            out.push(path);
        }
    }
}

#[test]
fn header_compiles_with_gcc_and_clang_c11() {
    let include = manifest_dir().join("include");
    let source =
        std::env::temp_dir().join(format!("polypack_engine_header_{}.c", std::process::id()));
    std::fs::write(
        &source,
        "#include \"polypack_engine.h\"\n\
         int main(void) { return (int)poly_engine_abi_version(); }\n",
    )
    .expect("写临时 C 源失败");

    let legs: [(&str, &[&str]); 2] = [
        ("gcc", &["-std=c11", "-Wall", "-Werror"]),
        ("clang", &["-std=c11"]),
    ];
    for (compiler, flags) in legs {
        if Command::new(compiler).arg("--version").output().is_err() {
            println!("SKIP: {compiler} 不在场，头文件编译腿未真跑");
            continue;
        }
        let output = Command::new(compiler)
            .args(flags)
            .arg("-fsyntax-only")
            .arg("-I")
            .arg(&include)
            .arg(&source)
            .output()
            .expect("启动编译器");
        assert!(
            output.status.success(),
            "{compiler} {flags:?} 编译头文件失败: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        println!("{compiler} {flags:?}: OK");
    }
    let _ = std::fs::remove_file(&source);
}

#[test]
fn c_abi_is_callable_and_prefixed() {
    for function in ffi::FFI_FUNCTIONS {
        assert!(
            function.name.starts_with("poly_engine_"),
            "C ABI 必须统一 poly_engine_* 前缀: {}",
            function.name
        );
    }
    assert_eq!(ffi::poly_engine_abi_version(), ffi::ABI_VERSION);
    assert!(!ffi::poly_engine_version().is_null());

    let stub = poly_core::job::JobSpec {
        id: "cffi".to_string(),
        profile: poly_core::job::Profile::Ci,
        targets: vec![poly_targets::TargetSpec::new_stub(
            "linux",
            1,
            &[poly_targets::Host::Linux],
        )],
        excluded: Vec::new(),
        secrets: Vec::new(),
        budget: poly_core::job::Budget::default(),
        plugins: Vec::new(),
    };
    let json = serde_json::to_vec(&stub).expect("JobSpec 可序列化");
    assert_eq!(
        ffi::poly_engine_validate_job_json(json.as_ptr(), json.len()),
        ffi::OK
    );
    assert_eq!(
        ffi::poly_engine_validate_job_json(b"not json".as_ptr(), 8),
        ffi::ERR_JSON
    );
    assert_eq!(
        ffi::poly_engine_validate_job_json(std::ptr::null(), 4),
        ffi::ERR_INPUT
    );

    let mut invalid: serde_json::Value = serde_json::from_slice(&json).expect("合法 JSON");
    invalid["targets"][0]["tier"] = serde_json::json!(9);
    let invalid = serde_json::to_vec(&invalid).expect("可序列化");
    assert_eq!(
        ffi::poly_engine_validate_job_json(invalid.as_ptr(), invalid.len()),
        ffi::ERR_SPEC
    );

    // 同一份 JSON 在引擎层与宿主侧解析结果一致。
    let text = std::str::from_utf8(&json).expect("UTF-8");
    assert_eq!(parse_job_spec(text).expect("引擎解析"), stub);

    // 解析层与 poly-targets 的校验面是同一个实现。
    let spec_json = serde_json::to_string(&stub.targets[0]).expect("TargetSpec 可序列化");
    assert_eq!(
        ffi::poly_engine_validate_target_json(spec_json.as_ptr(), spec_json.len()),
        ffi::OK
    );
}

#[test]
fn cache_key_is_identical_across_engine_and_host_pipeline() {
    let inputs = CacheInputs {
        sources: Digest::parse(&"aa".repeat(32)).expect("64 位十六进制"),
        lockfile: Digest::parse(&"bb".repeat(32)).expect("64 位十六进制"),
        target_spec: "linux-1".to_string(),
        toolchain: "poly-test/1".to_string(),
        profile: "ci".to_string(),
    };
    let engine_key = cache_key(&inputs);
    assert_eq!(
        engine_key,
        poly_cache::cache_key(&inputs),
        "引擎层与宿主流水线必须共用同一缓存键实现"
    );
    // golden：std 与 no_std 编译的是同一函数（无 cfg 分支），值由本常量钉死。
    assert_eq!(
        engine_key.as_str(),
        "263bcee0a06fbd80a257e7194b63af7ee5f9e7265f00ceb20a72d66c721754fb",
        "缓存键公式变化必须同步更新 golden 并递增 poly-cache 的 KEY_DOMAIN"
    );
}
