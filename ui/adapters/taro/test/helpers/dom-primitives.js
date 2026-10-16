// 测试代理原语（仅测试用，非 Taro web 实现）：把 Taro 原语映射到 DOM 标签，
// 使 react-dom/server 能对同一页面组件树做代理渲染（Linux / CI 可跑）。
// devtools 真渲染（微信 / 支付宝 / 抖音开发者工具）需本机装工具链：见 README 阻塞清单。
// 零导入 + React 注入：本文件落在 test/ 下会被 node --test 当测试文件加载（不装 react 也不得抛错）。

/** Taro 样式 → CSS 对象（丢弃非 CSS 键；数值即设计稿 px）。 */
function toCss(style) {
  const out = {};
  for (const part of [style].flat(Infinity)) {
    if (!part) continue;
    for (const [key, value] of Object.entries(part)) out[key] = value;
  }
  return out;
}

/** 创建 DOM 代理原语（形态与宿主注入的 Taro 原语一致）。 */
export function createDomPrimitives(React) {
  const { createElement } = React;
  return {
    // id → DOM id：与 devtools 走查的节点锚点对齐，代理渲染据此断言
    View: ({ id, style, children }) =>
      createElement("div", { id, style: toCss(style), "data-taro": "view" }, children),
    Text: ({ id, style, children }) =>
      createElement("span", { id, style: toCss(style), "data-taro": "text" }, children),
    // Button → <button>（Taro Button 的 onClick 与宿主程序化触发同路）
    Button: ({ id, style, onClick, children }) =>
      createElement(
        "button",
        { id, type: "button", style: toCss(style), "data-taro": "button", onClick },
        children,
      ),
    Image: ({ src, style }) =>
      createElement("img", { src, style: toCss(style), "data-taro": "image" }),
    iconSource: "./icon.svg",
  };
}
