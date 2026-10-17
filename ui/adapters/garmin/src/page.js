// Garmin 转译页模型（T-M7-09 spike）：输入 = adapter-wear 的紧凑 tokens / 安全区 / 能力面，
// 输出 = 与 Monkey C 发射器同构的确定性页模型（布局盒 + 共享焦点内核的节点序）。
//
// 交互降级口径：Garmin 以 UP/DOWN/SELECT/BACK 按键为主（无指针 → focusNav），2×2 网格在
// 三键设备上不可达右列，故按共享内核的节点序（engine.order）线性化；零手写邻接——
// 序即 adapter-wear 的 pageFocusNodes / createWearFocus 产物，改口径只改上游。

import {
  createWearFocus,
  layoutPage,
  pageFocusNodes,
  resolveCapabilities,
  tokensByForm,
  traversalSnapshot,
} from "../../wear/src/index.js";

/** 形态 → Connect IQ 参考设备 + 设计画布（= adapter-wear 参考视口，px）。 */
export const GARMIN_FORMS = Object.freeze({
  round: Object.freeze({
    product: "venu2",
    shape: "round",
    reference: Object.freeze({ width: 454, height: 454 }),
  }),
  square: Object.freeze({
    product: "venusq",
    shape: "square",
    reference: Object.freeze({ width: 320, height: 320 }),
  }),
});

/** T-M7-01 能力矩阵当前只登记 wearos；Garmin 借用穿戴基线（联合矩阵归 T-M7-10/11）。 */
const WEAR_CAPABILITY_TARGET = "wearos";

/**
 * 创建 Garmin 页模型。
 *
 * @param {"round"|"square"} [form] 形态（决定 wear token 集 / 安全区 / 参考设备）
 * @returns {object} { target, form, product, reference, tokens, viewport, safe, boxes,
 *   capabilities, focusOrder, initialFocus, traversal }
 */
export function createGarminPage(form = "round") {
  const spec = GARMIN_FORMS[form];
  if (!spec) {
    throw new TypeError(
      `adapter-garmin: 未知形态 "${form}"（支持: ${Object.keys(GARMIN_FORMS).join(", ")}）`,
    );
  }
  const tokens = tokensByForm[form];
  const layout = layoutPage(tokens);
  // 焦点引擎直接复用 adapter-wear（= adapter-tv / native 共享内核 + 几何推导），零新增导航。
  const engine = createWearFocus({ nodes: pageFocusNodes(tokens), initial: "counter" });
  // 按键基线 = 无指针降级：focusNav 置真（焦点环 + 焦点记忆由共享内核口径承接）。
  const capabilities = resolveCapabilities(WEAR_CAPABILITY_TARGET, { pointer: "none" });

  return Object.freeze({
    target: "garmin",
    form,
    product: spec.product,
    shape: spec.shape,
    reference: spec.reference,
    tokens,
    viewport: tokens.viewport,
    safe: layout.safe,
    boxes: layout.boxes,
    capabilities,
    focusOrder: Object.freeze([...engine.order]),
    initialFocus: engine.current,
    // 遍历快照仅作证据 / 后续四向设备的待接线口径，v0 运行时不消费。
    traversal: Object.freeze(traversalSnapshot(engine.graph)),
  });
}

/** 页模型的摘要（构建脚本 / 测试日志用）。 */
export function describePage(page) {
  return {
    form: page.form,
    product: page.product,
    viewport: `${page.viewport.width}x${page.viewport.height}`,
    safe: `${page.safe.width}x${page.safe.height}`,
    focusOrder: page.focusOrder.join(" → "),
    focusNav: page.capabilities.focusNav,
  };
}
