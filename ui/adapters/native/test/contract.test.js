// T-M2-04 契约测试：RendererAdapter 对象形状、mount 装配、原生模块注册位。
// 零依赖（node:test）；runtime 用桩注入，不要求安装 react-native。

import assert from "node:assert/strict";
import { test } from "node:test";

import { CAPABILITY_KEYS, createAdapter } from "../src/index.js";

const stubRuntime = (registered = []) => ({
  registered,
  AppRegistry: {
    registerComponent(appKey, factory) {
      registered.push({ appKey, factory });
    },
  },
});

const makeAdapter = (options = {}) =>
  createAdapter({ target: "android", runtime: stubRuntime(), ...options });

test("契约对象形状：mount / platformStyle / capabilities 三件套齐备且不可变", () => {
  const adapter = makeAdapter();
  assert.equal(adapter.target, "android");
  for (const name of ["mount", "platformStyle", "capabilities", "degradations"]) {
    assert.equal(typeof adapter[name], "function", `${name} 应为函数`);
  }
  assert.equal(typeof adapter.native.register, "function");
  assert.ok(Object.isFrozen(adapter), "契约对象应冻结（宿主不得改写装配层）");
});

test("capabilities()：恰好契约三键，值为布尔", () => {
  const caps = makeAdapter().capabilities();
  assert.deepEqual(Object.keys(caps), [...CAPABILITY_KEYS]);
  for (const key of CAPABILITY_KEYS) {
    assert.equal(typeof caps[key], "boolean", `${key} 应为布尔`);
  }
});

test("platformStyle()：默认与传参两条路径都返回蒙皮样式", () => {
  const adapter = makeAdapter();
  const skinned = adapter.platformStyle();
  assert.equal(typeof skinned.card.padding, "number");
  assert.equal(typeof skinned.focusRing.borderColor, "string");
  const custom = adapter.platformStyle(
    { ...skinned, color: { ...skinned.color, brand: "#123456" } },
    { compact: true },
  );
  assert.equal(custom.button.backgroundColor, "#123456", "platformStyle(tokens) 应消费传入 token 集");
  assert.equal(custom.space[4], 18, "compact 间距紧凑化应生效");
});

test("mount(root, app)：AppRegistry 注册工厂返回同一组件；mounted 可观测", () => {
  const registered = [];
  const adapter = makeAdapter({ runtime: stubRuntime(registered) });
  const App = () => null;

  const mounted = adapter.mount({ appKey: "PaxApp" }, App);
  assert.equal(registered.length, 1);
  assert.equal(registered[0].appKey, "PaxApp");
  assert.equal(registered[0].factory(), App, "注册工厂应返回装配的组件");
  assert.equal(mounted.appKey, "PaxApp");
  assert.equal(adapter.mounted, mounted);
  assert.ok(Object.isFrozen(mounted));
});

test("装配边界校验：未知目标 / 缺 runtime / 非法 root / 非法 app 快速失败", () => {
  assert.throws(() => createAdapter({ target: "symbian", runtime: stubRuntime() }), /未知目标/);
  assert.throws(() => createAdapter({ target: "android" }), /runtime/);
  assert.throws(() => createAdapter({ target: "android", runtime: {} }), /AppRegistry/);
  const adapter = makeAdapter();
  assert.throws(() => adapter.mount({}, () => null), /appKey/);
  assert.throws(() => adapter.mount({ appKey: "" }, () => null), /appKey/);
  assert.throws(() => adapter.mount({ appKey: "PaxApp" }, null), /app 组件/);
});

test("原生模块注册位：默认空，登记描述符后校验生效（T-M2-02 挂载点）", () => {
  const adapter = makeAdapter();
  assert.equal(adapter.native.has(), false);
  assert.equal(adapter.native.get(), null);
  assert.throws(() => adapter.native.register({ artifact: "", entryClass: "a.B" }), /artifact/);
  assert.throws(() => adapter.native.register({ artifact: "libs/core.aar" }), /entryClass/);

  const descriptor = adapter.native.register({
    artifact: "libs/polypack-core.aar",
    entryClass: "org.polypack.core.CoreBridge",
  });
  assert.deepEqual(descriptor, {
    artifact: "libs/polypack-core.aar",
    entryClass: "org.polypack.core.CoreBridge",
  });
  assert.equal(adapter.native.has(), true);
  assert.ok(Object.isFrozen(descriptor), "描述符应冻结（登记后不可被宿主改写）");
});
