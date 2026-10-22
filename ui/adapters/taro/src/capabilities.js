// capabilities 契约（ROADMAP §6）：`capabilities(): { focusNav, safeArea, gestures }`。
// 矩阵与 polypack-targets 内建 TargetSpec（crates/polypack-targets/src/builtin.rs）逐一对照；
// JS 侧不依赖 cargo，漂移由 test/capabilities.test.js 解析该单源文件哨兵断言。
//
// 小程序族基线（T-M3-05）：触屏指针（focusNav = false）、有安全区（刘海 / 状态栏，safeArea = true）、
// 有系统返回手势（gestures = true）。「无 DOM 直连」与「生命周期 / 路由映射」不是宿主可提示的能力位，
// 而是沙箱恒真约束，进降级分支清单（DEGRADATION_IDS）而非三键。

/** 目标 id → Taro 编译平台（`taro build --type` 值）。格式列于 polypack-targets 的目标声明 `ui`。 */
export const TARO_PLATFORMS = Object.freeze({
  wechat: "weapp",
  alipay: "alipay",
  douyin: "tt",
  quickapp: "quickapp",
});

/** 目标 id → 能力基线。只列本包（Taro 小程序族）承载的目标；RN / react-dom 族经本包询问会以
 *  「未知目标」快速失败——避免为一个不承载的宿主给出能力答复。
 *  新增 Taro 承载目标先补 builtin.rs 声明（ui 含 "taro"），再在此增行。 */
export const CAPABILITY_MATRIX = Object.freeze({
  wechat: Object.freeze({ focusNav: false, safeArea: true, gestures: true }),
  alipay: Object.freeze({ focusNav: false, safeArea: true, gestures: true }),
  douyin: Object.freeze({ focusNav: false, safeArea: true, gestures: true }),
  quickapp: Object.freeze({ focusNav: false, safeArea: true, gestures: true }),
});

/** 契约三键（输出顺序即此顺序）。 */
export const CAPABILITY_KEYS = Object.freeze(["focusNav", "safeArea", "gestures"]);

/** 降级分支全集（闭合集合）。小程序族恒真的两条 + 宿主驱动两条；
 *  具体渲染路径：no-direct-dom → apiguard.js 构建期拒绝 + 原语注入；lifecycle-mapping →
 *  routes.js 生命周期映射表；compact-spacing → platformStyle({ compact: true })；
 *  back-affordance → 显式返回入口（无系统手势的平台形态，如部分车机 / 电视上的小程序容器）。 */
export const DEGRADATION_IDS = Object.freeze([
  "no-direct-dom",
  "lifecycle-mapping",
  "compact-spacing",
  "back-affordance",
]);

const isBool = (v) => typeof v === "boolean";
const pick = (...values) => values.find((v) => v !== undefined);

/**
 * capabilities()：目标基线 + 宿主运行时提示（容器实测能力优先于目标默认）。
 *
 * @param {string} target 目标 id（须在矩阵内）
 * @param {{ pointer?: "touch"|"mouse"|"none", focusNav?: boolean, safeArea?: boolean, gestures?: boolean }} [host]
 *   pointer = "none"（无指针容器，如带遥控器的车机小程序）→ focusNav 置真（降级驱动开关）。
 */
export function resolveCapabilities(target, host = {}) {
  const base = CAPABILITY_MATRIX[target];
  if (!base) {
    throw new Error(
      `adapter-taro: 未知目标 "${target}"（已知: ${Object.keys(CAPABILITY_MATRIX).join(", ")}）`,
    );
  }
  for (const key of CAPABILITY_KEYS) {
    if (host[key] !== undefined && !isBool(host[key])) {
      throw new TypeError(`adapter-taro: 能力提示 ${key} 须为布尔值`);
    }
  }
  if (host.pointer !== undefined && !["touch", "mouse", "none"].includes(host.pointer)) {
    throw new TypeError(`adapter-taro: 未知 pointer "${host.pointer}"（touch / mouse / none）`);
  }
  return Object.freeze({
    focusNav: pick(
      host.focusNav,
      host.pointer === undefined ? undefined : host.pointer === "none",
      base.focusNav,
    ),
    safeArea: pick(host.safeArea, base.safeArea),
    gestures: pick(host.gestures, base.gestures),
  });
}

/**
 * degradations()：由 capabilities 声明 + 沙箱恒真约束驱动的降级分支清单（可枚举、闭合）。
 *
 * @param {{focusNav: boolean, safeArea: boolean, gestures: boolean}} caps
 * @param {{ compact?: boolean }} [options] compact = 小屏（由宿主视口判定后传入）
 */
export function resolveDegradations(caps, options = {}) {
  const branches = [
    {
      id: "no-direct-dom",
      active: true,
      reason: "小程序沙箱无 DOM：视图经 Taro 组件树渲染，document / window 直连由 apiguard 构建期拒绝",
    },
    {
      id: "lifecycle-mapping",
      active: true,
      reason: "无 ReactDOM 宿主生命周期：页面 onLoad / onShow / onHide / onUnload 经 routes.js 映射表接线",
    },
    {
      id: "compact-spacing",
      active: options.compact === true,
      reason: "小屏：间距 token 紧凑化（platformStyle compact）",
    },
    {
      id: "back-affordance",
      active: caps.gestures !== true,
      reason: "无系统返回手势：界面提供显式返回入口（路由栈 navigateBack 由宿主接线）",
    },
  ];
  return Object.freeze(branches.map((b) => Object.freeze(b)));
}
