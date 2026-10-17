// Connect IQ 工程结构自检（T-M7-09 spike）：不依赖 SDK / monkeyc 的静态断言。
//
// 覆盖三块（对应任务"包结构检查真跑"）：
//  1. 文件树：路径安全（拒绝绝对路径 / `..` / 反斜杠）+ 必需文件齐全；
//  2. manifest.xml / monkey.jungle / strings.xml：最小 XML 扫描 + Connect IQ 关键字段；
//  3. .iq 包结构：`manifest.xml` + ≥1 个 `.prg`（导出产物清单断言，不解压内容）。
//
// 语义边界：这是结构自检，不替代 Connect IQ 编译器的语法/语义校验（见结论 §3）。

import { assertSubset } from "./subset.js";

/** `.iq` 工程必需文件（monkey.jungle 的 sourcePath/resourcePath 指向的目录由文件树隐含）。 */
export const REQUIRED_FILES = Object.freeze([
  "manifest.xml",
  "monkey.jungle",
  "resources/strings/strings.xml",
  "source/PolypackApp.mc",
  "source/PolypackView.mc",
  "source/PolypackDelegate.mc",
]);

const IQ_NAMESPACE = "http://www.garmin.com/xml/connectiq";

export const isSafePath = (path) =>
  typeof path === "string" &&
  path.length > 0 &&
  !path.startsWith("/") &&
  !path.includes("\\") &&
  path.split("/").every((part) => part !== "" && part !== "." && part !== "..");

/**
 * 最小 XML 扫描器：返回元素标签序列并校验定界符平衡。
 * 只支持工程实际用到的 XML 子集（声明 / 注释 / 元素 / 属性 / 文本）；拒绝 DOCTYPE。
 */
export function parseXml(text) {
  if (typeof text !== "string") throw new TypeError("XML 须为字符串");
  if (/<!DOCTYPE/i.test(text)) throw new Error("XML 不允许 DOCTYPE");
  const elements = [];
  const stack = [];
  const tokenRe = /<\?[\s\S]*?\?>|<!--[\s\S]*?-->|<\/[^>]*>|<[^>]*>/g;
  let last = 0;
  for (const match of text.matchAll(tokenRe)) {
    const between = text.slice(last, match.index);
    if (between.trim() !== "" && stack.length === 0) throw new Error("根元素外存在文本");
    last = match.index + match[0].length;
    const token = match[0];
    if (token.startsWith("<?") || token.startsWith("<!--")) continue;
    if (token.startsWith("</")) {
      const name = token.slice(2, -1).trim();
      const top = stack.pop();
      if (top !== name) throw new Error(`XML 闭合不匹配：</${name}>（期望 </${top ?? "无"}>）`);
      continue;
    }
    const selfClosing = token.endsWith("/>");
    const body = token.slice(1, selfClosing ? -2 : -1);
    const nameMatch = /^([A-Za-z_][A-Za-z0-9_.:-]*)/.exec(body);
    if (!nameMatch) throw new Error(`XML 标签名非法：${token}`);
    const name = nameMatch[1];
    const attrs = {};
    const attrRe = /\s+([A-Za-z_][A-Za-z0-9_.:-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')/y;
    attrRe.lastIndex = name.length;
    let consumed = name.length;
    while (consumed < body.length) {
      const m = attrRe.exec(body);
      if (!m) {
        if (body.slice(consumed).trim() === "") break;
        throw new Error(`XML 属性非法：<${name}${body.slice(consumed)}>`);
      }
      if (attrs[m[1]] !== undefined) throw new Error(`XML 属性重复：<${name} ${m[1]}>`);
      attrs[m[1]] = m[2] ?? m[3];
      consumed = attrRe.lastIndex;
    }
    elements.push({ name, attrs, selfClosing });
    if (!selfClosing) stack.push(name);
  }
  if (text.slice(last).trim() !== "" && stack.length === 0) throw new Error("根元素外存在文本");
  if (stack.length > 0) throw new Error(`XML 未闭合：<${stack[stack.length - 1]}>`);
  if (elements.length === 0) throw new Error("XML 无元素");
  return elements;
}

const attr = (tag, key) => tag?.attrs?.[key];

/** 工程文件树 → violations（空数组 = 通过）。 */
export function checkGarminProject(files) {
  const violations = [];
  const paths = Object.keys(files);

  for (const path of paths) {
    if (!isSafePath(path)) violations.push(`文件路径不安全："${path}"`);
  }
  for (const required of REQUIRED_FILES) {
    if (!Object.hasOwn(files, required)) violations.push(`缺少必需文件："${required}"`);
  }
  if (violations.length > 0) return violations;

  // manifest.xml
  let tags = [];
  try {
    tags = parseXml(files["manifest.xml"]);
  } catch (error) {
    violations.push(`manifest.xml 不是良构 XML：${error.message}`);
    return violations;
  }
  const root = tags.find((tag) => !tag.name.startsWith("?"));
  if (root?.name !== "iq:manifest") violations.push(`根元素须为 iq:manifest，实为 ${root?.name}`);
  if (attr(root, "xmlns:iq") !== IQ_NAMESPACE) violations.push(`xmlns:iq 须为 ${IQ_NAMESPACE}`);
  if (attr(root, "version") !== "3") violations.push(`manifest version 须为 "3"`);

  const apps = tags.filter((tag) => tag.name === "iq:application");
  if (apps.length !== 1) violations.push(`iq:application 须恰好 1 个（实为 ${apps.length}）`);
  const app = apps[0];
  if (app) {
    if (!/^[0-9a-f]{32}$/.test(attr(app, "id") ?? "")) violations.push(`application id 须为 32 位小写 hex`);
    if (attr(app, "type") !== "watch-app") violations.push(`application type 须为 "watch-app"`);
    if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(attr(app, "entry") ?? "")) violations.push(`entry 须为类名`);
    if (!/^@Strings\.[A-Za-z_][A-Za-z0-9_]*$/.test(attr(app, "name") ?? "")) violations.push(`name 须为 @Strings.<id> 资源引用`);
    if (!/^\d+\.\d+\.\d+$/.test(attr(app, "version") ?? "")) violations.push(`version 须为数字三段`);
    if (!/^\d+\.\d+\.\d+$/.test(attr(app, "minSdkVersion") ?? "")) violations.push(`minSdkVersion 须为数字三段`);
  }
  const products = tags.filter((tag) => tag.name === "iq:product");
  if (products.length === 0) violations.push("iq:products 至少登记 1 个 iq:product");
  for (const product of products) {
    if (!/^[a-z0-9_]+$/.test(attr(product, "id") ?? "")) violations.push(`iq:product id 非法："${attr(product, "id")}"`);
  }

  // monkey.jungle
  const jungleKeys = new Map();
  for (const rawLine of files["monkey.jungle"].split("\n")) {
    const line = rawLine.trim();
    if (line === "" || line.startsWith("#")) continue;
    const m = /^([A-Za-z_][A-Za-z0-9_.]*)\s*=\s*(.+)$/.exec(line);
    if (!m) {
      violations.push(`monkey.jungle 行无法解析："${line}"`);
      continue;
    }
    if (jungleKeys.has(m[1])) violations.push(`monkey.jungle 键重复："${m[1]}"`);
    jungleKeys.set(m[1], m[2].trim());
  }
  const expectedJungle = {
    "project.manifest": "manifest.xml",
    "base.sourcePath": "source",
    "base.resourcePath": "resources",
  };
  for (const [key, value] of Object.entries(expectedJungle)) {
    if (jungleKeys.get(key) !== value) violations.push(`monkey.jungle 须有 ${key} = ${value}`);
  }

  // strings.xml + 入口类
  try {
    const strings = parseXml(files["resources/strings/strings.xml"]);
    if (!strings.some((tag) => tag.name === "strings")) violations.push("strings.xml 缺少 <strings> 根");
    const appName = /<string\s+id="AppName"\s*>([^<]*)<\/string>/.exec(files["resources/strings/strings.xml"]);
    if (!appName || appName[1].trim() === "") violations.push("strings.xml 缺少非空 AppName");
  } catch (error) {
    violations.push(`strings.xml 不是良构 XML：${error.message}`);
  }
  const entry = attr(app, "entry");
  if (entry) {
    const declared = new RegExp(`\\bclass\\s+${entry}\\s+extends\\s+App\\.AppBase\\b`).test(files["source/PolypackApp.mc"]);
    if (!declared) violations.push(`入口类 ${entry} 未在 source/PolypackApp.mc 声明（extends App.AppBase）`);
  }

  // 子集 / API 白名单
  try {
    assertSubset(files);
  } catch (error) {
    violations.push(...(error.violations ?? [error.message]));
  }
  return violations;
}

/** 断言工程文件树；失败抛错。 */
export function assertGarminProject(files) {
  const violations = checkGarminProject(files);
  if (violations.length > 0) {
    const error = new Error(`Connect IQ 工程结构自检失败 ${violations.length} 处：\n${violations.join("\n")}`);
    error.violations = violations;
    throw error;
  }
  return true;
}

/** 导出产物 `.iq`（ZIP）条目清单断言：根 manifest.xml + ≥1 个 .prg。 */
export function checkIqEntries(entries) {
  const violations = [];
  if (!Array.isArray(entries) || entries.length === 0) return ["`.iq` 条目清单为空"];
  const seen = new Set();
  for (const entry of entries) {
    // zip 目录条目以 "/" 结尾；剥掉后再做路径安全断言。
    const path = entry.endsWith("/") ? entry.slice(0, -1) : entry;
    if (!isSafePath(path)) violations.push(`.iq 条目路径不安全："${entry}"`);
    if (seen.has(path)) violations.push(`.iq 条目重复："${path}"`);
    seen.add(path);
  }
  if (!seen.has("manifest.xml")) violations.push(".iq 根缺少 manifest.xml");
  if (!entries.some((entry) => entry.endsWith(".prg"))) violations.push(".iq 缺少按设备编译的 .prg");
  return violations;
}

export function assertIqEntries(entries) {
  const violations = checkIqEntries(entries);
  if (violations.length > 0) {
    const error = new Error(`.iq 包结构断言失败：\n${violations.join("\n")}`);
    error.violations = violations;
    throw error;
  }
  return true;
}
