// @polypack/adapter-tv v0（T-M4-01）：ROADMAP §6 RendererAdapter 契约的电视（10-foot）
// 装配层——react-dom 挂载 + 10-foot 皮肤 tokens（远距字号 / 间距 / 安全区）+ 焦点导航内核。
// 契约：mount(root, app) / platformStyle(tokens) / capabilities(): { focusNav, safeArea, gestures }
// ——同一份示例页面组件树，按 capabilities 声明渲染（TV 焦点环恒开：遥控器无指针）。
// 本文件零 react 依赖（示例页面在 ./example 子路径），可在无 node_modules 环境直接导入。

import { resolveCapabilities, resolveDegradations } from "./capabilities.js";
import { platformStyle, safeAreaInsets } from "./tokens.js";

export {
  CAPABILITY_KEYS,
  CAPABILITY_MATRIX,
  DEGRADATION_IDS,
  resolveCapabilities,
  resolveDegradations,
} from "./capabilities.js";
export { SAFE_AREA, TV_SCALE, platformStyle, safeAreaInsets, tokens } from "./tokens.js";
export {
  DIRECTIONS,
  KEY_MAP,
  createFocusEngine,
  createTvFocus,
  handleKey,
  hasNoDeadEnds,
  reachableFrom,
  traversalSnapshot,
} from "./focus.js";
export { checkConnectivity, deriveFocusNodes } from "./geometry.js";

/**
 * 创建目标适配器（RendererAdapter 契约对象）。
 *
 * @param {object} options
 * @param {string} options.target 目标 id（须在内建矩阵；未知即抛错）
 * @param {object} options.runtime 宿主注入的 react-dom 运行面（生产：
 *   `{ React: import * as React, ReactDOM: import * as ReactDOM from "react-dom/client" }`；
 *   测试：注入桩）。显式注入而非内部 import：装配层不臆测宿主，CI 无需真装 react-dom。
 * @param {object} [options.host] 运行时能力提示（pointer / safeArea / gestures / focusNav）
 */
export function createAdapter({ target, runtime, host = {} } = {}) {
  // 边界校验（装配入口做一次）：未知目标与缺 runtime 在此快速失败
  resolveCapabilities(target, host);
  if (typeof runtime?.ReactDOM?.createRoot !== "function") {
    throw new TypeError(
      "adapter-tv: runtime 须提供 ReactDOM.createRoot（宿主注入 react-dom/client）",
    );
  }
  if (typeof runtime?.React?.createElement !== "function") {
    throw new TypeError(
      "adapter-tv: runtime 须提供 React.createElement（宿主注入 react）",
    );
  }
  let mounted = null;

  return Object.freeze({
    target,
    /** mount(root, app)：react-dom 挂载 = createRoot(root).render(<app/>)（root 为宿主 DOM 容器）。 */
    mount(root, app) {
      if (root === null || typeof root !== "object") {
        throw new TypeError("adapter-tv: mount 需 root 挂载容器（宿主 DOM 元素）");
      }
      if (typeof app !== "function") {
        throw new TypeError("adapter-tv: mount 需 app 组件（函数组件）");
      }
      const handle = runtime.ReactDOM.createRoot(root);
      handle.render(runtime.React.createElement(app));
      mounted = Object.freeze({ root, app, handle });
      return mounted;
    },
    /** 已装配记录（未装配为 null）——宿主诊断与测试用。 */
    get mounted() {
      return mounted;
    },
    /** platformStyle(tokens)：蒙皮为 react-dom 样式片段（10-foot：远距字号 / 间距 / 安全区）。 */
    platformStyle(t) {
      return platformStyle(t);
    },
    /** capabilities(hostHints)：契约三键，宿主运行时提示覆盖目标基线。 */
    capabilities(hostHints = {}) {
      return resolveCapabilities(target, { ...host, ...hostHints });
    },
    /** degradations(hostHints)：由 capabilities 声明驱动的降级分支清单。 */
    degradations(hostHints = {}) {
      return resolveDegradations(resolveCapabilities(target, { ...host, ...hostHints }));
    },
    /** 电视安全区像素内边距（默认 1080p 视口）——宿主外壳排布与冒烟观测用。 */
    safeArea(viewport) {
      return safeAreaInsets(viewport);
    },
  });
}
