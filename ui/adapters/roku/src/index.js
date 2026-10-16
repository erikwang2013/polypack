// @polypack/adapter-roku spike（T-M4-11）：Roku SceneGraph 子集受限渲染器。
// 与 adapter-dom / adapter-native / adapter-tv 的差异（spike 结论的浓缩）：Roku 无 JS 运行时，
// 「渲染器」只能是构建期发射器（子集描述 → SceneGraph XML）+ 原生 BrightScript 逻辑；
// 交互语义经共享焦点内核的遍历表移植（source/focusgraph.brs 由内核快照生成）。
// 边界与替代方案见同包 CAPABILITY-BOUNDARY-DRAFT.md（T-M4-12 决策输入）。

export { CANVAS, FONT_ROLES, SUBSET_ELEMENTS, createExampleSubset, validateSubset } from "./subset.js";
export { emitChannelFiles, toSceneGraphColor, xmlWellFormed } from "./emit.js";
export { ROKU_KEY_MAP, mapRokuKey } from "./roku-keys.js";
