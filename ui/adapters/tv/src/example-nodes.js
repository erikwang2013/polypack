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

/** 详情弹层域（1×2：播放 / 关闭）——层域与宿主域图不连通（域隔离，跨域无边）。 */
export const TV_MODAL_FOCUS_NODES = Object.freeze([
  Object.freeze({ id: "modal-play", rect: Object.freeze({ x: 620, y: 700, width: 320, height: 120 }) }),
  Object.freeze({ id: "modal-close", rect: Object.freeze({ x: 1000, y: 700, width: 320, height: 120 }) }),
]);

/** 播放器子层域（单节点：停止）——单节点图四向自环（检查器不判陷阱）。 */
export const TV_PLAYER_FOCUS_NODES = Object.freeze([
  Object.freeze({ id: "player-stop", rect: Object.freeze({ x: 780, y: 760, width: 400, height: 120 }) }),
]);
