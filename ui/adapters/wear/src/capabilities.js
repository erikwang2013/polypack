// 穿戴能力矩阵（T-M7-01）：ROADMAP §6 契约三键 { focusNav, safeArea, gestures }。
//
// 基线口径：Wear OS 触摸屏常态 → gestures true、focusNav false（无焦点环）；
// host 提示 pointer = "none"（无指针设备 / 表冠方向键调试 / TV 式走查）→ focusNav 置真，
// 焦点环路径由 adapter-tv / adapter-native 的共享内核承接（src/focus.js，零新导航代码）。
// safeArea 基线恒真（圆 / 方安全区，见 tokens.js）。
//
// 目标 id 口径：wearos（与 tvos / watchos 同形；T-M7-02 的 TargetSpec 登记后由
// test/capabilities.test.js 哨兵收口——内建 id 以 wear 开头者必在本矩阵）。

export const CAPABILITY_MATRIX = Object.freeze({
  wearos: Object.freeze({ focusNav: false, safeArea: true, gestures: true }),
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
 *   pointer = "none" → focusNav 置真（ROADMAP §6「无指针 → 焦点环」的降级驱动开关）。
 */
export function resolveCapabilities(target, host = {}) {
  const base = CAPABILITY_MATRIX[target];
  if (!base) {
    throw new Error(
      `adapter-wear: 未知目标 "${target}"（已知: ${Object.keys(CAPABILITY_MATRIX).join(", ")}）`,
    );
  }
  for (const key of CAPABILITY_KEYS) {
    if (host[key] !== undefined && !isBool(host[key])) {
      throw new TypeError(`adapter-wear: 能力提示 ${key} 须为布尔值`);
    }
  }
  if (host.pointer !== undefined && !["touch", "mouse", "none"].includes(host.pointer)) {
    throw new TypeError(`adapter-wear: 未知 pointer "${host.pointer}"（touch / mouse / none）`);
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
 * degradations()：由 capabilities 声明驱动的降级分支清单（可枚举、闭合）。
 * 渲染路径：focus-ring → src/focus.js（共享焦点内核）；safe-area-padding →
 * platformStyle().safeArea；back-affordance → 页面常驻显式「退出」按钮（示例页）。
 *
 * @param {{focusNav: boolean, safeArea: boolean, gestures: boolean}} caps
 */
export function resolveDegradations(caps) {
  const branches = [
    {
      id: "focus-ring",
      active: caps.focusNav === true,
      reason: "无指针：交互元素渲染焦点环，方向键遍历（复用 TV / native 共享焦点内核）",
    },
    {
      id: "safe-area-padding",
      active: caps.safeArea === true,
      reason: "圆 / 方安全区：外壳按安全区内边距排布（platformStyle().safeArea）",
    },
    {
      id: "back-affordance",
      active: caps.gestures !== true,
      reason: "无系统返回手势：页面提供显式退出入口（Back 焦点记忆交宿主裁决）",
    },
  ];
  return Object.freeze(branches.map((b) => Object.freeze(b)));
}
