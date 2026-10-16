// T-M4-13 mock 腿（每提交 / PR 可跑；零 npm install 亦可——与 adapter 系测试同口径）。
// 覆盖：期望漂移哨兵（现场推导 ≡ 提交金样）/ 死路与节点覆盖 / 履约哨兵（镜像 ≡ 真示例页，
// react 在场才跑，缺席 SKIP-with-print）/ 失败定位格式自证 / 两腿键表同源核对。

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { compareStep, descriptorFailure, stableStringify } from "../compare.mjs";
import { adbKeyevent, digitOf, domKey, kernelKeyOfDom } from "../keys.mjs";
import { TV_EXAMPLE_SCENES, createTvExampleSession, deriveTvExample } from "../scenarios.mjs";
import { checkConnectivity, createTvFocus, hasNoDeadEnds } from "../../../adapters/tv/src/index.js";
import { EXAMPLE_FOCUS_NODES, TV_MODAL_FOCUS_NODES, TV_PLAYER_FOCUS_NODES } from "../../../adapters/tv/src/example-nodes.js";

const HERE = dirname(fileURLToPath(import.meta.url));

test("期望漂移哨兵：现场推导 ≡ 提交的 expectations.json", () => {
  const committed = JSON.parse(readFileSync(join(HERE, "..", "expectations.json"), "utf8"));
  assert.equal(
    stableStringify(deriveTvExample()),
    stableStringify(committed),
    "内核或场景已变：跑 node ui/smoke/tv/emit-expectations.mjs --write 刷新金样后重跑",
  );
});

test("无死路：各焦点域全连通、方向键无陷阱、节点无域外指（静态保证）", () => {
  const domains = [
    ["宿主域", EXAMPLE_FOCUS_NODES, "counter"],
    ["详情层", TV_MODAL_FOCUS_NODES, "modal-play"],
    ["播放器层", TV_PLAYER_FOCUS_NODES, "player-stop"],
  ];
  for (const [name, nodes, initial] of domains) {
    const graph = createTvFocus({ nodes, initial }).graph;
    const conn = checkConnectivity(graph);
    assert.equal(conn.connected, true, `${name} 有孤岛：${conn.isolated.join(", ")}`);
    assert.deepEqual(conn.traps, [], `${name} 有方向陷阱（四向自环/缺失）：${conn.traps.join(", ")}`);
    assert.equal(hasNoDeadEnds(graph), true, `${name} 有域外指向（焦点死路来源）`);
  }
});

test("节点覆盖：全部可焦点节点在场景期望焦点里出现过（新增节点必须补场景）", () => {
  const anchors = new Set();
  for (const scene of deriveTvExample().scenes) for (const focus of scene.focus) anchors.add(focus);
  const all = [...EXAMPLE_FOCUS_NODES, ...TV_MODAL_FOCUS_NODES, ...TV_PLAYER_FOCUS_NODES].map((n) => n.id);
  const missing = all.filter((id) => !anchors.has(id));
  assert.deepEqual(missing, [], `场景未覆盖节点：${missing.join(", ")}（补场景键序列后 emit-expectations --write）`);
});

test("履约哨兵：镜像会话 ≡ 真示例页 onKey/getState（react 在场才跑）", async (t) => {
  let createExamplePage;
  let createAdapter;
  try {
    ({ createExamplePage } = await import("../../../adapters/tv/src/example.js"));
    ({ createAdapter } = await import("../../../adapters/tv/src/index.js"));
  } catch (err) {
    t.skip(`SKIP：react 不可解析（${err.code ?? err.message}）——本机/CI 未装 ui/adapters/tv 依赖；哨兵未执行（零 npm install 口径），nightly 真跑腿装依赖后执行`);
    return;
  }
  const runtime = { React: { createElement: () => null }, ReactDOM: { createRoot: () => ({ render() {} }) } };
  const adapter = createAdapter({ target: "tizen-tv", runtime, host: { pointer: "none" } });
  const primitives = { View: () => null, Text: () => null, Pressable: () => null, Image: () => null, iconSource: null };
  const page = createExamplePage(primitives, adapter); // 核心任务不经 React 渲染即可驱动（store 在 React 外）
  const mirror = createTvExampleSession();
  for (const scene of TV_EXAMPLE_SCENES) {
    for (const [i, key] of scene.keys.entries()) {
      const real = page.onKey(key);
      const mine = mirror.onKey(key);
      assert.equal(stableStringify(mine), stableStringify(real), `[${scene.id}] 第 ${i + 1} 键（${key}）镜像与真页描述符漂移`);
      assert.equal(stableStringify(mirror.getState()), stableStringify(page.getState()), `[${scene.id}] 第 ${i + 1} 键（${key}）状态漂移`);
    }
  }
});

test("失败定位自证：四类失效各命中一类且定位串非空", () => {
  const expect = { result: { type: "move", direction: "left", from: "reset", to: "counter" }, focus: "counter", clicks: 0, depth: 0, exitRequested: false };
  const base = { result: expect.result, focus: "counter", focusCount: 1, clicks: 0, depth: 0, exitHint: false, hint: "遥控器：…" };
  const verdict = (key, observed, prevFocus = null) => compareStep({ key, expect, observed, prevFocus });

  assert.equal(verdict("DPAD_LEFT", base).ok, true); // 基线：全对即过
  const stuck = verdict("DPAD_LEFT", { ...base, focus: "reset" }, "reset");
  assert.equal(stuck.failures[0].class, "focus-dead-end");
  assert.match(stuck.failures[0].detail, /期望.*实际/);
  const lost = verdict("DPAD_LEFT", { ...base, focus: null });
  assert.equal(lost.failures[0].class, "focus-dead-end");
  const noLine = verdict("DPAD_LEFT", { ...base, result: null });
  assert.equal(noLine.failures[0].class, "channel");
  const double = verdict("DPAD_LEFT", { ...base, focusCount: 2 });
  assert.equal(double.failures[0].class, "focus-dead-end");
  const backRoute = compareStep({
    key: "Back",
    expect: { ...expect, result: { type: "back", route: "memory", from: "detail", to: "exit" } },
    observed: { ...base, result: { type: "back", route: "exit", from: "detail", to: null } },
  });
  assert.equal(backRoute.failures[0].class, "route");
  const digitMiss = compareStep({
    key: "Digit1",
    expect: { ...expect, result: { type: "shortcut", digit: "1", from: "reset", to: "counter" } },
    observed: { ...base, result: { type: "shortcut", digit: "1", from: "reset", to: "detail" } },
  });
  assert.equal(digitMiss.failures[0].class, "shortcut");
  // Android 腿走 descriptorFailure 打标签（真值由 run-walk 裁定）
  assert.equal(descriptorFailure("Digit1", expect.result, null).class, "channel");
  assert.equal(descriptorFailure("Back", { type: "back", route: "exit" }, { type: "back", route: "memory" }).class, "route");
  assert.equal(descriptorFailure("DPAD_UP", expect.result, expect.result), null);
});

test("两腿键表同源：adb 表 ≡ walks.mjs keymap；DOM 表往返归一", async () => {
  const { buildExpectations } = await import("../../../adapters/native/device-walks/walks.mjs");
  for (const { kernel, adb } of buildExpectations().keymap) {
    assert.equal(adbKeyevent(kernel), adb, `adb 键表与 walks.mjs keymap 漂移：${kernel}`);
    assert.ok(domKey(kernel).code.length > 0, `DOM 键表缺 ${kernel}`);
  }
  for (const scene of TV_EXAMPLE_SCENES) {
    for (const key of scene.keys) {
      const back = kernelKeyOfDom(domKey(key));
      if (digitOf(key) !== null) assert.equal(digitOf(back), digitOf(key), `数字键归一漂移：${key}`);
      else assert.equal(back, key, `DOM 往返漂移：${key}`);
    }
  }
  assert.equal(kernelKeyOfDom({ key: "q", code: "KeyQ" }), null); // 无关键不误吞
});
