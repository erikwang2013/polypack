// 发射器测试：确定性 / 良构 / 转义 / 词汇封闭 / 焦点表与共享内核逐键等价（产物级核对）。

import assert from "node:assert/strict";
import { test } from "node:test";

import { EXAMPLE_FOCUS_NODES } from "../../tv/src/example-nodes.js";
import { createFocusEngine, traversalSnapshot } from "../../tv/src/focus.js";
import { emitChannelFiles, toSceneGraphColor, xmlWellFormed } from "../src/emit.js";
import { createExampleSubset, validateSubset } from "../src/subset.js";

const parseFocusTable = (brs) => {
  const line = brs.split("\n").find((l) => l.trim().startsWith("return "));
  return JSON.parse(line.trim().slice("return ".length));
};

const minimalSubset = (overrides = {}) => ({
  elements: [
    { type: "label", id: "x", translation: [0, 0], width: 10, height: 10, text: "hi", font: "font:Small", color: "#000000" },
  ],
  focus: { initial: "n1", nodes: [{ id: "n1", neighbors: {} }] },
  ...overrides,
});

test("发射件集合与确定性（两次发射逐字节相等）", () => {
  const a = emitChannelFiles(createExampleSubset());
  const b = emitChannelFiles(createExampleSubset());
  assert.deepEqual(Object.keys(a).sort(), ["components/PolypackScene.xml", "source/focusgraph.brs"]);
  assert.equal(a["components/PolypackScene.xml"], b["components/PolypackScene.xml"]);
  assert.equal(a["source/focusgraph.brs"], b["source/focusgraph.brs"]);
  assert.ok(xmlWellFormed(a["components/PolypackScene.xml"]).ok, "发射 XML 应良构");
});

test("焦点表逐键等价于共享内核快照（边界自环落点已解析）", () => {
  const { "source/focusgraph.brs": brs } = emitChannelFiles(createExampleSubset());
  const table = parseFocusTable(brs);
  const engine = createFocusEngine({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });
  assert.deepEqual(table.order, [...engine.order]);
  assert.deepEqual(table.edges, traversalSnapshot(engine.graph));
  assert.equal(table.initial, "counter");
});

test("文本/属性转义与自检扫描器（含反例）", () => {
  const subset = minimalSubset();
  subset.elements[0].text = `A & B <tag> "q"`;
  const { "components/PolypackScene.xml": xml } = emitChannelFiles(subset);
  assert.ok(xml.includes("A &amp; B &lt;tag&gt; &quot;q&quot;"));
  assert.ok(xmlWellFormed(xml).ok);
  assert.equal(xmlWellFormed("<a><b></a>").ok, false);
  assert.equal(xmlWellFormed("<a>5 & 3</a>").ok, false);
});

test("词汇封闭：未知类型 / 未知属性 / 未知邻接 / initial 缺位均抛错", () => {
  assert.throws(() => validateSubset(minimalSubset({ elements: [{ type: "webview", id: "w" }] })), /未知元素类型/);
  assert.throws(() => validateSubset(minimalSubset({ elements: [{ type: "label", id: "x", text: "t", src: "u" }] })), /不支持属性/);
  assert.throws(
    () => validateSubset(minimalSubset({ focus: { initial: "n1", nodes: [{ id: "n1", neighbors: { up: "n2" } }] } })),
    /未知邻接/,
  );
  assert.throws(() => validateSubset(minimalSubset({ focus: { initial: "nope", nodes: [{ id: "n1", neighbors: {} }] } })), /initial 不在图中/);
});

test("颜色转换：#rrggbb → 0xRRGGBBAA；非法形态抛错", () => {
  assert.equal(toSceneGraphColor("#ea580c"), "0xEA580CFF");
  assert.throws(() => toSceneGraphColor("ea580c"), /颜色须为/);
  assert.throws(() => toSceneGraphColor("#ea580c88"), /颜色须为/);
});
