// capabilities 契约（ROADMAP §6）：`capabilities(): { focusNav, safeArea, gestures }`。
// 矩阵与 polypack-targets 内建 TargetSpec（crates/polypack-targets/src/builtin.rs）逐一对照；
// JS 侧不依赖 cargo，漂移由 test/capabilities.test.js 解析该单源文件哨兵断言。

/** 目标 id → 能力基线。只列本包（RN / RNOH）承载的目标；react-dom 族是 adapter-dom 的职责，
 *  经本包询问会以「未知目标」快速失败——避免为一个不承载的宿主给出能力答复。
 *  新增 RN 承载目标先补 builtin.rs 声明，再在此增行。 */
export const CAPABILITY_MATRIX = Object.freeze({
  android: Object.freeze({ focusNav: false, safeArea: true, gestures: true }),
  ios: Object.freeze({ focusNav: false, safeArea: true, gestures: true }),
  harmonyos: Object.freeze({ focusNav: false, safeArea: true, gestures: true }),
  // HarmonyOS TV：RNOH TV 形态（deviceTypes 含 tv）→ 遥控器无指针，focusNav 常开、无触摸手势。
  "harmonyos-tv": Object.freeze({ focusNav: true, safeArea: true, gestures: false }),
  // GrapheneOS：Android 手机变体（复用 RN / uniffi 链）→ 触摸 + 手势常态。
  grapheneos: Object.freeze({ focusNav: false, safeArea: true, gestures: true }),
  // T-M4-04 Android TV / Fire TV：Android 家族 TV 形态（Leanback 遥控器恒无指针 → focusNav
  // 常开；清单强制 touchscreen=false → 无触摸手势）。host 提示仍可覆盖。
  "android-tv": Object.freeze({ focusNav: true, safeArea: true, gestures: false }),
  "fire-tv": Object.freeze({ focusNav: true, safeArea: true, gestures: false }),
  // T-M4-06 RN-tvOS：TV 基线（同 adapter-tv 矩阵口径——遥控器恒无指针 → focusNav 常开；
  // 无系统返回手势 → 走显式 Back 入口降级分支）。host 提示仍可覆盖。
  tvos: Object.freeze({ focusNav: true, safeArea: true, gestures: false }),
  // T-M6-11 Android Automotive：车机形态（触摸屏常态 → 手势开；航向/旋钮输入经 host 提示
  // 或 pointer=none 降级驱动 focusNav）。声明 react-native 承载，须在矩阵内。
  "android-automotive": Object.freeze({ focusNav: false, safeArea: true, gestures: true }),
  // T-M7-02 Wear OS：触摸 + 滑动手势常态 → 手势开；小屏安全区必开（圆 / 方形差异在
  // adapter-wear 的 layout 处理）。声明 react-native 承载，须在矩阵内（哨兵要求）。
  wearos: Object.freeze({ focusNav: false, safeArea: true, gestures: true }),
});

/** 契约三键（输出顺序即此顺序）。 */
export const CAPABILITY_KEYS = Object.freeze(["focusNav", "safeArea", "gestures"]);

/** 降级分支全集（闭合集合；M4 在此扩展完整导航图 / 手势栈分支）。 */
export const DEGRADATION_IDS = Object.freeze(["focus-ring", "compact-spacing", "back-affordance"]);

const isBool = (v) => typeof v === "boolean";
const pick = (...values) => values.find((v) => v !== undefined);

/**
 * capabilities()：目标基线 + 宿主运行时提示（真机能力优先于目标默认）。
 *
 * @param {string} target 目标 id（须在矩阵内）
 * @param {{ pointer?: "touch"|"mouse"|"none", focusNav?: boolean, safeArea?: boolean, gestures?: boolean }} [host]
 *   pointer = "none"（TV / 车机 / 无指针调试）→ focusNav 置真，即 ROADMAP §6 的降级驱动开关。
 */
export function resolveCapabilities(target, host = {}) {
  const base = CAPABILITY_MATRIX[target];
  if (!base) {
    throw new Error(
      `adapter-native: 未知目标 "${target}"（已知: ${Object.keys(CAPABILITY_MATRIX).join(", ")}）`,
    );
  }
  for (const key of CAPABILITY_KEYS) {
    if (host[key] !== undefined && !isBool(host[key])) {
      throw new TypeError(`adapter-native: 能力提示 ${key} 须为布尔值`);
    }
  }
  if (host.pointer !== undefined && !["touch", "mouse", "none"].includes(host.pointer)) {
    throw new TypeError(`adapter-native: 未知 pointer "${host.pointer}"（touch / mouse / none）`);
  }
  return Object.freeze({
    focusNav: pick(host.focusNav, host.pointer === undefined ? undefined : host.pointer === "none", base.focusNav),
    safeArea: pick(host.safeArea, base.safeArea),
    gestures: pick(host.gestures, base.gestures),
  });
}

/**
 * degradations()：由 capabilities 声明驱动的降级分支清单（可枚举、闭合）。
 * 每条分支只描述「是否生效 + 原因」，具体渲染路径：focus-ring → src/focus.js 焦点内核；
 * compact-spacing → platformStyle({ compact: true })；back-affordance → handleKey Back 语义（焦点记忆）。
 *
 * @param {{focusNav: boolean, safeArea: boolean, gestures: boolean}} caps
 * @param {{ compact?: boolean }} [options] compact = 小屏（由宿主视口判定后传入）
 */
export function resolveDegradations(caps, options = {}) {
  const branches = [
    {
      id: "focus-ring",
      active: caps.focusNav === true,
      reason: "无指针：交互元素渲染焦点环，方向键遍历（共享焦点内核）",
    },
    {
      id: "compact-spacing",
      active: options.compact === true,
      reason: "小屏：间距 token 紧凑化（platformStyle compact）",
    },
    {
      id: "back-affordance",
      active: caps.gestures !== true,
      reason: "无系统返回手势：界面提供显式返回入口（Back 焦点记忆接口）",
    },
  ];
  return Object.freeze(branches.map((b) => Object.freeze(b)));
}
