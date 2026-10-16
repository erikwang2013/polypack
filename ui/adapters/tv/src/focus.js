// TV 焦点内核（T-M4-01/02）：直接复用 adapter-native 的共享焦点内核（T-M2-05）——遍历 /
// 焦点记忆 / 键路由口径由构造共享（同一 createFocusEngine / handleKey），不复制实现；
// 键集合口径由行为对拍收口（test/focus.test.js 探针哨兵）。
// 跨包相对导入（而非 node_modules 解析）：本仓布局 ui/adapters/* 同树，CI 的 node 腿
// 零 npm install 即可加载（与 adapter-native 自身「无 node_modules 环境直接导入」同口径）。
//
// T-M4-02 增量：几何推导引擎 createTvFocus（rect → 邻居；focusOrder 全量覆盖；节点
// neighbors 逐向最高优先）+ 连通性检查器 checkConnectivity（见 geometry.js）。
// T-M4-03 增量：统一键路由 handleTvKey——分层 Back（层 → 焦点记忆 → 退出应用，三层
// 语义）与遥控器数字键（shortcuts.js；层打开时被拦截不穿透）；层域隔离见 layers.js。

import { createFocusEngine, handleKey } from "../../native/src/focus.js";

import { deriveFocusNodes } from "./geometry.js";
import { normalizeShortcutKey } from "./shortcuts.js";

export {
  DIRECTIONS,
  KEY_MAP,
  createFocusEngine,
  handleKey,
  hasNoDeadEnds,
  reachableFrom,
  traversalSnapshot,
} from "../../native/src/focus.js";
export { checkConnectivity, deriveFocusNodes } from "./geometry.js";

/** Back 键（遥控器返回 / 键盘 Escape）——口径与共享内核 BACK_KEYS 对拍（focus.test.js 哨兵）。 */
export const BACK_KEYS = Object.freeze(["Back", "Escape"]);

/**
 * TV 焦点引擎：几何推导遍历图 → native 共享内核实例（口径由构造保证）。
 *
 * @param {{ nodes: Array<{id: string, rect: object, neighbors?: object}>, initial?: string,
 *   focusOrder?: string[] }} spec nodes 带 rect（见 deriveFocusNodes）；initial 默认 order[0]。
 * @returns {ReturnType<typeof createFocusEngine>} 内核引擎（graph / order / current / focus /
 *   move / back）——下游（RN-tvOS / 鸿蒙 TV / Roku spike / 冒烟套件）按同一接口消费。
 */
export function createTvFocus(spec) {
  if (!spec || !Array.isArray(spec.nodes)) {
    throw new TypeError("focus: nodes 须为非空数组");
  }
  const nodes = deriveFocusNodes(spec.nodes, { focusOrder: spec.focusOrder });
  return createFocusEngine({ nodes, initial: spec.initial });
}

/**
 * TV 统一键路由（T-M4-03）：宿主事件源唯一入口。Back 三类路由 + 数字键快捷 +
 * 方向 / 激活经当前活动域（层栈顶或宿主）的共享内核 handleKey。
 *
 * @param {{ engine: object, layers?: object, shortcuts?: Record<string, string> }} session
 *   engine：宿主域焦点引擎（必需）；layers：createLayerStack()；shortcuts：createShortcutMap()。
 * @param {string} key 键名（方向/激活见 KEY_MAP，Back 见 BACK_KEYS，数字键见 shortcuts.js）
 * @param {{ activate?: (id: string) => void }} [handlers]
 * @returns {object} 描述符：move / activate（与 handleKey 同形）；back 带 route：
 *   "layer"（关层，layer = 被关层 id，to = 关层后应落焦点）| "memory"（焦点记忆回退）|
 *   "exit"（历史空，宿主裁决是否退出）；shortcut：{ type, digit, from, to } 或层内 { blocked: true }；
 *   未识别：{ type: "ignored", key }。宿主按描述符记录 / 冒烟断言；内核零副作用。
 */
export function handleTvKey(session, key, handlers = {}) {
  const { engine, layers = null, shortcuts = null } = session ?? {};
  if (!engine || !Array.isArray(engine.order)) {
    throw new TypeError("focus: handleTvKey 需 session.engine（宿主域焦点引擎）");
  }
  // Back 三层语义：层打开 → 关层（域切换即焦点恢复）；无层 → 焦点记忆；历史空 → 退出
  if (BACK_KEYS.includes(key)) {
    if (layers?.current) {
      const closed = layers.close();
      return {
        type: "back",
        route: "layer",
        layer: closed.id,
        from: closed.focus,
        to: (layers.current ?? engine).current,
      };
    }
    const from = engine.current;
    const to = engine.back();

    return to === null
      ? { type: "back", route: "exit", from, to: null }
      : { type: "back", route: "memory", from, to };
  }
  const target = layers?.current ?? engine;
  const digit = shortcuts === null ? null : normalizeShortcutKey(key);
  if (digit !== null && Object.hasOwn(shortcuts, digit)) {
    // 数字键只在宿主域路由：层打开时拦截（不穿透弹层）；未映射数字落回内核路由（ignored）
    if (target !== engine) return { type: "shortcut", digit, blocked: true };
    const id = shortcuts[digit];
    if (!engine.order.includes(id)) {
      throw new TypeError(`focus: 快捷目标不在宿主域: ${id}`);
    }
    const from = engine.current;
    return { type: "shortcut", digit, from, to: engine.focus(id) };
  }
  return handleKey(target, key, handlers);
}
