// 穿戴焦点内核（T-M7-01）：直接复用 adapter-tv / adapter-native 的共享焦点内核
// （T-M2-05 内核 + T-M4-02 几何推导）——遍历 / 焦点记忆 / 键路由口径由构造共享，
// 本包零新增导航代码。仅当 capabilities.focusNav 为真（无指针降级）时渲染焦点环；
// 触摸基线不启用焦点环（触摸按压即交互，不需要遍历态）。
// 跨包相对导入（而非 node_modules 解析）：与 adapter-tv 同口径，CI 零 npm install 可加载。

export {
  BACK_KEYS,
  DIRECTIONS,
  KEY_MAP,
  checkConnectivity,
  createFocusEngine,
  createTvFocus as createWearFocus,
  deriveFocusNodes,
  handleKey,
  hasNoDeadEnds,
  reachableFrom,
  traversalSnapshot,
} from "../../tv/src/focus.js";
