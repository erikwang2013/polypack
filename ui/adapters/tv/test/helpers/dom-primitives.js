// 测试代理原语（仅测试用，非 react-dom）：把页面原语映射到 DOM 标签，
// 使 react-dom/server 能对同一页面组件树做代理渲染（Linux / CI 可跑）。
// 相对 adapter-native / adapter-taro 版本的差异：透传 data-focus（焦点观测锚点，
// T-M4-13 冒烟读当前焦点 id）；其余 props 透传以便宿主壳按需扩展。
// 零导入 + React 注入：本文件落在 test/ 下会被 node --test 当测试文件加载（不装 react 也不得抛错）。

const SPLIT_PROPS = {
  marginVertical: ["marginTop", "marginBottom"],
  paddingVertical: ["paddingTop", "paddingBottom"],
  marginHorizontal: ["marginLeft", "marginRight"],
  paddingHorizontal: ["paddingLeft", "paddingRight"],
};

/** 页面样式 → CSS 对象（平面化数组、拆上下/左右、丢弃原语专有键）。 */
function toCss(style) {
  const out = {};
  for (const part of [style].flat(Infinity)) {
    if (!part) continue;
    for (const [key, value] of Object.entries(part)) {
      const split = SPLIT_PROPS[key];
      if (split) {
        for (const cssKey of split) out[cssKey] = value;
      } else {
        out[key] = value;
      }
    }
  }
  return out;
}

/** 创建 DOM 代理原语（形态与宿主注入的 react-dom 原语一致）。 */
export function createDomPrimitives(React) {
  const { createElement } = React;
  return {
    // testID → data-testid（react-native-web 同款约定）：冒烟与契约测试的节点锚点
    View: ({ testID, style, children, ...rest }) =>
      createElement("div", { "data-testid": testID, ...rest, style: toCss(style) }, children),
    Text: ({ testID, style, children, ...rest }) =>
      createElement("span", { "data-testid": testID, ...rest, style: toCss(style) }, children),
    Pressable: ({ testID, style, children, ...rest }) =>
      createElement(
        "button",
        { type: "button", "data-testid": testID, ...rest, style: toCss(style) },
        children,
      ),
    Image: ({ source, accessibilityLabel, testID, style, ...rest }) =>
      createElement("img", {
        src: source,
        alt: accessibilityLabel,
        "data-testid": testID,
        ...rest,
        style: toCss(style),
      }),
    iconSource: "./icon.svg",
  };
}
