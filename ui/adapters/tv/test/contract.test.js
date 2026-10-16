// T-M4-01 契约测试：RendererAdapter 对象形状、react-dom 挂载装配、边界校验、安全区。
// 零依赖（node:test）；runtime 用桩注入，不要求安装 react / react-dom。

import assert from "node:assert/strict";
import { test } from "node:test";

import { CAPABILITY_KEYS, CAPABILITY_MATRIX, createAdapter, tokens } from "../src/index.js";

/** react-dom 运行面桩：记录 createRoot / render 调用，不要求真装 react。 */
const stubRuntime = (calls = []) => ({
  React: { createElement: (app) => ({ element: app }) },
  ReactDOM: {
    createRoot(root) {
      calls.push({ method: "createRoot", root });
      return { render: (node) => calls.push({ method: "render", node }) };
    },
  },
});

const makeAdapter = (options = {}) =>
  createAdapter({ target: "tizen-tv", runtime: stubRuntime(), ...options });

test("契约对象形状：mount / platformStyle / capabilities 三件套齐备且不可变", () => {
  const adapter = makeAdapter();
  assert.equal(adapter.target, "tizen-tv");
  for (const name of ["mount", "platformStyle", "capabilities", "degradations", "safeArea"]) {
    assert.equal(typeof adapter[name], "function", `${name} 应为函数`);
  }
  assert.ok(Object.isFrozen(adapter), "契约对象应冻结（宿主不得改写装配层）");
});

test("target → 电视目标面：矩阵四目标齐备且均可装配", () => {
  for (const target of Object.keys(CAPABILITY_MATRIX)) {
    assert.equal(makeAdapter({ target }).target, target, target);
  }
  assert.deepEqual(
    Object.keys(CAPABILITY_MATRIX).sort(),
    ["smartcast-tv", "tizen-tv", "vidaa-tv", "webos-tv"],
    "电视目标面（react-dom 承载；poly-targets 登记落位后由 capabilities 哨兵收口）",
  );
});

test("capabilities()：恰好契约三键；TV 基线 focusNav/safeArea 恒真、gestures 恒假", () => {
  const caps = makeAdapter().capabilities();
  assert.deepEqual(Object.keys(caps), [...CAPABILITY_KEYS]);
  for (const key of CAPABILITY_KEYS) {
    assert.equal(typeof caps[key], "boolean", `${key} 应为布尔`);
  }
  assert.deepEqual(caps, { focusNav: true, safeArea: true, gestures: false });
});

test("platformStyle()：默认与传参两条路径都返回 10-foot 蒙皮样式", () => {
  const adapter = makeAdapter();
  const skinned = adapter.platformStyle();
  assert.equal(skinned.title.fontSize, tokens.font.body * 1.5, "远距字号由 token 派生");
  assert.equal(typeof skinned.card.padding, "number");
  assert.equal(typeof skinned.focusRing.borderColor, "string");
  const custom = adapter.platformStyle({
    ...skinned,
    color: { ...skinned.color, brand: "#123456" },
  });
  assert.equal(custom.button.backgroundColor, "#123456", "platformStyle(tokens) 应消费传入 token 集");
});

test("safeArea()：电视安全区像素内边距（默认 1080p；宿主可传视口）", () => {
  const adapter = makeAdapter();
  assert.deepEqual(adapter.safeArea(), { top: 54, right: 96, bottom: 54, left: 96 });
  assert.deepEqual(adapter.safeArea({ width: 1280, height: 720 }), {
    top: 36,
    right: 64,
    bottom: 36,
    left: 64,
  });
  assert.throws(() => adapter.safeArea({ width: 0, height: 720 }), /width/);
  assert.throws(() => adapter.safeArea({ width: 1920, height: "1080" }), /height/);
});

test("mount(root, app)：react-dom 装配 = createRoot(root).render(<app/>)；mounted 可观测", () => {
  const calls = [];
  const adapter = makeAdapter({ runtime: stubRuntime(calls) });
  const root = { nodeType: 1, id: "root" };
  const App = () => null;

  const mounted = adapter.mount(root, App);
  assert.deepEqual(calls, [
    { method: "createRoot", root },
    { method: "render", node: { element: App } },
  ]);
  assert.equal(mounted.root, root);
  assert.equal(mounted.app, App);
  assert.equal(adapter.mounted, mounted);
  assert.ok(Object.isFrozen(mounted));
  assert.equal(typeof mounted.handle.render, "function");
});

test("装配边界校验：未知目标 / 缺 runtime / 非法 root / 非法 app 快速失败", () => {
  assert.throws(() => createAdapter({ target: "symbian", runtime: stubRuntime() }), /未知目标/);
  assert.throws(() => createAdapter({ target: "tizen-tv" }), /createRoot/);
  assert.throws(
    () => createAdapter({ target: "tizen-tv", runtime: { ReactDOM: { createRoot() {} } } }),
    /createElement/,
  );
  const adapter = makeAdapter();
  assert.throws(() => adapter.mount(null, () => null), /root/);
  assert.throws(() => adapter.mount({}, null), /app 组件/);
});
