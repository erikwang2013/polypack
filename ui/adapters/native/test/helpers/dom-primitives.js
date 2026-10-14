// 测试代理原语（仅测试用，非 react-native-web）：把 RN 原语映射到 DOM 标签，
// 使 react-dom/server 能对同一页面组件树做代理渲染（Linux / CI 可跑）。
// 真机渲染（Android RN 宿主）需模拟器 + Gradle：仓库当前无，阻塞标注见 README。
// 零导入 + React 注入：本文件落在 test/ 下会被 node --test 当测试文件加载（不装 react 也不得抛错）。

const SPLIT_PROPS = {
  marginVertical: ["marginTop", "marginBottom"],
  paddingVertical: ["paddingTop", "paddingBottom"],
  marginHorizontal: ["marginLeft", "marginRight"],
  paddingHorizontal: ["paddingLeft", "paddingRight"],
};

/** RN 样式 → CSS 对象（平面化数组、拆上下/左右、丢弃 RN 专有键）。 */
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

/** 创建 DOM 代理原语（形态与宿主注入的 RN 原语一致）。 */
export function createDomPrimitives(React) {
  const { createElement } = React;
  return {
    View: ({ style, children }) => createElement("div", { style: toCss(style) }, children),
    Text: ({ style, children }) => createElement("span", { style: toCss(style) }, children),
    Pressable: ({ style, children }) =>
      createElement("button", { type: "button", style: toCss(style) }, children),
    Image: ({ source, accessibilityLabel, style }) =>
      createElement("img", { src: source, alt: accessibilityLabel, style: toCss(style) }),
    iconSource: "./icon.svg",
  };
}
