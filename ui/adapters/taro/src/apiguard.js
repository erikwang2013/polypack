// 宿主 API 白名单（T-M3-05）：小程序沙箱只认声明过的 API 面——构建期拒绝越界调用。
//
// 两个方向：
//  - `API_WHITELIST`：允许经 `Taro.*` 调用的宿主 API 名（v0 只列本包与示例使用 + wasm 桥入口，
//    宁可窄：白名单外即构建期报错，扩面走一次加行 + 测试）；
//  - `FORBIDDEN_GLOBALS`：DOM / 浏览器全局——小程序沙箱无 DOM（降级分支 no-direct-dom），
//    出现即构建期报错（语义上必然运行期 TypeError 的调用不该走到编译）。
//
// 扫描是行级词边界匹配（同 poly-core scan.rs 的行内标记口径）：够用、确定、无 parser 依赖；
// 字符串 / 注释里的同名词用同行标记 `poly:allow-api` 放行（构造样本与文档示例的出口）。
// 构建期入口：bin/check-api.mjs（poly-bundle 小程序打包器的编译前预检调用同一实现）。

/** 允许的 Taro API 名（`Taro.<name>`）；每一行都对应本包或示例的真实使用面。 */
export const API_WHITELIST = Object.freeze([
  // 路由（routes.js 使用）
  "navigateTo",
  "redirectTo",
  "switchTab",
  "reLaunch",
  "navigateBack",
  // 实例 / 生命周期（页面装配与 routes.js 映射接线）
  "getCurrentInstance",
  "getApp",
  "useLoad",
  "useReady",
  "useDidShow",
  "useDidHide",
  "useUnload",
  // 系统信息（safeArea / 紧凑判定输入）
  "getWindowInfo",
  "getSystemInfoSync",
  // 存储（示例核心任务：计数持久化）
  "getStorageSync",
  "setStorageSync",
  "removeStorageSync",
  // 用户反馈
  "showToast",
]);

/** 禁止的 DOM / 浏览器全局（小程序沙箱不存在或不可依赖）。 */
export const FORBIDDEN_GLOBALS = Object.freeze([
  "document",
  "window",
  "navigator",
  "location",
  "localStorage",
  "sessionStorage",
  "indexedDB",
  "XMLHttpRequest",
  "WebSocket",
  "fetch",
  "Worker",
  "requestAnimationFrame",
  "MutationObserver",
  "HTMLElement",
  "alert",
  "confirm",
  "prompt",
]);

/** 放行标记：同一行含任一标记即整行豁免（构造样本 / 文档示例）。 */
export const ALLOW_MARKERS = Object.freeze(["poly:allow-api"]);

/** 扫描的规则 id → 人话说明（报告与测试断言共用）。 */
export const VIOLATION_KINDS = Object.freeze({
  "forbidden-global": "DOM / 浏览器全局（小程序沙箱无此全局，降级分支 no-direct-dom）",
  "taro-api": "Taro API 不在白名单（宿主 API 面只认声明过的子集）",
});

const asPattern = (names) =>
  new RegExp(`(?:^|[^\\w$.])(${names.map((n) => n.replace(/\W/g, "\\$&")).join("|")})\\b`, "g");
const FORBIDDEN_PATTERN = asPattern([...FORBIDDEN_GLOBALS]);
const TARO_PATTERN = /Taro\s*(?:\.\s*([A-Za-z_$][\w$]*)|\[\s*["']([A-Za-z_$][\w$]*)["']\s*\])/g;

/**
 * 构建期扫描一段源码：返回违规清单（空数组 = 过）。
 *
 * @param {string} code 源码文本
 * @param {{ filename?: string, allow?: string[] }} [options] allow = 额外放行的 Taro API 名
 *   （目标特化扩面由调用方显式给出，不自带隐式默认）
 * @returns {{ filename: string, violations: Array<{ line: number, kind: string, name: string }> }}
 */
export function checkSource(code, { filename = "<source>", allow = [] } = {}) {
  const allowed = new Set([...API_WHITELIST, ...allow]);
  const violations = [];
  const lines = String(code).split("\n");
  for (const [index, line] of lines.entries()) {
    if (ALLOW_MARKERS.some((marker) => line.includes(marker))) continue;
    for (const match of line.matchAll(FORBIDDEN_PATTERN)) {
      violations.push({ line: index + 1, kind: "forbidden-global", name: match[1] });
    }
    for (const match of line.matchAll(TARO_PATTERN)) {
      const name = match[1] ?? match[2];
      if (!allowed.has(name)) {
        violations.push({ line: index + 1, kind: "taro-api", name });
      }
    }
  }
  return { filename, violations };
}

/** 违规清单 → 可读报告行（CLI 与打包器预检共用）。 */
export function formatViolations(report) {
  return report.violations.map(
    (v) => `${report.filename}:${v.line}: [${v.kind}] ${v.name}——${VIOLATION_KINDS[v.kind]}`,
  );
}
