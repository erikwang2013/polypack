// @polypack/adapter-native v0（T-M2-04）：ROADMAP §6 RendererAdapter 契约的 RN 宿主装配层。
// 契约：mount(root, app) / platformStyle(tokens) / capabilities(): { focusNav, safeArea, gestures }
// —— 同一份页面组件树，按 capabilities 声明降级交互（无指针 → 焦点环，见 src/focus.js）。
// 本文件零 react 依赖（示例页面在 ./example 子路径），可在无 node_modules 环境直接导入。

import { resolveCapabilities, resolveDegradations } from "./capabilities.js";
import { createNativeRegistry } from "./native-registry.js";
import { platformStyle, platformStyleTvos } from "./tokens.js";

export {
  TV_SAFE_AREA,
  TV_SCALE,
  platformStyle,
  platformStyleTvos,
  tokens,
  tvosTokens,
} from "./tokens.js";
export { HOST_PROP_OMISSIONS, bindPrimitives, mapProps } from "./component-map.js";
export {
  CAPABILITY_MATRIX,
  CAPABILITY_KEYS,
  DEGRADATION_IDS,
  resolveCapabilities,
  resolveDegradations,
} from "./capabilities.js";
export {
  DIRECTIONS,
  KEY_MAP,
  createFocusEngine,
  handleKey,
  hasNoDeadEnds,
  reachableFrom,
  traversalSnapshot,
} from "./focus.js";
export { createNativeRegistry } from "./native-registry.js";

/**
 * 创建目标适配器（RendererAdapter 契约对象）。
 *
 * @param {object} options
 * @param {string} options.target 目标 id（须在内建矩阵；未知即抛错）
 * @param {{ AppRegistry: { registerComponent: Function } }} options.runtime
 *   宿主注入的 RN runtime（生产：宿主 `import * as RN from "react-native"`；测试：注入桩）。
 *   显式注入而非内部 import：装配层不臆测宿主，Metro/CI 亦无需真装 react-native。
 * @param {object} [options.host] 运行时能力提示（pointer / safeArea / gestures / focusNav）
 */
export function createAdapter({ target, runtime, host = {} } = {}) {
  // 边界校验（装配入口做一次）：未知目标与缺 runtime 在此快速失败
  resolveCapabilities(target, host);
  if (typeof runtime?.AppRegistry?.registerComponent !== "function") {
    throw new TypeError(
      "adapter-native: runtime 须提供 AppRegistry.registerComponent（宿主注入 RN runtime）",
    );
  }
  const native = createNativeRegistry(target);
  let mounted = null;

  return Object.freeze({
    target,
    /** mount(root, app)：RN 宿主装配 = AppRegistry 注册（RN 无 DOM 挂载点，root = { appKey }）。 */
    mount(root, app) {
      const appKey = root?.appKey;
      if (typeof appKey !== "string" || appKey === "") {
        throw new TypeError("adapter-native: mount 需 root.appKey（非空字符串）");
      }
      if (typeof app !== "function") {
        throw new TypeError("adapter-native: mount 需 app 组件（函数组件）");
      }
      runtime.AppRegistry.registerComponent(appKey, () => app);
      mounted = Object.freeze({ appKey, app });
      return mounted;
    },
    /** 已装配记录（未装配为 null）——宿主诊断与测试用。 */
    get mounted() {
      return mounted;
    },
    /** platformStyle(tokens, options)：蒙皮为 RN 样式（compact = 小屏降级）。
     *  T-M4-06：target=tvos → 10-foot 蒙皮（页面零参直取即 TV 默认皮肤；TV 无 compact 分支）。 */
    platformStyle(t, options) {
      return target === "tvos" ? platformStyleTvos(t) : platformStyle(t, options);
    },
    /** capabilities(hostHints)：契约三键，宿主运行时提示覆盖目标基线。 */
    capabilities(hostHints = {}) {
      return resolveCapabilities(target, { ...host, ...hostHints });
    },
    /** degradations(hostHints, options)：由 capabilities 声明驱动的降级分支清单。 */
    degradations(hostHints = {}, options = {}) {
      return resolveDegradations(resolveCapabilities(target, { ...host, ...hostHints }), options);
    },
    /** 原生模块注册位（桥产物挂载点：Android AAR T-M2-02 / 鸿蒙 NAPI .so T-M3-01，按 target 定口径）。 */
    native,
  });
}
