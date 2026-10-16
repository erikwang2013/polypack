// T-M4-02 几何遍历图测试：rect → 方向邻居的推导裁定（主轴距 → 正交距 → 文档序）、
// focusOrder 全量覆盖、显式 neighbors 逐向覆盖、连通性静态检查器（验收：示例应用
// 0 孤立节点 / 无死路）与边界校验。
// 零依赖（node:test）；只消费 src/（不 import react 的示例组件面）。

import assert from "node:assert/strict";
import { test } from "node:test";

import { EXAMPLE_FOCUS_NODES } from "../src/example-nodes.js";
import {
  DIRECTIONS,
  checkConnectivity,
  createFocusEngine,
  createTvFocus,
  deriveFocusNodes,
  hasNoDeadEnds,
  traversalSnapshot,
} from "../src/index.js";

const neighborsOf = (nodes, id, options) =>
  deriveFocusNodes(nodes, options).find((n) => n.id === id).neighbors;

test("几何推导：2×2 网格四向邻接与遍历快照（严格投影裁定）", () => {
  const engine = createTvFocus({ nodes: EXAMPLE_FOCUS_NODES });
  assert.deepEqual(engine.graph.edges, {
    counter: { right: "reset", down: "detail" },
    reset: { left: "counter", down: "exit" },
    detail: { up: "counter", right: "exit" },
    exit: { up: "reset", left: "detail" },
  });
  // 快照含边界自环落点（无邻接方向原地）——「无死路」的构造性证据
  assert.deepEqual(traversalSnapshot(engine.graph), {
    counter: { up: "counter", down: "detail", left: "counter", right: "reset" },
    reset: { up: "reset", down: "exit", left: "counter", right: "reset" },
    detail: { up: "counter", down: "detail", left: "detail", right: "exit" },
    exit: { up: "reset", down: "exit", left: "detail", right: "exit" },
  });
});

test("四向移动冒烟：全节点 × 四方向落点恒在图内；边界原地（无死路）", () => {
  const engine = createTvFocus({ nodes: EXAMPLE_FOCUS_NODES });
  engine.focus("counter");
  assert.equal(engine.move("up"), "counter", "counter 无上方邻接 → 原地");
  assert.equal(engine.move("left"), "counter", "counter 无左方邻接 → 原地");
  engine.focus("exit");
  assert.equal(engine.move("down"), "exit");
  assert.equal(engine.move("right"), "exit");
  for (const id of engine.order) {
    engine.focus(id);
    for (const dir of DIRECTIONS) {
      assert.ok(engine.order.includes(engine.move(dir)), `${id} 按 ${dir} 落点越界`);
    }
  }
});

test("连通性检查器（T-M4-02 验收）：示例应用 0 孤立节点、无陷阱、无死路", () => {
  const engine = createTvFocus({ nodes: EXAMPLE_FOCUS_NODES });
  assert.deepEqual(checkConnectivity(engine.graph), {
    from: "counter",
    total: 4,
    reachable: ["counter", "reset", "detail", "exit"],
    isolated: [],
    traps: [],
    connected: true,
  });
  assert.ok(hasNoDeadEnds(engine.graph));
});

test("几何裁定：主轴中心距 → 正交中心距 → 文档序；贴边与同中心不算候选", () => {
  const box = (x, y) => ({ x, y, width: 100, height: 100 });
  const a = { id: "a", rect: box(0, 0) }; // 中心 (50, 50)
  const b = { id: "b", rect: box(200, 0) }; // 主轴 200 / 正交 0
  const c = { id: "c", rect: box(200, 80) }; // 主轴 200 / 正交 80（y 投影相交 20）
  const e = { id: "e", rect: box(400, 0) }; // 主轴 400 / 正交 0
  const d = { id: "d", rect: box(150, 100) }; // y 投影贴边（重叠 0）
  const right = (nodes) => neighborsOf(nodes, "a").right;

  assert.equal(right([a, b, c]), "b", "主轴平手 → 正交距小者胜");
  assert.equal(right([a, c, e]), "c", "主轴距离优先于正交距离");
  assert.equal(right([a, d]), undefined, "正交投影贴边（重叠 0）不算候选");
  assert.equal(right([a, { id: "twin", rect: box(0, 0) }]), undefined, "中心严格比较：同中心不互为邻居");
  assert.equal(
    right([a, { id: "first", rect: box(200, 0) }, { id: "dup", rect: box(200, 0) }]),
    "first",
    "主轴 / 正交双平手 → 文档序在先者胜",
  );
});

test("focusOrder 全量覆盖：输出序即给定序；几何邻接整体让位", () => {
  const derived = deriveFocusNodes(EXAMPLE_FOCUS_NODES, {
    focusOrder: ["counter", "reset", "detail", "exit"],
  });
  assert.deepEqual(derived.map((n) => n.id), ["counter", "reset", "detail", "exit"]);
  assert.deepEqual(
    derived.find((n) => n.id === "counter").neighbors,
    { right: "reset" },
    "全量覆盖：counter 不再有几何推导的 down",
  );

  // 行式布局（序与几何走向一致的代表用例）：反转序 → 前进键指向下一项
  const row = ["n1", "n2", "n3"].map((id, i) => ({
    id,
    rect: { x: i * 200, y: 0, width: 100, height: 100 },
  }));
  const reversed = deriveFocusNodes(row, { focusOrder: ["n3", "n2", "n1"] });
  assert.deepEqual(reversed.map((n) => n.id), ["n3", "n2", "n1"]);
  assert.deepEqual(
    Object.fromEntries(reversed.map((n) => [n.id, n.neighbors])),
    {
      n3: { left: "n2" },
      n2: { right: "n3", left: "n1" },
      n1: { right: "n2" },
    },
    "链沿主轴带符号：左向前进、右向回退",
  );

  assert.throws(() => deriveFocusNodes(EXAMPLE_FOCUS_NODES, { focusOrder: ["counter"] }), /排列/);
  assert.throws(
    () => deriveFocusNodes(EXAMPLE_FOCUS_NODES, { focusOrder: ["counter", "reset", "detail", "nope"] }),
    /排列/,
  );
  assert.throws(
    () => deriveFocusNodes(EXAMPLE_FOCUS_NODES, { focusOrder: ["counter", "counter", "detail", "exit"] }),
    /排列/,
  );
});

test("显式 neighbors 逐向覆盖：只替换给定方向，几何推导其余保留", () => {
  const tuned = EXAMPLE_FOCUS_NODES.map((n) =>
    n.id === "counter" ? { ...n, neighbors: { left: "exit" } } : n,
  );
  assert.deepEqual(neighborsOf(tuned, "counter"), {
    left: "exit",
    right: "reset",
    down: "detail",
  });
  assert.throws(
    () => deriveFocusNodes([{ ...tuned[0], neighbors: { left: "nope" } }, ...tuned.slice(1)]),
    /未知邻接/,
  );
});

test("推导边界校验与冻结：非法输入快速失败；产物冻结且确定性", () => {
  const rect = { x: 0, y: 0, width: 100, height: 100 };
  assert.throws(() => deriveFocusNodes([]), /非空数组/);
  assert.throws(() => deriveFocusNodes([{ rect }]), /非空 id/);
  assert.throws(() => deriveFocusNodes([{ id: "a", rect }, { id: "a", rect }]), /重复/);
  assert.throws(() => deriveFocusNodes([{ id: "a" }]), /rect/);
  assert.throws(() => deriveFocusNodes([{ id: "a", rect: { ...rect, x: NaN } }]), /rect/);
  assert.throws(() => deriveFocusNodes([{ id: "a", rect: { ...rect, width: 0 } }]), /正数/);

  const first = deriveFocusNodes(EXAMPLE_FOCUS_NODES);
  assert.ok(Object.isFrozen(first));
  assert.ok(first.every((n) => Object.isFrozen(n) && Object.isFrozen(n.neighbors)));
  assert.deepEqual(first, deriveFocusNodes(EXAMPLE_FOCUS_NODES), "两次推导相等（确定性）");
});

test("检查器判定面：孤立 / 陷阱 / 单节点 / from 覆盖与校验", () => {
  // 孤立：几何上与网格无任何投影相交 → 不可达（示例应用 0 孤立；此为判定面反例）
  const orphan = { id: "orphan", rect: { x: 1400, y: 0, width: 100, height: 100 } };
  const withOrphan = createTvFocus({ nodes: [...EXAMPLE_FOCUS_NODES, orphan] });
  const report = checkConnectivity(withOrphan.graph);
  assert.deepEqual(report.isolated, ["orphan"]);
  assert.deepEqual(report.traps, ["orphan"], "四向全无出边 → 陷阱");
  assert.equal(report.connected, false);

  // 陷阱（可达但出不去）：手工邻接到焦点内核
  const trap = createFocusEngine({ nodes: [{ id: "a", neighbors: { right: "b" } }, { id: "b" }] });
  assert.deepEqual(checkConnectivity(trap.graph), {
    from: "a",
    total: 2,
    reachable: ["a", "b"],
    isolated: [],
    traps: ["b"],
    connected: true,
  });

  // 单节点：不判陷阱（无「出不去」语义）
  assert.deepEqual(checkConnectivity(createFocusEngine({ nodes: [{ id: "solo" }] }).graph).traps, []);

  assert.equal(checkConnectivity(withOrphan.graph, { from: "exit" }).from, "exit");
  assert.throws(() => checkConnectivity(withOrphan.graph, { from: "nope" }), /未知起点/);
  assert.throws(() => checkConnectivity(null), /内核遍历图/);
  assert.throws(() => checkConnectivity({}), /内核遍历图/);
});
