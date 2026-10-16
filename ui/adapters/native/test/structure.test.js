// T-M3-02 结构快照比对（验收 ①）：同一示例页面组件树在 Android RN 与 RNOH 两宿主渲染一致。
// 两层证据：
//   ① 标记层：react-dom/server 代理渲染 HTML 逐字节比对（Taro 四目标同构同款形态）；
//   ② 结构层：记录型原语捕获组件序/嵌套/props——证明组件映射只剔宿主专属 prop（负向对照：Android 保留锚点）。
// react/react-dom 未安装（CI 无 npm install）时 skip 不 fail。
// 证据边界（如实）：两宿主均为 JS 层代理证据；RNOH 真机渲染阻塞（无 HarmonyOS 设备/模拟器，hdc 无目标），
// 设备腿与命令清单见 harmony-host-structure.test.js 末项与 host/harmony/README「阻塞」。

import assert from "node:assert/strict";
import { test } from "node:test";

// 只吞「react/react-dom 未安装」一类失败；本包加载错误照常炸出（同 render.test.js 的防假绿约束）
let mods = null;
try {
  mods = {
    react: await import("react"),
    server: await import("react-dom/server"),
    dom: await import("./helpers/dom-primitives.js"),
    adapter: await import("../src/index.js"),
    example: await import("../example.js"),
  };
} catch (error) {
  const reactMissing =
    error?.code === "ERR_MODULE_NOT_FOUND" && /Cannot find package 'react(-dom)?'/.test(error.message);
  if (!reactMissing) throw error;
}
const skip = mods === null ? "react/react-dom 未安装（npm install 后启用两宿主结构快照比对）" : false;

const stubRuntime = { AppRegistry: { registerComponent() {} } };

/** 宿主装配：android 原语原样（剔除表为空 → bindPrimitives 恒等）；harmonyos 经组件映射包裹。 */
function profile(target, hostHints = { pointer: "none" }) {
  const adapter = mods.adapter.createAdapter({ target, runtime: stubRuntime, host: hostHints });
  const primitives = mods.adapter.bindPrimitives(mods.dom.createDomPrimitives(mods.react), target, mods.react.createElement);
  const app = mods.example.createExampleApp(primitives, adapter);
  return { adapter, ...app };
}

const renderHtml = (target, hostHints) =>
  mods.server.renderToStaticMarkup(mods.react.createElement(profile(target, hostHints).App));

/** 记录型原语：渲染照常下钻（返回宿主标签承载 children），逐节点捕获 {type, props}。 */
function createRecorder(react) {
  const { createElement } = react;
  const nodes = [];
  const record = (type) => (props = {}) => {
    nodes.push({ type, props });
    return createElement(`x-${type.toLowerCase()}`, null, props.children);
  };
  const primitives = {
    View: record("View"),
    Text: record("Text"),
    Pressable: record("Pressable"),
    Image: (props = {}) => {
      nodes.push({ type: "Image", props });
      return null;
    },
    iconSource: null,
  };
  return { primitives, nodes };
}

/** 捕获一次渲染经过页面原语边界的全量节点（两宿主同序：文档序）。 */
function capture(target, hostHints = { pointer: "none" }) {
  const adapter = mods.adapter.createAdapter({ target, runtime: stubRuntime, host: hostHints });
  const { primitives, nodes } = createRecorder(mods.react);
  const bound = mods.adapter.bindPrimitives(primitives, target, mods.react.createElement);
  const { App } = mods.example.createExampleApp(bound, adapter);
  mods.server.renderToStaticMarkup(mods.react.createElement(App));
  return nodes;
}

/** 结构快照（比对口径）：组件序/嵌套（记录序即文档序）、锚点、样式、文本；不含宿主专属 prop。 */
const snapshot = (nodes) =>
  nodes.map(({ type, props }) => ({
    type,
    testID: props.testID ?? null,
    focusable: props.focusable ?? null,
    style: JSON.stringify(props.style ?? null),
    text: [props.children]
      .flat(Infinity)
      .filter((c) => typeof c === "string" || typeof c === "number")
      .join(""),
  }));

test("结构快照①（标记层）：同一页面在两宿主代理渲染逐字节一致，降级分支同款", { skip }, () => {
  const android = renderHtml("android");
  const harmonyos = renderHtml("harmonyos");
  assert.equal(harmonyos, android, "两宿主 HTML 应逐字节一致（页面组件树零改动）");

  for (const marker of ["PolyPack App", "版本 0.1.0 · 由 PolyPack 构建", "点击计数：0", "重置"]) {
    assert.ok(android.includes(marker), `代理渲染缺语义标记: ${marker}`);
  }
  for (const id of ["counter", "reset"]) {
    assert.ok(android.includes(`data-testid="${id}"`), `缺键节点锚点: ${id}`);
  }
  // 降级分支（pointer none → 焦点环）在两宿主同款：恰好一处（初始焦点 counter），宽度取自 token 蒙皮
  const rings = android.match(/#0e7490/gi) ?? [];
  assert.equal(rings.length, 1, "无指针形态焦点环恰好一处");
  assert.match(android, /border-width:2px/, "焦点环描边宽度取自 token 蒙皮（两宿主同源）");
  // 触屏形态两宿主同为「无环」
  assert.equal(renderHtml("harmonyos", { pointer: "touch" }), renderHtml("android", { pointer: "touch" }));
  assert.doesNotMatch(renderHtml("harmonyos", { pointer: "touch" }), /#0e7490/i);
});

test("结构快照②（结构层）：组件序/嵌套/锚点/样式两宿主一致；差异仅 android 专属锚点（映射生效负向对照）", { skip }, () => {
  const android = capture("android");
  const harmonyos = capture("harmonyos");
  assert.deepEqual(snapshot(harmonyos), snapshot(android), "结构快照应逐项一致");
  assert.ok(android.length >= 5, `捕获节点数异常（${android.length}）`);
  assert.deepEqual(
    [...new Set(android.map((n) => n.type))],
    ["View", "Text", "Pressable"],
    "组件映射面 = 页面实际使用的原语集（iconSource 缺失时无 Image 节点）",
  );

  // 负向对照：页面照写 Android 焦点锚点（页面零改动），两宿主一个保留一个剔除
  assert.ok(
    android.some(({ type, props }) => type === "View" && props.hasTVPreferredFocus === true),
    "Android 宿主保留 hasTVPreferredFocus（焦点锚点，设备链实证）",
  );
  assert.ok(
    !harmonyos.some(({ props }) => "hasTVPreferredFocus" in props),
    "RNOH 宿主应剔除 hasTVPreferredFocus（组件映射生效）",
  );
  // focusable（两宿主共同支持）必须保真
  assert.ok(harmonyos.some(({ props }) => props.focusable === true));
  assert.equal(harmonyos.filter(({ props }) => props.focusable === false).length, 2, "两按钮撤原生焦点轨的标记须保真");
});

test("焦点/导航基线（RNOH 侧内核同构）：方向键遍历 → 中心键激活 → Back 记忆回退", { skip }, () => {
  const { onKey, getState } = profile("harmonyos");
  assert.deepEqual(onKey("DPAD_DOWN"), { type: "move", direction: "down", from: "counter", to: "reset" });
  assert.equal(getState().focus, "reset");
  assert.deepEqual(onKey("Back"), { type: "back", from: "reset", to: "counter" });
  assert.equal(getState().focus, "counter", "Back 后焦点环回到上一焦点");
  assert.deepEqual(onKey("DPAD_CENTER"), { type: "activate", id: "counter" });
  assert.equal(getState().clicks, 1, "无指针环境下中心键激活（外接键盘 Enter/Space 同路由）");
  assert.deepEqual(onKey("KeyQ"), { type: "ignored", key: "KeyQ" });
});
