// TV 焦点内核（T-M4-01/02）：直接复用 adapter-native 的共享焦点内核（T-M2-05）——遍历 /
// 焦点记忆 / 键路由口径由构造共享（同一 createFocusEngine / handleKey），不复制实现；
// 键集合口径由行为对拍收口（test/focus.test.js 探针哨兵）。
// 跨包相对导入（而非 node_modules 解析）：本仓布局 ui/adapters/* 同树，CI 的 node 腿
// 零 npm install 即可加载（与 adapter-native 自身「无 node_modules 环境直接导入」同口径）。
//
// T-M4-02 增量：几何推导引擎 createTvFocus（rect → 邻居；focusOrder 全量覆盖；节点
// neighbors 逐向最高优先）+ 连通性检查器 checkConnectivity（见 geometry.js）。
// T-M4-03 增量：分层 Back（layers.js）与遥控器数字键（shortcuts.js）+ 统一键路由 handleTvKey。

import { createFocusEngine } from "../../native/src/focus.js";

import { deriveFocusNodes } from "./geometry.js";

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

// T-M4-03 在此续：BACK_KEYS / handleTvKey（分层 Back + 数字键路由）。
