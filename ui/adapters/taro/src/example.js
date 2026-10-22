// 示例页面（T-M3-05 验收的「同一页面组件树」）：与 polypack init 模板页
// （crates/polypack-cli/templates/app/ui/main.jsx）语义同形——图标 / 标题 / 版本行 / 计数按钮 /
// 提示行；差异仅在渲染原语（Taro View/Text/Button/Image vs RN / DOM）。
// 原语由宿主注入：宿主传 @tarojs/components；测试传 DOM 标签做代理渲染（test/helpers/dom-primitives.js）。

import { createElement as h, useSyncExternalStore } from "react";

/**
 * 创建示例页面（宿主装配的最后一个环节）。
 *
 * @param {object} primitives Taro 原语（View / Text / Button / Image / iconSource）
 * @param {ReturnType<import("./index.js").createAdapter>} adapter
 * @returns {{ Page: Function, press: (id: string) => object, getState: () => { clicks: number } }}
 *   press：交互统一入口（原语 onClick 与宿主程序化触发同一路）。
 *   getState：宿主诊断与冒烟断言（devtools 走查的期望值源）。
 */
export function createExamplePage(primitives, adapter) {
  const { View, Text, Button, Image, iconSource } = primitives;
  const styles = adapter.platformStyle();
  const caps = adapter.capabilities();

  // 计数放在内核侧而非组件 useState：宿主（devtools / 契约测试）可在 React 之外驱动核心任务，
  // 与 adapter-native 的宿主可见状态口径一致（宿主经 useSyncExternalStore 订阅）。
  const store = { clicks: 0 };
  const listeners = new Set();
  const subscribe = (listener) => {
    listeners.add(listener);
    return () => listeners.delete(listener);
  };
  const emit = () => {
    for (const listener of listeners) listener();
  };

  /** 按压 / 激活统一入口（幂等：原语 onClick 与宿主触发走同一路）。 */
  const press = (id) => {
    store.clicks = id === "counter" ? store.clicks + 1 : 0;
    emit();
    return { type: "press", id };
  };

  function Page() {
    const clicks = useSyncExternalStore(subscribe, () => store.clicks, () => store.clicks);
    return h(
      View,
      { style: styles.card },
      // 图标资产由 Pax 派生管线产出（T-M1-14）；未就位时省略节点而非渲染空图
      iconSource ? h(Image, { src: iconSource, style: { width: 64, height: 64 } }) : null,
      h(Text, { style: styles.title }, "PolyPack App"),
      h(
        Text,
        { style: styles.muted },
        `版本 0.1.0 · 由 PolyPack 构建（polypack build --target ${adapter.target}）`,
      ),
      // testID → Taro 的 `id` 属性：devtools 走查与契约测试的节点锚点
      h(
        Button,
        { id: "counter", style: styles.button, onClick: () => press("counter") },
        h(Text, { style: styles.buttonLabel }, `点击计数：${clicks}`),
      ),
      h(
        Button,
        { id: "reset", style: styles.button, onClick: () => press("reset") },
        h(Text, { style: styles.buttonLabel }, "重置"),
      ),
      h(
        Text,
        { style: styles.muted },
        caps.focusNav
          ? "无指针容器：方向键切换焦点，中心键激活。"
          : "点击按钮计数；页面生命周期由宿主接线（routes.js 映射表）。",
      ),
    );
  }

  return Object.freeze({ Page, press, getState: () => ({ ...store }) });
}
