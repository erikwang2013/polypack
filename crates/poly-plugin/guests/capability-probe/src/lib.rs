//! 沙箱逃逸负向探针：导入 filesystem / network 能力接口，按 `case` 触发越权访问或
//! 资源耗尽。宿主测试断言拒绝码与 trap 类别，不作为发布制品。

#![no_std]
extern crate alloc;

use alloc::format;
use alloc::string::String;

wit_bindgen::generate!({ world: "polypack-capability-probe", path: "../../wit" });

// 接口导入在 `polypack:target-spec` 包命名空间下。
use polypack::target_spec::{filesystem, network};

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

struct Probe;

/// 能力调用的错误码回传给宿主；成功则回 `ok:<长度>`。
fn outcome(result: Result<alloc::vec::Vec<u8>, Error>) -> String {
    match result {
        Ok(bytes) => format!("ok:{}", bytes.len()),
        Err(err) => format!("{}|{}", err.code, err.message),
    }
}

impl Guest for Probe {
    fn run(case: String, arg: String) -> Result<String, Error> {
        match case.as_str() {
            "read" => Ok(outcome(filesystem::read(&arg))),
            "write" => Ok(outcome(
                filesystem::write(&arg, b"probe").map(|written| alloc::vec![0; written as usize]),
            )),
            "fetch" => match network::fetch(&arg) {
                Ok(body) => Ok(format!("ok:{}", body.len())),
                Err(err) => Ok(format!("{}|{}", err.code, err.message)),
            },
            // 无限循环：验证 fuel / epoch 上限（trap 由宿主归类）。
            "loop" => loop {
                core::hint::black_box(0);
            },
            // 资源耗尽：申请 64 MiB，宿主内存上限下 `memory.grow` 必须失败。
            "memory" => {
                let pages = 1024;
                let previous = core::arch::wasm32::memory_grow(0, pages);
                if previous == usize::MAX {
                    Ok(String::from("denied"))
                } else {
                    Ok(format!("grew:{previous}"))
                }
            }
            other => Err(Error {
                code: String::from("probe::E_UNKNOWN_CASE"),
                message: format!("未知探针用例: {other}"),
            }),
        }
    }
}

export!(Probe);
