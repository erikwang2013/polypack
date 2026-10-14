// T-M2-05 焦点/导航基线测试：遍历图确定性（快照断言）、焦点顺序稳定、
// Back 返回上一焦点、无死路（最小图可证明）。共享内核 = src/focus.js（M4 TV 复用）。

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  createFocusEngine,
  handleKey,
  hasNoDeadEnds,
  reachableFrom,
  traversalSnapshot,
} from "../src/index.js";
import { EXAMPLE_FOCUS_NODES } from "../src/example-nodes.js";

const makeEngine = () => createFocusEngine({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });

test("遍历快照（确定性）：顺序即文档序，边界原地自环——快照断言钉住遍历图", () => {
  const snapshot = traversalSnapshot(makeEngine().graph);
  assert.deepEqual(snapshot, {
    counter: { up: "counter", down: "reset", left: "counter", right: "counter" },
    reset: { up: "counter", down: "reset", left: "reset", right: "reset" },
  });
  assert.deepEqual(traversalSnapshot(makeEngine().graph), snapshot, "两次构造快照须一致");
});

test("焦点顺序稳定：order 与声明序一致，初始焦点确定性", () => {
  const a = makeEngine();
  const b = makeEngine();
  assert.deepEqual([...a.order], ["counter", "reset"]);
  assert.equal(a.current, "counter");
  assert.equal(b.current, "counter");
});

test("方向键遍历：移动 / 边界原位（无死路），非法方向快速失败", () => {
  const engine = makeEngine();
  assert.equal(engine.move("down"), "reset");
  assert.equal(engine.current, "reset");
  assert.equal(engine.move("up"), "counter");
  assert.equal(engine.move("up"), "counter", "边界方向键原地不动");
  assert.equal(engine.move("left"), "counter", "无邻接方向原地不动");
  assert.throws(() => engine.move("forward"), /非法方向/);
  assert.throws(() => engine.move(undefined), /非法方向/);
});

test("Back 返回上一焦点（焦点记忆栈）；栈空返回 null 由宿主决定退出", () => {
  const engine = makeEngine();
  assert.equal(engine.back(), null, "初始无历史");
  engine.focus("reset");
  assert.equal(engine.back(), "counter");
  assert.equal(engine.current, "counter");
  engine.move("down");
  assert.equal(engine.back(), "counter", "方向键移动同样入历史");
  assert.equal(engine.back(), null);
  assert.equal(engine.current, "counter", "返回 null 时不改变当前焦点");
});

test("handleKey：方向 / 激活 / Back / 未知键路由（键盘与 d-pad 两族键名）", () => {
  const engine = makeEngine();
  let activations = 0;

  assert.deepEqual(handleKey(engine, "ArrowDown"), {
    type: "move",
    direction: "down",
    from: "counter",
    to: "reset",
  });
  assert.deepEqual(handleKey(engine, "DPAD_UP"), {
    type: "move",
    direction: "up",
    from: "reset",
    to: "counter",
  });
  assert.deepEqual(handleKey(engine, "Enter", { activate: () => (activations += 1) }), {
    type: "activate",
    id: "counter",
  });
  assert.equal(activations, 1);

  engine.move("down");
  assert.deepEqual(handleKey(engine, "Back"), { type: "back", from: "reset", to: "counter" });
  assert.equal(engine.current, "counter");
  assert.deepEqual(handleKey(engine, "KeyX"), { type: "ignored", key: "KeyX" });
});

test("无死路（最小图可证明）：任一 (节点, 方向) 落点合法且全图可达", () => {
  const engine = makeEngine();
  assert.equal(hasNoDeadEnds(engine.graph), true);
  for (const id of engine.order) {
    for (const dir of ["up", "down", "left", "right"]) {
      engine.focus(id);
      const next = engine.move(dir);
      assert.ok(engine.order.includes(next), `${id}.${dir} -> ${next} 越出遍历图`);
    }
  }
  assert.deepEqual([...reachableFrom(engine.graph, "counter")], ["counter", "reset"], "无指针环境全图可达");
});

test("构造边界校验：空图 / 重复 id / 悬空邻接 / 未知初值全部快速失败", () => {
  assert.throws(() => createFocusEngine({}), /非空数组/);
  assert.throws(() => createFocusEngine({ nodes: [] }), /非空数组/);
  assert.throws(
    () => createFocusEngine({ nodes: [{ id: "a" }, { id: "a" }] }),
    /重复/,
  );
  assert.throws(
    () => createFocusEngine({ nodes: [{ id: "a", neighbors: { down: "ghost" } }] }),
    /未知邻接/,
  );
  assert.throws(() => createFocusEngine({ nodes: [{ id: "a" }], initial: "b" }), /initial/);
  const engine = makeEngine();
  assert.throws(() => engine.focus("ghost"), /未知节点/);
});
