// 生命周期与路由映射（T-M3-05）：小程序族的宿主生命周期 → React 侧语义的显式映射表 +
// 声明式路由（页面清单校验 + 方法面收口）。
//
// 为什么是「映射表」而不是 hook 包装：装配层零 react 依赖（同 adapter-native 的取舍），
// 页面侧接线由宿主页面模块经 runtime 注入的 Taro API 落地；本文件只固化映射关系与校验，
// 使「哪些生命周期存在、哪些路由方法可用、目标页是否声明」在构建期可断言。

/** 小程序页面生命周期全集（闭合；Taro 页面 config 的同名键）。 */
export const LIFECYCLE_EVENTS = Object.freeze([
  "onLoad",
  "onReady",
  "onShow",
  "onHide",
  "onUnload",
  "onPullDownRefresh",
  "onReachBottom",
  "onPageScroll",
  "onShareAppMessage",
]);

/** React 侧对应语义（映射表；宿主页面模块据此接线 hooks）。 */
export const LIFECYCLE_MAP = Object.freeze({
  onLoad: "mount", // useLoad：页面入栈（参数经 options 传入）
  onReady: "first-render", // useReady：首帧就绪
  onShow: "activate", // useDidShow
  onHide: "deactivate", // useDidHide
  onUnload: "unmount", // useUnload
  onPullDownRefresh: "refresh",
  onReachBottom: "reach-end",
  onPageScroll: "scroll",
  onShareAppMessage: "share",
});

/** 路由方法面（Taro 导航 API 子集；navigateBack 不走页面清单校验）。 */
export const ROUTE_METHODS = Object.freeze([
  "navigateTo",
  "redirectTo",
  "switchTab",
  "reLaunch",
  "navigateBack",
]);

/** 页面路径归一化：去首斜杠与查询串（小程序路由 API 的 url 形态为 `/pages/x/index?a=1`）。 */
export function normalizePath(path) {
  if (typeof path !== "string" || path.trim() === "") {
    throw new TypeError("adapter-taro: 路由 path 须为非空字符串");
  }
  const trimmed = path.trim().replace(/^\/+/, "");
  const [clean] = trimmed.split(/[?#]/);
  if (clean === "") {
    throw new TypeError(`adapter-taro: 路由 path "${path}" 不含页面路径`);
  }
  return clean;
}

/** 导航 url 形态：前导斜杠 + 保留查询串（去哈希；小程序 url 不带锚点）。 */
function toUrl(path) {
  normalizePath(path); // 边界校验共用一条口径
  return `/${path.trim().replace(/^\/+/, "").split("#")[0]}`;
}

/**
 * 生命周期映射接线：校验键集（闭合全集内）与处理器类型，返回冻结的 Page 生命周期对象。
 * 直接交给 Taro 页面 config（`definePageConfig` 之外的 Page({ onShow }) 形态）。
 *
 * @param {Record<string, Function>} handlers 键 ∈ LIFECYCLE_EVENTS
 */
export function createPageLifecycle(handlers = {}) {
  const out = {};
  for (const [name, handler] of Object.entries(handlers)) {
    if (!LIFECYCLE_EVENTS.includes(name)) {
      throw new Error(
        `adapter-taro: 未知生命周期 "${name}"（已知: ${LIFECYCLE_EVENTS.join(", ")}）`,
      );
    }
    if (typeof handler !== "function") {
      throw new TypeError(`adapter-taro: 生命周期 ${name} 须为函数`);
    }
    out[name] = handler;
  }
  return Object.freeze(out);
}

/**
 * 声明式路由：页面清单来自 runtime.config()（Taro 工程 `app.config.js` 的 pages 数组），
 * 未声明的目标页在调用点即拒——路由映射在构建期 / 启动期可测，不靠运行期 404。
 *
 * @param {{ config: () => { pages: string[] }, [key: string]: Function }} runtime 宿主注入
 */
export function createRouter(runtime) {
  if (typeof runtime?.config !== "function") {
    throw new TypeError("adapter-taro: runtime 须提供 config()（返回 app.config 的 pages 清单）");
  }
  const pages = runtime.config()?.pages;
  if (!Array.isArray(pages) || pages.length === 0) {
    throw new TypeError("adapter-taro: app.config 的 pages 清单须为非空数组（路由映射的声明源）");
  }
  const declared = new Set(pages.map(normalizePath));

  const route = (method) => {
    if (!ROUTE_METHODS.includes(method)) {
      throw new Error(
        `adapter-taro: 未知路由方法 "${method}"（已知: ${ROUTE_METHODS.join(", ")}）`,
      );
    }
    if (method === "navigateBack") {
      return (options = {}) => {
        const delta = options.delta ?? 1;
        if (!Number.isInteger(delta) || delta < 1) {
          throw new TypeError("adapter-taro: navigateBack.delta 须为正整数");
        }
        return callRuntime(runtime, method, { delta });
      };
    }
    return (path, options = {}) => {
      const clean = normalizePath(path);
      if (!declared.has(clean)) {
        throw new Error(
          `adapter-taro: 目标页 "${clean}" 未在 app.config 的 pages 中声明（路由映射拒绝）`,
        );
      }
      // 校验用去查询的干净路径；导航 url 保留查询串（页面参数是路由语义的一部分）
      return callRuntime(runtime, method, { url: toUrl(path), ...options });
    };
  };

  return Object.freeze({
    /** 声明的页面清单（归一化后，冻结）。 */
    pages: Object.freeze([...declared]),
    navigateTo: route("navigateTo"),
    redirectTo: route("redirectTo"),
    switchTab: route("switchTab"),
    reLaunch: route("reLaunch"),
    navigateBack: route("navigateBack"),
  });
}

/** 经 runtime 调宿主 API：缺实现即显式报错（装配层不臆测宿主，也不静默吞回路由调用）。 */
function callRuntime(runtime, method, args) {
  if (typeof runtime[method] !== "function") {
    throw new TypeError(`adapter-taro: runtime 未提供路由方法 ${method}（宿主注入缺失）`);
  }
  return runtime[method](args);
}
