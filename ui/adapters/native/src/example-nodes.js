// 示例页面的最小遍历图（两节点纵向环；TV 侧完整导航图留 M4）。
// 单独成文件：焦点测试与快照断言需在未安装 react 的环境导入（src/example.js 依赖 react）。

/** 遍历图节点（顺序即文档序 + 初始焦点 = counter）。 */
export const EXAMPLE_FOCUS_NODES = Object.freeze([
  Object.freeze({ id: "counter", neighbors: Object.freeze({ down: "reset" }) }),
  Object.freeze({ id: "reset", neighbors: Object.freeze({ up: "counter" }) }),
]);
