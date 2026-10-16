// @polypack/adapter-taro v0（T-M3-05）：ROADMAP §6 RendererAdapter 契约的小程序族装配层
// （Taro，React 语法：微信 weapp / 支付宝 alipay / 抖音 tt / 快应用 quickapp）。
// 契约：mount(root, app) / platformStyle(tokens) / capabilities(): { focusNav, safeArea, gestures }
// ——同一份页面组件树，按 capabilities 声明降级（无 DOM 直连、生命周期与路由映射、小屏紧凑）。
// 本文件零 react 依赖（示例页面在 ./example 子路径），可在无 node_modules 环境直接导入。

import { resolveCapabilities, resolveDegradations, TARO_PLATFORMS } from "./capabilities.js";
import { createRouter, normalizePath } from "./routes.js";
import { platformStyle } from "./tokens.js";

export { API_WHITELIST, FORBIDDEN_GLOBALS, checkSource, formatViolations } from "./apiguard.js";
export {
  CAPABILITY_KEYS,
  CAPABILITY_MATRIX,
  DEGRADATION_IDS,
  TARO_PLATFORMS,
  resolveCapabilities,
  resolveDegradations,
} from "./capabilities.js";
export {
  LIFECYCLE_EVENTS,
  LIFECYCLE_MAP,
  ROUTE_METHODS,
  createPageLifecycle,
  createRouter,
  normalizePath,
} from "./routes.js";
export { platformStyle, tokens } from "./tokens.js";

/**
 * 创建目标适配器（RendererAdapter 契约对象）。
 *
 * @param {object} options
 * @param {string} options.target 目标 id（须在内建矩阵；未知即抛错）
 * @param {object} options.runtime 宿主注入的 Taro 运行面（生产：宿主 `import Taro from "@tarojs/taro"`
 *   + `config()` 读 `app.config.js`；测试：注入桩）。
 *   显式注入而非内部 import：装配层不臆测宿主，devtools / CI 亦无需真装 @tarojs/taro。
 * @param {object} [options.host] 运行时能力提示（pointer / safeArea / gestures / focusNav）
 */
export function createAdapter({ target, runtime, host = {} } = {}) {
  // 边界校验（装配入口做一次）：未知目标与缺 runtime 在此快速失败
  resolveCapabilities(target, host);
  if (typeof runtime !== "object" || runtime === null) {
    throw new TypeError(
      "adapter-taro: runtime 须为宿主注入的 Taro 运行面（生产：@tarojs/taro + config()）",
    );
  }
  const platform = TARO_PLATFORMS[target];
  let mounted = null;

  return Object.freeze({
    target,
    /** Taro 编译平台（`taro build --type` 值；打包器四目标配置据此派生）。 */
    platform,
    /**
     * mount(root, app)：Taro 无 DOM 挂载点也无 AppRegistry——页面装配 = 组件绑定到声明的路由。
     * 页面模块把返回的组件作为 default export，Taro 编译器按 app.config 的 pages 接线。
     * root.pagePath 必须已在 `runtime.config().pages` 声明（路由映射前置校验）。
     */
    mount(root, app) {
      const pagePath = normalizePath(root?.pagePath);
      if (typeof app !== "function") {
        throw new TypeError("adapter-taro: mount 需 app 组件（函数组件）");
      }
      const pages = runtime.config?.()?.pages;
      if (!Array.isArray(pages) || !pages.map(normalizePath).includes(pagePath)) {
        throw new Error(
          `adapter-taro: 页面 "${pagePath}" 未在 app.config 的 pages 中声明（路由映射拒绝）`,
        );
      }
      mounted = Object.freeze({ pagePath, app, platform });
      return mounted;
    },
    /** 已装配记录（未装配为 null）——宿主诊断与测试用。 */
    get mounted() {
      return mounted;
    },
    /** platformStyle(tokens, options)：蒙皮为 Taro 样式片段（compact = 小屏降级）。 */
    platformStyle(t, options) {
      return platformStyle(t, options);
    },
    /** capabilities(hostHints)：契约三键，宿主运行时提示覆盖目标基线。 */
    capabilities(hostHints = {}) {
      return resolveCapabilities(target, { ...host, ...hostHints });
    },
    /** degradations(hostHints, options)：由 capabilities 声明 + 沙箱恒真约束驱动的降级分支清单。 */
    degradations(hostHints = {}, options = {}) {
      return resolveDegradations(resolveCapabilities(target, { ...host, ...hostHints }), options);
    },
    /** 声明式路由（页面清单校验 + 方法面收口）；runtime 缺 config() 时在此快速失败。 */
    router() {
      return createRouter(runtime);
    },
  });
}
