// Monkey C 子集 / API 白名单断言（T-M7-09 spike）。
//
// 这是"能力边界"的机器可执行部分：只允许转译器真正发射的语言构造与 Toybox API；
// 白名单外的任何调用都判失败，防止生成代码悄悄扩大依赖面。检查是词法级（去注释 /
// 字符串后匹配），不是 Monkey C 语法解析器——边界见 CAPABILITY-BOUNDARY-DRAFT.md §3。

/** using 行白名单：模块 → 别名（不允许多发射一个模块）。 */
export const USING_ALLOWLIST = Object.freeze({
  "Toybox.Application": "App",
  "Toybox.WatchUi": "Ui",
  "Toybox.Graphics": "Gfx",
});

/** Graphics.Dc 白名单（API 文档实证：Graphics.Dc 同页方法）。 */
export const DC_METHODS = Object.freeze(
  new Set(["clear", "setColor", "fillRectangle", "drawRectangle", "drawText", "setPenWidth", "getWidth", "getHeight"]),
);

/** Graphics 常量白名单：字体 / 对齐枚举按官方页逐项登记；颜色按 COLOR_* 形态放行。 */
const GFX_FONTS = Object.freeze(new Set(["FONT_XTINY", "FONT_SMALL"]));
const GFX_JUSTIFY = Object.freeze(
  new Set(["TEXT_JUSTIFY_LEFT", "TEXT_JUSTIFY_RIGHT", "TEXT_JUSTIFY_CENTER", "TEXT_JUSTIFY_VCENTER"]),
);
const isGfxConstant = (name) =>
  GFX_FONTS.has(name) || GFX_JUSTIFY.has(name) || /^COLOR_[A-Z0-9_]+$/.test(name);

/** 类基类型白名单。 */
export const BASE_CLASSES = Object.freeze(new Set(["App.AppBase", "Ui.View", "Ui.BehaviorDelegate"]));

/** 显式排除的语言构造（含 API 侧不做的循环 / 异常 / 枚举等）。 */
const FORBIDDEN_KEYWORDS = Object.freeze([
  "try", "catch", "finally", "throw", "switch", "case", "default", "enum", "typedef",
  "instanceof", "has", "do", "break", "continue", "and", "or", "not", "native", "import",
  "private", "protected", "public", "module", "for", "while",
]);

/** 本工程自有成员/函数（生成器模板的一部分）。 */
const OWN_MEMBERS = Object.freeze(
  new Set([
    "initialize", "getInitialView", "onLayout", "onShow", "onUpdate", "onHide",
    "onSelect", "onBack", "onNextPage", "onPreviousPage", "onKey",
    "activate", "move", "back", "remember", "drawButton", "scaleX", "scaleY",
    "mFocus", "mView",
  ]),
);

/** 语言内置方法白名单。 */
const BUILTIN_METHODS = Object.freeze(new Set(["toString", "equals", "size", "length", "toNumber"]));

/** 裸调用（非点号接收者）白名单 = 自有函数 + 类构造器 + 控制关键字。 */
const BARE_CALLS = Object.freeze(
  new Set([
    "if", "return", "PolypackApp", "PolypackView", "PolypackDelegate",
    ...OWN_MEMBERS,
  ]),
);

/** 去掉注释与字符串字面量（字符串替换为 ""，保留行号）。 */
export function sanitize(source) {
  let out = "";
  let i = 0;
  while (i < source.length) {
    const ch = source[i];
    const next = source[i + 1];
    if (ch === "/" && next === "/") {
      while (i < source.length && source[i] !== "\n") i += 1;
      continue;
    }
    if (ch === "/" && next === "*") {
      i += 2;
      while (i < source.length && !(source[i] === "*" && source[i + 1] === "/")) {
        if (source[i] === "\n") out += "\n";
        i += 1;
      }
      i += 2;
      continue;
    }
    if (ch === '"' || ch === "'") {
      const quote = ch;
      i += 1;
      while (i < source.length && source[i] !== quote) {
        if (source[i] === "\\") i += 1;
        i += 1;
      }
      i += 1;
      out += '""';
      continue;
    }
    out += ch;
    i += 1;
  }
  return out;
}

const lineAt = (text, index) => text.slice(0, index).split("\n").length;

/**
 * 校验一份 Monkey C 源文本。
 *
 * @param {string} name 文件名（报告用）
 * @param {string} source
 * @returns {string[]} violations（空数组 = 通过）
 */
export function checkMonkeyC(name, source) {
  const violations = [];
  const add = (index, message) => violations.push(`${name}:${lineAt(clean, index)}: ${message}`);
  const clean = sanitize(source);

  // 1) 定界符平衡（词法级粗检，挡截断/嵌套错误）。
  const pairs = { "{": "}", "(": ")", "[": "]" };
  const stack = [];
  for (let i = 0; i < clean.length; i += 1) {
    const ch = clean[i];
    if (pairs[ch]) stack.push({ ch, i });
    else if (ch === "}" || ch === ")" || ch === "]") {
      const top = stack.pop();
      if (!top || pairs[top.ch] !== ch) {
        add(i, `定界符不匹配：多余的 "${ch}"`);
        break;
      }
    }
  }
  if (stack.length > 0) add(stack[stack.length - 1].i, `定界符未闭合："${stack[stack.length - 1].ch}"`);

  // 2) using 行：仅允许 3 个模块，且别名固定。
  for (const match of clean.matchAll(/^\s*using\s+([^\n;]+)/gm)) {
    const spec = match[1].trim();
    const m = /^(Toybox\.[A-Za-z]+)\s+as\s+([A-Za-z_][A-Za-z0-9_]*)$/.exec(spec);
    if (!m) {
      add(match.index, `using 形态不在子集内："${spec}"`);
    } else if (USING_ALLOWLIST[m[1]] === undefined) {
      add(match.index, `模块不在白名单："${m[1]}"`);
    } else if (USING_ALLOWLIST[m[1]] !== m[2]) {
      add(match.index, `别名不匹配："${m[1]}" 须为 "${USING_ALLOWLIST[m[1]]}"`);
    }
  }

  // 3) 禁止关键字（词边界；含 API 侧不做循环的 for/while）。
  for (const keyword of FORBIDDEN_KEYWORDS) {
    const re = new RegExp(`\\b${keyword}\\b`, "g");
    for (const match of clean.matchAll(re)) add(match.index, `禁止关键字："${keyword}"`);
  }

  // 4) 类声明：基类必须在白名单。
  for (const match of clean.matchAll(/\bclass\s+([A-Za-z_][A-Za-z0-9_]*)\s+extends\s+([A-Za-z_][A-Za-z0-9_.]*)/g)) {
    if (!BASE_CLASSES.has(match[2])) add(match.index, `基类不在白名单："${match[2]}"`);
  }
  for (const match of clean.matchAll(/\bclass\s+[A-Za-z_][A-Za-z0-9_]*\b(?!\s+extends)/g)) {
    add(match.index, "class 必须显式 extends 白名单基类");
  }

  // 5) 点号 API：按接收者查白名单。
  for (const match of clean.matchAll(/\b([A-Za-z_][A-Za-z0-9_]*)\s*\.\s*([A-Za-z_][A-Za-z0-9_]*)/g)) {
    const [, receiver, member] = match;
    const index = match.index;
    const lineStart = clean.lastIndexOf("\n", index) + 1;
    const linePrefix = clean.slice(lineStart, index).trimStart();
    if (receiver === "Toybox") {
      if (!linePrefix.startsWith("using")) add(index, `直接引用 Toybox 命名空间："Toybox.${member}"`);
    }
    else if (receiver === "Sys" || receiver === "System") add(index, `System API 不在子集："${receiver}.${member}"`);
    else if (receiver === "Gfx") {
      if (!isGfxConstant(member)) add(index, `Graphics 常量不在白名单："Gfx.${member}"`);
    } else if (receiver === "dc") {
      if (!DC_METHODS.has(member)) add(index, `Dc 方法不在白名单："dc.${member}"`);
    } else if (receiver === "Ui") {
      if (!["View", "BehaviorDelegate", "requestUpdate"].includes(member)) add(index, `WatchUi 成员不在白名单："Ui.${member}"`);
    } else if (receiver === "App") {
      if (member !== "AppBase") add(index, `Application 成员不在白名单："App.${member}"`);
    } else if (receiver === "AppBase") {
      if (member !== "initialize") add(index, `AppBase 成员不在白名单："AppBase.${member}"`);
    } else if (receiver === "View" || receiver === "BehaviorDelegate") {
      if (member !== "initialize") add(index, `基类成员不在白名单："${receiver}.${member}"`);
    } else if (!BUILTIN_METHODS.has(member) && !OWN_MEMBERS.has(member)) {
      add(index, `未知成员调用："${receiver}.${member}"`);
    }
  }

  // 6) 裸调用：自有函数 / 构造器 / if。
  for (const match of clean.matchAll(/(?<![.\w])([A-Za-z_][A-Za-z0-9_]*)\s*\(/g)) {
    if (!BARE_CALLS.has(match[1])) add(match.index, `未知函数调用："${match[1]}"`);
  }

  // 7) 构造器：只允许本工程两个 UI 类。
  for (const match of clean.matchAll(/\bnew\s+([A-Za-z_][A-Za-z0-9_]*)/g)) {
    if (!["PolypackView", "PolypackDelegate"].includes(match[1])) add(match.index, `构造器不在白名单："new ${match[1]}"`);
  }

  return violations;
}

/** 对整棵产物文件树跑子集断言；失败抛错（构建脚本直接 [FAIL]）。 */

export function assertSubset(files) {
  const violations = [];
  for (const [path, content] of Object.entries(files)) {
    if (path.endsWith(".mc")) violations.push(...checkMonkeyC(path, content));
  }
  if (violations.length > 0) {
    const error = new Error(`Monkey C 子集断言失败 ${violations.length} 处：\n${violations.join("\n")}`);
    error.violations = violations;
    throw error;
  }
  return true;
}
