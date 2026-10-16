// T-M3-02 组件映射测试（零依赖）：宿主专属 prop 剔除表、mapProps 纯函数、bindPrimitives 装配边界。
// createElement 以桩注入（同 runtime / 原语注射家规），不要求安装 react。

import assert from "node:assert/strict";
import { test } from "node:test";

import { CAPABILITY_MATRIX, HOST_PROP_OMISSIONS, bindPrimitives, mapProps } from "../src/index.js";

test("剔除表边界：每个键都是 capabilities 矩阵内的已知目标（防 'harmony' 之类笔误目标）", () => {
  const targets = Object.keys(HOST_PROP_OMISSIONS);
  assert.ok(targets.length >= 3);
  for (const target of targets) {
    assert.ok(
      Object.hasOwn(CAPABILITY_MATRIX, target),
      `剔除表目标 "${target}" 不在 capabilities 矩阵（目标 id 与 builtin.rs 口径一致）`,
    );
  }
  // 鸿蒙（T-M3-02 承载分支）：RNOH 无 Android TV 原生焦点锚点对应
  assert.deepEqual(HOST_PROP_OMISSIONS.harmonyos, ["hasTVPreferredFocus"]);
  assert.deepEqual(HOST_PROP_OMISSIONS.android, []);
  assert.deepEqual(HOST_PROP_OMISSIONS.ios, []);
});

test("mapProps()：无差异宿主恒等（同一引用）；鸿蒙只剔锚点，其余 prop 逐个保真", () => {
  const props = { style: { padding: 24 }, focusable: true, hasTVPreferredFocus: true, testID: "root" };
  assert.equal(mapProps("android", props), props, "空剔除表应返回原对象（不留包裹痕迹）");

  const mapped = mapProps("harmonyos", props);
  assert.ok(!("hasTVPreferredFocus" in mapped), "RNOH 无对应 prop，应剔除");
  assert.deepEqual(mapped, { style: props.style, focusable: true, testID: "root" });
  assert.equal(mapped.style, props.style, "其余 prop 保真（引用级）");
  assert.deepEqual(mapProps("harmonyos"), {}, "缺省 props 合法");
});

test("mapProps() / bindPrimitives()：未知目标快速失败（同 capabilities 口径）", () => {
  assert.throws(() => mapProps("harmony", {}), /未知目标/);
  assert.throws(() => mapProps("symbian", {}), /未知目标/);
  assert.throws(() => bindPrimitives({}, "harmony"), /未知目标/);
});

test("bindPrimitives()：无差异宿主返回原对象（Android 宿主零改动的装配保证）", () => {
  const primitives = { View: () => null, iconSource: null };
  assert.equal(bindPrimitives(primitives, "android", () => null), primitives);
});

test("bindPrimitives()：鸿蒙分支包裹组件值、透传非组件值，prop 剔除落在装配边界", () => {
  const calls = [];
  const createElement = (type, props) => {
    calls.push({ type, props });
    return { type, props };
  };
  const View = () => null;
  const primitives = { View, Text: function Text() {}, iconSource: "./icon.svg", asset: 42 };
  const bound = bindPrimitives(primitives, "harmonyos", createElement);

  assert.notEqual(bound.View, View, "组件值应被包裹");
  assert.equal(bound.iconSource, "./icon.svg", "非组件值（资产）原样透传");
  assert.equal(bound.asset, 42, "非组件值（数值）原样透传");

  // 页面根 View 的真实 props 形态（src/example.js）：锚点被剔除，其余原样进宿主原语
  bound.View({ style: { maxWidth: 420 }, focusable: true, hasTVPreferredFocus: true });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].type, View, "包裹层应把元素交回首传原语");
  assert.deepEqual(calls[0].props, { style: { maxWidth: 420 }, focusable: true });
});

test("bindPrimitives()：需注入 createElement（鸿蒙分支才校验）；返回对象冻结", () => {
  const primitives = { View: () => null };
  assert.throws(() => bindPrimitives(primitives, "harmonyos"), /createElement/);
  assert.ok(Object.isFrozen(bindPrimitives(primitives, "harmonyos", () => null)));
});
