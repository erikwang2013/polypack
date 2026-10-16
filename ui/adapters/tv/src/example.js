// 示例页面（T-M4-01 验收的「同一页面组件树」）：与 poly init 模板页
// （crates/poly-cli/templates/app/ui/main.jsx）语义同形——图标 / 标题 / 版本行 /
// 计数按钮 / 提示行；TV 增量：10-foot 蒙皮、安全区外框、焦点环（遥控器无指针恒开）。
// 原语由宿主注入（react-dom 元素）；测试传 DOM 标签做代理渲染（test/helpers/dom-primitives.js）。
// 可焦点节点带 data-focus="true"（当前焦点锚点）——T-M4-13 遥控器冒烟的 DOM 观测通道。

import { createElement as h, useSyncExternalStore } from "react";

import { EXAMPLE_FOCUS_NODES } from "./example-nodes.js";
import { createFocusEngine, handleKey } from "./focus.js";

export { EXAMPLE_FOCUS_NODES } from "./example-nodes.js";

/**
 * 创建示例页面（宿主装配的最后一个环节）。
 *
 * @param {object} primitives react-dom 原语（View / Text / Pressable / Image / iconSource）
 * @param {ReturnType<import("./index.js").createAdapter>} adapter
 * @returns {{ Page: Function, onKey: (key: string) => object, focus: (id: string) => string,
 *   getState: () => { focus: string, clicks: number } }}
 *   onKey：宿主事件源统一入口（键盘 / 遥控器）——遍历 / 激活全部经此（T-M4-03 起含 Back / 数字键）。
 *   focus：宿主直接置焦点；getState：宿主诊断与冒烟断言。
 */
export function createExamplePage(primitives, adapter) {
  const { View, Text, Pressable, Image, iconSource } = primitives;
  const styles = adapter.platformStyle();
  const caps = adapter.capabilities();
  const engine = createFocusEngine({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });

  // 宿主可见状态（焦点 + 计数）经 useSyncExternalStore 供 React 订阅：核心任务在 React 之外
  // 可驱动（无指针环境的 OK 键通路），与 adapter-native / adapter-taro 口径一致。
  const store = { focus: engine.current, clicks: 0 };
  const listeners = new Set();
  const subscribe = (listener) => {
    listeners.add(listener);
    return () => listeners.delete(listener);
  };
  const emit = () => {
    for (const listener of listeners) listener();
  };

  /** 按压 / 激活统一入口（原语 onPress 与中心键/OK 同一路）。 */
  const press = (id) => {
    store.clicks = id === "counter" ? store.clicks + 1 : 0;
    emit();
    return { type: "press", id };
  };

  const onKey = (key) => {
    const result = handleKey(engine, key, { activate: (id) => press(id) });
    if (result.type !== "ignored") {
      store.focus = engine.current;
      emit();
    }
    return result;
  };

  /** 宿主直接置焦点（带通知；勿直接操作内核以免视图失同步）。 */
  const focus = (id) => {
    store.focus = engine.focus(id);
    emit();
    return store.focus;
  };

  function Page() {
    const focus2 = useSyncExternalStore(subscribe, () => store.focus, () => store.focus);
    const clicks = useSyncExternalStore(subscribe, () => store.clicks, () => store.clicks);
    // 焦点环：无指针（focusNav）环境渲染——TV 基线恒真。
    // 样式为 react-dom 合法对象（单对象合并，非数组）；button 常驻透明边框（同环宽）→ 聚焦零抖动。
    const ring = (id) => (caps.focusNav && focus2 === id ? styles.focusRing : null);
    const button = (id, label) =>
      h(
        Pressable,
        {
          testID: id,
          "data-focus": focus2 === id ? "true" : undefined,
          style: ring(id) ? { ...styles.button, ...ring(id) } : styles.button,
          onPress: () => press(id),
        },
        h(Text, { style: styles.buttonLabel }, label),
      );

    return h(
      View,
      // 安全区外框：百分比内边距（platformStyle().safeArea）防过扫描遮挡
      {
        testID: "tv-frame",
        style: {
          paddingTop: styles.safeArea.top,
          paddingRight: styles.safeArea.right,
          paddingBottom: styles.safeArea.bottom,
          paddingLeft: styles.safeArea.left,
        },
      },
      h(
        View,
        { testID: "tv-card", style: styles.card },
        // 图标资产由 Pax 派生管线产出（T-M1-14）；未就位时省略节点而非渲染空图
        iconSource
          ? h(Image, { source: iconSource, accessibilityLabel: "Pax", style: { width: 128, height: 128 } })
          : null,
        h(Text, { style: styles.title }, "PolyPack App"),
        h(
          Text,
          { style: styles.muted },
          `版本 0.1.0 · 由 PolyPack 构建（poly build --target ${adapter.target}）`,
        ),
        button("counter", `点击计数：${clicks}`),
        button("reset", "重置"),
        h(
          Text,
          { style: styles.muted },
          caps.focusNav
            ? "遥控器：方向键移动焦点；OK 激活。焦点环随焦点移动。"
            : "点击按钮计数。",
        ),
      ),
    );
  }

  return Object.freeze({ Page, onKey, focus, getState: () => ({ ...store }) });
}
