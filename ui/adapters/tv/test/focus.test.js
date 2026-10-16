// T-M4-02 焦点内核对齐探针：TV 入口（createTvFocus）与 adapter-native 共享内核（T-M2-05）
// 的口径由构造共享（同一 createFocusEngine / handleKey），本测试以行为对拍钉住——
// 遍历快照恒等、键路由描述符形态一致、边界自环、Back 焦点记忆。
// 分层 Back（T-M4-03）与数字键路由在 back.test.js / shortcuts.test.js 续。

import assert from "node:assert/strict";
import { test } from "node:test";

import { EXAMPLE_FOCUS_NODES } from "../src/example-nodes.js";
import { createFocusEngine, createTvFocus, deriveFocusNodes, handleKey, traversalSnapshot } from "../src/index.js";

test("口径对拍：createTvFocus ≡ createFocusEngine(deriveFocusNodes(...))（遍历快照恒等）", () => {
  const tv = createTvFocus({ nodes: EXAMPLE_FOCUS_NODES });
  const direct = createFocusEngine({ nodes: deriveFocusNodes(EXAMPLE_FOCUS_NODES) });
  assert.deepEqual(tv.order, direct.order, "文档序（焦点顺序确定性来源）恒等");
  assert.deepEqual(traversalSnapshot(tv.graph), traversalSnapshot(direct.graph), "遍历图恒等");
});

test("键路由描述符：方向 / 激活 / Back / 未识别四类形态（宿主记录与冒烟断言面）", () => {
  const tv = createTvFocus({ nodes: EXAMPLE_FOCUS_NODES });
  assert.deepEqual(handleKey(tv, "DPAD_DOWN"), {
    type: "move",
    direction: "down",
    from: "counter",
    to: "detail",
  });
  assert.deepEqual(handleKey(tv, "ArrowRight"), {
    type: "move",
    direction: "right",
    from: "detail",
    to: "exit",
  });
  let activated = null;
  assert.deepEqual(
    handleKey(tv, "DPAD_CENTER", { activate: (id) => (activated = id) }),
    { type: "activate", id: "exit" },
  );
  assert.equal(activated, "exit", "激活回调收到激活时节点 id");
  assert.equal(tv.current, "exit", "激活不移动焦点");
  assert.deepEqual(handleKey(tv, "VolumeUp"), { type: "ignored", key: "VolumeUp" });
});

test("Back 焦点记忆（内核基线）：逐级回退至历史空；空历史返回 null 交宿主裁决", () => {
  const tv = createTvFocus({ nodes: EXAMPLE_FOCUS_NODES });
  handleKey(tv, "DPAD_DOWN"); // counter → detail
  handleKey(tv, "ArrowRight"); // detail → exit
  assert.deepEqual(handleKey(tv, "Escape"), { type: "back", from: "exit", to: "detail" });
  assert.deepEqual(handleKey(tv, "Back"), { type: "back", from: "detail", to: "counter" });
  assert.deepEqual(handleKey(tv, "Back"), { type: "back", from: "counter", to: null });
  assert.equal(tv.current, "counter");
});

test("入口边界：nodes 缺失 / initial 不在图中即快速失败", () => {
  assert.throws(() => createTvFocus(), /非空数组/);
  assert.throws(() => createTvFocus({ nodes: [] }), /非空数组/);
  assert.throws(() => createTvFocus({ nodes: EXAMPLE_FOCUS_NODES, initial: "nope" }), /不在图中/);
});
