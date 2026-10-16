// SceneGraph 子集渲染器（spike，T-M4-11）：受限子集描述 → 频道内两份生成件——
//   components/PolypackScene.xml  结构与视觉（Group/Rectangle/Label/Poster/button 复合）
//   source/focusgraph.brs         焦点遍历表（由共享焦点内核解析生成——边界自环落点）
// 纯函数、无副作用、零依赖（除子集描述与内核口径）；确定性输出（同一子集两次发射逐字节相等）。
// 生成件不得手改：改子集描述（src/subset.js）后重跑 scripts/build-and-check.mjs。

import { createFocusEngine, traversalSnapshot } from "../../tv/src/focus.js";

import { CANVAS, validateSubset } from "./subset.js";

/** #rrggbb → SceneGraph 0xRRGGBBAA（不透明）。 */
export function toSceneGraphColor(hex) {
  if (typeof hex !== "string" || !/^#[0-9a-fA-F]{6}$/.test(hex)) {
    throw new TypeError(`emit: 颜色须为 #rrggbb 形态: ${hex}`);
  }
  return `0x${hex.slice(1).toUpperCase()}FF`;
}

const escText = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const escAttr = (s) => escText(s).replace(/"/g, "&quot;");
const attr = (name, value) => ` ${name}="${escAttr(value)}"`;
const translation = (t) => `[${t[0]},${t[1]}]`;

function elementLines(el, indent) {
  const pad = " ".repeat(indent);
  const tr = el.translation ?? [0, 0];
  const common = (el.id ? attr("id", el.id) : "") + attr("translation", translation(tr));
  switch (el.type) {
    case "group": {
      const lines = [`${pad}<Group${common}>`];
      for (const child of el.children) lines.push(...elementLines(child, indent + 2));
      lines.push(`${pad}</Group>`);
      return lines;
    }
    case "rect":
      return [
        `${pad}<Rectangle${common}${attr("width", el.width)}${attr("height", el.height)}${attr("color", toSceneGraphColor(el.color))}${el.visible === false ? attr("visible", "false") : ""} />`,
      ];
    case "label":
      return [
        `${pad}<Label${common}${attr("width", el.width)}${attr("height", el.height)}${attr("text", el.text)}${attr("font", el.font)}${attr("color", toSceneGraphColor(el.color))}${el.horizAlign ? attr("horizAlign", el.horizAlign) : ""}${el.vertAlign ? attr("vertAlign", el.vertAlign) : ""} />`,
      ];
    case "poster":
      return [
        `${pad}<Poster${common}${attr("width", el.width)}${attr("height", el.height)}${attr("uri", el.uri)} />`,
      ];
    case "button": {
      // 复合糖：可聚焦 Group + 焦点环（发见器切换 visible）+ 底色 + 文字。
      // 节点 id 口径：内核 id X → node_X / ring_X / label_X（BRS 侧按前缀寻址）。
      const rw = el.ringWidth;
      return [
        `${pad}<Group${attr("id", `node_${el.id}`)}${attr("translation", translation(tr))}${attr("focusable", "true")}>`,
        `${pad}  <Rectangle${attr("id", `ring_${el.id}`)}${attr("translation", translation([-rw, -rw]))}${attr("width", el.width + rw * 2)}${attr("height", el.height + rw * 2)}${attr("color", toSceneGraphColor(el.ringColor))}${attr("visible", "false")} />`,
        `${pad}  <Rectangle${attr("id", `btn_${el.id}`)}${attr("width", el.width)}${attr("height", el.height)}${attr("color", toSceneGraphColor(el.color))} />`,
        `${pad}  <Label${attr("id", `label_${el.id}`)}${attr("width", el.width)}${attr("height", el.height)}${attr("text", el.label)}${attr("font", "font:Medium")}${attr("color", toSceneGraphColor(el.labelColor))}${attr("horizAlign", "center")}${attr("vertAlign", "center")} />`,
        `${pad}</Group>`,
      ];
    }
    default:
      throw new TypeError(`emit: 未知元素类型 ${el.type}`);
  }
}

/**
 * 发射频道生成件：{ "components/PolypackScene.xml": …, "source/focusgraph.brs": … }。
 * focusgraph 表 = 共享焦点内核 traversalSnapshot（显式邻接 → 每方向落点解析，边界落点 = 自身），
 * 即 BRS 侧无需再做边界特判——语义在数据里（与内核逐键等价，test/emit.test.js 钉住）。
 */
export function emitChannelFiles(subsetInput) {
  const subset = validateSubset(subsetInput);
  const engine = createFocusEngine({ nodes: subset.focus.nodes, initial: subset.focus.initial });
  const snapshot = traversalSnapshot(engine.graph);

  const xml = [
    '<?xml version="1.0" encoding="utf-8" ?>',
    "<!-- 由 @polypack/adapter-roku spike 发射器生成（T-M4-11）——勿手改：改 src/subset.js 后重跑 scripts/build-and-check.mjs -->",
    `<component name="PolypackScene" extends="Scene"${attr("initialFocus", `node_${subset.focus.initial}`)}>`,
    "  <children>",
    ...subset.elements.flatMap((el) => elementLines(el, 4)),
    "  </children>",
    '  <script type="text/brightscript" uri="pkg:/components/PolypackScene.brs" />',
    "</component>",
    "",
  ].join("\n");

  const table = { order: engine.order, edges: snapshot, initial: subset.focus.initial };
  const brs = [
    "' 由发射器生成（勿手改）——焦点遍历表：每方向落点均在图内（边界落点 = 自身，无死路）。",
    "' 口径源：@polypack/adapter-tv 焦点内核 traversalSnapshot（显式邻接，与 JS 侧逐键等价）。",
    "function ppFocusGraph() as Object",
    `    return ${JSON.stringify(table)}`,
    "end function",
    "",
  ].join("\n");

  return Object.freeze({ "components/PolypackScene.xml": xml, "source/focusgraph.brs": brs });
}

/**
 * 最小 XML 良构检查器（spike 自检用，不经第三方解析器）：标签配对 + 文本段无未转义字符。
 * 只覆盖发射器产出的语法子集（元素/属性/自闭合/注释/XML 声明），够用即止。
 */
export function xmlWellFormed(xml) {
  const body = xml.replace(/<!--[\s\S]*?-->/g, "").replace(/^<\?xml[^>]*\?>\s*/, "");
  const tag = /<(\/?)([A-Za-z_][\w.-]*)((?:\s+[\w.-]+="[^"<>]*")*)\s*(\/?)>/g;
  const stack = [];
  let idx = 0;
  for (let m; (m = tag.exec(body)); ) {
    const text = body.slice(idx, m.index);
    if (/[<>]/.test(text) || /&(?!(amp|lt|gt|quot|apos);)/.test(text)) {
      return { ok: false, error: `文本段含未转义字符: ${JSON.stringify(text.slice(0, 40))}` };
    }
    idx = tag.lastIndex;
    const [, closing, name, , selfClose] = m;
    if (closing) {
      if (stack.pop() !== name) return { ok: false, error: `闭合标签不匹配: ${name}` };
    } else if (!selfClose) {
      stack.push(name);
    }
  }
  if (body.slice(idx).trim() !== "") return { ok: false, error: "尾段残留字符" };
  if (stack.length > 0) return { ok: false, error: `未闭合标签: ${stack.join(", ")}` };
  return { ok: true };
}

export { CANVAS };
