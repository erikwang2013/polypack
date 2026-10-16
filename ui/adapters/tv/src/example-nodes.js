// 示例页面的焦点节点（T-M4-02 起：2×2 几何网格，rect 为 1920×1080 参考画布的布局盒）。
// 邻接一律由 src/geometry.js 从 rect 推导（零手写 neighbors）——「0 孤立节点」由
// test/geometry.test.js 的连通性检查器证明；testID 即节点 id（观测锚点）。
// 单独成文件：焦点测试与遍历断言需在未安装 react 的环境导入（src/example.js 依赖 react）。

/** 宿主域网格（上排 counter / reset，下排 detail / exit）。 */
export const EXAMPLE_FOCUS_NODES = Object.freeze([
  Object.freeze({ id: "counter", rect: Object.freeze({ x: 400, y: 420, width: 320, height: 120 }) }),
  Object.freeze({ id: "reset", rect: Object.freeze({ x: 780, y: 420, width: 320, height: 120 }) }),
  Object.freeze({ id: "detail", rect: Object.freeze({ x: 400, y: 600, width: 320, height: 120 }) }),
  Object.freeze({ id: "exit", rect: Object.freeze({ x: 780, y: 600, width: 320, height: 120 }) }),
]);
