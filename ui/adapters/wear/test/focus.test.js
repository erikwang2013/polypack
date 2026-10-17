// T-M7-01 焦点环复用探针：wear 入口（createWearFocus）与 adapter-tv / adapter-native
// 的共享内核口径由构造共享（createWearFocus 即 createTvFocus 别名），本测试以行为对拍钉住——
// 遍历快照恒等、圆 / 方两形态连通性、键路由描述符形态、Back 焦点记忆。
// 不新写导航系统：本文件不实现任何遍历 / 几何 / 键路由逻辑，只调用共享内核。

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  checkConnectivity,
  createFocusEngine,
  createWearFocus,
  deriveFocusNodes,
  handleKey,
  pageFocusNodes,
  tokensByForm,
  traversalSnapshot,
} from "../src/index.js";

const FORMS = Object.freeze(["round", "square"]);

test("内核复用：createWearFocus ≡ createFocusEngine(deriveFocusNodes(...))（遍历快照恒等）", () => {
  for (const form of FORMS) {
    const nodes = pageFocusNodes(tokensByForm[form]);
    const wear = createWearFocus({ nodes });
    const direct = createFocusEngine({ nodes: deriveFocusNodes(nodes) });
    assert.deepEqual(wear.order, direct.order, `${form}: 文档序恒等`);
    assert.deepEqual(
      traversalSnapshot(wear.graph),
      traversalSnapshot(direct.graph),
      `${form}: 遍历图恒等`,
    );
  }
});

test("连通性：两种形态均 4 节点连通、0 孤立、0 陷阱（2×2 四向可达）", () => {
  for (const form of FORMS) {
    const engine = createWearFocus({ nodes: pageFocusNodes(tokensByForm[form]) });
    const connectivity = checkConnectivity(engine.graph);
    assert.equal(connectivity.total, 4, form);
    assert.equal(connectivity.connected, true, form);
    assert.deepEqual(connectivity.isolated, [], form);
    assert.deepEqual(connectivity.traps, [], form);
  }
});

test("键路由描述符：方向 / 激活 / Back / 未识别四类形态（宿主记录与冒烟断言面）", () => {
  const engine = createWearFocus({ nodes: pageFocusNodes(tokensByForm.round) });
  assert.deepEqual(handleKey(engine, "ArrowRight"), {
    type: "move",
    direction: "right",
    from: "counter",
    to: "reset",
  });
  assert.deepEqual(handleKey(engine, "ArrowDown"), {
    type: "move",
    direction: "down",
    from: "reset",
    to: "exit",
  });
  assert.deepEqual(handleKey(engine, "ArrowLeft"), {
    type: "move",
    direction: "left",
    from: "exit",
    to: "detail",
  });
  assert.deepEqual(handleKey(engine, "ArrowUp"), {
    type: "move",
    direction: "up",
    from: "detail",
    to: "counter",
  });

  let activated = null;
  assert.deepEqual(
    handleKey(engine, "DPAD_CENTER", { activate: (id) => (activated = id) }),
    { type: "activate", id: "counter" },
  );
  assert.equal(activated, "counter", "激活回调收到激活时节点 id");
  assert.equal(engine.current, "counter", "激活不移动焦点");
  assert.deepEqual(handleKey(engine, "VolumeUp"), { type: "ignored", key: "VolumeUp" });
});

test("Back 焦点记忆（共享内核基线）与入口边界", () => {
  const engine = createWearFocus({ nodes: pageFocusNodes(tokensByForm.round) });
  handleKey(engine, "ArrowDown"); // counter → detail
  handleKey(engine, "ArrowRight"); // detail → exit
  assert.deepEqual(handleKey(engine, "Escape"), { type: "back", from: "exit", to: "detail" });
  assert.deepEqual(handleKey(engine, "Back"), { type: "back", from: "detail", to: "counter" });
  assert.deepEqual(handleKey(engine, "Back"), { type: "back", from: "counter", to: null });
  assert.equal(engine.current, "counter");

  assert.throws(() => createWearFocus(), /非空数组/);
  assert.throws(() => createWearFocus({ nodes: [] }), /非空数组/);
  assert.throws(
    () => createWearFocus({ nodes: pageFocusNodes(tokensByForm.round), initial: "nope" }),
    /不在图中/,
  );
});
