#!/usr/bin/env node
/**
 * render-diagrams.mjs — PolyPack 文档图示流水线
 *
 * 输入:  docs/diagrams/<name>.<type>.json   (Archify JSON IR)
 * 输出:  docs/assets/<name>.svg            (GitHub README 可直接嵌入的独立 SVG)
 *        docs/diagrams/build/<name>.html   (Archify 交互式产物, 供本地探索, 不提交)
 *
 * 步骤: 1) archify validate  2) archify deliver (渲染 + 产物校验 + 原子提交)
 *       3) 从产物 HTML 中提取内嵌 SVG, 按官方导出管线烘焙为独立 SVG:
 *          - 仅保留语义 CSS (svg/:root/[data-theme/[data-preset/.c-/.t-/.a-/.m-)
 *          - 解析亮色主题变量, 追加为 :root, svg { … } (后置, 级联胜出)
 *          - 注入 <style> 与背景 <rect class="c-bg-rect"> 作为 SVG 首子元素
 *          - 根节点补 xmlns / width / height, 去掉 data-theme
 *
 * 零依赖。用法:
 *   node tools/render-diagrams.mjs             重新生成全部 SVG（默认在右下角盖 Pax 微缩角标）
 *   node tools/render-diagrams.mjs --no-brand  关闭右下角角标
 */
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ARCHIFY = '/home/erik/.claude/skills/archify/bin/archify.mjs';
const SRC = path.join(ROOT, 'docs', 'diagrams');
const OUT = path.join(ROOT, 'docs', 'assets');
const BUILD = path.join(SRC, 'build');
const PAX_MARK = path.join(OUT, 'pax-mark.svg'); // 角标单一源: tools/build-pax.mjs 生成
const BRAND = !process.argv.includes('--no-brand');

const FONT_STACK =
  "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, Consolas, 'DejaVu Sans Mono', " +
  "'Liberation Mono', 'Noto Sans Mono CJK SC', 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei', monospace";

/* ── SVG 根节点尺寸 ── */
function viewBoxSize(svg) {
  const m = svg.match(/viewBox="([^"]+)"/);
  if (!m) throw new Error('SVG 根节点缺少 viewBox');
  const [, , w, h] = m[1].split(/\s+/).map(Number);
  return { w, h };
}

/* ── CSS: 顶层规则解析 (跳过 @-规则, 与运行时 rule.type !== 1 行为一致) ── */
function parseTopLevelCss(css) {
  css = css.replace(/\/\*[\s\S]*?\*\//g, '');
  const rules = [];
  let depth = 0;
  let buf = '';
  let sel = '';
  for (const ch of css) {
    if (ch === '{') {
      depth += 1;
      if (depth === 1) { sel = buf.trim(); buf = ''; }
    } else if (ch === '}') {
      depth -= 1;
      if (depth === 0) { if (sel && !sel.startsWith('@')) rules.push({ sel, body: buf }); sel = ''; buf = ''; }
    } else {
      buf += ch;
    }
  }
  return rules;
}

/* ── 亮色主题解析: 依文件顺序合并所有命中"亮色根上下文"的变量块 ── */
function resolveLightVars(css) {
  const merged = new Map();
  const rootish = (part) => {
    const p = part.trim().toLowerCase();
    return p === ':root' || p === '[data-theme="light"]' || p === "[data-theme='light']";
  };
  for (const rule of parseTopLevelCss(css)) {
    const parts = rule.sel.split(',');
    if (!parts.some(rootish)) continue;
    for (const m of rule.body.matchAll(/--([a-zA-Z0-9-]+)\s*:\s*([^;]+);/g)) merged.set(m[1], m[2].trim());
  }
  return [...merged].map(([k, v]) => `--${k}: ${v};`).join(' ');
}

/* ── 从 Archify 产物 HTML 提取独立 SVG ── */
function extractStandaloneSvg(html) {
  const styleMatch = html.match(/<style>([\s\S]*?)<\/style>/);
  const svgMatch = html.match(/<svg[\s\S]*?<\/svg>/);
  if (!styleMatch || !svgMatch) throw new Error('产物 HTML 中找不到 <style> 或 <svg>');
  const css = styleMatch[1];
  let svg = svgMatch[0];

  // HTML 布尔属性 (如 data-detail-anchor) 必须补成空值: 独立 SVG 走严格 XML 解析
  svg = svg.replace(/(\s)(data-[\w-]+|aria-[\w-]+)(?=\s|>)/g, '$1$2=""');

  const semantic = /(^|,)\s*(svg|:root|\[data-theme|\[data-preset|\.c-|\.t-|\.a-|\.m-)/;
  const keptRules = parseTopLevelCss(css)
    .filter((r) => semantic.test(r.sel))
    .map((r) => `${r.sel} {${r.body}}`)
    .join('\n');
  const lightVars = resolveLightVars(css);

  const fontFallback = [400, 500, 600, 700]
    .map((w) => `@font-face { font-family: 'JetBrains Mono'; font-weight: ${w}; src: local('JetBrains Mono'), local('JetBrainsMono-Regular'); }`)
    .join('\n');

  const styleText = [
    fontFallback,
    `svg { font-family: ${FONT_STACK}; }`,
    keptRules,
    `:root, svg { ${lightVars} }`,
    // 冗余规则: 部分非浏览器渲染器 (librsvg 系) 遇到 :root 会整条丢弃,
    // 单独一条 svg { vars } 保证它们也能拿到主题变量
    `svg { ${lightVars} }`,
    'rect.c-bg-rect { fill: var(--bg); }',
  ].join('\n').replace(/&/g, '&amp;').replace(/</g, '&lt;');

  // 根节点: 补 xmlns / width / height, 去掉 data-theme
  const { w, h } = viewBoxSize(svg);
  svg = svg.replace(/\sdata-theme="[^"]*"/g, '');
  svg = svg.replace(/^<svg/, `<svg width="${w}" height="${h}" xmlns="http://www.w3.org/2000/svg"`);
  const gt = svg.indexOf('>');
  svg = svg.slice(0, gt + 1) + `<style>${styleText}</style><rect class="c-bg-rect" width="100%" height="100%"/>` + svg.slice(gt + 1);
  return svg;
}

/* ── 右下角 Pax 微缩角标 (品牌约定: 生成图片默认盖章, --no-brand 关闭) ── */
function applyBrand(svg) {
  if (!BRAND) return svg;
  if (!fs.existsSync(PAX_MARK)) {
    console.warn('warn: docs/assets/pax-mark.svg 不存在，跳过角标（先运行 node tools/build-pax.mjs）');
    return svg;
  }
  const inner = fs.readFileSync(PAX_MARK, 'utf8')
    .replace(/^[\s\S]*?<svg[^>]*>/, '')
    .replace(/<\/svg>\s*$/, '')
    .replace(/<title[\s\S]*?<\/title>\s*/g, '')
    .replace(/<desc[\s\S]*?<\/desc>\s*/g, '');
  const { w, h } = viewBoxSize(svg);
  const badge = `<g class="pax-brand" transform="translate(${w - 48} ${h - 48}) scale(0.5)" opacity="0.92">${inner}</g>`;
  return svg.replace(/<\/svg>\s*$/, `${badge}\n</svg>\n`);
}

/* ── 主流程 ── */
fs.mkdirSync(OUT, { recursive: true });
fs.mkdirSync(BUILD, { recursive: true });

const sources = fs.readdirSync(SRC).filter((f) => /\.(architecture|sequence|dataflow|lifecycle|workflow)\.json$/.test(f)).sort();
if (sources.length === 0) { console.error('docs/diagrams/ 下没有图示源文件'); process.exit(1); }

let failed = 0;
for (const file of sources) {
  const full = path.join(SRC, file);
  const type = file.match(/\.([a-z]+)\.json$/)[1];
  const base = file.replace(new RegExp(`\\.${type}\\.json$`), '');
  const ir = JSON.parse(fs.readFileSync(full, 'utf8'));
  const quality = ir.meta?.quality_profile;
  const qualityArgs = quality ? ['--quality', quality] : [];
  const htmlPath = path.join(BUILD, `${base}.html`);

  try {
    process.stdout.write(`▸ ${file}  validate…  `);
    execFileSync('node', [ARCHIFY, 'validate', type, full, '--json', ...qualityArgs], { stdio: 'pipe' });
    process.stdout.write('deliver…  ');
    execFileSync('node', [ARCHIFY, 'deliver', type, full, htmlPath, '--json', ...qualityArgs], { stdio: 'pipe' });
    const svg = applyBrand(extractStandaloneSvg(fs.readFileSync(htmlPath, 'utf8')));
    const svgPath = path.join(OUT, `${base}.svg`);
    fs.writeFileSync(svgPath, svg);
    console.log(`OK  → docs/assets/${base}.svg (${(svg.length / 1024).toFixed(1)} KiB)`);
  } catch (err) {
    failed += 1;
    console.log('FAIL');
    const out = [err.stdout?.toString(), err.stderr?.toString()].filter(Boolean).join('\n').trim();
    console.error(out || err.message);
  }
}
console.log(failed ? `\n${failed} 个图示未通过` : `\n全部 ${sources.length} 个图示已生成到 docs/assets/`);
process.exit(failed ? 1 : 0);
