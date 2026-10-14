// 示例页面（T-M2-04 验收的「同一示例页面组件树」）：与 poly init 模板页
// （crates/poly-cli/templates/app/ui/main.jsx）语义同形——图标 / 标题 / 版本行 /
// 计数按钮 / 提示行；差异仅在渲染原语（RN View/Text/Pressable vs DOM）。
// 原语由宿主注入：宿主传 react-native；测试传 DOM 标签做代理渲染（test/helpers/dom-primitives.js）。

import { createElement as h, useSyncExternalStore } from "react";

import { EXAMPLE_FOCUS_NODES } from "./example-nodes.js";
import { createFocusEngine, handleKey } from "./focus.js";

export { EXAMPLE_FOCUS_NODES } from "./example-nodes.js";

/**
 * 创建示例应用（宿主装配的最后一个环节）。
 *
 * @param {object} primitives RN 原语（View / Text / Pressable / Image / iconSource）
 * @param {ReturnType<import("./index.js").createAdapter>} adapter
 * @returns {{ App: Function, onKey: (key: string) => object, focus: (id: string) => string,
 *   getState: () => { focus: string, clicks: number } }}
 *   onKey：宿主事件源统一入口（键盘 / d-pad / 遥控器）——遍历 / 激活 / Back 全部经此。
 *   focus：宿主直接置焦点（带视图通知）；getState：宿主诊断与冒烟断言。
 */
export function createExampleApp(primitives, adapter) {
  const { View, Text, Pressable, Image, iconSource } = primitives;
  const styles = adapter.platformStyle();
  const caps = adapter.capabilities();
  const engine = createFocusEngine({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });

  // 宿主可见状态（焦点 + 计数）经 useSyncExternalStore 供 React 订阅。
  // 计数放在内核侧而非组件 useState：中心键激活（DPAD_CENTER）须在 React 之外驱动核心任务
  // ——无指针环境的按压通路（T-M2-05 冒烟形态）；原语 onPress 与 onKey 走同一 press()，幂等。
  const store = { focus: engine.current, clicks: 0 };
  const listeners = new Set();
  const subscribe = (listener) => {
    listeners.add(listener);
    return () => listeners.delete(listener);
  };
  const emit = () => {
    for (const listener of listeners) listener();
  };

  /** 按压 / 激活统一入口。 */
  const press = (id) => {
    store.clicks = id === "counter" ? store.clicks + 1 : 0;
    emit();
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

  function ExampleApp() {
    const focused = useSyncExternalStore(subscribe, () => store.focus, () => store.focus);
    const clicks = useSyncExternalStore(subscribe, () => store.clicks, () => store.clicks);
    // 无指针（focusNav）环境才渲染焦点环——降级由 capabilities 声明驱动（ROADMAP §6）
    const ring = (id) => (caps.focusNav && focused === id ? styles.focusRing : null);

    return h(
      View,
      { style: styles.card },
      // 图标资产由 Pax 派生管线产出（T-M1-14）；未就位时省略节点而非渲染空图
      iconSource
        ? h(Image, { source: iconSource, accessibilityLabel: "Pax", style: { width: 64, height: 64 } })
        : null,
      h(Text, { style: styles.title }, "PolyPack App"),
      h(Text, { style: styles.muted }, "版本 0.1.0 · 由 PolyPack 构建（poly build --target android）"),
      h(
        Pressable,
        { style: [styles.button, ring("counter")], onPress: () => press("counter") },
        h(Text, { style: styles.buttonLabel }, `点击计数：${clicks}`),
      ),
      h(
        Pressable,
        { style: [styles.button, ring("reset")], onPress: () => press("reset") },
        h(Text, { style: styles.buttonLabel }, "重置"),
      ),
      h(Text, { style: styles.muted }, "方向键上下切换焦点；中心键激活；Back 返回上一焦点。"),
    );
  }

  return Object.freeze({ App: ExampleApp, onKey, focus, getState: () => ({ ...store }) });
}
