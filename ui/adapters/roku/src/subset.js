// SceneGraph 子集渲染器的受限词汇（spike，T-M4-11）：元素类型 = 白名单、每类属性 = 封闭集，
// 未知即抛错——「可承载组件子集清单」的机器可读形态（与 CAPABILITY-BOUNDARY-DRAFT.md §支持项同表）。
//
// 示例子集与 @polypack/adapter-tv 的示例页面同源：取示例页 2 按钮核心（counter / reset +
// OK 激活 + Back 焦点记忆；标题 / 版本行 / 提示行同语义）。tv 示例页在 T-M4-02/03 已演进为
// 4 节点网格 + 分层 Back，spike 按 2 按钮核心收敛；焦点图口径与 tv 同源——布局盒（rect）
// 交 adapter-tv T-M4-02 几何推导（deriveFocusNodes），零手写邻接。
// 差异只在表达宿主：布局为手算坐标（SceneGraph 无 flex——能力边界文档记录的缺口之一），
// 数值口径 = Roku 设计画布 1920×1080（fhd）自有 + 安全区/配色/间距自 adapter-tv tokens 派生。

import { tokens, safeAreaInsets } from "../../tv/src/tokens.js";

/** 受支持元素类型 → 封闭属性集。button 为复合糖：Group(focusable) + 环 Rectangle + 底 Rectangle + Label。 */
export const SUBSET_ELEMENTS = Object.freeze({
  group: Object.freeze({ props: Object.freeze(["id", "translation", "children"]) }),
  rect: Object.freeze({
    props: Object.freeze(["id", "translation", "width", "height", "color", "visible"]),
  }),
  label: Object.freeze({
    props: Object.freeze([
      "id", "translation", "width", "height", "text", "color", "font", "horizAlign", "vertAlign",
    ]),
  }),
  poster: Object.freeze({
    props: Object.freeze(["id", "translation", "width", "height", "uri"]),
  }),
  button: Object.freeze({
    props: Object.freeze([
      "id", "translation", "width", "height", "label", "color", "labelColor", "ringColor", "ringWidth",
    ]),
  }),
});

/** 抽象字级 → Roku 内置字体 role（近似；精确 px 需 Font 节点或 BRS 赋值——见文档 §缺口）。 */
export const FONT_ROLES = Object.freeze({
  title: "font:Large",
  body: "font:Medium",
  small: "font:Small",
});

/** Roku 设计画布（fhd）。坐标即 SceneGraph px。 */
export const CANVAS = Object.freeze({ width: 1920, height: 1080 });

const validateNode = (node, path) => {
  if (node === null || typeof node !== "object" || Array.isArray(node)) {
    throw new TypeError(`subset: ${path} 须为对象`);
  }
  const spec = SUBSET_ELEMENTS[node.type];
  if (!spec) {
    throw new TypeError(
      `subset: ${path} 未知元素类型 "${node.type}"（支持: ${Object.keys(SUBSET_ELEMENTS).join(", ")}）`,
    );
  }
  for (const key of Object.keys(node)) {
    if (key !== "type" && !spec.props.includes(key)) {
      throw new TypeError(`subset: ${path} 类型 ${node.type} 不支持属性 "${key}"`);
    }
  }
  if (node.children !== undefined) {
    if (node.type !== "group") throw new TypeError(`subset: ${path} 仅 group 可带 children`);
    node.children.forEach((child, i) => validateNode(child, `${path}.children[${i}]`));
  }
};

/**
 * 子集描述校验（发射前 fail fast）：元素词汇封闭 + 焦点图邻接目标在图内 + initial 落点存在。
 * 焦点图 = 布局盒几何推导（adapter-tv T-M4-02 口径，deriveFocusNodes 在发射器侧调用；
 * 节点 rect 缺失/非法由其抛错）；显式 neighbors 可逐向覆盖，此处校验其目标在图内。
 */
export function validateSubset(subset) {
  if (!subset || typeof subset !== "object") throw new TypeError("subset: 须为对象");
  if (!Array.isArray(subset.elements) || subset.elements.length === 0) {
    throw new TypeError("subset: elements 须为非空数组");
  }
  subset.elements.forEach((el, i) => validateNode(el, `elements[${i}]`));
  const nodes = subset.focus?.nodes;
  const initial = subset.focus?.initial;
  if (!Array.isArray(nodes) || nodes.length === 0) throw new TypeError("subset: focus.nodes 须为非空数组");
  const ids = nodes.map((n) => n.id);
  if (new Set(ids).size !== ids.length) throw new TypeError("subset: focus.nodes id 重复");
  if (!ids.includes(initial)) throw new TypeError(`subset: focus.initial 不在图中: ${initial}`);
  for (const node of nodes) {
    for (const [dir, target] of Object.entries(node.neighbors ?? {})) {
      if (!ids.includes(target)) throw new TypeError(`subset: 未知邻接 ${node.id}.${dir} -> ${target}`);
    }
  }
  return subset;
}

/**
 * 示例页面子集（TV 示例页语义的 SceneGraph 表达；布局手算，堆叠高度自洽推导）。
 * 派生：安全区 = safeAreaInsets(CANVAS)（adapter-tv 口径），间距/配色 = adapter-tv tokens。
 */
export function createExampleSubset() {
  const insets = safeAreaInsets(CANVAS); // 5% → { top: 54, right: 96, bottom: 54, left: 96 }
  const pad = tokens.space[4]; // 48
  const gapSm = tokens.space[2]; // 16
  const gapXs = tokens.space[1]; // 8
  const cardWidth = 1280; // 与 adapter-tv platformStyle().card.maxWidth 同值
  const border = 1; // 描边经双层矩形（SceneGraph Rectangle 无 border 字段——见文档 §缺口）
  const inner = cardWidth - border * 2;
  const titleH = 60;
  const lineH = 44;
  const button = { width: 560, height: 80, ringWidth: 4 };

  // 纵向堆叠（相对内容原点）；卡高 = 堆叠底 + 底边距，自洽推导（无 flex 的手算模型）
  const stackY = {
    title: pad,
    version: pad + titleH + gapXs,
    counter: pad + titleH + gapXs + lineH + pad,
    reset: pad + titleH + gapXs + lineH + pad + button.height + gapSm,
    hint: pad + titleH + gapXs + lineH + pad + button.height + gapSm + button.height + pad,
  };
  const cardHeight = stackY.hint + lineH + pad;
  const cardX = Math.round((CANVAS.width - cardWidth) / 2) - insets.left;
  const cardY = Math.round((CANVAS.height - cardHeight) / 2) - insets.top;
  const buttonX = Math.round((inner - button.width) / 2);

  const buttonBlock = (id, label, y) => ({
    type: "button",
    id,
    translation: [buttonX, y],
    width: button.width,
    height: button.height,
    label,
    color: tokens.color.brand,
    labelColor: "#ffffff",
    ringColor: tokens.color.accent,
    ringWidth: button.ringWidth,
  });

  // 焦点图节点 = 各按钮画布绝对盒（frame+card+content 平移累加）——几何推导的输入。
  const focusNode = (id, y) => ({
    id,
    rect: {
      x: insets.left + cardX + border + buttonX,
      y: insets.top + cardY + border + y,
      width: button.width,
      height: button.height,
    },
  });

  const subset = {
    elements: [
      { type: "rect", id: "bg", translation: [0, 0], width: CANVAS.width, height: CANVAS.height, color: tokens.color.text },
      {
        type: "group",
        id: "frame",
        translation: [insets.left, insets.top],
        children: [
          {
            type: "group",
            id: "card",
            translation: [cardX, cardY],
            children: [
              { type: "rect", id: "card_border", width: cardWidth, height: cardHeight, color: tokens.color.brandStrong },
              { type: "rect", id: "card_fill", translation: [border, border], width: inner, height: cardHeight - border * 2, color: "#ffffff" },
              {
                type: "group",
                id: "content",
                translation: [border, border],
                children: [
                  { type: "label", id: "title", translation: [0, stackY.title], width: inner, height: titleH, text: "PolyPack App", font: FONT_ROLES.title, color: tokens.color.text, horizAlign: "center" },
                  { type: "label", id: "version", translation: [0, stackY.version], width: inner, height: lineH, text: "版本 0.1.0 · 由 PolyPack 构建（poly build --target roku）", font: FONT_ROLES.small, color: tokens.color.muted, horizAlign: "center" },
                  buttonBlock("counter", "点击计数：0", stackY.counter),
                  buttonBlock("reset", "重置", stackY.reset),
                  { type: "label", id: "hint", translation: [0, stackY.hint], width: inner, height: lineH, text: "遥控器：方向键移动焦点；OK 激活；Back 返回上一焦点。", font: FONT_ROLES.small, color: tokens.color.muted, horizAlign: "center" },
                ],
              },
            ],
          },
        ],
      },
    ],
    focus: {
      initial: "counter",
      nodes: [focusNode("counter", stackY.counter), focusNode("reset", stackY.reset)],
    },
  };
  return subset;
}
