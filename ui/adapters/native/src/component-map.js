// 组件映射（T-M3-02）：同一页面组件树 → 各宿主原语。页面不感知宿主（组件树零改动），
// 宿主差异只在此一处声明：宿主不支持的原生 prop 在装配边界剔除（页面照写 Android 焦点锚点）。
// createElement 注入（同「runtime / 原语注射」家规）：本模块零 react 依赖，CI 无 node_modules 也可测。

/** 宿主不支持的原生 prop（闭合集合；M4 TV 分支在此扩展）。 */
export const HOST_PROP_OMISSIONS = Object.freeze({
  android: Object.freeze([]),
  ios: Object.freeze([]),
  // RNOH/ArkUI 无 Android TV 原生焦点锚点对应：焦点经 ArkTS 根容器 onKeyEvent → polyKeyEvent
  // 通道驱动（见 host/harmony/index.js），页面照写 hasTVPreferredFocus 由本表剔除。
  harmonyos: Object.freeze(["hasTVPreferredFocus"]),
});

/** mapProps(target, props)：按宿主剔除不支持的原生 prop（未知目标快速失败，同 capabilities）。 */
export function mapProps(target, props = {}) {
  const omitted = HOST_PROP_OMISSIONS[target];
  if (omitted === undefined) {
    throw new Error(
      `component-map: 未知目标 "${target}"（已知: ${Object.keys(HOST_PROP_OMISSIONS).join(", ")}）`,
    );
  }
  if (omitted.length === 0) return props;
  const out = {};
  for (const [key, value] of Object.entries(props)) {
    if (!omitted.includes(key)) out[key] = value;
  }
  return out;
}

/** 是否 React 组件值（函数 / forwardRef / memo / lazy；普通数据对象不算）。 */
const isComponent = (v) =>
  typeof v === "function" || (typeof v === "object" && v !== null && v.$$typeof !== undefined);

/**
 * bindPrimitives(primitives, target, createElement)：包裹组件原语，在装配边界应用 mapProps。
 * 无差异宿主（剔除表为空）返回原对象，保持原语同一性（Android 宿主零改动）。
 * 非组件值（如 iconSource 资产）原样透传。
 */
export function bindPrimitives(primitives, target, createElement) {
  const omitted = HOST_PROP_OMISSIONS[target];
  if (omitted === undefined) {
    throw new Error(
      `component-map: 未知目标 "${target}"（已知: ${Object.keys(HOST_PROP_OMISSIONS).join(", ")}）`,
    );
  }
  if (omitted.length === 0) return primitives;
  if (typeof createElement !== "function") {
    throw new TypeError("component-map: bindPrimitives 须注入 createElement（react.createElement）");
  }
  const bound = {};
  for (const [key, value] of Object.entries(primitives)) {
    bound[key] = isComponent(value) ? (props) => createElement(value, mapProps(target, props)) : value;
  }
  return Object.freeze(bound);
}
