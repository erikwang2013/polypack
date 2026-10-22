#!/usr/bin/env node
// 小程序沙箱的 Node 模拟宿主 —— polypack-core `tests/wasm.rs` / `tests/wasm_real.rs` 的夹具。
//
// 刻意只给沙箱三样：WebAssembly、模块系统（require，仅包内相对路径）、console。
// 不给 TextEncoder / TextDecoder（复刻部分小程序基础库的缺位形态——适配层需自装垫片），
// 不给 process / fs / fetch（产物若依赖宿主原生能力即走不通，等价「无原生模块依赖」的
// 运行期判据）。wasm 字节由「宿主」在沙箱外读出后传入（对应小程序 fs 读代码包文件）。
//
// 用法：node harness.js <dist-dir> <wasm-file> <calls-json>
//   calls-json: [{"fn":"greet","args":["world"]}, ...]
// 输出（stdout 单行 JSON）：
//   {"ok":bool,"results":[...],"error":"...","api_keys":[...],"functions":[...],
//    "sandbox":{"text_encoder_before":"undefined","text_encoder_after":"function",...}}
"use strict";

const fs = require("fs");
const path = require("path");
const vm = require("vm");

function emit(payload) {
    process.stdout.write(JSON.stringify(payload) + "\n");
}

const [distDir, wasmFile, callsJson] = process.argv.slice(2);
const out = { ok: false, results: [], sandbox: {} };

try {
    const dist = path.resolve(distDir);
    const calls = JSON.parse(callsJson || "[]");

    // 包内模块系统：只解析 <dist>/…/x.js，越界（绝对路径 / ..）一律拒绝
    const moduleCache = new Map();
    function requireInPackage(id) {
        if (typeof id !== "string" || path.isAbsolute(id) || id.split(/[\\/]/).includes("..")) {
            throw new Error("sandbox require: 只允许包内相对路径: " + id);
        }
        const file = path.resolve(dist, id.endsWith(".js") ? id : id + ".js");
        if (file !== dist && !file.startsWith(dist + path.sep)) {
            throw new Error("sandbox require: 越出包目录: " + id);
        }
        if (moduleCache.has(file)) {
            return moduleCache.get(file).exports;
        }
        const module = { exports: {} };
        moduleCache.set(file, module);
        const source = fs.readFileSync(file, "utf8");
        const wrapper = vm.runInContext(
            "(function (module, exports, require) {" + source + "\n})",
            context,
            { filename: file }
        );
        wrapper(module, module.exports, requireInPackage);
        return module.exports;
    }

    const context = vm.createContext({ console });
    context.require = requireInPackage;
    // wasm 字节：沙箱外读出，只把字节本身交进沙箱（Buffer 的 .buffer 可能是共享池，需切片）
    const buffer = fs.readFileSync(wasmFile);
    context.__wasmBuffer = buffer.buffer.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength);

    out.sandbox.text_encoder_before = vm.runInContext("typeof TextEncoder", context);
    out.sandbox.text_decoder_before = vm.runInContext("typeof TextDecoder", context);
    out.sandbox.process = vm.runInContext("typeof process", context);
    out.sandbox.fetch = vm.runInContext("typeof fetch", context);
    // 导入表证据（「产物无原生模块依赖」的字节级判据）
    out.sandbox.imports = vm.runInContext(
        "WebAssembly.Module.imports(new WebAssembly.Module(new Uint8Array(__wasmBuffer)))" +
            ".map(function (item) { return item.module + \".\" + item.name; })",
        context
    );

    const runner = vm.runInContext(
        "(function (calls) {\n" +
            '    var loader = require("./polypack-loader.js");\n' +
            "    var api = loader.load(new Uint8Array(__wasmBuffer));\n" +
            "    var results = calls.map(function (call) { return api[call.fn].apply(api, call.args); });\n" +
            "    return {\n" +
            "        api_keys: Object.keys(api),\n" +
            "        functions: loader.manifest.functions.map(function (fn) { return fn.name; }),\n" +
            "        results: results,\n" +
            "    };\n" +
            "})",
        context,
        { filename: "polypack-sandbox-runner.js" }
    );

    const run = runner(calls);
    out.ok = true;
    out.results = run.results;
    out.api_keys = run.api_keys;
    out.functions = run.functions;
    out.sandbox.text_encoder_after = vm.runInContext("typeof TextEncoder", context);
} catch (error) {
    out.error = String((error && error.message) || error);
}

emit(out);
