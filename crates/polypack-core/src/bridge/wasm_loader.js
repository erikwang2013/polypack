// 小程序沙箱的加载 / 初始化适配层 —— 由 `polypack bridge wasm` 生成（T-M3-07），随代码包分发。
//
// 契约：宿主从代码包读出 wasm 字节（如小程序 fs 读文件得到 ArrayBuffer）后调用
// `load(bytes)`。本层先做**加载期**两道核对（导入模块 / 胶水函数面），再初始化
// wasm-bindgen 胶水，返回**只含白名单函数**的调用面。除宿主提供的 WebAssembly 与
// require（小程序模块系统）外不依赖任何平台 API：fetch / document / 原生模块一概不用。
"use strict";

var MANIFEST = /*__POLYPACK_MANIFEST__*/;

// 部分小程序基础库没有 TextEncoder / TextDecoder，而 wasm-bindgen 胶水在模块求值时就
// 构造它们（无 typeof 守卫）——缺则先装最小 UTF-8 垫片，且必须在 require 胶水之前。
function installTextCodecs() {
    var root = typeof globalThis !== "undefined" ? globalThis
        : (typeof self !== "undefined" ? self : this);
    if (typeof root.TextEncoder === "undefined") {
        root.TextEncoder = function () {
            this.encode = function (string) {
                var bytes = [];
                for (var i = 0; i < string.length; i++) {
                    var code = string.codePointAt(i);
                    if (code > 0xffff) { i++; }
                    if (code < 0x80) { bytes.push(code); }
                    else if (code < 0x800) { bytes.push(0xc0 | (code >> 6), 0x80 | (code & 63)); }
                    else if (code < 0x10000) { bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63)); }
                    else { bytes.push(0xf0 | (code >> 18), 0x80 | ((code >> 12) & 63), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63)); }
                }
                return new Uint8Array(bytes);
            };
        };
    }
    if (typeof root.TextDecoder === "undefined") {
        root.TextDecoder = function () {
            this.decode = function (bytes) {
                if (bytes === undefined) { return ""; }
                var out = "";
                var i = 0;
                while (i < bytes.length) {
                    var b = bytes[i++];
                    if (b < 0x80) { out += String.fromCharCode(b); }
                    else if (b < 0xe0) { out += String.fromCharCode(((b & 31) << 6) | (bytes[i++] & 63)); }
                    else if (b < 0xf0) { out += String.fromCharCode(((b & 15) << 12) | ((bytes[i++] & 63) << 6) | (bytes[i++] & 63)); }
                    else {
                        var cp = ((b & 7) << 18) | ((bytes[i++] & 63) << 12) | ((bytes[i++] & 63) << 6) | (bytes[i++] & 63);
                        cp -= 0x10000;
                        out += String.fromCharCode(0xd800 + (cp >> 10), 0xdc00 + (cp & 1023));
                    }
                }
                return out;
            };
        };
    }
}

function fail(message) {
    throw new Error("polypack-wasm: " + message);
}

function load(wasmBytes, glueOverride) {
    if (typeof WebAssembly === "undefined") {
        fail("宿主没有 WebAssembly（小程序基础库需支持 wasm）");
    }
    installTextCodecs();
    var glue = glueOverride;
    if (!glue) {
        if (typeof require !== "function") {
            fail("宿主模块系统不可用（require 缺失）：无法加载胶水 " + MANIFEST.glue);
        }
        glue = require(MANIFEST.glue);
    }

    var module = new WebAssembly.Module(wasmBytes);

    // ① 产物无原生模块依赖：导入只允许来自胶水自身模块（其余一律拒绝）
    WebAssembly.Module.imports(module).forEach(function (item) {
        if (item.module !== MANIFEST.import_module) {
            fail("产物导入了非沙箱模块 " + item.module + "（产物必须无原生模块依赖）");
        }
    });

    // ② API 面白名单：胶水暴露的函数必须都在白名单内（多一个即拒绝）
    var allowed = {};
    MANIFEST.functions.forEach(function (fn) { allowed[fn.name] = true; });
    Object.keys(glue).forEach(function (key) {
        if (typeof glue[key] === "function" && key !== "initSync" && !allowed[key]) {
            fail("白名单外接口 " + key + "：编译期白名单未放行（接口定义未含，或类型越出沙箱子集）");
        }
    });

    glue.initSync({ module: module });

    // ③ 白名单函数必须齐备；对外只暴露白名单调用面
    var api = {};
    MANIFEST.functions.forEach(function (fn) {
        if (typeof glue[fn.name] !== "function") {
            fail("白名单接口 " + fn.name + " 在产物中缺失");
        }
        api[fn.name] = glue[fn.name];
    });
    return api;
}

module.exports = { load: load, manifest: MANIFEST };
