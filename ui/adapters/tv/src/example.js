// 示例页面（T-M4-01/02/03 验收的「同一页面组件树」）：与 poly init 模板页
// （crates/poly-cli/templates/app/ui/main.jsx）语义同形——图标 / 标题 / 版本行 /
// 计数按钮 / 提示行；TV 增量：10-foot 蒙皮、安全区外框、2×2 几何遍历网格（四向可达）、
// 焦点环（遥控器无指针恒开）、详情弹层 + 播放器子层（分层 Back 逐层回退 + 数字键快捷）。
// 原语由宿主注入（react-dom 元素）；测试传 DOM 标签做代理渲染（test/helpers/dom-primitives.js）。
// 可焦点节点带 data-focus="true"（当前焦点锚点）——T-M4-13 遥控器冒烟的 DOM 观测通道；
// 层叠时仅活动域（栈顶层）节点可成为锚点。

import { createElement as h, useSyncExternalStore } from "react";

import {
  EXAMPLE_FOCUS_NODES,
  TV_MODAL_FOCUS_NODES,
  TV_PLAYER_FOCUS_NODES,
} from "./example-nodes.js";
import { createTvFocus, handleTvKey } from "./focus.js";
import { createLayerStack } from "./layers.js";
import { createShortcutMap } from "./shortcuts.js";

export { EXAMPLE_FOCUS_NODES, TV_MODAL_FOCUS_NODES, TV_PLAYER_FOCUS_NODES } from "./example-nodes.js";

/**
 * 创建示例页面（宿主装配的最后一个环节）。
 *
 * @param {object} primitives react-dom 原语（View / Text / Pressable / Image / iconSource）
 * @param {ReturnType<import("./index.js").createAdapter>} adapter
 * @returns {{ Page: Function, onKey: (key: string) => object, focus: (id: string) => string,
 *   getState: () => { focus: string, clicks: number, depth: number, exitRequested: boolean } }}
 *   onKey：宿主事件源统一入口（键盘 / 遥控器）——handleTvKey 三层 Back + 数字键路由。
 *   focus：宿主直接置焦点（活动域）；getState：宿主诊断与冒烟断言。
 */
export function createExamplePage(primitives, adapter) {
  const { View, Text, Pressable, Image, iconSource } = primitives;
  const styles = adapter.platformStyle();
  const caps = adapter.capabilities();
  const engine = createTvFocus({ nodes: EXAMPLE_FOCUS_NODES, initial: "counter" });
  const layers = createLayerStack();
  // 数字键快捷：1 计数 / 2 重置 / 3 详情（宿主域直达；层打开时被 handleTvKey 拦截）
  const shortcuts = createShortcutMap({ 1: "counter", 2: "reset", 3: "detail" });
  const session = { engine, layers, shortcuts };

  // 宿主可见状态（焦点 / 计数 / 层深 / 退出请求）经 useSyncExternalStore 供 React 订阅：
  // 核心任务在 React 之外可驱动（无指针环境的 OK 键通路），与 adapter-native / adapter-taro 口径一致。
  const store = { focus: engine.current, clicks: 0, depth: 0, exitRequested: false };
  const listeners = new Set();
  const subscribe = (listener) => {
    listeners.add(listener);
    return () => listeners.delete(listener);
  };
  const emit = () => {
    for (const listener of listeners) listener();
  };
  /** 活动域（栈顶层或宿主）→ 视图可见状态。 */
  const sync = () => {
    store.focus = (layers.current ?? engine).current;
    store.depth = layers.depth;
  };

  /** 按压 / 激活统一入口（原语 onPress 与中心键/OK 同一路）。 */
  const press = (id) => {
    if (id === "counter") store.clicks += 1;
    else if (id === "reset") store.clicks = 0;
    else if (id === "detail") layers.open({ id: "detail", nodes: TV_MODAL_FOCUS_NODES, initial: "modal-play" });
    else if (id === "modal-play") layers.open({ id: "player", nodes: TV_PLAYER_FOCUS_NODES, initial: "player-stop" });
    else if (id === "modal-close" || id === "player-stop") layers.close();
    sync();
    emit();
    return { type: "press", id };
  };

  const onKey = (key) => {
    const result = handleTvKey(session, key, { activate: (id) => press(id) });
    if (result.type === "back" && result.route === "exit") store.exitRequested = true;
    sync();
    if (result.type !== "ignored") emit();
    return result;
  };

  /** 宿主直接置焦点（带通知；活动域内；勿直接操作内核以免视图失同步）。 */
  const focus = (id) => {
    store.focus = (layers.current ?? engine).focus(id);
    emit();
    return store.focus;
  };

  function Page() {
    const focused = useSyncExternalStore(subscribe, () => store.focus, () => store.focus);
    const clicks = useSyncExternalStore(subscribe, () => store.clicks, () => store.clicks);
    const depth = useSyncExternalStore(subscribe, () => store.depth, () => store.depth);
    const exitRequested = useSyncExternalStore(
      subscribe,
      () => store.exitRequested,
      () => store.exitRequested,
    );
    // 焦点环：TV 基线 focusNav 恒真；节点 id 全域唯一 → 活动域外节点不可能命中
    //（层打开时 store.focus 为层内节点，宿主按钮环自动熄灭）。
    // 样式为 react-dom 合法对象（单对象合并，非数组）；button 常驻透明边框（同环宽）→ 聚焦零抖动。
    const ring = (id) => (caps.focusNav && focused === id ? styles.focusRing : null);
    const button = (id, label) =>
      h(
        Pressable,
        {
          testID: id,
          "data-focus": focused === id ? "true" : undefined,
          style: ring(id) ? { ...styles.button, ...ring(id) } : styles.button,
          onPress: () => press(id),
        },
        h(Text, { style: styles.buttonLabel }, label),
      );
    const row = (children) =>
      h(
        View,
        { style: { display: "flex", justifyContent: "center", gap: styles.space[3] } },
        children,
      );
    const topLayer = layers.depth > 0 ? layers.ids[depth - 1] : null;

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
        row([button("counter", `点击计数：${clicks}`), button("reset", "重置")]),
        row([button("detail", "详情"), button("exit", "退出")]),
        h(
          Text,
          { style: styles.muted },
          exitRequested
            ? "已请求退出应用（宿主接管）。"
            : depth > 0
              ? "返回键：逐层关闭叠加层并恢复原焦点。"
              : caps.focusNav
                ? "遥控器：四向移动 / OK 激活；返回键逐层回退；数字键 1-3 直达（计数 / 重置 / 详情）。"
                : "点击按钮交互。",
        ),
      ),
      // 叠加层（详情弹层 / 播放器子层）：显示栈顶层的焦点域（域隔离——下层不可聚焦）
      topLayer === null
        ? null
        : h(
            View,
            { testID: "tv-overlay", style: styles.overlay },
            h(
              View,
              { testID: "tv-modal", style: styles.card },
              h(Text, { style: styles.title }, topLayer === "player" ? "播放器" : "详情"),
              topLayer === "player"
                ? row([button("player-stop", "停止")])
                : row([button("modal-play", "播放"), button("modal-close", "关闭")]),
            ),
          ),
    );
  }

  return Object.freeze({ Page, onKey, focus, getState: () => ({ ...store }) });
}
