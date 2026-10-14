// T-M2-04 代理渲染测试：同一示例页面组件树经 react-dom/server 静态渲染。
// 定位：真机渲染（Android RN 宿主）为阻塞项（无 SDK / 模拟器），本测试是代理证据，非真机证据；
// 断言两件事：① 页面语义标记与 poly init 模板页一致；② 焦点环由 capabilities 声明驱动。
// react/react-dom 未安装（CI 无 npm install）时 skip 不 fail。

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TEMPLATE_MAIN_JSX = path.resolve(
  HERE,
  "../../../../crates/poly-cli/templates/app/ui/main.jsx",
);

// 只吞「react/react-dom 未安装」这一类失败；本包自身的加载错误必须炸出来，
// 否则 src/ 被改坏的回归会以「全绿（skip）」形式混过 T-M2-04 的替代证据。
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
const skip = mods === null ? "react/react-dom 未安装（npm install 后启用代理渲染断言）" : false;

const stubRuntime = { AppRegistry: { registerComponent() {} } };

/** 代理渲染：host 提示决定 capabilities（降级分支的驱动源）。 */
function render(host) {
  const adapter = mods.adapter.createAdapter({ target: "android", runtime: stubRuntime, host });
  const { App } = mods.example.createExampleApp(mods.dom.createDomPrimitives(mods.react), adapter);
  return mods.server.renderToStaticMarkup(mods.react.createElement(App));
}

test("代理渲染：同一示例页面组件树可渲染，语义标记与 poly init 模板页一致", { skip }, () => {
  const html = render({ pointer: "touch" });
  const template = fs.readFileSync(TEMPLATE_MAIN_JSX, "utf8");
  for (const marker of ["PolyPack App", "版本 0.1.0 · 由 PolyPack 构建", "点击计数："]) {
    assert.ok(template.includes(marker), `模板页缺语义标记: ${marker}`);
    assert.ok(html.includes(marker), `代理渲染缺语义标记: ${marker}`);
  }
  assert.match(html, /点击计数：0/, "初始计数为 0");
  assert.match(html, /<img[^>]+alt="Pax"/, "图标原语渲染（accessibilityLabel → alt）");
});

test("降级驱动：无指针 → 焦点环落在初始焦点；触屏 → 不渲染焦点环", { skip }, () => {
  const touch = render({ pointer: "touch" });
  const noPointer = render({ pointer: "none" });

  assert.doesNotMatch(touch, /#0e7490/i, "触屏形态无焦点环（capabilities.focusNav = false）");
  const rings = noPointer.match(/#0e7490/gi) ?? [];
  assert.equal(rings.length, 1, "无指针形态焦点环恰好一处（初始焦点 counter）");
  assert.match(noPointer, /border-width:2px/, "焦点环描边宽度取自 token 蒙皮");
  assert.match(noPointer, /点击计数：0/, "焦点环不改变页面语义");
});

test("交互路径：示例页 onKey 遍历/Back 推进焦点（宿主重渲染的数据源）", { skip }, () => {
  const adapter = mods.adapter.createAdapter({
    target: "android",
    runtime: stubRuntime,
    host: { pointer: "none" },
  });
  const { onKey, focus, getState } = mods.example.createExampleApp(
    mods.dom.createDomPrimitives(mods.react),
    adapter,
  );

  assert.deepEqual(onKey("DPAD_DOWN"), {
    type: "move",
    direction: "down",
    from: "counter",
    to: "reset",
  });
  assert.equal(getState().focus, "reset");
  assert.deepEqual(onKey("Back"), { type: "back", from: "reset", to: "counter" });
  assert.equal(getState().focus, "counter", "Back 后焦点环应回到上一焦点");
  assert.deepEqual(onKey("KeyQ"), { type: "ignored", key: "KeyQ" });
  assert.equal(focus("reset"), "reset", "宿主直接置焦点须带视图通知");
  assert.equal(getState().focus, "reset");
});

test("无指针核心任务走查（JS 层）：方向键 → 中心键激活 → Back 返回", { skip }, () => {
  const adapter = mods.adapter.createAdapter({
    target: "android",
    runtime: stubRuntime,
    host: { pointer: "none" },
  });
  const { onKey, getState } = mods.example.createExampleApp(
    mods.dom.createDomPrimitives(mods.react),
    adapter,
  );

  assert.deepEqual(getState(), { focus: "counter", clicks: 0 });
  assert.deepEqual(onKey("DPAD_CENTER"), { type: "activate", id: "counter" });
  assert.equal(getState().clicks, 1, "中心键激活当前焦点（无指针环境的按压通路）");
  onKey("DPAD_DOWN");
  onKey("DPAD_CENTER");
  assert.equal(getState().clicks, 0, "焦点移到重置后激活即清零");
  onKey("Back");
  assert.equal(onKey("DPAD_CENTER").id, "counter", "Back 后激活对象回到上一焦点");
  assert.equal(getState().clicks, 1);
});

test("两形态渲染确定性一致：除焦点环外标记相同", { skip }, () => {
  const strip = (html) =>
    html
      .replace(/border-color:#0e7490;?/gi, "")
      .replace(/border-width:2px;?/gi, "")
      .replace(/;;+/g, ";")
      .replace(/;"/g, '"');
  assert.equal(strip(render({ pointer: "none" })), strip(render({ pointer: "touch" })));
});
