#!/usr/bin/env node
/**
 * build-pax.mjs — Pax（项目宠物）单一事实源生成器
 *
 * 输入: 本文件内的共享小机器人身体 + 十二种彩色屏幕表情
 *   （笑/欢乐/哭/沮丧/惊恐/愤怒/撒娇/卖萌/讨好/敲打/捂脸/吐舌头）
 * 输出:
 *   docs/assets/pax.svg           动态版: 空闲浮动 + 十二表情循环 (纯 CSS 动画, <img> 嵌入可播放)
 *   docs/assets/pax-<id>.svg      单表情动态版: 每个形态自带专属动效
 *   docs/assets/pax-mark.svg      项目图标
 *   docs/assets/pax-moods.svg     十二形态联系表 (6×2)
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
const CYCLE_SECONDS = 24; // 12 表情 × 2s

const FONT = "'JetBrains Mono', ui-monospace, 'PingFang SC', 'Microsoft YaHei', monospace";

/* ── 场景: 投递路线与平台图标 (不随表情/浮动变化) ── */
const SCENE = `
  <defs>
    <marker id="pax-arrow" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
      <path d="M0,0 L10,5 L0,10 z" fill="#94a3b8"/>
    </marker>
    <clipPath id="pax-screen-clip"><rect x="248" y="144" width="104" height="76" rx="12"/></clipPath>
  </defs>
  <path d="M400,210 C 450,140 500,90 548,76" fill="none" stroke="#94a3b8" stroke-width="2" stroke-dasharray="5 6" marker-end="url(#pax-arrow)"/>
  <path d="M392,226 C 480,190 560,160 622,144" fill="none" stroke="#94a3b8" stroke-width="2" stroke-dasharray="5 6" marker-end="url(#pax-arrow)"/>
  <path d="M372,292 C 440,318 520,315 586,300" fill="none" stroke="#94a3b8" stroke-width="2" stroke-dasharray="5 6" marker-end="url(#pax-arrow)"/>
  <g stroke="#64748b" stroke-width="3" fill="#ffffff" stroke-linejoin="round">
    <rect x="552" y="50" width="28" height="48" rx="7"/>
    <rect x="626" y="126" width="26" height="32" rx="7"/>
    <path d="M630,126 v-10 h18 v10 M630,158 v10 h18 v-10" fill="none"/>
    <rect x="588" y="276" width="66" height="42" rx="7"/>
    <path d="M621,318 v9 M607,327 h28" fill="none"/>
  </g>
  <circle cx="566" cy="88" r="2.6" fill="#64748b" stroke="none"/>
  <ellipse cx="300" cy="352" rx="88" ry="9" fill="#0f172a" opacity="0.07"/>`;

/* ── 共享身体: 短腿 / 纸箱 / 脖子 / 屏幕头 / 耳钉 / 天线 / 手臂与包裹 ── */
const LEGS = `
    <g stroke="#9a3412" stroke-width="7" stroke-linecap="round" fill="none">
      <path d="M282,318 L278,338"/>
      <path d="M318,318 L322,338"/>
    </g>
    <rect x="262" y="336" width="34" height="13" rx="6.5" fill="#ea580c" stroke="#9a3412" stroke-width="2.5"/>
    <rect x="304" y="336" width="34" height="13" rx="6.5" fill="#ea580c" stroke="#9a3412" stroke-width="2.5"/>`;

const BODY = `
    <rect x="252" y="244" width="96" height="76" rx="14" fill="#d4a373" stroke="#9a3412" stroke-width="3"/>
    <rect x="292" y="244" width="16" height="76" fill="#b08968" opacity="0.85"/>
    <rect x="262" y="256" width="44" height="30" rx="4" fill="#fffdf7" stroke="#b08968" stroke-width="1.5"/>
    <text x="284" y="270" text-anchor="middle" font-size="8.5" font-weight="700" font-family="${FONT}" fill="#7c4a2a">POLYPACK</text>
    <path d="M266,276 h36 M268,280 h32 M272,284 h24" stroke="#7c4a2a" stroke-width="1.5"/>
    <circle cx="330" cy="272" r="12" fill="#ffffff" stroke="#0891b2" stroke-width="2" stroke-dasharray="2.5 2"/>
    <ellipse cx="330" cy="272" rx="8" ry="3.2" fill="none" stroke="#0891b2" stroke-width="1.5" transform="rotate(-30 330 272)"/>
    <circle cx="330" cy="272" r="1.8" fill="#0891b2"/>
    <circle cx="330" cy="300" r="3" fill="#22c55e"/>`;

const NECK = `<rect x="286" y="232" width="28" height="14" rx="4" fill="#94a3b8" stroke="#475569" stroke-width="2"/>`;

const HEAD = `
    <rect x="232" y="128" width="136" height="108" rx="22" fill="#f8fafc" stroke="#7c2d12" stroke-width="3"/>
    <rect x="248" y="144" width="104" height="76" rx="12" fill="#0f172a" stroke="#0e7490" stroke-width="2.5"/>`;

const BODY_TOP = LEGS + BODY + NECK + HEAD;

const EARS = `<circle cx="232" cy="170" r="9" fill="#ea580c" stroke="#9a3412" stroke-width="2.5"/><circle cx="368" cy="170" r="9" fill="#ea580c" stroke="#9a3412" stroke-width="2.5"/>`;

const ANTENNA = `
    <path d="M300,128 L300,106" stroke="#9a3412" stroke-width="4" stroke-linecap="round"/>
    <circle cx="300" cy="103" r="6" fill="#ea580c" stroke="#9a3412" stroke-width="2"/>
    <path d="M305,100 L322,105 L305,111 Z" fill="#0891b2" stroke="#0e7490" stroke-width="1.5" stroke-linejoin="round"/>`;

const ARMS = `
    <path d="M252,262 L224,286" stroke="#ea580c" stroke-width="9" stroke-linecap="round"/>
    <circle cx="218" cy="292" r="11" fill="#ea580c" stroke="#9a3412" stroke-width="2.5"/>
    <circle cx="204" cy="300" r="10" fill="#f8fafc" stroke="#9a3412" stroke-width="2.5"/>
    <circle cx="204" cy="300" r="4" fill="#b08968"/>
    <path d="M348,262 L378,238" stroke="#ea580c" stroke-width="9" stroke-linecap="round"/>
    <rect x="374" y="202" width="28" height="26" rx="3" fill="#d4a373" stroke="#9a3412" stroke-width="2"/>
    <path d="M388,202 v26 M374,212 h28" stroke="#b08968" stroke-width="3"/>
    <circle cx="384" cy="232" r="11" fill="#ea580c" stroke="#9a3412" stroke-width="2.5"/>`;

const BODY_TAIL = EARS + ANTENNA + ARMS;

/* ── 屏幕表情零件 (眼 / 嘴 / 腮红 / 道具) ── */
const BLUSH = `<ellipse cx="258" cy="193" rx="7" ry="4" fill="#fb7185" opacity="0.8"/><ellipse cx="342" cy="193" rx="7" ry="4" fill="#fb7185" opacity="0.8"/>`;
const SMILE_LINE = `<path d="M290,199 Q300,207 310,199" fill="none" stroke="#e0f2fe" stroke-width="3" stroke-linecap="round"/>`;
const CRY_MOUTH = `<path d="M290,206 Q300,195 310,206" fill="none" stroke="#e0f2fe" stroke-width="3" stroke-linecap="round"/>`;
const FROWN = `<path d="M290,204 Q300,197 310,204" fill="none" stroke="#e0f2fe" stroke-width="3" stroke-linecap="round"/>`;
const OPEN_SMILE = `<path d="M288,196 Q300,214 312,196 Z" fill="#67e8f9"/><ellipse cx="300" cy="209" rx="4" ry="2" fill="#fb7185"/>`;
const POUT = `<ellipse cx="300" cy="202" rx="4.5" ry="5.5" fill="none" stroke="#e0f2fe" stroke-width="2.5"/>`;
const GRIT_TEETH = `<rect x="288" y="196" width="24" height="9" rx="2" fill="#e0f2fe"/><path d="M296,196 v9 M304,196 v9" stroke="#0e7490" stroke-width="1.5"/>`;
const SHOUT = `<ellipse cx="300" cy="203" rx="6.5" ry="7" fill="#67e8f9"/><ellipse cx="300" cy="206" rx="4" ry="2.5" fill="#fb7185"/>`;
const WAVY_MOUTH = `<path d="M288,201 q4,4 8,0 q4,-4 8,0 q4,4 8,0" fill="none" stroke="#e0f2fe" stroke-width="3" stroke-linecap="round"/>`;
const TONGUE_MOUTH = `<path d="M288,197 Q300,211 312,197 Z" fill="#67e8f9"/><path d="M296,205 q4,10 8,0 z" fill="#fb7185" stroke="#0f172a" stroke-width="1"/>`;
const ROUND_EYES = `<circle cx="279" cy="171" r="8.5" fill="#f8fafc"/><circle cx="321" cy="171" r="8.5" fill="#f8fafc"/><circle cx="281" cy="173" r="4" fill="#0f172a"/><circle cx="319" cy="173" r="4" fill="#0f172a"/>`;
const HAPPY_EYES = `<path d="M270,173 Q279,163 288,173" fill="none" stroke="#e0f2fe" stroke-width="3.5" stroke-linecap="round"/><path d="M312,173 Q321,163 330,173" fill="none" stroke="#e0f2fe" stroke-width="3.5" stroke-linecap="round"/>`;
const DROOPY_EYES = `<path d="M270,169 Q279,178 288,169" fill="none" stroke="#e0f2fe" stroke-width="3.5" stroke-linecap="round"/><path d="M312,169 Q321,178 330,169" fill="none" stroke="#e0f2fe" stroke-width="3.5" stroke-linecap="round"/>`;
const LASHED_EYES = `${HAPPY_EYES}<path d="M269,166 l-4,-4 M331,166 l4,-4" fill="none" stroke="#e0f2fe" stroke-width="2.5" stroke-linecap="round"/>`;
const WINK = `<circle cx="279" cy="171" r="8.5" fill="#f8fafc"/><circle cx="281" cy="173" r="4" fill="#0f172a"/><path d="M312,173 Q321,163 330,173" fill="none" stroke="#e0f2fe" stroke-width="3.5" stroke-linecap="round"/>`;
const FOCUS_EYES = `<circle cx="279" cy="171" r="8.5" fill="#f8fafc"/><circle cx="321" cy="171" r="8.5" fill="#f8fafc"/><circle cx="283" cy="173" r="4" fill="#0f172a"/><circle cx="325" cy="173" r="4" fill="#0f172a"/>`;
const PUPPY_EYES = `<path d="M268,155 Q279,150 290,155" fill="none" stroke="#e0f2fe" stroke-width="2.5" stroke-linecap="round"/><path d="M310,155 Q321,150 332,155" fill="none" stroke="#e0f2fe" stroke-width="2.5" stroke-linecap="round"/><circle cx="279" cy="172" r="10" fill="#f8fafc"/><circle cx="321" cy="172" r="10" fill="#f8fafc"/><circle cx="281" cy="175" r="5.5" fill="#0f172a"/><circle cx="319" cy="175" r="5.5" fill="#0f172a"/><circle cx="283" cy="171" r="1.8" fill="#ffffff"/><circle cx="321" cy="171" r="1.8" fill="#ffffff"/>`;
const HEART_EYES = `<g transform="translate(279,171)"><path d="M0,3 C-2,-4 -9,-1 -7,4 C-5,8 0,12 0,12 C0,12 5,8 7,4 C9,-1 2,-4 0,3 Z" fill="#fb7185" stroke="#fda4af" stroke-width="1"/></g><g transform="translate(321,171)"><path d="M0,3 C-2,-4 -9,-1 -7,4 C-5,8 0,12 0,12 C0,12 5,8 7,4 C9,-1 2,-4 0,3 Z" fill="#fb7185" stroke="#fda4af" stroke-width="1"/></g>`;
const ANGRY_BROWS = `<path d="M268,157 L290,163" fill="none" stroke="#f87171" stroke-width="3.5" stroke-linecap="round"/><path d="M310,163 L332,157" fill="none" stroke="#f87171" stroke-width="3.5" stroke-linecap="round"/>`;
const DETERMINED_BROWS = `<path d="M270,158 L290,162" fill="none" stroke="#fbbf24" stroke-width="3" stroke-linecap="round"/><path d="M310,162 L330,158" fill="none" stroke="#fbbf24" stroke-width="3" stroke-linecap="round"/>`;
const SHOCK_EYES = `<circle cx="279" cy="170" r="10.5" fill="#f8fafc"/><circle cx="321" cy="170" r="10.5" fill="#f8fafc"/><circle cx="279" cy="169" r="2.5" fill="#0f172a"/><circle cx="321" cy="169" r="2.5" fill="#0f172a"/>`;
const SHOCK_MOUTH = `<ellipse cx="300" cy="204" rx="5.5" ry="7" fill="none" stroke="#e0f2fe" stroke-width="3"/>`;
const EMBARRASSED_EYES = `<path d="M268,164 L282,171 L268,178" fill="none" stroke="#e0f2fe" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/><path d="M332,164 L318,171 L332,178" fill="none" stroke="#e0f2fe" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/><path d="M292,201 h16" stroke="#e0f2fe" stroke-width="3" stroke-linecap="round"/>`;
const TEARS = `<g class="pax-tear pax-tear-a"><path d="M267,181 C270,185 270,189 267,191 C264,189 264,185 267,181 Z" fill="#7dd3fc"/></g><g class="pax-tear pax-tear-b"><path d="M333,181 C336,185 336,189 333,191 C330,189 330,185 333,181 Z" fill="#7dd3fc"/></g>`;
const SWEAT = `<g class="pax-sweat"><path d="M382,140 C386,146 386,152 382,155 C378,152 378,146 382,140 Z" fill="#bae6fd" stroke="#0891b2" stroke-width="1.5"/></g>`;
const SPARKLES = `
      <g transform="translate(212,118)"><path class="pax-sparkle" d="M0,-8 L2.1,-2.1 L8,0 L2.1,2.1 L0,8 L-2.1,2.1 L-8,0 L-2.1,-2.1 Z" fill="#fbbf24"/></g>
      <g transform="translate(392,108)"><path class="pax-sparkle pax-sparkle-b" d="M0,-7 L1.9,-1.9 L7,0 L1.9,1.9 L0,7 L-1.9,1.9 L-7,0 L-1.9,-1.9 Z" fill="#f59e0b"/></g>
      <g transform="translate(404,254)"><path class="pax-sparkle pax-sparkle-c" d="M0,-6 L1.6,-1.6 L6,0 L1.6,1.6 L0,6 L-1.6,1.6 L-6,0 L-1.6,-1.6 Z" fill="#fbbf24"/></g>`;
const HEARTS = `
      <g transform="translate(362,104)"><path class="pax-heart pax-heart-a" d="M0,4 C-3,-3 -11,0 -8,6 C-6,10 0,15 0,15 C0,15 6,10 8,6 C11,0 3,-3 0,4 Z" fill="#fb7185" stroke="#f43f5e" stroke-width="1.5"/></g>
      <g transform="translate(384,122)"><path class="pax-heart pax-heart-b" d="M0,3 C-2,-2 -8,0 -6,4 C-4,7 0,11 0,11 C0,11 4,7 6,4 C8,0 2,-2 0,3 Z" fill="#fda4af"/></g>`;
const SIGH = `<g transform="translate(232,110)"><g class="pax-sigh" fill="#cbd5e1"><ellipse cx="0" cy="0" rx="11" ry="6"/><ellipse cx="9" cy="-5" rx="7" ry="5"/><ellipse cx="-8" cy="-4" rx="6" ry="4"/></g></g>`;
const VEIN = `<g transform="translate(342,112)"><g class="pax-vein" stroke="#dc2626" stroke-width="4" stroke-linecap="round"><path d="M-7,-7 L7,7 M7,-7 L-7,7"/></g></g>`;
const STEAM = `
      <g transform="translate(214,140)"><g class="pax-steam" fill="#e2e8f0" stroke="#94a3b8" stroke-width="2"><circle cx="0" cy="0" r="6"/><circle cx="8" cy="-6" r="4.5"/><circle cx="-7" cy="-5" r="4"/></g></g>
      <g transform="translate(386,140)"><g class="pax-steam pax-steam-b" fill="#e2e8f0" stroke="#94a3b8" stroke-width="2"><circle cx="0" cy="0" r="6"/><circle cx="8" cy="-6" r="4.5"/><circle cx="-7" cy="-5" r="4"/></g></g>`;
const IMPACT = `<g transform="translate(400,216)"><g class="pax-impact" stroke="#f59e0b" stroke-width="4" stroke-linecap="round"><path d="M10,-12 L16,-18"/><path d="M14,0 L22,0"/><path d="M10,12 L16,18"/></g></g>`;

/* ── 关键帧: 身体动作与表情道具 ── */
const KF = {
  bob: '@keyframes pax-bob { 0%, 100% { transform: translateY(0); } 50% { transform: translateY(-4px); } }',
  bounce: '@keyframes pax-bounce { 0%, 100% { transform: translateY(0); } 50% { transform: translateY(-6px); } }',
  sway: '@keyframes pax-sway { 0%, 100% { transform: rotate(-1.8deg); } 50% { transform: rotate(1.8deg); } }',
  droop: '@keyframes pax-droop { 0%, 100% { transform: translateY(0); } 50% { transform: translateY(4px); } }',
  shiver: '@keyframes pax-shiver { 0%, 100% { transform: translateX(0); } 25% { transform: translateX(-1.2px); } 75% { transform: translateX(1.2px); } }',
  rage: '@keyframes pax-rage { 0%, 100% { transform: translateX(0); } 25% { transform: translateX(-2px); } 75% { transform: translateX(2px); } }',
  tap: '@keyframes pax-tap { 0%, 100% { transform: translateX(0); } 30% { transform: translateX(4px); } 55% { transform: translateX(-1px); } }',
  tear: '@keyframes pax-tear { 0% { transform: translateY(0); opacity: 0; } 15% { opacity: 1; } 100% { transform: translateY(12px); opacity: 0; } }',
  drip: '@keyframes pax-drip { 0% { transform: translateY(0); opacity: 0.3; } 40% { opacity: 1; } 100% { transform: translateY(9px); opacity: 0; } }',
  sparkle: '@keyframes pax-sparkle { 0%, 100% { opacity: 0.25; transform: scale(0.72); } 50% { opacity: 1; transform: scale(1.18); } }',
  heart: '@keyframes pax-heart { 0% { opacity: 0; transform: translateY(8px) scale(0.7); } 25% { opacity: 1; } 100% { opacity: 0; transform: translateY(-22px) scale(1.05); } }',
  heartEye: '@keyframes pax-heart-eye { 0%, 100% { transform: scale(0.92); } 50% { transform: scale(1.08); } }',
  steam: '@keyframes pax-steam { 0% { opacity: 0; transform: translateY(6px) scale(0.8); } 30% { opacity: 1; } 100% { opacity: 0; transform: translateY(-16px) scale(1.1); } }',
  impact: '@keyframes pax-impact { 0%, 100% { opacity: 0.2; transform: scale(0.7); } 35% { opacity: 1; transform: scale(1); } }',
  vein: '@keyframes pax-vein { 0%, 100% { opacity: 0.75; transform: scale(0.9); } 50% { opacity: 1; transform: scale(1.15); } }',
  tongue: '@keyframes pax-tongue { 0%, 100% { transform: rotate(-6deg); } 50% { transform: rotate(6deg); } }',
  sigh: '@keyframes pax-sigh { 0% { opacity: 0; transform: translateX(0) scale(0.85); } 30% { opacity: 1; } 100% { opacity: 0; transform: translateX(14px) scale(1.05); } }',
};

/* ── 十二种彩色屏幕表情: 图层 + 专属动效 (单文件与联系表共用同一份定义) ── */
const MOODS = {
  smile: {
    zh: '笑', en: 'smile', src: 'pax-smile.svg',
    face: `<g class="pax-face pax-face-smile"><g clip-path="url(#pax-screen-clip)">${BLUSH}${SMILE_LINE}${ROUND_EYES}</g></g>`,
    motion: [['.pax-bot', 'animation: pax-bob 3s ease-in-out infinite;']],
    kf: ['bob'],
  },
  joy: {
    zh: '欢乐', en: 'joy', src: 'pax-joy.svg',
    face: `<g class="pax-face pax-face-joy"><g clip-path="url(#pax-screen-clip)">${BLUSH}${OPEN_SMILE}${HAPPY_EYES}</g>${SPARKLES}</g>`,
    motion: [['.pax-bot', 'animation: pax-bounce 1.7s ease-in-out infinite;'], ['.pax-sparkle', 'animation: pax-sparkle 1.4s ease-in-out infinite;'], ['.pax-sparkle-b', 'animation-delay: -0.45s;'], ['.pax-sparkle-c', 'animation-delay: -0.9s;']],
    kf: ['bounce', 'sparkle'],
  },
  cry: {
    zh: '哭', en: 'crying', src: 'pax-cry.svg',
    face: `<g class="pax-face pax-face-cry"><g clip-path="url(#pax-screen-clip)">${CRY_MOUTH}${ROUND_EYES}${TEARS}</g></g>`,
    motion: [['.pax-bot', 'animation: pax-bob 4s ease-in-out infinite;'], ['.pax-tear-a', 'animation: pax-tear 1.15s ease-in infinite;'], ['.pax-tear-b', 'animation: pax-tear 1.15s ease-in 0.2s infinite;']],
    kf: ['bob', 'tear'],
  },
  dejected: {
    zh: '沮丧', en: 'dejected', src: 'pax-dejected.svg',
    face: `<g class="pax-face pax-face-dejected"><g clip-path="url(#pax-screen-clip)">${DROOPY_EYES}${FROWN}</g>${SIGH}</g>`,
    motion: [['.pax-bot', 'animation: pax-droop 4.5s ease-in-out infinite;'], ['.pax-sigh', 'animation: pax-sigh 3s ease-out infinite;']],
    kf: ['droop', 'sigh'],
  },
  shock: {
    zh: '惊恐', en: 'shocked', src: 'pax-shock.svg',
    face: `<g class="pax-face pax-face-shock"><g clip-path="url(#pax-screen-clip)">${SHOCK_EYES}${SHOCK_MOUTH}</g>${SWEAT}</g>`,
    motion: [['.pax-bot', 'animation: pax-shiver 0.16s linear infinite;'], ['.pax-sweat', 'animation: pax-drip 1.4s ease-in infinite;']],
    kf: ['shiver', 'drip'],
  },
  angry: {
    zh: '愤怒', en: 'angry', src: 'pax-angry.svg',
    face: `<g class="pax-face pax-face-angry"><g clip-path="url(#pax-screen-clip)">${ROUND_EYES}${ANGRY_BROWS}${GRIT_TEETH}</g>${VEIN}${STEAM}</g>`,
    motion: [['.pax-bot', 'animation: pax-rage 0.32s linear infinite;'], ['.pax-vein', 'animation: pax-vein 0.7s ease-in-out infinite;'], ['.pax-steam', 'animation: pax-steam 1.3s ease-out infinite;'], ['.pax-steam-b', 'animation-delay: -0.65s;']],
    kf: ['rage', 'vein', 'steam'],
  },
  coquettish: {
    zh: '撒娇', en: 'coy', src: 'pax-coquettish.svg',
    face: `<g class="pax-face pax-face-coquettish"><g clip-path="url(#pax-screen-clip)">${BLUSH}${LASHED_EYES}${POUT}</g>${HEARTS}</g>`,
    motion: [['.pax-bot', 'transform-origin: 300px 300px; animation: pax-sway 3s ease-in-out infinite;'], ['.pax-heart-a', 'animation: pax-heart 2.4s ease-out infinite;'], ['.pax-heart-b', 'animation: pax-heart 2.4s ease-out 1.2s infinite;']],
    kf: ['sway', 'heart'],
  },
  cute: {
    zh: '卖萌', en: 'cute', src: 'pax-cute.svg',
    face: `<g class="pax-face pax-face-cute"><g clip-path="url(#pax-screen-clip)">${BLUSH}${OPEN_SMILE}${HEART_EYES}</g>${SPARKLES}</g>`,
    motion: [['.pax-bot', 'animation: pax-bounce 1.5s ease-in-out infinite;'], ['.pax-heart-eye', 'transform-origin: 0 8px; animation: pax-heart-eye 1.4s ease-in-out infinite;'], ['.pax-sparkle', 'animation: pax-sparkle 1.2s ease-in-out infinite;'], ['.pax-sparkle-b', 'animation-delay: -0.4s;'], ['.pax-sparkle-c', 'animation-delay: -0.8s;']],
    kf: ['bounce', 'heartEye', 'sparkle'],
  },
  ingratiating: {
    zh: '讨好', en: 'ingratiating', src: 'pax-ingratiating.svg',
    face: `<g class="pax-face pax-face-ingratiating"><g clip-path="url(#pax-screen-clip)">${BLUSH}${PUPPY_EYES}${SMILE_LINE}</g>${SPARKLES}</g>`,
    motion: [['.pax-bot', 'animation: pax-bounce 2.1s ease-in-out infinite;'], ['.pax-sparkle', 'animation: pax-sparkle 1.6s ease-in-out infinite;'], ['.pax-sparkle-b', 'animation-delay: -0.5s;'], ['.pax-sparkle-c', 'animation-delay: -1s;']],
    kf: ['bounce', 'sparkle'],
  },
  knock: {
    zh: '敲打', en: 'knocking', src: 'pax-knock.svg',
    face: `<g class="pax-face pax-face-knock"><g clip-path="url(#pax-screen-clip)">${FOCUS_EYES}${DETERMINED_BROWS}${SHOUT}</g>${IMPACT}</g>`,
    motion: [['.pax-bot', 'animation: pax-tap 0.9s ease-in-out infinite;'], ['.pax-impact', 'animation: pax-impact 0.9s ease-out infinite;']],
    kf: ['tap', 'impact'],
  },
  facepalm: {
    zh: '捂脸', en: 'covering face', src: 'pax-facepalm.svg',
    face: `<g class="pax-face pax-face-facepalm"><g clip-path="url(#pax-screen-clip)">${BLUSH}${EMBARRASSED_EYES}</g>${SWEAT}</g>`,
    motion: [['.pax-bot', 'transform-origin: 300px 300px; animation: pax-sway 3.2s ease-in-out infinite;'], ['.pax-sweat', 'animation: pax-drip 2s ease-in infinite;']],
    kf: ['sway', 'drip'],
  },
  tongue: {
    zh: '吐舌头', en: 'sticking tongue', src: 'pax-tongue.svg',
    face: `<g class="pax-face pax-face-tongue"><g clip-path="url(#pax-screen-clip)">${BLUSH}${WINK}${TONGUE_MOUTH}</g></g>`,
    motion: [['.pax-bot', 'animation: pax-bob 2.6s ease-in-out infinite;'], ['.pax-tongue', 'animation: pax-tongue 0.9s ease-in-out infinite;']],
    kf: ['bob', 'tongue'],
  },
};

/* 单表情文件: 基态 = 该形态静态; 动效仅在 no-preference 下播放 */
function moodStyle(mood) {
  const rules = mood.motion.map(([sel, decl]) => `${sel} { ${decl} }`).join('\n    ');
  return `@media (prefers-reduced-motion: no-preference) {\n    ${rules}\n  }\n  ${mood.kf.map((n) => KF[n]).join('\n  ')}`;
}

/* 全表情循环: 每张脸按 1/12 时间片显隐, 身体恒定浮动, 道具动效全程运行 */
function cycleCss() {
  const ids = Object.keys(MOODS);
  const step = 100 / ids.length;
  const faceRules = ids
    .map((id) => `.pax-face-${id} { animation: pax-mood-${id} ${CYCLE_SECONDS}s linear infinite; }`)
    .join('\n    ');
  const moodKf = ids.map((id, i) => {
    const start = i * step;
    const end = (i + 1) * step;
    const on = `${(start + 0.25).toFixed(2)}%`;
    const off = `${(end - 0.25).toFixed(2)}%`;
    return i === 0
      ? `@keyframes pax-mood-${id} { 0%, ${off} { opacity: 1; } ${end.toFixed(2)}%, 100% { opacity: 0; } }`
      : `@keyframes pax-mood-${id} { 0%, ${start.toFixed(2)}% { opacity: 0; } ${on}, ${off} { opacity: 1; } ${end.toFixed(2)}%, 100% { opacity: 0; } }`;
  });
  const seen = new Set();
  const accessory = [];
  for (const mood of Object.values(MOODS)) {
    for (const [sel, decl] of mood.motion) {
      if (sel === '.pax-bot' || seen.has(sel)) continue;
      seen.add(sel);
      accessory.push(`${sel} { ${decl} }`);
    }
  }
  const kfNames = new Set(['bob']);
  for (const mood of Object.values(MOODS)) mood.kf.forEach((n) => kfNames.add(n));
  return `.pax-face { opacity: 0; }
  .pax-face-${ids[0]} { opacity: 1; }
  @media (prefers-reduced-motion: no-preference) {
    .pax-bot { animation: pax-bob 3.4s ease-in-out infinite; }
    ${faceRules}
    ${accessory.join('\n    ')}
  }
  ${[...kfNames].map((n) => KF[n]).join('\n  ')}
  ${moodKf.join('\n  ')}`;
}

/* ── SVG 组装 ── */
function svg({ title, desc, faces, animated, css = cycleCss() }) {
  const style = animated ? `\n  <style>${css}\n  </style>` : '';
  const botOpen = animated ? '<g class="pax-bot">' : '<g>';
  return `<svg width="${WIDTH}" height="${HEIGHT}" viewBox="${VIEWBOX}" xmlns="http://www.w3.org/2000/svg" role="img" aria-labelledby="pax-t pax-d">
  <title id="pax-t">${title}</title>
  <desc id="pax-d">${desc}</desc>${style}
${SCENE}
  ${botOpen}${BODY_TOP}
${faces}${BODY_TAIL}
  </g>
</svg>
`;
}

/* ── 项目图标 pax-mark: 64×64 微缩机器人头 (favicon / 图示右下角角标 / 未来应用图标母版) ── */
const MARK_INNER = `
  <rect x="1.5" y="1.5" width="61" height="61" rx="14" fill="#ea580c" stroke="#9a3412" stroke-width="3"/>
  <path d="M32,16 v-6" stroke="#7c2d12" stroke-width="2"/>
  <circle cx="32" cy="8" r="3" fill="#0891b2" stroke="#0e7490" stroke-width="1.2"/>
  <rect x="12" y="16" width="40" height="32" rx="8" fill="#f8fafc" stroke="#7c2d12" stroke-width="2.5"/>
  <rect x="17" y="21" width="30" height="21" rx="5" fill="#0f172a" stroke="#0e7490" stroke-width="1.5"/>
  <circle cx="26" cy="30" r="3.2" fill="#e0f2fe"/>
  <circle cx="38" cy="30" r="3.2" fill="#e0f2fe"/>
  <circle cx="26.5" cy="30.5" r="1.4" fill="#0f172a"/>
  <circle cx="37.5" cy="30.5" r="1.4" fill="#0f172a"/>
  <path d="M27,36 Q32,40 37,36" fill="none" stroke="#67e8f9" stroke-width="2" stroke-linecap="round"/>
  <circle cx="12" cy="30" r="3" fill="#0891b2"/>
  <circle cx="52" cy="30" r="3" fill="#0891b2"/>
  <rect x="22" y="48" width="20" height="7" rx="2" fill="#d4a373" stroke="#7c2d12" stroke-width="1.5"/>`;

const MARK_SVG = `<svg width="64" height="64" viewBox="0 0 64 64" xmlns="http://www.w3.org/2000/svg" role="img" aria-labelledby="pax-mark-t pax-mark-d">
  <title id="pax-mark-t">Pax — PolyPack 项目图标</title>
  <desc id="pax-mark-d">橙底圆角芯片上的打包机器人 Pax 徽章：白色头部、深色屏幕脸、青色天线与纸箱色胸甲。</desc>${MARK_INNER}
</svg>
`;

/* ── 表情联系表 pax-moods: 十二个全身像 (6×2, 含短腿与包裹) + 中英标签 ── */
const SHEET_VIEWBOX = '150 92 280 272'; // 全身裁剪窗 (包裹与敲打火星都在内)
const SHEET_COLS = 6;
const SHEET_ART_W = 130;
const SHEET_ART_H = Math.round((SHEET_ART_W * 272) / 280); // 126
const SHEET_CELL_W = 150;
const SHEET_GAP = 10;
const SHEET_MARGIN = 20;
const SHEET_CELL_H = SHEET_ART_H + 74; // 200 (含两行标签)

function moodSheet() {
  const entries = Object.entries(MOODS);
  const rows = Math.ceil(entries.length / SHEET_COLS);
  const cells = entries
    .map(([id, mood], i) => {
      const col = i % SHEET_COLS;
      const y = Math.floor(i / SHEET_COLS) * (SHEET_CELL_H + SHEET_GAP);
      const x = SHEET_MARGIN + col * (SHEET_CELL_W + SHEET_GAP);
      const cx = x + SHEET_CELL_W / 2;
      const art = `${BODY_TOP}${mood.face}${BODY_TAIL}`; // 全身: 腿 + 身体 + 表情 + 头饰与手臂
      return `  <g class="cell cell-${id}">
    <rect x="${x}" y="${14 + y}" width="${SHEET_CELL_W}" height="${SHEET_ART_H + 12}" rx="12" fill="#ffffff" stroke="#e2e8f0"/>
    <svg x="${x + 10}" y="${20 + y}" width="${SHEET_ART_W}" height="${SHEET_ART_H}" viewBox="${SHEET_VIEWBOX}" preserveAspectRatio="xMidYMid meet">
      <g class="pax-bot">${art}</g>
    </svg>
    <text x="${cx}" y="${SHEET_ART_H + 44 + y}" text-anchor="middle" font-size="13" font-weight="700" fill="#0f172a" font-family="${FONT}">${mood.zh}</text>
    <text x="${cx}" y="${SHEET_ART_H + 59 + y}" text-anchor="middle" font-size="10.5" fill="#64748b" font-family="${FONT}">${mood.en}</text>
  </g>`;
    })
    .join('\n');
  const motion = entries
    .flatMap(([id, mood], i) =>
      mood.motion.map(([sel, decl]) => {
        const delay = sel === '.pax-bot' ? ` animation-delay: ${(-i * 0.45).toFixed(2)}s;` : '';
        return `.cell-${id} ${sel} { ${decl}${delay} }`;
      }),
    )
    .join('\n    ');
  const kfNames = new Set();
  for (const mood of Object.values(MOODS)) mood.kf.forEach((n) => kfNames.add(n));
  const css = [
    `@media (prefers-reduced-motion: no-preference) {\n    ${motion}\n  }`,
    ...[...kfNames].map((n) => KF[n]),
  ].join('\n  ');
  const width = SHEET_MARGIN * 2 + SHEET_COLS * SHEET_CELL_W + (SHEET_COLS - 1) * SHEET_GAP;
  const height = rows * SHEET_CELL_H + (rows - 1) * SHEET_GAP;
  return `<svg width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" xmlns="http://www.w3.org/2000/svg" role="img" aria-labelledby="pax-moods-t pax-moods-d">
  <title id="pax-moods-t">Pax 的十二种彩色表情（各自带动态）— 笑 / 欢乐 / 哭 / 沮丧 / 惊恐 / 愤怒 / 撒娇 / 卖萌 / 讨好 / 敲打 / 捂脸 / 吐舌头</title>
  <desc id="pax-moods-d">小打包机器人 Pax 的全身联系表：十二种彩色屏幕表情（含纸箱身体、短腿与小包裹）及中英文标签，每个形态各自带有专属动效（笑=浮动、欢乐=弹跳加星光、哭=屏幕泪滴、沮丧=叹气下沉、惊恐=发抖加汗滴、愤怒=怒纹加蒸汽、撒娇=爱心上浮、卖萌=爱心眼、讨好=星星眼、敲打=包裹敲击火星、捂脸=屏幕 >_&lt; 、吐舌头=舌头摆动）。</desc>
  <style>${css}</style>
  <rect width="${width}" height="${height}" fill="#f8fafc"/>
${cells}
</svg>
`;
}

const files = new Map();

files.set(
  path.join(ASSETS, 'pax.svg'),
  svg({
    title: 'Pax · 打包机器人 — 十二种彩色表情循环',
    desc: 'PolyPack 项目宠物 Pax——一只巴掌大的打包机器人：纸箱身体、屏幕脸、手持小包裹，彩色表情依次循环 笑 → 欢乐 → 哭 → 沮丧 → 惊恐 → 愤怒 → 撒娇 → 卖萌 → 讨好 → 敲打 → 捂脸 → 吐舌头。纯 CSS 动画，无脚本。',
    faces: Object.values(MOODS).map((m) => m.face).join(''),
    animated: true,
  }),
);

for (const [id, mood] of Object.entries(MOODS)) {
  files.set(
    path.join(ASSETS, mood.src),
    svg({
      title: `Pax · 打包机器人 — ${mood.zh}表情（动态）`,
      desc: `PolyPack 项目宠物 Pax 的${mood.zh}表情（${mood.en}），带该表情专属的 CSS 动效；减弱动态效果时定格为静态形态。`,
      faces: mood.face,
      animated: true,
      css: moodStyle(MOODS[id]),
    }),
  );
}

/* 项目图标 (favicon / 图示角标 / 应用图标母版) 与表情联系表 */
files.set(path.join(ASSETS, 'pax-mark.svg'), MARK_SVG);
files.set(path.join(ASSETS, 'pax-moods.svg'), moodSheet());

/* ── 本地游乐场: 按钮切换 img 源 (img 内 SVG 是独立文档, 样式互不泄漏) ── */
const buttons = [
  { id: 'auto', label: '自动循环', src: 'assets/pax.svg' },
  ...Object.entries(MOODS).map(([id, m]) => ({ id, label: m.zh, src: `assets/${m.src}` })),
];

files.set(
  path.join(ROOT, 'docs', 'pax.html'),
  `<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<link rel="icon" type="image/svg+xml" href="assets/pax-mark.svg">
<title>Pax · 打包机器人 — PolyPack 游乐场</title>
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
  <h1>Pax · 打包机器人</h1>
  <p class="sub">PolyPack 项目宠物 · 十二形态彩色表情游乐场</p>
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
