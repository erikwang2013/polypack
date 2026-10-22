// T-M3-05 代理渲染测试：同一示例页面组件树经 react-dom/server 静态渲染。
// 定位：devtools 真预览（微信 / 支付宝 / 抖音）需本机工具链，本测试是代理证据，非真预览证据；
// 断言两件事：① 页面语义标记与 polypack init 模板页一致；② 焦点环由 capabilities 声明驱动。
// react/react-dom 未安装（CI 无 npm install）时 skip 不 fail。

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TEMPLATE_MAIN_JSX = path.resolve(
  HERE,
  "../../../../crates/polypack-cli/templates/app/ui/main.jsx",
);

// 只吞「react/react-dom 未安装」这一类失败；本包自身的加载错误必须炸出来，
// 否则 src/ 被改坏的回归会以「全绿（skip）」形式混过 T-M3-05 的替代证据。
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

const stubRuntime = {
  config: () => ({ pages: ["pages/index/index"] }),
  navigateTo() {},
};

/** 代理渲染：host 提示决定 capabilities（降级分支的驱动源）。 */
function render(host) {
  const adapter = mods.adapter.createAdapter({
    target: "wechat",
    runtime: stubRuntime,
    host,
  });
  const { Page } = mods.example.createExamplePage(mods.dom.createDomPrimitives(mods.react), adapter);
  return mods.server.renderToStaticMarkup(mods.react.createElement(Page));
}

test("代理渲染：同一示例页面组件树可渲染，语义标记与 polypack init 模板页一致", { skip }, () => {
  const html = render({ pointer: "touch" });
  const template = fs.readFileSync(TEMPLATE_MAIN_JSX, "utf8");
  for (const marker of ["PolyPack App", "版本 0.1.0 · 由 PolyPack 构建", "点击计数："]) {
    assert.ok(template.includes(marker), `模板页缺语义标记: ${marker}`);
    assert.ok(html.includes(marker), `代理渲染缺语义标记: ${marker}`);
  }
  assert.match(html, /点击计数：0/, "初始计数为 0");
  assert.match(html, /data-taro="image"/, "图标原语渲染（Pax 派生资产）");
  // 键节点锚点：devtools 走查与契约测试的节点定位（id 属性）
  for (const id of ["counter", "reset"]) {
    assert.ok(html.includes(`id="${id}"`), `缺键节点锚点: ${id}`);
  }
});

test("降级驱动：触屏微信基线不渲染焦点环；无指针容器渲染焦点环（capabilities 声明驱动）", { skip }, () => {
  const touch = render({ pointer: "touch" });
  const noPointer = render({ pointer: "none" });

  assert.doesNotMatch(touch, /#0e7490/i, "触屏形态无焦点环（capabilities.focusNav = false）");
  assert.match(touch, /点击按钮计数/, "触屏形态提示行讲到点击通路");
  assert.match(noPointer, /无指针容器/, "无指针形态提示行切到焦点通路");
  assert.match(noPointer, /方向键切换焦点/, "无指针形态说明焦点遍历");
});

test("交互路径：示例页 press 计数推进（宿主重渲染的数据源）", { skip }, () => {
  const adapter = mods.adapter.createAdapter({ target: "wechat", runtime: stubRuntime });
  const { press, getState } = mods.example.createExamplePage(
    mods.dom.createDomPrimitives(mods.react),
    adapter,
  );

  assert.deepEqual(getState(), { clicks: 0 });
  assert.deepEqual(press("counter"), { type: "press", id: "counter" });
  assert.equal(getState().clicks, 1, "计数按钮推进");
  press("counter");
  assert.equal(getState().clicks, 2);
  press("reset");
  assert.equal(getState().clicks, 0, "重置清零");
  press("reset");
  assert.equal(getState().clicks, 0, "重置幂等（非 counter 一律清零）");
});

test("四目标同构：页面组件树对每个目标渲染一致（除版本行目标名）", { skip }, () => {
  const renders = Object.keys(mods.adapter.TARO_PLATFORMS).map((target) => {
    const adapter = mods.adapter.createAdapter({ target, runtime: stubRuntime });
    const { Page } = mods.example.createExamplePage(
      mods.dom.createDomPrimitives(mods.react),
      adapter,
    );
    return mods.server
      .renderToStaticMarkup(mods.react.createElement(Page))
      .replace(new RegExp(`--target ${target}`, "g"), "--target <t>");
  });
  for (const html of renders.slice(1)) {
    assert.equal(html, renders[0], "四目标渲染应逐字节一致（版本行目标名除外）");
  }
});
