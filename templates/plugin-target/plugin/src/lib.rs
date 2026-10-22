//! PolyPack 插件模板：用 `world polypack-target`（target-spec/v1）编译的宿主无关组件。
//!
//! `describe` 声明目标规格；`package` 把 UI 文件与核心库按
//! `<name>\n<bytes>\n` 顺序拼成 `example-bin` 产物。按需改写这两个函数，
//! 并保持 `spec-version` 为 `target-spec/v1`。
//!
//! 改写后同步 `contract/src/lib.rs` 的 `EXPECTED_FORMAT` / `EXPECTED_PACKAGE`
//! （契约测试唯一需要随插件修改的断言），再跑 `cargo test`。

#![no_std]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

wit_bindgen::generate!({ world: "polypack-target", path: "wit" });

/// 插件 id（由 `polypack plugin new` 替换；与 contract 测试的断言保持同步）。
const PLUGIN_ID: &str = "__polypack_plugin_id__";

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

struct Plugin;

impl Guest for Plugin {
    fn describe() -> TargetSpec {
        // 宿主提供的只读构建输入；模板仅演示调用，按需用于 id / 字段派生。
        let _inputs = inputs();
        TargetSpec {
            spec_version: String::from("target-spec/v1"),
            id: String::from(PLUGIN_ID),
            tier: 3,
            runtime: String::from("wasm"),
            bridge: String::from("wasm-bindgen"),
            package_format: String::from("example-bin"),
            builder: String::from(PLUGIN_ID),
            hosts: alloc::vec![
                String::from("linux"),
                String::from("macos"),
                String::from("windows"),
            ],
        }
    }

    fn package(bundle: UiBundle, lib: CoreLib) -> Result<Artifact, Error> {
        let mut bytes = Vec::new();
        for (name, contents) in bundle.files {
            bytes.extend_from_slice(name.as_bytes());
            bytes.push(b'\n');
            bytes.extend_from_slice(&contents);
            bytes.push(b'\n');
        }
        bytes.extend_from_slice(&lib.bytes);
        Ok(Artifact {
            format: String::from("example-bin"),
            bytes,
        })
    }
}

export!(Plugin);
