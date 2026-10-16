// TV 焦点内核（T-M4-01）：直接复用 adapter-native 的共享焦点内核（T-M2-05）——遍历 /
// 焦点记忆 / 键路由口径由构造共享（同一 createFocusEngine / handleKey），不复制实现。
// 跨包相对导入（而非 node_modules 解析）：本仓布局 ui/adapters/* 同树，CI 的 node 腿
// 零 npm install 即可加载（与 adapter-native 自身「无 node_modules 环境直接导入」同口径）。
//
// T-M4-02 在此之上加几何推导（geometry.js：rect → 邻居 + focusOrder 覆盖 + 连通性检查器，
// createTvFocus 组合）；T-M4-03 加分层 Back（layers.js）与遥控器数字键（shortcuts.js）。

export {
  DIRECTIONS,
  KEY_MAP,
  createFocusEngine,
  handleKey,
  hasNoDeadEnds,
  reachableFrom,
  traversalSnapshot,
} from "../../native/src/focus.js";
