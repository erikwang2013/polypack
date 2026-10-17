// T-M7-01 页面树验收：同一 createExamplePage 产物在圆 / 方两种 Wear 形态下结构一致；
// capabilities 缺失能力时交互按契约降级（无指针 → 焦点环随共享内核移动；无手势 → 显式退出
// 入口；安全区失活 → 零内边距）。零依赖：页面树是纯描述对象，不需要 react / node_modules。

import assert from "node:assert/strict";
import { test } from "node:test";

import { createAdapter, createExamplePage } from "../src/index.js";

const stubRuntime = { AppRegistry: { registerComponent() {} } };
const make = (options = {}) =>
  createAdapter({ target: "wearos", runtime: stubRuntime, ...options });

const flatten = (node, out = []) => {
  out.push(node);
  for (const child of node.children ?? []) flatten(child, out);
  return out;
};
const byId = (tree, id) => flatten(tree).find((node) => node.testID === id);
const ids = (tree) => flatten(tree).map((node) => node.testID).filter(Boolean);
const focused = (tree) => flatten(tree).filter((node) => node.focus === true).map((node) => node.testID);
const ACCENT = "#0e7490"; // focusRing.borderColor（tokens.color.accent）

test("同一页面组件树：圆 / 方两形态结构一致，安全区与卡片宽按形态派生", () => {
  const round = createExamplePage(make({ form: "round" }));
  const square = createExamplePage(make({ form: "square" }));
  assert.deepEqual(ids(round.view()), ids(square.view()), "同一树：节点 id 序列一致");
  assert.deepEqual(ids(round.view()), [
    "wear-frame",
    "wear-card",
    "counter",
    "reset",
    "detail",
    "exit",
  ]);

  const roundFrame = byId(round.view(), "wear-frame");
  const squareFrame = byId(square.view(), "wear-frame");
  assert.equal(roundFrame.style.paddingTop, "14.65%");
  assert.equal(squareFrame.style.paddingTop, "8%");
  assert.equal(byId(round.view(), "wear-card").style.maxWidth, 320);
  assert.equal(byId(square.view(), "wear-card").style.maxWidth, 268);

  assert.deepEqual(round.getState(), {
    focus: "counter",
    clicks: 0,
    detail: false,
    exitRequested: false,
  });
  assert.throws(() => createExamplePage(null), /createAdapter 产物/);
});

test("触摸基线（focusNav false）：不渲染焦点环（触摸按压即交互）", () => {
  const page = createExamplePage(make());
  assert.deepEqual(focused(page.view()), []);
  page.onKey("ArrowRight");
  assert.deepEqual(focused(page.view()), [], "焦点引擎可移动但触摸基线不渲染环");
});

test("无指针降级（pointer=none）：焦点环出现且随共享内核四向移动（零布局抖动）", () => {
  const page = createExamplePage(make({ host: { pointer: "none" } }));
  let tree = page.view();
  assert.deepEqual(focused(tree), ["counter"], "初始焦点环 = counter");
  assert.equal(byId(tree, "counter").style.borderColor, ACCENT);
  assert.equal(byId(tree, "reset").style.borderColor, "transparent");
  assert.equal(flatten(tree).filter((n) => n.style?.borderColor === ACCENT).length, 1);

  page.onKey("ArrowRight");
  tree = page.view();
  assert.deepEqual(focused(tree), ["reset"]);
  assert.equal(byId(tree, "counter").style.borderColor, "transparent");
  assert.equal(byId(tree, "reset").style.borderColor, ACCENT);
  assert.equal(flatten(tree).filter((n) => n.style?.borderColor === ACCENT).length, 1, "旧位置无残留");

  // 边界方向原地且环不灭（共享内核无死路语义）
  assert.deepEqual(page.onKey("ArrowRight"), {
    type: "move",
    direction: "right",
    from: "reset",
    to: "reset",
  });
  assert.deepEqual(focused(page.view()), ["reset"]);
});

test("交互路径：OK 激活推进计数 / 重置 / 详情切换 / 退出请求（无指针通路）", () => {
  const page = createExamplePage(make({ host: { pointer: "none" } }));
  assert.deepEqual(page.onKey("Enter"), { type: "activate", id: "counter" });
  assert.equal(page.getState().clicks, 1, "计数按钮推进");
  assert.equal(byId(page.view(), "counter").label, "计数：1");

  page.onKey("ArrowRight");
  assert.deepEqual(page.onKey("Enter"), { type: "activate", id: "reset" });
  assert.equal(page.getState().clicks, 0, "重置清零");

  page.onKey("ArrowLeft"); // reset → counter
  page.onKey("ArrowDown"); // counter → detail
  assert.deepEqual(page.onKey("Enter"), { type: "activate", id: "detail" });
  assert.equal(page.getState().detail, true, "详情切换");
  assert.ok(
    flatten(page.view()).some((n) => n.text === "详情：Wear 演示页"),
    "提示行反映详情状态",
  );

  page.onKey("ArrowRight"); // detail → exit
  assert.deepEqual(page.onKey("Enter"), { type: "activate", id: "exit" });
  assert.equal(page.getState().exitRequested, true, "显式退出入口（back-affordance）");
  assert.ok(flatten(page.view()).some((n) => n.text === "已请求退出（宿主接管）"));
});

test("Back 三层：焦点记忆逐级回退，历史空才请求退出（宿主裁决）", () => {
  const page = createExamplePage(make());
  assert.deepEqual(page.onKey("ArrowDown"), {
    type: "move",
    direction: "down",
    from: "counter",
    to: "detail",
  });
  assert.deepEqual(page.onKey("ArrowRight"), {
    type: "move",
    direction: "right",
    from: "detail",
    to: "exit",
  });
  assert.deepEqual(page.onKey("Back"), { type: "back", from: "exit", to: "detail" });
  assert.equal(page.getState().focus, "detail");
  assert.deepEqual(page.onKey("Back"), { type: "back", from: "detail", to: "counter" });
  assert.deepEqual(page.onKey("Back"), { type: "back", from: "counter", to: null });
  assert.equal(page.getState().exitRequested, true, "第三层：宿主接管退出");
});

test("capabilities 缺失能力：degradations 分支与页面渲染同步降级", () => {
  // 基线：触摸常态，只需安全区
  assert.deepEqual(
    make().degradations().filter((b) => b.active).map((b) => b.id),
    ["safe-area-padding"],
  );

  // 无返回手势 → 显式退出入口（页面常驻 exit 按钮）
  const noGestures = make({ host: { gestures: false } });
  assert.deepEqual(noGestures.capabilities(), {
    focusNav: false,
    safeArea: true,
    gestures: false,
  });
  assert.deepEqual(
    noGestures.degradations().filter((b) => b.active).map((b) => b.id),
    ["safe-area-padding", "back-affordance"],
  );
  assert.ok(byId(createExamplePage(noGestures).view(), "exit"), "back-affordance 入口存在");

  // 三项全缺 → 焦点环 + 安全区 + 显式退出
  const bare = make({ host: { pointer: "none", gestures: false } });
  assert.deepEqual(
    bare.degradations().filter((b) => b.active).map((b) => b.id),
    ["focus-ring", "safe-area-padding", "back-affordance"],
  );
  assert.deepEqual(focused(createExamplePage(bare).view()), ["counter"]);

  // 安全区失活（调试形态）→ 外框零内边距
  const debug = make({ host: { safeArea: false } });
  assert.deepEqual(debug.degradations().filter((b) => b.active).map((b) => b.id), []);
  assert.deepEqual(byId(createExamplePage(debug).view(), "wear-frame").style, {
    paddingTop: 0,
    paddingRight: 0,
    paddingBottom: 0,
    paddingLeft: 0,
  });
});
