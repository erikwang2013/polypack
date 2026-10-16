// T-M4-13 web 腿宿主：把 @polypack/adapter-tv 示例页挂到真实 react-dom（同一份页面树，
// 非重写），键盘事件源归一为内核键，每键一行单参数 `POLYPACK_KEY <json>`——口径与
// ui/adapters/tv/README.md「宿主记录口径（跨端同形）」及其 RN 宿主（ui/adapters/native/host）
// 一致（RN 的 console 多参会坏，故一律单参数拼接）。
//
// 页面树零 JSX（createElement，与 src/example.js 同形）；原语 = 真 DOM 标签代理：
// testID → data-testid（DOM 锚点），data-focus 直通（焦点环锚点），onPress → onClick。
// 目标经 ?target= 覆盖（默认 tizen-tv）；同一 dist 供 web 系各目标分支复用。

import * as React from "react";
import * as ReactDOM from "react-dom/client";

import { createAdapter } from "../../../adapters/tv/src/index.js";
import { createExamplePage } from "../../../adapters/tv/src/example.js";
import { kernelKeyOfDom } from "../keys.mjs";

const h = React.createElement;
const target = new URLSearchParams(location.search).get("target") ?? "tizen-tv";

/** 原语（宿主注入面）：样式已是 react-dom 合法对象（tokens.js platformStyle 契约）。 */
const primitives = {
  View: ({ testID, style, children, ...rest }) =>
    h("div", { "data-testid": testID, style, ...rest }, children),
  Text: ({ style, children, ...rest }) => h("span", { style, ...rest }, children),
  Pressable: ({ testID, onPress, style, children, ...rest }) =>
    h(
      "button",
      { type: "button", "data-testid": testID, onClick: onPress, style, ...rest },
      children,
    ),
  Image: ({ style, ...rest }) => h("img", { style, ...rest }),
  iconSource: null, // 图标资产未就位 → 页面自行省略（与 example.js 注释同口径）
};

const adapter = createAdapter({
  target,
  runtime: { React, ReactDOM },
  host: { pointer: "none" }, // 遥控器环境：无指针
});
const page = createExamplePage(primitives, adapter);
adapter.mount(document.getElementById("root"), page.Page);

addEventListener("keydown", (event) => {
  const key = kernelKeyOfDom(event);
  if (key === null) return;
  event.preventDefault();
  const result = page.onKey(key);
  console.log("POLYPACK_KEY " + JSON.stringify(result)); // 单参数：跨端同形契约
});

// 就绪与诊断面：驱动在 console 收到 POLYPACK_READY 且 tv-frame 锚点在场后才注键
//（DOM 锚点确认首帧已落地——window 标志先置会与首帧竞态）。
window.__polypack = Object.freeze({ target, onKey: page.onKey, getState: page.getState });
console.log("POLYPACK_READY " + JSON.stringify({ target, profile: "tv-example" }));
