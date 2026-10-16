// 分层焦点域（T-M4-03）：每层是独立焦点引擎（独立遍历域与焦点记忆）。
// 「关闭层」= 域切换——宿主域（或其下某层）引擎原地不动 → 焦点记忆结构性恢复，
// 不污染内核历史栈；层内四向 / 激活只在层引擎内路由（域隔离）。
// Back 三层语义由 handleTvKey 路由：层打开 → 关层；无层 → 焦点记忆回退；历史空 → 退出应用。

import { createTvFocus } from "./focus.js";

/**
 * 层栈（子页 / 弹层逐层叠加）。层 id 在栈内唯一（重入 id 快速失败——堆栈语义）。
 *
 * @returns {{ open: Function, close: Function, current: object|null, depth: number, ids: readonly string[] }}
 *   open({ id, nodes, initial, focusOrder }) → 层焦点引擎；close() → { id, focus } | null（空栈）。
 *   current：栈顶层焦点引擎（无层为 null）；ids：自底向顶的层 id。
 */
export function createLayerStack() {
  const stack = [];

  return Object.freeze({
    open(spec) {
      if (spec === null || typeof spec !== "object" || typeof spec.id !== "string" || spec.id === "") {
        throw new TypeError("layers: 层须带非空 id");
      }
      if (stack.some((layer) => layer.id === spec.id)) {
        throw new TypeError(`layers: 层 id 重复: ${spec.id}`);
      }
      const engine = createTvFocus({
        nodes: spec.nodes,
        initial: spec.initial,
        focusOrder: spec.focusOrder,
      });
      stack.push({ id: spec.id, engine });
      return engine;
    },
    /** 关闭栈顶层（宿主域引擎不受影响——焦点恢复是结构性的）。 */
    close() {
      const top = stack.pop();
      return top === undefined ? null : Object.freeze({ id: top.id, focus: top.engine.current });
    },
    get current() {
      return stack.length === 0 ? null : stack[stack.length - 1].engine;
    },
    get depth() {
      return stack.length;
    },
    get ids() {
      return Object.freeze(stack.map((layer) => layer.id));
    },
  });
}
