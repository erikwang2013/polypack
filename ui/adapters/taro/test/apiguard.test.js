// T-M3-05 API 白名单断言：构建期拒绝越界调用（两个正例方向 + 三个负例方向），
// 以及 CLI（bin/check-api.mjs）的非零退出——打包器预检调用的就是这条链。

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  API_WHITELIST,
  FORBIDDEN_GLOBALS,
  ROUTE_METHODS,
  checkSource,
  formatViolations,
} from "../src/index.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CLI = path.resolve(HERE, "../bin/check-api.mjs");

test("白名单自洽：路由方法全部在白名单内，清单无重复", () => {
  for (const method of ROUTE_METHODS) {
    assert.ok(API_WHITELIST.includes(method), `路由方法 ${method} 在白名单外——自相矛盾的守卫`);
  }
  assert.equal(new Set(API_WHITELIST).size, API_WHITELIST.length, "白名单不得重复");
  assert.equal(new Set(FORBIDDEN_GLOBALS).size, FORBIDDEN_GLOBALS.length);
});

test("过：白名单内 API 与普通代码零违规", () => {
  const code = [
    'import Taro from "@tarojs/taro";',
    "const info = Taro.getWindowInfo();",
    "Taro.setStorageSync('clicks', 1);",
    "Taro.navigateTo({ url: '/pages/index/index' });",
    "const count = 1 + 2;",
    "const fetching = wrap(fetchLater);",
  ].join("\n");
  assert.deepEqual(checkSource(code, { filename: "ok.js" }).violations, []);
});

test("拒：DOM / 浏览器全局（document / window / localStorage / fetch）逐行点名", () => {
  const code = [
    "const el = document.getElementById('app');",
    "window.scrollTo(0, 0);",
    "localStorage.setItem('k', 'v');",
    "const res = await fetch('/api');",
  ].join("\n");
  const { violations } = checkSource(code);
  assert.deepEqual(
    violations.map((v) => [v.line, v.kind, v.name]),
    [
      [1, "forbidden-global", "document"],
      [2, "forbidden-global", "window"],
      [3, "forbidden-global", "localStorage"],
      [4, "forbidden-global", "fetch"],
    ],
  );
});

test("拒：白名单外的 Taro API（含裸下标形态）", () => {
  const code = [
    "Taro.request({ url: 'https://example.com' });",
    "Taro['createSelectorQuery']();",
  ].join("\n");
  const { violations } = checkSource(code);
  assert.deepEqual(
    violations.map((v) => [v.line, v.kind, v.name]),
    [
      [1, "taro-api", "request"],
      [2, "taro-api", "createSelectorQuery"],
    ],
  );
});

test("放行标记 poly:allow-api：同行豁免（构造样本 / 文档示例的出口）", () => {
  const code = [
    "const doc = document; // poly:allow-api 构造样本",
    "Taro.request({}); // poly:allow-api",
  ].join("\n");
  assert.deepEqual(checkSource(code).violations, []);
});

test("formatViolations：报告行含文件 / 行号 / 规则与人话说明", () => {
  const report = checkSource("window.scrollTo(0, 0);\nalert('x');", { filename: "page.js" });
  const lines = formatViolations(report);
  assert.equal(lines.length, 2, "window 与 alert 各一条");
  assert.match(lines[0], /^page\.js:1: \[forbidden-global\] window——/);
  assert.match(lines[1], /^page\.js:2: \[forbidden-global\] alert——/);
});

test("CLI：含越界调用的目录非零退出并打印报告；干净目录零退出", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "polypack-taro-api-"));
  try {
    fs.writeFileSync(path.join(dir, "bad.js"), "document.title = 'x';\n");
    const bad = spawnSync(process.execPath, [CLI, dir], { encoding: "utf8" });
    assert.equal(bad.status, 1, "越界调用须构建期拒绝（非零退出）");
    assert.match(bad.stderr, /bad\.js:1: \[forbidden-global\] document/);
    assert.match(bad.stderr, /白名单预检失败/);

    fs.rmSync(path.join(dir, "bad.js"));
    fs.writeFileSync(path.join(dir, "good.js"), "Taro.showToast({ title: 'ok' });\n");
    const good = spawnSync(process.execPath, [CLI, dir], { encoding: "utf8" });
    assert.equal(good.status, 0, good.stderr);
    assert.match(good.stdout, /白名单预检通过/);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("CLI：无参数 → 用法退出码 2（不静默通过）", () => {
  const result = spawnSync(process.execPath, [CLI], { encoding: "utf8" });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /用法/);
});
