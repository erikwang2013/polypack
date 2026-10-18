//! 版本协商负向夹具：声明 `target-spec/v2`，宿主必须拒绝并给迁移提示。
//! 仅用于测试，不作为发布制品。

#![no_std]
extern crate alloc;

use alloc::string::String;

wit_bindgen::generate!({ world: "polypack-target", path: "../../wit" });

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

struct OutdatedVersion;

impl Guest for OutdatedVersion {
    fn describe() -> TargetSpec {
        TargetSpec {
            spec_version: String::from("target-spec/v2"),
            id: String::from("outdated"),
            tier: 3,
            runtime: String::from("wasm"),
            bridge: String::from("wasm-bindgen"),
            package_format: String::from("outdated-bin"),
            builder: String::from("outdated-version"),
            hosts: alloc::vec![String::from("linux")],
        }
    }

    fn package(_bundle: UiBundle, _lib: CoreLib) -> Result<Artifact, Error> {
        Ok(Artifact {
            format: String::from("outdated-bin"),
            bytes: alloc::vec![],
        })
    }
}

export!(OutdatedVersion);
