//! 引擎层稳定 C ABI（T-M6-02）：`polypack_engine_*` 前缀。
//!
//! 本文件同时是 C 头文件的**唯一接口定义来源**：[`FFI_CONSTANTS`] / [`FFI_FUNCTIONS`]
//! 由 [`super::header::c_header`] 生成 `include/polypack_engine.h`；仓库内不存在第二份
//! 手写头文件。Rust 侧函数紧挨清单实现，`nm` 导出与清单由测试对齐。
//!
//! 安全边界只有一处：调用方传入的 C 指针按头文件契约在调用期间有效；除此之外全部是
//! 安全 Rust。故仅本模块放宽 `unsafe_code`（workspace 其余部分仍是 deny）。
//!
//! 静态库产物（打包器 / 构建链最小面）：宿主自包含产物用
//! `cargo rustc -p polypack-core --lib --crate-type staticlib`；四端（QNX / AUTOSAR /
//! VxWorks / Zephyr / FreeRTOS）以 rlib 链入应用 crate（应用提供 global_allocator 与
//! panic_handler），交叉构建命令为
//! `cargo build -p polypack-core --no-default-features --target <triple>`。

#![allow(unsafe_code)]
// C ABI 契约：指针有效性由头文件约定并由 C 调用方保证；Rust 侧只供测试 / 打包器使用。
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use alloc::ffi::CString;
use alloc::string::String;
use core::ffi::{CStr, c_char};
use core::ptr;

use crate::engine::{cache_key, parse_job_spec, parse_target_spec};
use polypack_cache::{CacheInputs, Digest};

/// ABI 版本：函数签名或返回码语义破坏性变更时递增。
pub const ABI_VERSION: u32 = 1;

/// 成功。
pub const OK: i32 = 0;
/// 入参非法（空指针 / 非 UTF-8 / 摘要不是 64 位十六进制）。
pub const ERR_INPUT: i32 = -1;
/// JSON 语法或字段类型错误。
pub const ERR_JSON: i32 = -2;
/// 解析成功但声明未通过校验。
pub const ERR_SPEC: i32 = -3;

/// C ABI 常量（进头文件）。
pub struct FfiConstant {
    /// C 宏名。
    pub name: &'static str,
    /// 整数值。
    pub value: i32,
    /// 一行说明。
    pub doc: &'static str,
}

/// C ABI 函数声明（进头文件；Rust 侧的唯一定义来源）。
pub struct FfiFunction {
    /// C 函数名。
    pub name: &'static str,
    /// C 返回类型。
    pub returns: &'static str,
    /// 参数 `(名字, C 类型)`；空表示 `(void)`。
    pub params: &'static [(&'static str, &'static str)],
    /// 一行说明。
    pub doc: &'static str,
}

/// 常量清单（头文件生成的唯一来源之一）。
pub const FFI_CONSTANTS: &[FfiConstant] = &[
    FfiConstant {
        name: "POLYPACK_ENGINE_OK",
        value: OK,
        doc: "成功",
    },
    FfiConstant {
        name: "POLYPACK_ENGINE_ERR_INPUT",
        value: ERR_INPUT,
        doc: "入参非法（空指针 / 非 UTF-8 / 摘要格式错误）",
    },
    FfiConstant {
        name: "POLYPACK_ENGINE_ERR_JSON",
        value: ERR_JSON,
        doc: "JSON 语法或字段类型错误",
    },
    FfiConstant {
        name: "POLYPACK_ENGINE_ERR_SPEC",
        value: ERR_SPEC,
        doc: "解析成功但 JobSpec / TargetSpec 校验失败",
    },
];

/// 函数清单（头文件生成的唯一来源之一）。
pub const FFI_FUNCTIONS: &[FfiFunction] = &[
    FfiFunction {
        name: "polypack_engine_abi_version",
        returns: "uint32_t",
        params: &[],
        doc: "ABI 版本（与头文件宏 POLYPACK_ENGINE_ABI_VERSION 一致）",
    },
    FfiFunction {
        name: "polypack_engine_version",
        returns: "const char *",
        params: &[],
        doc: "引擎版本串（静态存储，无需释放）",
    },
    FfiFunction {
        name: "polypack_engine_validate_job_json",
        returns: "int32_t",
        params: &[("json", "const uint8_t *"), ("len", "size_t")],
        doc: "解析并校验 JobSpec JSON；返回 POLYPACK_ENGINE_* 码",
    },
    FfiFunction {
        name: "polypack_engine_validate_target_json",
        returns: "int32_t",
        params: &[("json", "const uint8_t *"), ("len", "size_t")],
        doc: "解析并校验 TargetSpec JSON；返回 POLYPACK_ENGINE_* 码",
    },
    FfiFunction {
        name: "polypack_engine_cache_key",
        returns: "char *",
        params: &[
            ("sources_hex", "const char *"),
            ("lockfile_hex", "const char *"),
            ("target_spec", "const char *"),
            ("toolchain", "const char *"),
            ("profile", "const char *"),
        ],
        doc: "计算缓存键（64 位十六进制，调用方用 polypack_engine_string_free 释放）；失败返回 NULL",
    },
    FfiFunction {
        name: "polypack_engine_string_free",
        returns: "void",
        params: &[("text", "char *")],
        doc: "释放 polypack_engine_cache_key 返回的字符串（NULL 安全）",
    },
];

static VERSION: &[u8] = concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes();

/// ABI 版本。
#[unsafe(no_mangle)]
pub extern "C" fn polypack_engine_abi_version() -> u32 {
    ABI_VERSION
}

/// 引擎版本串（`'\0'` 结尾的静态字节）。
#[unsafe(no_mangle)]
pub extern "C" fn polypack_engine_version() -> *const c_char {
    VERSION.as_ptr().cast()
}

/// 读调用方内存；空指针且长度非零 → `None`。
fn read_bytes<'a>(json: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    if json.is_null() {
        return None;
    }
    // SAFETY: 头文件契约要求 [json, json+len) 在调用期间有效。
    Some(unsafe { core::slice::from_raw_parts(json, len) })
}

/// 解析并校验 JobSpec JSON。
#[unsafe(no_mangle)]
pub extern "C" fn polypack_engine_validate_job_json(json: *const u8, len: usize) -> i32 {
    let Some(bytes) = read_bytes(json, len) else {
        return ERR_INPUT;
    };
    let Ok(text) = core::str::from_utf8(bytes) else {
        return ERR_INPUT;
    };
    let Ok(job) = parse_job_spec(text) else {
        return ERR_JSON;
    };
    match job.validate() {
        Ok(()) => OK,
        Err(_) => ERR_SPEC,
    }
}

/// 解析并校验 TargetSpec JSON。
#[unsafe(no_mangle)]
pub extern "C" fn polypack_engine_validate_target_json(json: *const u8, len: usize) -> i32 {
    let Some(bytes) = read_bytes(json, len) else {
        return ERR_INPUT;
    };
    let Ok(text) = core::str::from_utf8(bytes) else {
        return ERR_INPUT;
    };
    let Ok(spec) = parse_target_spec(text) else {
        return ERR_JSON;
    };
    match spec.validate() {
        Ok(()) => OK,
        Err(_) => ERR_SPEC,
    }
}

/// 读 NUL 结尾的 C 字符串。
fn read_c_str<'a>(text: *const c_char) -> Option<&'a str> {
    if text.is_null() {
        return None;
    }
    // SAFETY: 头文件契约要求指针指向 NUL 结尾的可读字符串。
    unsafe { CStr::from_ptr(text) }.to_str().ok()
}

/// 计算缓存键；任何入参非法返回 NULL。
#[unsafe(no_mangle)]
pub extern "C" fn polypack_engine_cache_key(
    sources_hex: *const c_char,
    lockfile_hex: *const c_char,
    target_spec: *const c_char,
    toolchain: *const c_char,
    profile: *const c_char,
) -> *mut c_char {
    let (Some(sources), Some(lockfile), Some(target_spec), Some(toolchain), Some(profile)) = (
        read_c_str(sources_hex),
        read_c_str(lockfile_hex),
        read_c_str(target_spec),
        read_c_str(toolchain),
        read_c_str(profile),
    ) else {
        return ptr::null_mut();
    };
    let (Ok(sources), Ok(lockfile)) = (Digest::parse(sources), Digest::parse(lockfile)) else {
        return ptr::null_mut();
    };
    let key = cache_key(&CacheInputs {
        sources,
        lockfile,
        target_spec: String::from(target_spec),
        toolchain: String::from(toolchain),
        profile: String::from(profile),
        app_id: None,
    });
    match CString::new(key.as_str()) {
        Ok(text) => text.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/// 释放 [`polypack_engine_cache_key`] 返回的字符串；NULL 安全。
#[unsafe(no_mangle)]
pub extern "C" fn polypack_engine_string_free(text: *mut c_char) {
    if text.is_null() {
        return;
    }
    // SAFETY: 指针必须由 polypack_engine_cache_key 返回，且只释放一次。
    unsafe { drop(CString::from_raw(text)) };
}
