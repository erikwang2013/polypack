// 共享焦点内核（T-M2-05）：方向键遍历图 + 焦点环驱动 + Back 焦点记忆。
// 宿主无关、零依赖——M4 TV（@polypack/adapter-tv、RN-tvOS）复用同一内核：
// 届时只新增遥控器事件源与几何自动布局（显式邻接 → 自动推导），不改本文件接口。

/** 遍历方向（闭合集）。 */
export const DIRECTIONS = Object.freeze(["up", "down", "left", "right"]);

/** 键名 → 方向（键盘与 Android d-pad 两族；宿主事件源负责把平台键码映射到这些键名）。 */
export const KEY_MAP = Object.freeze({
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  DPAD_UP: "up",
  DPAD_DOWN: "down",
  DPAD_LEFT: "left",
  DPAD_RIGHT: "right",
});

/** 激活键（宿主也可走原语自身按压路径，如 RN Pressable.onPress）。 */
const ACTIVATE_KEYS = Object.freeze(["Enter", " ", "DPAD_CENTER"]);

/** Back 键（焦点记忆回退；Android 硬件返回键 / 遥控器返回键映射到此）。 */
const BACK_KEYS = Object.freeze(["Back", "Escape"]);

/**
 * 创建焦点引擎。遍历图为显式邻接（确定性由构造保证，无几何猜测）。
 *
 * @param {{ nodes: Array<{ id: string, neighbors?: Record<string, string> }>, initial?: string }} spec
 *   nodes 顺序即文档序（焦点顺序的确定性来源）；无邻接 = 边界自环（move 原地不动）。
 */
export function createFocusEngine(spec) {
  if (!spec || !Array.isArray(spec.nodes) || spec.nodes.length === 0) {
    throw new TypeError("focus: nodes 须为非空数组");
  }
  const order = [];
  for (const node of spec.nodes) {
    if (typeof node?.id !== "string" || node.id === "") {
      throw new TypeError("focus: 节点须带非空 id");
    }
    if (order.includes(node.id)) {
      throw new TypeError(`focus: 节点 id 重复: ${node.id}`);
    }
    order.push(node.id);
  }
  const edges = {};
  for (const node of spec.nodes) {
    const row = {};
    for (const dir of DIRECTIONS) {
      const next = node.neighbors?.[dir];
      if (next === undefined) continue;
      if (!order.includes(next)) {
        throw new TypeError(`focus: 未知邻接 ${node.id}.${dir} -> ${next}`);
      }
      row[dir] = next;
    }
    edges[node.id] = Object.freeze(row);
  }
  const graph = Object.freeze({ order: Object.freeze(order.slice()), edges: Object.freeze(edges) });

  let current = spec.initial ?? order[0];
  if (!order.includes(current)) {
    throw new TypeError(`focus: initial 不在图中: ${current}`);
  }
  const history = [];

  /** 聚焦指定节点；变更入历史（Back 记忆）。 */
  const focus = (id) => {
    if (!Object.hasOwn(graph.edges, id)) {
      throw new TypeError(`focus: 未知节点 ${id}`);
    }
    if (id !== current) history.push(current);
    current = id;
    return current;
  };

  /** 按方向移动；边界（无邻接）原地不动——无指针环境不出现死路。 */
  const move = (direction) => {
    if (!DIRECTIONS.includes(direction)) {
      throw new TypeError(`focus: 非法方向 ${direction}`);
    }
    const next = graph.edges[current][direction];
    return next === undefined ? current : focus(next);
  };

  /** Back：返回上一焦点；历史为空返回 null（是否退出宿主由调用方决定）。 */
  const back = () => (history.length > 0 ? (current = history.pop()) : null);

  return Object.freeze({
    graph,
    order: graph.order,
    get current() {
      return current;
    },
    focus,
    move,
    back,
  });
}

/**
 * 遍历快照（确定性来源）：order × DIRECTIONS → 落点 id 的纯数据映射。
 * 顺序即文档序，落点为显式邻接或原地；快照断言（test/focus.test.js）用它钉住遍历图。
 */
export function traversalSnapshot(graph) {
  const snapshot = {};
  for (const id of graph.order) {
    const row = {};
    for (const dir of DIRECTIONS) {
      const next = graph.edges[id][dir];
      row[dir] = next === undefined ? id : next;
    }
    snapshot[id] = row;
  }
  return snapshot;
}

/** 从 fromId 出发按遍历图可达的全部节点（BFS，按文档序）——「无死路」断言的证明手段。 */
export function reachableFrom(graph, fromId) {
  if (!graph.order.includes(fromId)) {
    throw new TypeError(`focus: 未知起点 ${fromId}`);
  }
  const seen = new Set([fromId]);
  const queue = [fromId];
  while (queue.length > 0) {
    const id = queue.shift();
    for (const dir of DIRECTIONS) {
      const next = graph.edges[id][dir];
      if (next !== undefined && !seen.has(next)) {
        seen.add(next);
        queue.push(next);
      }
    }
  }
  return graph.order.filter((id) => seen.has(id));
}

/** 无死路判定：任一 (节点, 方向) 的落点都在图内（构造性成立；测试作为回归哨兵）。 */
export function hasNoDeadEnds(graph) {
  return graph.order.every((id) =>
    DIRECTIONS.every((dir) => {
      const next = graph.edges[id][dir];
      return next === undefined || graph.order.includes(next);
    }),
  );
}

/**
 * 键事件路由（宿主事件源 → 焦点内核）。返回结果描述符，供宿主记录与冒烟断言。
 *
 * @param {ReturnType<typeof createFocusEngine>} engine
 * @param {string} key 键名（见 KEY_MAP / ACTIVATE_KEYS / BACK_KEYS）
 * @param {{ activate?: () => void }} [handlers]
 */
export function handleKey(engine, key, handlers = {}) {
  if (ACTIVATE_KEYS.includes(key)) {
    const id = engine.current; // 先取后调：处理器内若移动焦点，回报的 id 仍是激活时的节点
    handlers.activate?.(id);
    return { type: "activate", id };
  }
  if (BACK_KEYS.includes(key)) {
    const from = engine.current;
    const to = engine.back();
    return { type: "back", from, to };
  }
  const direction = KEY_MAP[key];
  if (direction === undefined) {
    return { type: "ignored", key };
  }
  const from = engine.current;
  const to = engine.move(direction);
  return { type: "move", direction, from, to };
}
