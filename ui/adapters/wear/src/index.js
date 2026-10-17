// @polypack/adapter-wear v0（T-M7-01）：ROADMAP §6 RendererAdapter 契约的穿戴装配层——
// 紧凑 tokens（圆 / 方两种安全区）+ 能力降级（capabilities 缺失能力时按契约降级；
// 无指针 → 复用 adapter-tv / native 共享焦点内核的焦点环路径，零新导航代码）。
// 契约：mount(root, app) / platformStyle(tokens) / capabilities(): { focusNav, safeArea, gestures }
// 本文件零 npm / 零 react 依赖（示例页面在 ./example 子路径），可在无 node_modules 环境直接导入。

import { resolveCapabilities, resolveDegradations } from "./capabilities.js";
import { WEAR_FORMS, platformStyle, safeAreaInsets, tokensByForm } from "./tokens.js";

export {
  SAFE_AREA,
  WEAR_FORMS,
  WEAR_SCALE,
  platformStyle,
  safeAreaInsets,
  tokens,
  tokensByForm,
} from "./tokens.js";
export {
  CAPABILITY_KEYS,
  CAPABILITY_MATRIX,
  DEGRADATION_IDS,
  resolveCapabilities,
  resolveDegradations,
} from "./capabilities.js";
export {
  BACK_KEYS,
  DIRECTIONS,
  KEY_MAP,
  checkConnectivity,
  createFocusEngine,
  createWearFocus,
  deriveFocusNodes,
  handleKey,
  hasNoDeadEnds,
  reachableFrom,
  traversalSnapshot,
} from "./focus.js";
export { layoutPage, pageFocusNodes } from "./layout.js";
export { createExamplePage } from "./example.js";

/**
 * 创建目标适配器（RendererAdapter 契约对象）。
 *
 * @param {object} options
 * @param {string} options.target 目标 id（须在内建矩阵；未知即抛错）
 * @param {{ AppRegistry: { registerComponent: Function } }} options.runtime
 *   宿主注入的 RN runtime（Wear OS UI 运行时为 RN beta，T-M7-02；测试注入桩）。
 * @param {"round"|"square"} [options.form] Wear 形态（决定 token 集 / 安全区；默认圆形）
 * @param {object} [options.host] 运行时能力提示（pointer / safeArea / gestures / focusNav）
 */
export function createAdapter({ target, runtime, form = "round", host = {} } = {}) {
  // 边界校验（装配入口做一次）：未知目标 / 形态与缺 runtime 在此快速失败
  resolveCapabilities(target, host);
  if (!Object.hasOwn(WEAR_FORMS, form)) {
    throw new TypeError(`adapter-wear: 未知形态 "${form}"（round / square）`);
  }
  if (typeof runtime?.AppRegistry?.registerComponent !== "function") {
    throw new TypeError(
      "adapter-wear: runtime 须提供 AppRegistry.registerComponent（宿主注入 RN runtime）",
    );
  }
  const formTokens = tokensByForm[form];
  let mounted = null;

  return Object.freeze({
    target,
    form,
    /** 当前形态的 token 集（页面树 / 盒模型同源；只读）。 */
    get tokens() {
      return formTokens;
    },
    /** mount(root, app)：RN 宿主装配 = AppRegistry 注册（root = { appKey }）。 */
    mount(root, app) {
      const appKey = root?.appKey;
      if (typeof appKey !== "string" || appKey === "") {
        throw new TypeError("adapter-wear: mount 需 root.appKey（非空字符串）");
      }
      if (typeof app !== "function") {
        throw new TypeError("adapter-wear: mount 需 app 组件（函数组件）");
      }
      runtime.AppRegistry.registerComponent(appKey, () => app);
      mounted = Object.freeze({ appKey, app });
      return mounted;
    },
    /** 已装配记录（未装配为 null）——宿主诊断与测试用。 */
    get mounted() {
      return mounted;
    },
    /** platformStyle(tokens)：蒙皮为 RN 合法样式片段（当前形态默认；可传自定义 token 集）。 */
    platformStyle(t = formTokens) {
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
    /** 当前形态的安全区像素内边距（默认该形态参考视口；宿主可传视口）。 */
    safeArea(viewport = WEAR_FORMS[form]) {
      return safeAreaInsets(form, viewport);
    },
  });
}
