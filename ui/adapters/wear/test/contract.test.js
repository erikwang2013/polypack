// T-M7-01 契约测试：RendererAdapter 对象形状、RN 挂载装配、形态 / 能力边界校验、安全区。
// 零依赖（node:test）；runtime 用桩注入，不要求安装 react-native。

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  CAPABILITY_KEYS,
  CAPABILITY_MATRIX,
  WEAR_FORMS,
  createAdapter,
  tokensByForm,
} from "../src/index.js";

/** RN 运行面桩：记录 registerComponent 调用，不要求真装 react-native。 */
const stubRuntime = (calls = []) => ({
  AppRegistry: {
    registerComponent(appKey, factory) {
      calls.push({ method: "registerComponent", appKey, factory });
    },
  },
});

const makeAdapter = (options = {}) =>
  createAdapter({ target: "wearos", runtime: stubRuntime(), ...options });

test("契约对象形状：mount / platformStyle / capabilities 三件套齐备且不可变", () => {
  const adapter = makeAdapter();
  assert.equal(adapter.target, "wearos");
  assert.equal(adapter.form, "round");
  for (const name of ["mount", "platformStyle", "capabilities", "degradations", "safeArea"]) {
    assert.equal(typeof adapter[name], "function", `${name} 应为函数`);
  }
  assert.ok(Object.isFrozen(adapter), "契约对象应冻结（宿主不得改写装配层）");
});

test("target / form：wearos 两种形态均可装配；未知 target / 形态快速失败", () => {
  assert.deepEqual(Object.keys(CAPABILITY_MATRIX), ["wearos"]);
  for (const form of Object.keys(WEAR_FORMS)) {
    const adapter = makeAdapter({ form });
    assert.equal(adapter.form, form);
    assert.equal(adapter.tokens, tokensByForm[form], "tokens getter = 当前形态 token 集");
  }
  assert.throws(() => createAdapter({ target: "symbian", runtime: stubRuntime() }), /未知目标/);
  assert.throws(() => makeAdapter({ form: "oval" }), /未知形态/);
});

test("capabilities()：恰好契约三键；Wear 基线 = 触摸常态", () => {
  const caps = makeAdapter().capabilities();
  assert.deepEqual(Object.keys(caps), [...CAPABILITY_KEYS]);
  for (const key of CAPABILITY_KEYS) {
    assert.equal(typeof caps[key], "boolean", `${key} 应为布尔`);
  }
  assert.deepEqual(caps, { focusNav: false, safeArea: true, gestures: true });
});

test("platformStyle()：默认取当前形态蒙皮；自定义 token 集直通", () => {
  const round = makeAdapter({ form: "round" }).platformStyle();
  const square = makeAdapter({ form: "square" }).platformStyle();
  assert.equal(round.card.maxWidth, 320);
  assert.equal(square.card.maxWidth, 268);
  assert.equal(typeof round.title.fontSize, "number");

  const custom = makeAdapter().platformStyle({
    ...round,
    color: { ...round.color, brand: "#123456" },
  });
  assert.equal(custom.button.backgroundColor, "#123456", "platformStyle(tokens) 应消费传入 token 集");
});

test("safeArea()：当前形态安全区像素内边距（默认参考视口；宿主可传视口）", () => {
  assert.deepEqual(makeAdapter({ form: "round" }).safeArea(), {
    top: 67,
    right: 67,
    bottom: 67,
    left: 67,
  });
  assert.deepEqual(makeAdapter({ form: "square" }).safeArea(), {
    top: 26,
    right: 26,
    bottom: 26,
    left: 26,
  });
  assert.deepEqual(makeAdapter({ form: "round" }).safeArea({ width: 454, height: 908 }), {
    top: 133,
    right: 67,
    bottom: 133,
    left: 67,
  });
  assert.throws(() => makeAdapter().safeArea({ width: 0, height: 454 }), /width/);
  assert.throws(() => makeAdapter().safeArea({ width: 454, height: "454" }), /height/);
});

test("mount(root, app)：RN 装配 = AppRegistry.registerComponent；mounted 可观测", () => {
  const calls = [];
  const adapter = makeAdapter({ runtime: stubRuntime(calls) });
  const App = () => null;

  const mounted = adapter.mount({ appKey: "wear" }, App);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].method, "registerComponent");
  assert.equal(calls[0].appKey, "wear");
  assert.equal(calls[0].factory(), App, "注册工厂返回 app 组件");
  assert.equal(mounted.appKey, "wear");
  assert.equal(mounted.app, App);
  assert.equal(adapter.mounted, mounted);
  assert.ok(Object.isFrozen(mounted));
});

test("装配边界校验：缺 runtime / 非法 root / 非法 app 快速失败", () => {
  assert.throws(() => createAdapter({ target: "wearos" }), /registerComponent/);
  const adapter = makeAdapter();
  assert.throws(() => adapter.mount(null, () => null), /appKey/);
  assert.throws(() => adapter.mount({ appKey: "" }, () => null), /appKey/);
  assert.throws(() => adapter.mount({ appKey: "wear" }, null), /app 组件/);
});
