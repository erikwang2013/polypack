// 构建 + 自检脚本端到端（node 子进程真跑）：成功径（含 SKIP-with-print）与失败径（manifest 缺项）。
// 依据：任务预案「无设备 → 结构自检 + SKIP-with-print」——失败径必须红，绝不假装绿。

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PKG = path.resolve(HERE, "..");
const SCRIPT = path.join(PKG, "scripts", "build-and-check.mjs");

const cleanEnv = () => {
  const env = { ...process.env };
  delete env.POLY_ROKU_REAL;
  delete env.ROKU_DEV_TARGET;
  delete env.ROKU_DEV_PASSWORD;
  return env;
};

const runScript = (args) => spawnSync(process.execPath, [SCRIPT, ...args], { cwd: PKG, encoding: "utf8", env: cleanEnv() });

test("端到端：出 zip + 结构自检全过 + 侧载 SKIP-with-print（无真机预案）", () => {
  const out = fs.mkdtempSync(path.join(os.tmpdir(), "roku-build-"));
  try {
    const r = runScript(["--out", out]);
    assert.equal(r.status, 0, `脚本应退出 0；stderr: ${r.stderr}`);
    assert.match(r.stdout, /结构自检 PASS/);
    assert.match(r.stdout, /\[SKIP\] 无真机侧载/);
    assert.match(r.stdout, /plugin_install/); // 手册命令在场
    const zip = path.join(out, "polypack-spike-channel.zip");
    assert.ok(fs.existsSync(zip), "zip 产物应生成");
    const list = spawnSync("unzip", ["-Z1", zip], { encoding: "utf8" });
    assert.match(list.stdout, /^manifest$/m);
    assert.match(list.stdout, /^source\/focusgraph\.brs$/m);
    assert.match(list.stdout, /^components\/PolypackScene\.xml$/m);
  } finally {
    fs.rmSync(out, { recursive: true, force: true });
  }
});

test("失败径：manifest 缺 title → 退出码 1 且给出定位", () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "roku-broken-"));
  try {
    const src = path.join(tmp, "channel");
    fs.cpSync(path.join(PKG, "channel"), src, { recursive: true });
    const manifest = fs.readFileSync(path.join(src, "manifest"), "utf8");
    fs.writeFileSync(path.join(src, "manifest"), manifest.replace(/^title=.*\n/m, ""));
    const r = runScript(["--src", src, "--out", path.join(tmp, "out")]);
    assert.equal(r.status, 1);
    assert.match(r.stdout, /\[FAIL\] manifest 缺必填字段: title/);
    assert.match(r.stdout, /结构自检 FAIL/);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
