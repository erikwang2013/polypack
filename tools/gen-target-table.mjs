#!/usr/bin/env node
/**
 * gen-target-table.mjs — L6 嵌入式 / 车机状态表生成器（T-M6-12）
 *
 * 单一事实源：crates/polypack-targets/src/builtin.rs 的 `TargetSpec` 字面量。
 * 本脚本解析 Rust 源码（不编译、不依赖 Rust 工具链），按 id 排序渲染中英双语
 * Markdown 表格，写入 README / ROADMAP 的标记块；`--check` 校验标记块与
 * TargetSpec 是否漂移（CI 用，漂移即非零退出）。
 *
 * 字段逐字复现：runtime / bridge / package / signing / verify / publish /
 * experimental / notes。notes 为 TargetSpec 原文（中文），英文表不改写。
 *
 * 零依赖。用法:
 *   node tools/gen-target-table.mjs           生成 / 更新四份文档的标记块
 *   node tools/gen-target-table.mjs --check   仅校验，漂移 exit 1
 *   node tools/gen-target-table.mjs --print   打印生成结果（不写盘）
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SPEC_FILE = 'crates/polypack-targets/src/builtin.rs';

export const BEGIN = '<!-- BEGIN GENERATED: l6-embedded-status -->';
export const END = '<!-- END GENERATED: l6-embedded-status -->';

/** 本表覆盖的十一个 L6 嵌入式 / 车机 / 穿戴目标（防解析静默漏项；新增 tier-6 目标时同步扩展）。 */
const EXPECTED_IDS = [
  'android-automotive',
  'autosar',
  'fitbit',
  'freertos',
  'garmin',
  'qnx',
  'vxworks',
  'watchos',
  'wearos',
  'zepp',
  'zephyr',
];

/** 生成目标文件（语言决定表头；标记块内容必须完全一致地可复现）。 */
export const DOCS = [
  { file: 'README.md', lang: 'zh' },
  { file: 'README.en.md', lang: 'en' },
  { file: 'docs/ROADMAP.md', lang: 'zh' },
  { file: 'docs/ROADMAP.en.md', lang: 'en' },
];

/* ── Rust 源码解析（仅覆盖 builtin.rs 的结构化字面量） ── */

/** 跳过字符串字面量（src[i] === '"'），返回闭引号下标。 */
function skipString(src, i) {
  for (let j = i + 1; j < src.length; j += 1) {
    if (src[j] === '\\') j += 1;
    else if (src[j] === '"') return j;
  }
  throw new Error('未闭合的字符串字面量');
}

/** 去掉 // 与 /* *\/ 注释，保留字符串内容。 */
function stripComments(src) {
  let out = '';
  let i = 0;
  while (i < src.length) {
    const ch = src[i];
    if (ch === '"') {
      const end = skipString(src, i);
      out += src.slice(i, end + 1);
      i = end + 1;
    } else if (ch === '/' && src[i + 1] === '/') {
      while (i < src.length && src[i] !== '\n') i += 1;
    } else if (ch === '/' && src[i + 1] === '*') {
      i += 2;
      while (i < src.length && !(src[i] === '*' && src[i + 1] === '/')) i += 1;
      i += 2;
    } else {
      out += ch;
      i += 1;
    }
  }
  return out;
}

/** 从 openIdx（'{'）起找匹配的 '}'，跳过字符串。 */
function matchBrace(src, openIdx) {
  let depth = 0;
  for (let i = openIdx; i < src.length; i += 1) {
    const ch = src[i];
    if (ch === '"') i = skipString(src, i);
    else if (ch === '{') depth += 1;
    else if (ch === '}' && --depth === 0) return i;
  }
  return -1;
}

/** 拆分结构体 / 调用参数中的顶层逗号分段。 */
function splitFields(body) {
  const parts = [];
  let start = 0;
  let depth = 0;
  for (let i = 0; i < body.length; i += 1) {
    const ch = body[i];
    if (ch === '"') i = skipString(body, i);
    else if ('{[('.includes(ch)) depth += 1;
    else if ('}])'.includes(ch)) depth -= 1;
    else if (ch === ',' && depth === 0) {
      parts.push(body.slice(start, i));
      start = i + 1;
    }
  }
  parts.push(body.slice(start));
  return parts.map((part) => part.trim()).filter(Boolean);
}

/** 解析 `key: value` 分段为 Map。 */
function fieldMap(body) {
  const map = new Map();
  for (const part of splitFields(body)) {
    const m = /^([A-Za-z_][A-Za-z0-9_]*)\s*:\s*([\s\S]*)$/.exec(part);
    if (!m) throw new Error(`无法解析字段: ${part.slice(0, 80)}`);
    map.set(m[1], m[2].trim());
  }
  return map;
}

/** Rust 字符串转义（含反斜杠续行：跳过换行与行首空白）。 */
function unescapeRust(s) {
  let out = '';
  for (let i = 0; i < s.length; i += 1) {
    const ch = s[i];
    if (ch !== '\\') {
      out += ch;
      continue;
    }
    const next = s[i + 1];
    if (next === '\n' || next === '\r') {
      i += 1;
      if (next === '\r' && s[i + 1] === '\n') i += 1;
      while (i + 1 < s.length && /\s/.test(s[i + 1])) i += 1;
      continue;
    }
    switch (next) {
      case 'n': out += '\n'; i += 1; break;
      case 'r': out += '\r'; i += 1; break;
      case 't': out += '\t'; i += 1; break;
      case '0': out += '\0'; i += 1; break;
      case '\\': out += '\\'; i += 1; break;
      case '"': out += '"'; i += 1; break;
      case "'": out += "'"; i += 1; break;
      case 'x':
        out += String.fromCharCode(parseInt(s.slice(i + 2, i + 4), 16));
        i += 3;
        break;
      case 'u': {
        const close = s.indexOf('}', i + 2);
        out += String.fromCodePoint(parseInt(s.slice(i + 3, close), 16));
        i = close;
        break;
      }
      default:
        out += next;
        i += 1;
    }
  }
  return out;
}

const RUNTIME = {
  Native: 'native',
  Wasm: 'wasm',
  Staticlib: 'staticlib',
  Html5: 'html5',
  None: 'none',
};
const BRIDGE = {
  Uniffi: 'uniffi',
  Napi: 'napi',
  WasmBindgen: 'wasm-bindgen',
  Cffi: 'cffi',
  TauriIpc: 'tauri-ipc',
  Html5: 'html5',
  None: 'none',
};
const asEnum = (raw, table, label) => {
  const m = /^[A-Za-z]+::([A-Za-z0-9]+)$/.exec(raw.trim());
  if (!m || !(m[1] in table)) throw new Error(`无法解析${label}: ${raw}`);
  return table[m[1]];
};

/** 解析一个 Rust 值字面量（字符串 / Some / None / vec! / Vec::new / enum / 数字 / 布尔 / PackageSpec）。 */
function parseValue(raw, label = '值') {
  // rustfmt 会在 Some( ... , ) 的内层留尾逗号；两者都在递归里统一剥掉。
  let value = raw.trim().replace(/,\s*$/, '').trim();
  // 多数声明带 .to_string()；Some(...) 的内层也在此递归剥掉。
  value = value.replace(/\.to_string\(\)\s*$/, '').trim();
  let m;
  if ((m = /^"((?:\\[\s\S]|[^"\\])*)"$/.exec(value))) {
    return { kind: 'string', value: unescapeRust(m[1]) };
  }
  if (/^Some\([\s\S]*\)$/.test(value)) {
    return { kind: 'some', value: parseValue(value.slice(5, -1), label) };
  }
  if (value === 'None') return { kind: 'none' };
  if (value === 'Vec::new()') return { kind: 'vec', value: [] };
  if ((m = /^vec!\[([\s\S]*)\]$/.exec(value))) {
    return {
      kind: 'vec',
      value: splitFields(m[1]).map((item) => parseValue(item, label)),
    };
  }
  if ((m = /^PackageSpec\s*\{([\s\S]*)\}$/.exec(value))) {
    return { kind: 'package', value: fieldMap(m[1]) };
  }
  if (/^(true|false)$/.test(value)) return { kind: 'bool', value: value === 'true' };
  if (/^\d+$/.test(value)) return { kind: 'number', value: Number(value) };
  if (/^[A-Za-z][A-Za-z0-9]*::[A-Za-z][A-Za-z0-9]*$/.test(value)) return { kind: 'enum', value };
  throw new Error(`无法解析${label}: ${value.slice(0, 120)}`);
}

const expectString = (parsed, label) => {
  if (parsed.kind !== 'string') throw new Error(`${label} 必须是字符串`);
  return parsed.value;
};
const expectStringList = (parsed, label) => {
  if (parsed.kind !== 'vec') throw new Error(`${label} 必须是 vec!`);
  return parsed.value.map((item) => expectString(item, label));
};
const expectOptionalString = (parsed, label) => {
  if (parsed.kind === 'none') return null;
  if (parsed.kind === 'some') return expectString(parsed.value, label);
  throw new Error(`${label} 必须是 Some(...) 或 None`);
};

/** 提取 builtin.rs 中全部 TargetSpec 字面量并解析为状态表所需的数据。 */
export function parseTargetSpecs(source = fs.readFileSync(path.join(ROOT, SPEC_FILE), 'utf8')) {
  const src = stripComments(source);
  const specs = [];
  const re = /\bTargetSpec\s*\{/g;
  let m;
  while ((m = re.exec(src))) {
    const open = m.index + m[0].length - 1;
    const close = matchBrace(src, open);
    if (close < 0) throw new Error('TargetSpec 结构体未闭合');
    const fields = fieldMap(src.slice(open + 1, close));
    const pkg = parseValue(fields.get('package') ?? '', 'package');
    if (pkg.kind !== 'package') throw new Error('package 必须是 PackageSpec');
    specs.push({
      id: expectString(parseValue(fields.get('id') ?? '', 'id'), 'id'),
      tier: parseValue(fields.get('tier') ?? '', 'tier').value,
      runtime: asEnum(fields.get('runtime') ?? '', RUNTIME, 'runtime'),
      bridge: asEnum(fields.get('bridge') ?? '', BRIDGE, 'bridge'),
      ui: expectStringList(parseValue(fields.get('ui') ?? 'Vec::new()', 'ui'), 'ui'),
      format: expectString(parseValue(pkg.value.get('format') ?? '', 'format'), 'format'),
      builder: expectString(parseValue(pkg.value.get('builder') ?? '', 'builder'), 'builder'),
      signing: expectOptionalString(parseValue(fields.get('signing') ?? 'None', 'signing'), 'signing'),
      verify: expectStringList(parseValue(fields.get('verify') ?? 'vec![]', 'verify'), 'verify'),
      publish: expectStringList(parseValue(fields.get('publish') ?? 'vec![]', 'publish'), 'publish'),
      experimental: parseValue(fields.get('experimental') ?? 'false', 'experimental').value,
      notes: expectOptionalString(parseValue(fields.get('notes') ?? 'None', 'notes'), 'notes'),
    });
    re.lastIndex = close + 1;
  }
  return specs.sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
}

/** tier 6 目标（排序确定；并守卫任务约定 id 均在册）。 */
export function embeddedTargets(specs = parseTargetSpecs()) {
  const tier6 = specs.filter((spec) => spec.tier === 6);
  const ids = tier6.map((spec) => spec.id);
  const missing = EXPECTED_IDS.filter((id) => !ids.includes(id));
  const extra = ids.filter((id) => !EXPECTED_IDS.includes(id));
  if (missing.length || extra.length) {
    throw new Error(
      `tier-6 目标集合与预期不符（缺少: ${missing.join(', ') || '无'}；多出: ${extra.join(', ') || '无'}）`,
    );
  }
  return tier6;
}

/* ── Markdown 渲染 ── */

export const cell = (text) => String(text).replace(/\|/g, '\\|').replace(/\r?\n/g, ' ');
export const code = (text) => `\`${cell(text)}\``;
export const codeList = (items) => items.map(code).join(', ');

/** 渲染一张状态表。notes 逐字复现（仅做表格管道符转义）。 */
export function renderTable(specs, lang = 'zh') {
  const header =
    lang === 'zh'
      ? ['目标', '运行时', '桥', '打包器与产物', '签名', '产物校验', '发布渠道', 'experimental', 'notes']
      : ['Target', 'Runtime', 'Bridge', 'Packager & artifact', 'Signing', 'Artifact verification', 'Publishing channel', 'experimental', 'notes'];
  const none = lang === 'zh' ? '—' : 'none';
  const lines = [
    `| ${header.join(' | ')} |`,
    `| ${header.map(() => '---').join(' | ')} |`,
  ];
  for (const spec of specs) {
    lines.push(
      `| ${[
        code(spec.id),
        code(spec.runtime),
        code(spec.bridge),
        `${code(spec.format)} · ${code(spec.builder)}`,
        spec.signing ? code(spec.signing) : none,
        spec.verify.length ? codeList(spec.verify) : none,
        spec.publish.length ? codeList(spec.publish) : none,
        code(String(spec.experimental)),
        spec.notes ? cell(spec.notes) : none,
      ].join(' | ')} |`,
    );
  }
  return lines.join('\n');
}

const escapeRe = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** 读取文档中的标记块内容（不含标记行）。供其它生成器复用，默认 L6 标记。 */
export function readBlock(text, file, begin = BEGIN, end = END) {
  const re = new RegExp(`${escapeRe(begin)}([\\s\\S]*?)${escapeRe(end)}`, 'g');
  const hits = [...text.matchAll(re)];
  if (hits.length !== 1) {
    throw new Error(`${file}: 期望恰好 1 个状态表标记块，实际 ${hits.length} 个`);
  }
  return hits[0][1];
}

/** 比较期望块与实际块，返回首个差异行。 */
export function firstDiff(actual, expected) {
  const a = actual.split('\n');
  const e = expected.split('\n');
  for (let i = 0; i < Math.max(a.length, e.length); i += 1) {
    const doc = a[i] ?? '<EOF>';
    const want = e[i] ?? '<EOF>';
    if (doc !== want) {
      let k = 0;
      while (k < Math.min(doc.length, want.length) && doc[k] === want[k]) k += 1;
      const from = Math.max(0, k - 40);
      return `第 ${i + 1} 行第 ${k + 1} 列: 文档=${JSON.stringify(doc.slice(from, k + 80))} 期望=${JSON.stringify(want.slice(from, k + 80))}`;
    }
  }
  return '';
}

/** 校验四份文档的标记块是否与 TargetSpec 一致。返回 { failed, lines }。 */
export function checkGeneratedDocs() {
  const tableByLang = { zh: renderTable(embeddedTargets(), 'zh'), en: renderTable(embeddedTargets(), 'en') };
  const lines = [];
  let failed = 0;
  for (const { file, lang } of DOCS) {
    const full = path.join(ROOT, file);
    try {
      const text = fs.readFileSync(full, 'utf8');
      const actual = readBlock(text, file).trim();
      const expected = tableByLang[lang].trim();
      if (actual === expected) {
        lines.push({ ok: true, text: `ok   ${file} · 状态表 ↔ TargetSpec` });
      } else {
        failed += 1;
        lines.push({ ok: false, text: `DRIFT ${file} · 状态表 ≠ TargetSpec（${firstDiff(actual, expected)}）` });
      }
    } catch (err) {
      failed += 1;
      lines.push({ ok: false, text: `DRIFT ${file} · ${err.message}` });
    }
  }
  return { failed, lines };
}

/** 生成 / 更新四份文档的标记块。返回变更文件列表。 */
export function writeGeneratedDocs() {
  const tableByLang = { zh: renderTable(embeddedTargets(), 'zh'), en: renderTable(embeddedTargets(), 'en') };
  const changed = [];
  for (const { file, lang } of DOCS) {
    const full = path.join(ROOT, file);
    const text = fs.readFileSync(full, 'utf8');
    if (!text.includes(BEGIN) || !text.includes(END)) {
      throw new Error(`${file}: 缺少状态表标记块 ${BEGIN} / ${END}`);
    }
    readBlock(text, file); // 同时校验标记块唯一
    const re = new RegExp(`${escapeRe(BEGIN)}[\\s\\S]*?${escapeRe(END)}`);
    const next = text.replace(re, `${BEGIN}\n${tableByLang[lang]}\n${END}`);
    if (next !== text) {
      fs.writeFileSync(full, next);
      changed.push(file);
    }
  }
  return changed;
}

function main() {
  const check = process.argv.includes('--check');
  const print = process.argv.includes('--print');
  try {
    const specs = embeddedTargets();
    if (print) {
      console.log(`## zh\n${renderTable(specs, 'zh')}\n\n## en\n${renderTable(specs, 'en')}`);
      return;
    }
    if (check) {
      const { failed, lines } = checkGeneratedDocs();
      for (const line of lines) (line.ok ? console.log : console.error)(line.text);
      console.log(failed ? `\n${failed} 项状态表漂移` : '\n四份文档状态表与 TargetSpec 一致');
      process.exit(failed ? 1 : 0);
    }
    const changed = writeGeneratedDocs();
    console.log(
      changed.length
        ? `已更新: ${changed.join(', ')}（${specs.length} 个 tier-6 目标）`
        : `无变化（${specs.length} 个 tier-6 目标）`,
    );
  } catch (err) {
    console.error(`gen-target-table: ${err.message}`);
    process.exit(1);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
