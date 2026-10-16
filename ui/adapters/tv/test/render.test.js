// T-M4-01 代理渲染测试：同一示例页面组件树经 react-dom/server 静态渲染。
// 定位：TV 真机 / 模拟器预览需设备链，本测试是代理证据，非真机证据；
// 断言四件事：① 语义标记与 poly init 模板页一致；② 焦点环随方向键移动（capabilities 驱动）；
// ③ data-focus 观测锚点（T-M4-13 冒烟通道）；④ 安全区外框由 platformStyle().safeArea 派生。
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
const ACCENT = "#0e7490"; // focusRing.borderColor（tokens.color.accent）

// 只吞「react/react-dom 未安装」这一类失败；本包自身的加载错误必须炸出来，
// 否则 src/ 被改坏的回归会以「全绿（skip）」形式混过替代证据。
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
  React: { createElement: () => null },
  ReactDOM: { createRoot: () => ({ render() {} }) },
};

/** 装配示例页面（host 提示决定 capabilities）。 */
function setup(target = "tizen-tv") {
  const adapter = mods.adapter.createAdapter({ target, runtime: stubRuntime });
  return { adapter, page: mods.example.createExamplePage(mods.dom.createDomPrimitives(mods.react), adapter) };
}

const render = (page) => mods.server.renderToStaticMarkup(mods.react.createElement(page.Page));

/** 当前焦点元素：data-focus="true" 的锚点 id。 */
function focusedId(html) {
  return html.match(/data-testid="([^"]+)"[^>]*data-focus="true"/)?.[1] ?? null;
}

/** 焦点环出现次数（border 颜色计数；每帧恰好 1 个节点带环）。 */
const ringCount = (html) => html.split(ACCENT).length - 1;

test("代理渲染：同一示例页面组件树可渲染，语义标记与 poly init 模板页一致", { skip }, () => {
  const { page } = setup();
  const html = render(page);
  const template = fs.readFileSync(TEMPLATE_MAIN_JSX, "utf8");
  for (const marker of ["PolyPack App", "版本 0.1.0 · 由 PolyPack 构建", "点击计数："]) {
    assert.ok(template.includes(marker), `模板页缺语义标记: ${marker}`);
    assert.ok(html.includes(marker), `代理渲染缺语义标记: ${marker}`);
  }
  assert.match(html, /点击计数：0/, "初始计数为 0");
  assert.match(html, /data-testid="tv-frame"/, "安全区外框锚点");
  assert.match(html, /data-testid="tv-card"/, "页卡锚点");
});

test("降级驱动：TV 基线（focusNav true）渲染焦点环；方向键移动环随焦点", { skip }, () => {
  const { page } = setup();
  const first = render(page);
  assert.equal(focusedId(first), "counter", "初始焦点 = counter");
  assert.equal(ringCount(first), 1, "恰好一个节点带焦点环");

  assert.deepEqual(page.onKey("ArrowDown"), { type: "move", direction: "down", from: "counter", to: "reset" });
  const moved = render(page);
  assert.equal(focusedId(moved), "reset", "方向键移动后焦点锚点随动");
  assert.equal(ringCount(moved), 1, "环随焦点移动（旧位置无残留）");
  assert.ok(moved.indexOf(`data-testid="reset"`) < moved.indexOf(ACCENT), "环落在 reset 节点上");

  assert.deepEqual(page.onKey("ArrowUp"), { type: "move", direction: "up", from: "reset", to: "counter" });
  assert.equal(focusedId(render(page)), "counter", "方向键回移恢复");
});

test("安全区外框：padding 由 platformStyle().safeArea 派生（CSS 百分比形态）", { skip }, () => {
  const { page } = setup();
  const html = render(page);
  const frame = html.match(/data-testid="tv-frame"[^>]*style="([^"]*)"/)?.[1] ?? "";
  for (const side of ["top", "right", "bottom", "left"]) {
    assert.match(
      frame,
      new RegExp(`padding-${side}:\\s*5%`),
      `安全区 ${side} 应为 5%（贪心匹配失败则回退逐侧断言）`,
    );
  }
});

test("交互路径：OK 键激活推进计数（无指针环境的中心键通路）", { skip }, () => {
  const { page } = setup();
  assert.deepEqual(page.getState(), { focus: "counter", clicks: 0 });
  assert.deepEqual(page.onKey("Enter"), { type: "activate", id: "counter" });
  assert.equal(page.getState().clicks, 1, "计数按钮推进");
  page.onKey("ArrowDown");
  assert.deepEqual(page.onKey("DPAD_CENTER"), { type: "activate", id: "reset" });
  assert.equal(page.getState().clicks, 0, "重置清零");
  assert.equal(focusedId(render(page)), "reset", "激活不移动焦点");
});

test("data-focus 观测锚点：恒有且仅有一个当前焦点节点（T-M4-13 冒烟通道）", { skip }, () => {
  const { page } = setup();
  let html = render(page);
  for (const key of ["ArrowDown", "ArrowUp", "ArrowDown", "ArrowDown"]) {
    page.onKey(key);
    html = render(page);
    const matches = html.match(/data-focus="true"/g) ?? [];
    assert.equal(matches.length, 1, `每帧恰一个 data-focus 锚点（键 ${key} 后）`);
  }
});
