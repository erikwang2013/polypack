//! PolyPack 参考插件：用 `world polypack-target`（target-spec/v1）编译的宿主无关组件。
//!
//! 只做最小但真实的 describe + package：`package` 把核心库与 UI 文件按
//! `<name>\n<bytes>\n` 顺序拼成 `reference-bin` 产物，供宿主契约测试断言。

#![no_std]
extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

wit_bindgen::generate!({ world: "polypack-target", path: "../../wit" });

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

struct ReferencePlugin;

impl Guest for ReferencePlugin {
    fn describe() -> TargetSpec {
        let input = inputs();
        TargetSpec {
            spec_version: String::from("target-spec/v1"),
            id: format!("{}-reference", input.target),
            tier: 3,
            runtime: String::from("wasm"),
            bridge: String::from("wasm-bindgen"),
            package_format: String::from("reference-bin"),
            builder: String::from("reference-plugin"),
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
            format: String::from("reference-bin"),
            bytes,
        })
    }
}

export!(ReferencePlugin);
