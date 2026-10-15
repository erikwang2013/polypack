// T-M2-05 设备走查对拍数据（device-walks/）：期望值与焦点内核同源，供 m2-android 的设备链执行。
// ① 提交的 expectations.json ≡ 内核现场推导（漂移哨兵，react-free）
// ② 键表覆盖内核路由语义（每个 adb 键在内核里确有 move/activate/back 落点）
// ③ 走查形态与覆盖面（遍历 / 边界 / 未定义方向 / 激活 / Back 记忆 / 未映射键）
// ④ clicks / focus 期望由示例页履约（react 缺席 skip，与代理渲染同口径）

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { KEYMAP, buildExpectations } from "../device-walks/walks.mjs";
import { EXAMPLE_FOCUS_NODES } from "../src/example-nodes.js";
import { createFocusEngine, handleKey } from "../src/focus.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const EXPECT_FILE = path.resolve(HERE, "../device-walks/expectations.json");
const expectations = buildExpectations();

test("对拍数据与内核同源：expectations.json ≡ 现场推导（改内核后跑 emit-expectations.mjs --write）", () => {
  assert.deepEqual(JSON.parse(fs.readFileSync(EXPECT_FILE, "utf8")), expectations);
});

test("键表覆盖内核路由：每个 adb 键在内核里都有落点", () => {
  const kinds = {};
  for (const { kernel } of KEYMAP) {
    kinds[kernel] = handleKey(createFocusEngine({ nodes: EXAMPLE_FOCUS_NODES }), kernel).type;
  }
  assert.deepEqual(kinds, {
    DPAD_UP: "move",
    DPAD_DOWN: "move",
    DPAD_LEFT: "move",
    DPAD_RIGHT: "move",
    DPAD_CENTER: "activate",
    Back: "back",
  });
});

test("走查形态与覆盖面：前置 + 每条键/命令/期望等长，六类路径齐备", () => {
  assert.equal(expectations.precondition.focus, "counter");
  assert.ok(expectations.precondition.reset.length > 0, "须给出回到初始状态的前置命令");
  for (const walk of expectations.walks) {
    assert.equal(walk.adb.length, walk.keys.length, `${walk.id}: 命令数须与键数一致`);
    assert.equal(walk.expect.results.length, walk.keys.length, `${walk.id}: results 长度`);
    assert.equal(walk.expect.focus.length, walk.keys.length, `${walk.id}: focus 长度`);
    assert.equal(walk.expect.clicks.length, walk.keys.length, `${walk.id}: clicks 长度`);
    assert.ok(walk.purpose, `${walk.id}: 须带 purpose`);
  }
  const ids = expectations.walks.map((w) => w.id);
  for (const id of [
    "traverse-cycle",
    "boundary-hold",
    "lateral-hold",
    "activate-counter",
    "reset-then-back",
    "unknown-key-ignored",
  ]) {
    assert.ok(ids.includes(id), `缺走查: ${id}`);
  }
  const types = expectations.walks.flatMap((w) => w.expect.results.map((r) => r.type));
  for (const type of ["move", "activate", "back", "ignored"]) {
    assert.ok(types.includes(type), `走查未覆盖 ${type} 路径`);
  }
});

// 只吞「react 未安装」一类失败；本包自身的加载错误照常炸出（与 render.test.js 同口径）。
let mods = null;
try {
  mods = { example: await import("../example.js"), adapter: await import("../src/index.js") };
} catch (error) {
  const reactMissing =
    error?.code === "ERR_MODULE_NOT_FOUND" && /Cannot find package 'react(-dom)?'/.test(error.message);
  if (!reactMissing) throw error;
}
const skip = mods === null ? "react 未安装（npm install 后启用走查履约断言）" : false;

test("clicks / focus 期望由示例页履约（每条走查独立起状态）", { skip }, () => {
  const adapter = mods.adapter.createAdapter({
    target: "android",
    runtime: { AppRegistry: { registerComponent() {} } },
    host: { pointer: "none" },
  });
  for (const walk of expectations.walks) {
    const { onKey, getState } = mods.example.createExampleApp({}, adapter);
    assert.deepEqual(
      getState(),
      { focus: expectations.precondition.focus, clicks: 0 },
      `${walk.id}: 初始状态`,
    );
    walk.keys.forEach((key, i) => {
      onKey(key);
      assert.equal(getState().focus, walk.expect.focus[i], `${walk.id}: 第 ${i + 1} 键(${key})后焦点`);
      assert.equal(getState().clicks, walk.expect.clicks[i], `${walk.id}: 第 ${i + 1} 键后计数`);
    });
  }
});
