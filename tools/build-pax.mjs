#!/usr/bin/env node
/**
 * build-pax.mjs — Pax（项目宠物）单一事实源生成器
 *
 * 输入: 本文件内的共享身体图层 + 五种表情图层（笑/哭/惊恐/疑惑/眨眼）
 * 输出:
 *   docs/assets/pax.svg           动态版: 空闲浮动 + 定期眨眼 + 五种表情循环 (纯 CSS 动画, <img> 嵌入可播放)
 *   docs/assets/pax-happy.svg     静态: 笑
 *   docs/assets/pax-cry.svg       静态: 哭
 *   docs/assets/pax-shock.svg     静态: 惊恐
 *   docs/assets/pax-confused.svg  静态: 疑惑
 *   docs/assets/pax-blink.svg     静态: 眨眼
 *   docs/pax.html                 本地游乐场: 手动切换表情 (img 切换, 零依赖)
 *
 * 零依赖。用法:
 *   node tools/build-pax.mjs           生成全部文件
 *   node tools/build-pax.mjs --check   校验磁盘上的文件与生成结果一致 (CI 用), 不一致 exit 1
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ASSETS = path.join(ROOT, 'docs', 'assets');
const VIEWBOX = '0 0 720 432';
const WIDTH = 640;
const HEIGHT = 384; // 640 * 432 / 720

const FONT = "'JetBrains Mono', ui-monospace, 'PingFang SC', 'Microsoft YaHei', monospace";

/* ── 场景: 投递路线与平台图标 (不随表情/浮动变化) ── */
const SCENE = `
  <defs>
    <marker id="pax-arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="#94a3b8"/>
    </marker>
  </defs>
  <path d="M170,206 C 300,60 470,40 548,72" fill="none" stroke="#94a3b8" stroke-width="2" stroke-dasharray="5 6" marker-end="url(#pax-arrow)"/>
  <path d="M482,178 C 540,140 580,124 622,128" fill="none" stroke="#94a3b8" stroke-width="2" stroke-dasharray="5 6" marker-end="url(#pax-arrow)"/>
  <path d="M478,212 C 520,252 552,276 586,288" fill="none" stroke="#94a3b8" stroke-width="2" stroke-dasharray="5 6" marker-end="url(#pax-arrow)"/>
  <g stroke="#64748b" stroke-width="3" fill="#ffffff" stroke-linejoin="round">
    <rect x="552" y="50" width="28" height="48" rx="7"/>
    <rect x="626" y="126" width="26" height="32" rx="7"/>
    <path d="M630,126 v-10 h18 v10 M630,158 v10 h18 v-10" fill="none"/>
    <rect x="588" y="276" width="66" height="42" rx="7"/>
    <path d="M621,318 v9 M607,327 h28" fill="none"/>
  </g>
  <circle cx="566" cy="88" r="2.6" fill="#64748b" stroke="none"/>
  <ellipse cx="300" cy="396" rx="152" ry="15" fill="#0f172a" opacity="0.06"/>`;

/* ── 共享身体: 腿 / 身 / 腹 / 眼柄 / 帽 / 臂 / 包裹 / 钳 (表情之外的全部图层) ── */
const LEGS = `
    <g stroke="#c2410c" stroke-width="11" stroke-linecap="round" fill="none">
      <path d="M214,318 L182,356"/>
      <path d="M246,338 L228,380"/>
      <path d="M282,348 L276,388"/>
      <path d="M386,318 L418,356"/>
      <path d="M354,338 L372,380"/>
      <path d="M318,348 L324,388"/>
    </g>`;

const CORE = `
    <ellipse cx="300" cy="268" rx="110" ry="78" fill="#ea580c" stroke="#9a3412" stroke-width="3"/>
    <ellipse cx="300" cy="292" rx="72" ry="46" fill="#fdba74" opacity="0.85"/>`;

const BODY_TOP = LEGS + CORE;

const HEAD = `
    <g stroke="#9a3412" stroke-width="6" stroke-linecap="round">
      <path d="M252,206 L238,162"/>
      <path d="M348,206 L362,162"/>
    </g>
    <path d="M246,192 q0,-54 54,-54 q54,0 54,54 z" fill="#0891b2" stroke="#0e7490" stroke-width="3"/>
    <rect x="234" y="188" width="132" height="14" rx="7" fill="#0e7490"/>
    <circle cx="300" cy="166" r="10" fill="#fdba74" stroke="#0e7490" stroke-width="2.5"/>
    <rect x="295.5" y="161.5" width="9" height="8.5" rx="2" fill="#9a3412"/>`;

const ARMS = `
    <path d="M206,258 L148,216" stroke="#ea580c" stroke-width="16" stroke-linecap="round"/>
    <g transform="rotate(-8 118 253)">
      <rect x="64" y="210" width="108" height="86" rx="10" fill="#d4a373" stroke="#b08968" stroke-width="3"/>
      <rect x="107" y="210" width="22" height="86" fill="#b08968" opacity="0.85"/>
      <circle cx="150" cy="240" r="17" fill="#ffffff" stroke="#0891b2" stroke-width="2" stroke-dasharray="3 2.6"/>
      <ellipse cx="150" cy="240" rx="11.5" ry="4.4" fill="none" stroke="#0891b2" stroke-width="2" transform="rotate(-32 150 240)"/>
      <circle cx="150" cy="240" r="2.4" fill="#0891b2"/>
      <text x="80" y="282" font-size="13" font-family="${FONT}" font-weight="700" fill="#7c4a2a">POLY</text>
    </g>
    <circle cx="128" cy="194" r="22" fill="#ea580c" stroke="#9a3412" stroke-width="3"/>
    <path d="M112,184 q14,-9 28,-3" fill="none" stroke="#9a3412" stroke-width="2.5" stroke-linecap="round"/>
    <path d="M394,254 L452,208" stroke="#ea580c" stroke-width="16" stroke-linecap="round"/>
    <circle cx="464" cy="192" r="24" fill="#ea580c" stroke="#9a3412" stroke-width="3"/>
    <path d="M448,180 q16,-10 32,-4" fill="none" stroke="#9a3412" stroke-width="2.5" stroke-linecap="round"/>`;

const BODY_TAIL = HEAD + ARMS;

/* ── 五种表情 (含各自的道具: 泪滴 / 汗滴 / 问号) ── */
const BLUSH = `
      <ellipse cx="242" cy="300" rx="12" ry="7.5" fill="#fb7185" opacity="0.55"/>
      <ellipse cx="358" cy="300" rx="12" ry="7.5" fill="#fb7185" opacity="0.55"/>`;

const OPEN_SMILE = `
      <path d="M276,283 Q300,318 324,283 Z" fill="#7c2d12" stroke="#9a3412" stroke-width="3" stroke-linejoin="round"/>
      <ellipse cx="300" cy="303" rx="11" ry="6.5" fill="#fb7185"/>`;

const ROUND_EYES = `
      <circle cx="236" cy="154" r="17" fill="#ffffff" stroke="#9a3412" stroke-width="3"/>
      <circle cx="364" cy="154" r="17" fill="#ffffff" stroke="#9a3412" stroke-width="3"/>
      <circle cx="240" cy="158" r="7.5" fill="#0f172a"/>
      <circle cx="360" cy="158" r="7.5" fill="#0f172a"/>`;

const FACES = {
  // 笑: 圆眼 + 张嘴大笑 + 腮红
  happy: `
  <g class="pax-face pax-face-happy">${BLUSH}${OPEN_SMILE}${ROUND_EYES}
  </g>`,
  // 哭: 皱眉嘴 + 瞳孔上移 + 两行泪 (动态版泪滴循环下落)
  cry: `
  <g class="pax-face pax-face-cry">
      <path d="M282,298 Q300,282 318,298" fill="none" stroke="#9a3412" stroke-width="4" stroke-linecap="round"/>
      <circle cx="236" cy="154" r="17" fill="#ffffff" stroke="#9a3412" stroke-width="3"/>
      <circle cx="364" cy="154" r="17" fill="#ffffff" stroke="#9a3412" stroke-width="3"/>
      <circle cx="241" cy="149" r="7" fill="#0f172a"/>
      <circle cx="359" cy="149" r="7" fill="#0f172a"/>
      <g class="pax-tear pax-tear-a"><path d="M228,178 C233,185 233,192 228,196 C223,192 223,185 228,178 Z" fill="#7dd3fc" stroke="#0891b2" stroke-width="2"/></g>
      <g class="pax-tear pax-tear-b"><path d="M372,178 C377,185 377,192 372,196 C367,192 367,185 372,178 Z" fill="#7dd3fc" stroke="#0891b2" stroke-width="2"/></g>
  </g>`,
  // 惊恐: 大眼睛 + 极小瞳孔 + O 形嘴 + 汗滴 (无腮红)
  shock: `
  <g class="pax-face pax-face-shock">
      <circle cx="234" cy="152" r="21" fill="#ffffff" stroke="#9a3412" stroke-width="3"/>
      <circle cx="366" cy="152" r="21" fill="#ffffff" stroke="#9a3412" stroke-width="3"/>
      <circle cx="237" cy="148" r="4.2" fill="#0f172a"/>
      <circle cx="363" cy="148" r="4.2" fill="#0f172a"/>
      <ellipse cx="300" cy="294" rx="11" ry="14" fill="#7c2d12" stroke="#9a3412" stroke-width="3"/>
      <g class="pax-sweat"><path d="M398,96 C404,105 404,114 398,119 C392,114 392,105 398,96 Z" fill="#bae6fd" stroke="#0891b2" stroke-width="2"/></g>
  </g>`,
  // 疑惑: 左眼圆睁 + 右眼眯起 + 波浪嘴 + 浮动问号
  confused: `
  <g class="pax-face pax-face-confused">${BLUSH}
      <path d="M280,288 q8,8 16,0 q8,-8 16,0 q8,8 16,0" fill="none" stroke="#9a3412" stroke-width="4" stroke-linecap="round"/>
      <circle cx="236" cy="154" r="17" fill="#ffffff" stroke="#9a3412" stroke-width="3"/>
      <circle cx="240" cy="158" r="7.5" fill="#0f172a"/>
      <path d="M348,154 Q364,140 380,154" fill="none" stroke="#9a3412" stroke-width="4.5" stroke-linecap="round"/>
      <g class="pax-question"><text x="356" y="116" font-size="54" font-weight="800" text-anchor="middle" fill="#0891b2" font-family="${FONT}">?</text></g>
  </g>`,
  // 眨眼: 双眼闭合弧线 + 保留笑容
  blink: `
  <g class="pax-face pax-face-blink">${BLUSH}${OPEN_SMILE}
      <path d="M220,154 Q236,142 252,154" fill="none" stroke="#9a3412" stroke-width="4.5" stroke-linecap="round"/>
      <path d="M348,154 Q364,142 380,154" fill="none" stroke="#9a3412" stroke-width="4.5" stroke-linecap="round"/>
  </g>`,
};

/* ── 动画 CSS (仅动态版): 空闲浮动 + 表情循环 14s + 泪滴下落 + 道具浮动 ── */
const ANIMATION_CSS = `
  .pax-face { opacity: 0; }
  .pax-face-happy { opacity: 1; }
  @media (prefers-reduced-motion: no-preference) {
    .pax-crab { animation: pax-bob 3.4s ease-in-out infinite; }
    .pax-face-happy { animation: pax-mood 14s linear infinite; }
    .pax-face-blink { animation: pax-mood-blink 14s linear infinite; }
    .pax-face-confused { animation: pax-mood-confused 14s linear infinite; }
    .pax-face-cry { animation: pax-mood-cry 14s linear infinite; }
    .pax-face-shock { animation: pax-mood-shock 14s linear infinite; }
    .pax-tear-a { animation: pax-tear 1.15s ease-in infinite; }
    .pax-tear-b { animation: pax-tear 1.15s ease-in 0.2s infinite; }
    .pax-question { animation: pax-float 2.2s ease-in-out infinite; }
    .pax-sweat { animation: pax-float 1.6s ease-in-out infinite; }
  }
  @keyframes pax-bob { 0%, 100% { transform: translateY(0); } 50% { transform: translateY(-5px); } }
  @keyframes pax-mood { 0%, 24.9% { opacity: 1; } 25.1%, 99.9% { opacity: 0; } 100% { opacity: 1; } }
  @keyframes pax-mood-blink { 0%, 24.9% { opacity: 0; } 25.15%, 26.5% { opacity: 1; } 26.7%, 100% { opacity: 0; } }
  @keyframes pax-mood-confused { 0%, 26.6% { opacity: 0; } 26.8%, 49.9% { opacity: 1; } 50.1%, 100% { opacity: 0; } }
  @keyframes pax-mood-cry { 0%, 50% { opacity: 0; } 50.2%, 74.9% { opacity: 1; } 75.1%, 100% { opacity: 0; } }
  @keyframes pax-mood-shock { 0%, 75% { opacity: 0; } 75.2%, 99.9% { opacity: 1; } 100% { opacity: 0; } }
  @keyframes pax-tear { 0% { transform: translateY(0); opacity: 0; } 15% { opacity: 1; } 100% { transform: translateY(30px); opacity: 0; } }
  @keyframes pax-float { 0%, 100% { transform: translateY(0); } 50% { transform: translateY(-6px); } }`;

/* ── SVG 组装 ── */
function svg({ title, desc, faces, animated }) {
  const style = animated ? `\n  <style>${ANIMATION_CSS}\n  </style>` : '';
  const crabOpen = animated ? '<g class="pax-crab">' : '<g>';
  return `<svg width="${WIDTH}" height="${HEIGHT}" viewBox="${VIEWBOX}" xmlns="http://www.w3.org/2000/svg" role="img" aria-labelledby="pax-t pax-d">
  <title id="pax-t">${title}</title>
  <desc id="pax-d">${desc}</desc>${style}
${SCENE}
  ${crabOpen}${BODY_TOP}
${faces}${BODY_TAIL}
  </g>
</svg>
`;
}

const MOODS = {
  happy: { zh: '笑', en: 'happy', src: 'pax-happy.svg' },
  cry: { zh: '哭', en: 'crying', src: 'pax-cry.svg' },
  shock: { zh: '惊恐', en: 'shocked', src: 'pax-shock.svg' },
  confused: { zh: '疑惑', en: 'puzzled', src: 'pax-confused.svg' },
  blink: { zh: '眨眼', en: 'blinking', src: 'pax-blink.svg' },
};

/* ── 项目图标 pax-mark: 64×64 微缩芯片 (favicon / 图示右下角角标 / 未来应用图标母版) ──
   内部标记单独导出为 MARK_INNER, 供图示流水线盖章复用 (单一事实源) */
const MARK_INNER = `
  <rect x="1.5" y="1.5" width="61" height="61" rx="14" fill="#ea580c" stroke="#9a3412" stroke-width="3"/>
  <path d="M14,31 a18,15 0 0 1 36,0 z" fill="#0e7490" stroke="#0b5e75" stroke-width="2"/>
  <rect x="12" y="28" width="40" height="6.5" rx="3.25" fill="#0b5e75"/>
  <circle cx="22.5" cy="41" r="7" fill="#ffffff" stroke="#7c2d12" stroke-width="2.5"/>
  <circle cx="41.5" cy="41" r="7" fill="#ffffff" stroke="#7c2d12" stroke-width="2.5"/>
  <circle cx="24" cy="42.5" r="3.2" fill="#0f172a"/>
  <circle cx="40" cy="42.5" r="3.2" fill="#0f172a"/>
  <ellipse cx="14.5" cy="50" rx="3" ry="2" fill="#fda4af" opacity="0.8"/>
  <ellipse cx="49.5" cy="50" rx="3" ry="2" fill="#fda4af" opacity="0.8"/>
  <path d="M24,53 q8,6.5 16,0" fill="none" stroke="#7c2d12" stroke-width="3" stroke-linecap="round"/>`;

const MARK_SVG = `<svg width="64" height="64" viewBox="0 0 64 64" xmlns="http://www.w3.org/2000/svg" role="img" aria-labelledby="pax-mark-t pax-mark-d">
  <title id="pax-mark-t">Pax — PolyPack 项目图标</title>
  <desc id="pax-mark-d">橙底圆角芯片上的邮差蟹 Pax 头部：青色邮政帽、双眼与微笑。</desc>${MARK_INNER}
</svg>
`;

/* ── 表情联系表 pax-moods: 五个头部特写 + 中英标签 (README 直接展示多形态) ── */
const SHEET_VIEWBOX = '184 68 232 268'; // 头部特写裁剪窗
const SHEET_ART_W = 150;
const SHEET_ART_H = Math.round((SHEET_ART_W * 268) / 232); // 173
const SHEET_CELL_W = 162;
const SHEET_GAP = 10;
const SHEET_MARGIN = 20;

function moodSheet() {
  const defs = Object.entries(FACES)
    .map(([id, face]) => `    <g id="pax-art-${id}">${CORE}${face}${HEAD}\n    </g>`)
    .join('\n');
  const cells = Object.entries(MOODS)
    .map(([id, mood], i) => {
      const x = SHEET_MARGIN + i * (SHEET_CELL_W + SHEET_GAP);
      const cx = x + SHEET_CELL_W / 2;
      return `  <rect x="${x}" y="14" width="${SHEET_CELL_W}" height="${SHEET_ART_H + 12}" rx="12" fill="#ffffff" stroke="#e2e8f0"/>
  <svg x="${x + 6}" y="20" width="${SHEET_ART_W}" height="${SHEET_ART_H}" viewBox="${SHEET_VIEWBOX}" preserveAspectRatio="xMidYMid meet"><use href="#pax-art-${id}"/></svg>
  <text x="${cx}" y="${SHEET_ART_H + 44}" text-anchor="middle" font-size="13" font-weight="700" fill="#0f172a" font-family="${FONT}">${mood.zh}</text>
  <text x="${cx}" y="${SHEET_ART_H + 59}" text-anchor="middle" font-size="10.5" fill="#64748b" font-family="${FONT}">${mood.en}</text>`;
    })
    .join('\n');
  const width = SHEET_MARGIN * 2 + 5 * SHEET_CELL_W + 4 * SHEET_GAP;
  const height = SHEET_ART_H + 74;
  return `<svg width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" role="img" aria-labelledby="pax-moods-t pax-moods-d">
  <title id="pax-moods-t">Pax 的五种表情 — 笑 / 哭 / 惊恐 / 疑惑 / 眨眼</title>
  <desc id="pax-moods-d">邮差蟹 Pax 的头部特写联系表：五种表情及中英文标签。</desc>
  <rect width="${width}" height="${height}" fill="#f8fafc"/>
  <defs>
${defs}
  </defs>
${cells}
</svg>
`;
}

function allFaces() {
  return Object.values(FACES).join('');
}

const files = new Map();

files.set(
  path.join(ASSETS, 'pax.svg'),
  svg({
    title: 'Pax · 邮差蟹 — 动态表情循环',
    desc: '戴着邮政帽的 Rust 橙螃蟹 Pax：空闲浮动并眨着眼，表情依次循环 笑 → 眨眼 → 疑惑 → 哭 → 惊恐。纯 CSS 动画，无脚本。',
    faces: allFaces(),
    animated: true,
  }),
);

for (const [id, mood] of Object.entries(MOODS)) {
  files.set(
    path.join(ASSETS, mood.src),
    svg({
      title: `Pax · 邮差蟹 — ${mood.zh}表情`,
      desc: `PolyPack 项目宠物 Pax 的${mood.zh}表情（${mood.en}）。`,
      faces: FACES[id],
      animated: false,
    }),
  );
}

/* 项目图标 (favicon / 图示角标 / 应用图标母版) 与表情联系表 */
files.set(path.join(ASSETS, 'pax-mark.svg'), MARK_SVG);
files.set(path.join(ASSETS, 'pax-moods.svg'), moodSheet());

/* ── 本地游乐场: 按钮切换 img 源 (img 内 SVG 是独立文档, 样式互不泄漏) ── */
const buttons = [
  { id: 'auto', label: '自动循环', zh: '动态版', src: 'assets/pax.svg' },
  ...Object.entries(MOODS).map(([id, m]) => ({ id, label: m.zh, zh: m.zh, src: `assets/${m.src}` })),
];

files.set(
  path.join(ROOT, 'docs', 'pax.html'),
  `<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<link rel="icon" type="image/svg+xml" href="assets/pax-mark.svg">
<title>Pax · 邮差蟹 — PolyPack 游乐场</title>
<style>
  :root { color-scheme: light; }
  body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: #f8fafc;
         font-family: 'JetBrains Mono', ui-monospace, 'PingFang SC', 'Microsoft YaHei', monospace; color: #0f172a; }
  main { width: min(760px, 92vw); padding: 28px 24px 24px; background: #ffffff; border: 1px solid #e2e8f0;
         border-radius: 18px; text-align: center; }
  h1 { margin: 0 0 4px; font-size: 20px; }
  p.sub { margin: 0 0 18px; color: #64748b; font-size: 13px; }
  .stage { background: #f8fafc; border: 1px dashed #e2e8f0; border-radius: 14px; padding: 10px; }
  .stage img { width: min(560px, 100%); height: auto; display: block; margin: 0 auto; }
  .moods { display: flex; flex-wrap: wrap; gap: 10px; justify-content: center; margin-top: 18px; }
  button { font: inherit; font-size: 13px; padding: 8px 16px; border-radius: 999px; cursor: pointer;
           border: 1px solid #e2e8f0; background: #ffffff; color: #0f172a; }
  button:hover { border-color: #94a3b8; }
  button.active { background: #ea580c; border-color: #ea580c; color: #ffffff; }
  footer { margin-top: 14px; color: #64748b; font-size: 12px; }
  code { background: #f1f5f9; padding: 1px 6px; border-radius: 6px; }
</style>
</head>
<body>
<main>
  <h1>Pax · 邮差蟹</h1>
  <p class="sub">PolyPack 项目宠物 · 五形态表情游乐场</p>
  <div class="stage"><img id="stage" src="assets/pax.svg" alt="Pax 的表情预览" width="640" height="384"></div>
  <div class="moods" role="group" aria-label="切换 Pax 的表情">
${buttons.map((b) => `    <button data-src="${b.src}"${b.id === 'auto' ? ' class="active"' : ''}>${b.label}</button>`).join('\n')}
  </div>
  <footer>动画在 SVG 内部由纯 CSS 驱动；本页按钮仅切换文件（<code>img</code> 内 SVG 为独立文档，互不干扰）。</footer>
</main>
<script>
  var stage = document.getElementById('stage');
  var buttons = Array.prototype.slice.call(document.querySelectorAll('button[data-src]'));
  buttons.forEach(function (btn) {
    btn.addEventListener('click', function () {
      stage.src = btn.dataset.src;
      buttons.forEach(function (b) { b.classList.toggle('active', b === btn); });
    });
  });
</script>
</body>
</html>
`,
);

/* ── 写盘 / 校验 ── */
const check = process.argv.includes('--check');
let failed = 0;
for (const [file, content] of files) {
  const rel = path.relative(ROOT, file);
  if (check) {
    const existing = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : null;
    if (existing === content) {
      console.log(`ok   ${rel}`);
    } else {
      failed += 1;
      console.error(`DRIFT ${rel} — 运行 node tools/build-pax.mjs 重新生成`);
    }
  } else {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, content);
    console.log(`wrote ${rel} (${(content.length / 1024).toFixed(1)} KiB)`);
  }
}
process.exit(failed ? 1 : 0);
