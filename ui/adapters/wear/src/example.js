// 示例页面（T-M7-01 验收的「同一页面组件树」）：宿主无关的纯描述树（零 react / 零 npm）。
// 结构：frame（安全区内边距）> card > 标题 / 版本 / 2×2 按钮网格 / 提示行——与 src/layout.js
// 的盒模型同构；同一 createExamplePage 产物在圆 / 方两种形态下都按 platformStyle 排布。
// 按钮 id 即焦点节点 id（testID 观测锚点）；无指针（caps.focusNav）时焦点环由 adapter-tv /
// native 共享内核驱动（src/focus.js），触摸基线不渲染焦点环。
// 宿主把 type（View / Text / Pressable）映射到自身原语；onKey 是遥控器 / 表冠事件统一入口。

import { createWearFocus, handleKey } from "./focus.js";
import { pageFocusNodes } from "./layout.js";
import { tokensByForm } from "./tokens.js";

/**
 * 创建示例页面（宿主装配的最后一个环节）。
 *
 * @param {ReturnType<import("./index.js").createAdapter>} adapter 适配器实例（形态 / 能力来源）
 * @returns {{ view: () => object, onKey: (key: string) => object, focus: (id: string) => string,
 *   getState: () => { focus: string, clicks: number, detail: boolean, exitRequested: boolean } }}
 *   view：当前状态的页面描述树（每次调用返回新树）；focus：宿主直接置焦点；
 *   onKey：共享内核键路由（方向 / 激活 / Back 记忆）。
 */
export function createExamplePage(adapter) {
  if (adapter === null || typeof adapter !== "object" || typeof adapter.platformStyle !== "function") {
    throw new TypeError("adapter-wear: createExamplePage 需 createAdapter 产物");
  }
  const styles = adapter.platformStyle();
  const caps = adapter.capabilities();
  const engine = createWearFocus({
    nodes: pageFocusNodes(tokensByForm[adapter.form]),
    initial: "counter",
  });

  // 宿主可见状态（焦点 / 计数 / 详情 / 退出请求）——纯对象，host 经 view() / getState() 读取。
  const store = { focus: engine.current, clicks: 0, detail: false, exitRequested: false };

  /** 按压 / 激活统一入口（触摸按压与 OK 键同一路）。 */
  const press = (id) => {
    if (id === "counter") store.clicks += 1;
    else if (id === "reset") store.clicks = 0;
    else if (id === "detail") store.detail = !store.detail;
    else if (id === "exit") store.exitRequested = true;
    return Object.freeze({ type: "press", id });
  };

  const onKey = (key) => {
    const result = handleKey(engine, key, { activate: (id) => press(id) });
    store.focus = engine.current;
    // Back 第三层（历史空）：是否退出应用由宿主裁决（内核零副作用）
    if (result.type === "back" && result.to === null) store.exitRequested = true;
    return result;
  };

  /** 宿主直接置焦点（活动域内；勿直接操作内核以免视图失同步）。 */
  const focus = (id) => {
    store.focus = engine.focus(id);
    return store.focus;
  };

  const hint = () =>
    store.exitRequested
      ? "已请求退出（宿主接管）"
      : store.detail
        ? "详情：Wear 演示页"
        : caps.focusNav
          ? "方向键 / OK 键操作"
          : "轻触按钮操作";

  const view = () => {
    // 焦点环：仅 caps.focusNav（无指针降级）时渲染；环色从 focusRing 合并，按钮常驻边框
    // 宽度与环一致 → 聚焦零布局抖动。
    const button = (id, label) => {
      const ring = caps.focusNav && store.focus === id ? styles.focusRing : null;
      return {
        type: "Pressable",
        testID: id,
        accessibilityRole: "button",
        focus: ring !== null,
        style: ring === null ? styles.button : { ...styles.button, ...ring },
        label,
      };
    };
    const row = (children) => ({ type: "View", style: styles.row, children });
    // 安全区外框：caps.safeArea 失活（调试形态）→ 零内边距
    const pad = caps.safeArea
      ? styles.safeArea
      : { top: 0, right: 0, bottom: 0, left: 0 };

    return {
      type: "View",
      testID: "wear-frame",
      style: {
        paddingTop: pad.top,
        paddingRight: pad.right,
        paddingBottom: pad.bottom,
        paddingLeft: pad.left,
      },
      children: [
        {
          type: "View",
          testID: "wear-card",
          style: styles.card,
          children: [
            { type: "Text", style: styles.title, text: "PolyPack App" },
            { type: "Text", style: styles.muted, text: "版本 0.1.0" },
            row([
              button("counter", `计数：${store.clicks}`),
              button("reset", "重置"),
            ]),
            row([button("detail", "详情"), button("exit", "退出")]),
            { type: "Text", style: styles.muted, text: hint() },
          ],
        },
      ],
    };
  };

  return Object.freeze({ view, onKey, focus, getState: () => ({ ...store }) });
}
