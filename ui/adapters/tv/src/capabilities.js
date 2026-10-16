// TV 能力矩阵（T-M4-01）：react-dom 承载的电视目标（ROADMAP §3.4 web 运行时族 + §6 adapter-tv）。
//
// 与 adapter-native 的差异（有意为之，非漂移）：
//  - 遥控器恒有方向键 → focusNav 基线 true，宿主 pointer 提示不翻转它（Magic Remote 的指针
//    不取消焦点环；与 RN 侧「pointer none → focusNav」的推导方向相反）；
//  - 无触摸手势（gestures false）→ back-affordance 降级由遥控器 Back 键承接（分层 Back 语义）。
//
// 目标 id 口径：L3 六枚（lead 定案——android-tv / fire-tv / tizen-tv / webos-tv / vidaa-tv /
// smartcast-tv；导出面不带 -tv 后缀的 tvos 归 T-M4-07 单独声明，不在本矩阵）。android-tv /
// fire-tv 由 RN 承载（adapter-native），能力面与 web 电视族同口径（TV 基线）——本矩阵是
// L3 能力声明的单一入口。poly-targets 登记（T-M4-04/08/09/10 落位）后由
// test/capabilities.test.js 哨兵收口（内建 `*-tv` 目标 ⊆ 本矩阵）。
export const CAPABILITY_MATRIX = Object.freeze({
  "android-tv": Object.freeze({ focusNav: true, safeArea: true, gestures: false }),
  "fire-tv": Object.freeze({ focusNav: true, safeArea: true, gestures: false }),
  "tizen-tv": Object.freeze({ focusNav: true, safeArea: true, gestures: false }),
  "webos-tv": Object.freeze({ focusNav: true, safeArea: true, gestures: false }),
  "vidaa-tv": Object.freeze({ focusNav: true, safeArea: true, gestures: false }),
  "smartcast-tv": Object.freeze({ focusNav: true, safeArea: true, gestures: false }),
});

/** 契约三键（输出顺序即此顺序）。 */
export const CAPABILITY_KEYS = Object.freeze(["focusNav", "safeArea", "gestures"]);

/** 降级分支全集（闭合集合）。 */
export const DEGRADATION_IDS = Object.freeze([
  "focus-ring",
  "safe-area-padding",
  "back-affordance",
]);

const isBool = (v) => typeof v === "boolean";
const pick = (...values) => values.find((v) => v !== undefined);

/**
 * capabilities()：目标基线 + 宿主运行时提示（真机能力优先于目标默认）。
 *
 * @param {string} target 目标 id（须在矩阵内）
 * @param {{ pointer?: "touch"|"mouse"|"none", focusNav?: boolean, safeArea?: boolean, gestures?: boolean }} [host]
 *   pointer 仅校验/留存（宿主信息面与 native / taro 同形）；TV 侧不参与 focusNav 推导。
 */
export function resolveCapabilities(target, host = {}) {
  const base = CAPABILITY_MATRIX[target];
  if (!base) {
    throw new Error(
      `adapter-tv: 未知目标 "${target}"（已知: ${Object.keys(CAPABILITY_MATRIX).join(", ")}）`,
    );
  }
  for (const key of CAPABILITY_KEYS) {
    if (host[key] !== undefined && !isBool(host[key])) {
      throw new TypeError(`adapter-tv: 能力提示 ${key} 须为布尔值`);
    }
  }
  if (host.pointer !== undefined && !["touch", "mouse", "none"].includes(host.pointer)) {
    throw new TypeError(`adapter-tv: 未知 pointer "${host.pointer}"（touch / mouse / none）`);
  }
  return Object.freeze({
    focusNav: pick(host.focusNav, base.focusNav),
    safeArea: pick(host.safeArea, base.safeArea),
    gestures: pick(host.gestures, base.gestures),
  });
}

/**
 * degradations()：由 capabilities 声明驱动的降级分支清单（可枚举、闭合）。
 * 渲染路径：focus-ring → src/focus.js 焦点内核（遍历图 + 焦点环）；
 * safe-area-padding → platformStyle().safeArea；back-affordance → 分层 Back 语义（src/layers.js）。
 *
 * @param {{focusNav: boolean, safeArea: boolean, gestures: boolean}} caps
 */
export function resolveDegradations(caps) {
  const branches = [
    {
      id: "focus-ring",
      active: caps.focusNav === true,
      reason: "遥控器无指针：交互元素渲染焦点环，方向键遍历（TV 焦点内核）",
    },
    {
      id: "safe-area-padding",
      active: caps.safeArea === true,
      reason: "电视安全区：外壳按安全区内边距排布（platformStyle().safeArea），防过扫描遮挡",
    },
    {
      id: "back-affordance",
      active: caps.gestures !== true,
      reason: "无系统返回手势：遥控器 Back 键 = 分层返回（弹层 → 焦点记忆 → 退出）",
    },
  ];
  return Object.freeze(branches.map((b) => Object.freeze(b)));
}
