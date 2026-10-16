// 几何遍历图推导 + 连通性静态检查器（T-M4-02）：布局盒（rect）→ 方向键邻居。
// 覆盖优先级：节点 neighbors（逐向显式，内核形态，最高）> options.focusOrder（全量线性
// 覆盖）> 几何推导。
// 几何规则（每方向，确定性三重裁定）：候选 = 中心严格位于本方向侧 且 正交轴投影相交
// （重叠 > 0）；取主轴中心距最小 → 正交中心距最小 → 文档序。无候选 = 无邻接
// （内核语义为边界自环——无指针环境不出现死路）。
import { DIRECTIONS, reachableFrom } from "../../native/src/focus.js";

const isNum = (v) => typeof v === "number" && Number.isFinite(v);

const cx = (r) => r.x + r.width / 2;
const cy = (r) => r.y + r.height / 2;

/** 一维区间重叠量（> 0 才算正交投影相交；贴边不算）。 */
const overlap = (a1, a2, b1, b2) => Math.min(a2, b2) - Math.max(a1, b1);

/** 方向谓词：候选中心在 node 的方向侧，且正交轴投影相交。 */
const ELIGIBLE = {
  left: (a, b) => cx(b) < cx(a) && overlap(a.y, a.y + a.height, b.y, b.y + b.height) > 0,
  right: (a, b) => cx(b) > cx(a) && overlap(a.y, a.y + a.height, b.y, b.y + b.height) > 0,
  up: (a, b) => cy(b) < cy(a) && overlap(a.x, a.x + a.width, b.x, b.x + b.width) > 0,
  down: (a, b) => cy(b) > cy(a) && overlap(a.x, a.x + a.width, b.x, b.x + b.width) > 0,
};
const PRIMARY = {
  left: (a, b) => Math.abs(cx(a) - cx(b)),
  right: (a, b) => Math.abs(cx(a) - cx(b)),
  up: (a, b) => Math.abs(cy(a) - cy(b)),
  down: (a, b) => Math.abs(cy(a) - cy(b)),
};
const ORTHO = {
  left: (a, b) => Math.abs(cy(a) - cy(b)),
  right: (a, b) => Math.abs(cy(a) - cy(b)),
  up: (a, b) => Math.abs(cx(a) - cx(b)),
  down: (a, b) => Math.abs(cx(a) - cx(b)),
};

/** 几何邻居：每方向取三重裁定的最优候选（无候选 = 无边）。 */
function geometricNeighbors(node, others) {
  const row = {};
  for (const dir of DIRECTIONS) {
    let best = null;
    for (const other of others) {
      if (other.id === node.id || !ELIGIBLE[dir](node.rect, other.rect)) continue;
      const primary = PRIMARY[dir](node.rect, other.rect);
      const ortho = ORTHO[dir](node.rect, other.rect);
      if (
        best === null ||
        primary < best.primary ||
        (primary === best.primary && ortho < best.ortho)
      ) {
        best = { id: other.id, primary, ortho };
      }
    }
    if (best !== null) row[dir] = best.id;
  }
  return row;
}

/** focusOrder 全量覆盖：按给定序连成线性链（主轴按中心差裁定：竖向优先 down/up，横向 right/left）。 */
function chainNeighbors(nodes, focusOrder) {
  const ids = nodes.map((n) => n.id);
  const valid =
    Array.isArray(focusOrder) &&
    focusOrder.length === ids.length &&
    new Set(focusOrder).size === ids.length &&
    focusOrder.every((id) => ids.includes(id));
  if (!valid) {
    throw new TypeError("focus: focusOrder 须为全部节点 id 的排列（全量覆盖；逐向微调请用节点 neighbors）");
  }
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const row = Object.fromEntries(ids.map((id) => [id, {}]));
  for (let i = 0; i + 1 < focusOrder.length; i += 1) {
    const [a, b] = [focusOrder[i], focusOrder[i + 1]];
    const [ra, rb] = [byId.get(a).rect, byId.get(b).rect];
    if (Math.abs(cy(rb) - cy(ra)) >= Math.abs(cx(rb) - cx(ra))) {
      row[a].down = b;
      row[b].up = a;
    } else {
      row[a].right = b;
      row[b].left = a;
    }
  }
  return row;
}

/**
 * 推导焦点节点集（内核形态）：rect 几何 → neighbors；产物直接喂 createFocusEngine。
 *
 * @param {Array<{id: string, rect: {x, y, width, height}, neighbors?: Record<string, string>}>} nodes
 *   rect 为 1080 参考画布的布局盒；neighbors 为逐向显式覆盖（最高优先级）。
 * @param {{ focusOrder?: string[] }} [options] 显式遍历序（全量覆盖几何推导；order 亦为此序）。
 * @returns {ReadonlyArray<{id: string, neighbors: Readonly<Record<string, string>>}>}
 */
export function deriveFocusNodes(nodes, options = {}) {
  if (!Array.isArray(nodes) || nodes.length === 0) {
    throw new TypeError("focus: nodes 须为非空数组");
  }
  const seen = new Set();
  for (const node of nodes) {
    if (typeof node?.id !== "string" || node.id === "") {
      throw new TypeError("focus: 节点须带非空 id");
    }
    if (seen.has(node.id)) throw new TypeError(`focus: 节点 id 重复: ${node.id}`);
    seen.add(node.id);
    const r = node.rect;
    if (!r || !isNum(r.x) || !isNum(r.y) || !isNum(r.width) || !isNum(r.height)) {
      throw new TypeError(`focus: 节点 ${node.id} 须带 rect { x, y, width, height }（有限数值）`);
    }
    if (r.width <= 0 || r.height <= 0) {
      throw new TypeError(`focus: 节点 ${node.id} 的 rect 宽高须为正数`);
    }
  }
  const ids = nodes.map((n) => n.id);
  const { focusOrder } = options;
  const ordered =
    focusOrder === undefined ? nodes : focusOrder.map((id) => nodes.find((n) => n.id === id));
  const rows =
    focusOrder === undefined
      ? Object.fromEntries(nodes.map((node) => [node.id, geometricNeighbors(node, nodes)]))
      : chainNeighbors(nodes, focusOrder);

  // 显式 neighbors 覆盖（逐向，最高优先级）
  for (const node of nodes) {
    if (node.neighbors === undefined) continue;
    for (const dir of DIRECTIONS) {
      const next = node.neighbors[dir];
      if (next === undefined) continue;
      if (!ids.includes(next)) {
        throw new TypeError(`focus: 未知邻接 ${node.id}.${dir} -> ${next}`);
      }
      rows[node.id][dir] = next;
    }
  }
  return Object.freeze(
    ordered.map((node) => Object.freeze({ id: node.id, neighbors: Object.freeze(rows[node.id]) })),
  );
}

/**
 * 连通性静态检查器（T-M4-02 验收：0 孤立节点 / 无死路）。
 * 「孤立」= 自 initial 不可达（进不去）；「陷阱」= 四向全无出边（出不去，n > 1 时才判）。
 * 已知边界：不做强连通判定（单向边不在此报——无死路出口由 traps + 焦点内核边界自环兜底）。
 *
 * @param {ReturnType<import("./focus.js").createTvFocus>["graph"]} graph 内核实遍历图
 * @param {{ from?: string }} [options] 起点（默认 order[0] = 初始焦点）
 */
export function checkConnectivity(graph, options = {}) {
  if (!graph || !Array.isArray(graph.order) || typeof graph.edges !== "object") {
    throw new TypeError("focus: 需内核遍历图（createTvFocus / createFocusEngine 的 graph）");
  }
  const from = options.from ?? graph.order[0];
  const reachable = reachableFrom(graph, from); // 起点未知即内核抛错
  const isolated = graph.order.filter((id) => !reachable.includes(id));
  const traps =
    graph.order.length > 1
      ? graph.order.filter((id) =>
          DIRECTIONS.every((dir) => {
            const next = graph.edges[id][dir];
            return next === undefined || next === id;
          }),
        )
      : [];
  return Object.freeze({
    from,
    total: graph.order.length,
    reachable: Object.freeze(reachable),
    isolated: Object.freeze(isolated),
    traps: Object.freeze(traps),
    connected: isolated.length === 0,
  });
}
