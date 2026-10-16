// T-M4-03 契约测试：遥控器数字键快捷映射——四键族归一、构造期校验、
// 宿主域路由（描述符 from/to）与层打开时的拦截（不穿透弹层）。
// 零依赖（node:test）。

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  createLayerStack,
  createShortcutMap,
  createTvFocus,
  handleTvKey,
  normalizeShortcutKey,
} from "../src/index.js";
import { EXAMPLE_FOCUS_NODES, TV_MODAL_FOCUS_NODES } from "../src/example-nodes.js";

test("归一：四键族（键盘 / Digit / Numpad / web 外壳 KEY_）→ 数字；其余 → null", () => {
  assert.equal(normalizeShortcutKey("3"), "3");
  assert.equal(normalizeShortcutKey("Digit5"), "5");
  assert.equal(normalizeShortcutKey("Numpad9"), "9");
  assert.equal(normalizeShortcutKey("KEY_1"), "1");
  for (const bad of ["0", "10", "Digit0", "KEY_10", "Numpad", "VolumeUp", "Back", "", "３"]) {
    assert.equal(normalizeShortcutKey(bad), null, bad);
  }
  assert.equal(normalizeShortcutKey(undefined), null);
  assert.equal(normalizeShortcutKey(3), null, "非字符串不归一");
});

test("createShortcutMap：键归一化收敛 + 构造期校验 + 冻结", () => {
  const map = createShortcutMap({ 1: "counter", Digit3: "detail" });
  assert.deepEqual(map, { 1: "counter", 3: "detail" }, "Digit3 归一为 3");
  assert.ok(Object.isFrozen(map));
  assert.throws(() => createShortcutMap({ 0: "counter" }), /数字键/);
  assert.throws(() => createShortcutMap({ Home: "counter" }), /数字键/);
  assert.throws(() => createShortcutMap({ 1: "" }), /非空节点 id/);
  assert.throws(() => createShortcutMap({ 1: "counter", Digit1: "reset" }), /重复/);
  assert.throws(() => createShortcutMap(null), /映射须为对象/);
});

test("路由：宿主域数字键直达焦点（from/to 描述符）；未映射数字不触发", () => {
  const engine = createTvFocus({ nodes: EXAMPLE_FOCUS_NODES });
  const layers = createLayerStack();
  const session = { engine, layers, shortcuts: createShortcutMap({ 1: "counter", 2: "reset", 3: "detail" }) };

  assert.deepEqual(handleTvKey(session, "3"), { type: "shortcut", digit: "3", from: "counter", to: "detail" });
  assert.equal(engine.current, "detail", "快捷即聚焦直达");
  assert.deepEqual(handleTvKey(session, "Numpad2"), { type: "shortcut", digit: "2", from: "detail", to: "reset" });
  assert.deepEqual(handleTvKey(session, "9"), { type: "ignored", key: "9" }, "未映射数字落回内核路由");
  assert.equal(engine.current, "reset", "未映射数字不移动焦点");
});

test("层打开：数字键被拦截（blocked，不穿透弹层；两域焦点皆不动）", () => {
  const engine = createTvFocus({ nodes: EXAMPLE_FOCUS_NODES });
  const layers = createLayerStack();
  const session = { engine, layers, shortcuts: createShortcutMap({ 1: "counter", 3: "detail" }) };

  engine.focus("detail");
  layers.open({ id: "detail", nodes: TV_MODAL_FOCUS_NODES, initial: "modal-play" });
  assert.deepEqual(handleTvKey(session, "1"), { type: "shortcut", digit: "1", blocked: true });
  assert.equal(engine.current, "detail", "宿主域焦点不动");
  assert.equal(layers.current.current, "modal-play", "层内焦点不动");

  // 层关闭后数字键恢复
  layers.close();
  assert.deepEqual(handleTvKey(session, "1"), { type: "shortcut", digit: "1", from: "detail", to: "counter" });
});

test("快捷配置错误快速失败：目标不在宿主域（含层内节点——域隔离）", () => {
  const engine = createTvFocus({ nodes: EXAMPLE_FOCUS_NODES });
  assert.throws(
    () => handleTvKey({ engine, shortcuts: createShortcutMap({ 1: "nope" }) }, "1"),
    /不在宿主域/,
  );
  assert.throws(
    () => handleTvKey({ engine, shortcuts: createShortcutMap({ 1: "modal-play" }) }, "1"),
    /不在宿主域/,
  );
});
