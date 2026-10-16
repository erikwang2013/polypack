// 示例页面的焦点节点（T-M4-01 骨架：两节点纵向环，显式邻接 = 内核形态）。
// T-M4-02 起换为几何推导（节点带 rect，邻居由 src/geometry.js 推导）。
// 单独成文件：焦点测试与快照断言需在未安装 react 的环境导入（src/example.js 依赖 react）。

/** 遍历图节点（顺序即文档序 + 初始焦点 = counter）。 */
export const EXAMPLE_FOCUS_NODES = Object.freeze([
  Object.freeze({ id: "counter", neighbors: Object.freeze({ down: "reset" }) }),
  Object.freeze({ id: "reset", neighbors: Object.freeze({ up: "counter" }) }),
]);
