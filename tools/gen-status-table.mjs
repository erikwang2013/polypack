#!/usr/bin/env node
/**
 * gen-status-table.mjs — 49 目标现状表生成器（T-M8-12）
 *
 * 单一事实源：已声明平台来自 builtin.rs 的 `TargetSpec`（复用 gen-target-table.mjs
 * 的解析器，不编译 Rust）；未声明平台来自本文件 PLATFORMS 声明清单并显式标注
 * 「规划中 / 已停用 / 已关闭」等状态。两者渲染成 49 行现状表写入 README / ROADMAP
 * 双语标记块；`--check` 同时校验标记块漂移、49 行完整性、L1–L7 分级计数与 README
 * 平台矩阵一致性（增删时按 §11 第 4 条提示待更新项）。
 *
 * 零依赖。用法: node tools/gen-status-table.mjs [--check|--print]
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { cell, code, codeList, firstDiff, parseTargetSpecs, readBlock } from './gen-target-table.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

export const BEGIN = '<!-- BEGIN GENERATED: status-49 -->';
export const END = '<!-- END GENERATED: status-49 -->';

const TOTAL = 49;
/** README 平台矩阵的现有分级计数（T-M8-12 验收口径）。 */
const TIER_COUNTS = { 1: 7, 2: 9, 3: 9, 4: 4, 5: 7, 6: 10, 7: 3 };

const SECTION_HINT =
  '依 §11 第 4 条：平台矩阵增删须同时更新 README 分级表（×2 语言）+ ROADMAP 逐平台明细（×2 语言）+ 里程碑验收 + TargetSpec，并同步 tools/gen-status-table.mjs 的 PLATFORMS 清单。';

const bi = (zh, en) => ({ zh, en });
const declaredRow = (tier, zh, en, ...targets) => ({ tier, zh, en, targets });
const plannedRow = (tier, zh, en, fields, status = 'planned') => ({ tier, zh, en, planned: fields, status });

/** 未声明 TargetSpec 行的状态标签（保持 49 行与分级计数不变，仅改语义标注）。 */
const PLANNED_LABELS = {
  planned: bi('规划中', 'planned'),
  retired: bi('已停用 / retired', 'retired'),
  closed: bi('已关闭 · 下次复审 2027-01-20', 'closed · next review 2027-01-20'),
  conditional: bi('条件支持 · 部署模式', 'conditional · deploy mode'),
};

/**
 * 49 个平台，顺序与 README 平台矩阵一致。
 * `targets` 行由 TargetSpec 派生；`planned` 行是尚未声明 TargetSpec 的平台，
 * 字段由本清单提供，`status` 标注规划 / 停用 / 关闭 / 条件支持。
 */
const PLATFORMS = [
  // ── L1 一等公民（7）──
  declaredRow(1, 'Android', 'Android', 'android'),
  declaredRow(1, 'iOS', 'iOS', 'ios'),
  declaredRow(1, 'HarmonyOS', 'HarmonyOS', 'harmonyos'),
  declaredRow(1, 'Windows', 'Windows', 'windows'),
  declaredRow(1, 'macOS', 'macOS', 'macos'),
  declaredRow(1, 'Linux', 'Linux', 'linux'),
  declaredRow(1, 'Web', 'Web', 'web'),

  // ── L2 小程序与即时应用（9）：后 3 个为规划平台 ──
  declaredRow(2, '微信小程序', 'WeChat Mini Program', 'wechat'),
  declaredRow(2, '支付宝小程序', 'Alipay Mini Program', 'alipay'),
  declaredRow(2, '抖音小程序', 'Douyin Mini Program', 'douyin'),
  declaredRow(2, '快应用', 'Quick App (快应用)', 'quickapp'),
  declaredRow(2, 'TikTok Minis', 'TikTok Minis', 'tiktok-minis'),
  plannedRow(2, 'Google Play Instant', 'Google Play Instant', {
    runtime: bi('native', 'native'),
    bridge: bi('uniffi', 'uniffi'),
    artifact: bi('instant-aab · android-aapt2', 'instant-aab · android-aapt2'),
    signing: bi('android-keystore', 'android-keystore'),
    verify: bi('bundletool-validate', 'bundletool-validate'),
    publish: bi('google-play', 'google-play'),
    notes: bi(
      '已停用（retired）：Google 自 2025-12 起停止受理 Instant Apps 发布、Play services Instant API 停用；保留行以说明历史，不声明 TargetSpec。',
      'Retired: Google stopped accepting Instant Apps submissions and shut down Play services Instant APIs in Dec 2025; the row is kept for history, with no TargetSpec.',
    ),
  }, 'retired'),
  declaredRow(2, 'Facebook Instant Games', 'Facebook Instant Games', 'facebook-instant-games'),
  plannedRow(2, 'Snapchat Mini', 'Snapchat Mini', {
    runtime: bi('HTML5', 'HTML5'),
    bridge: bi('HTML5', 'HTML5'),
    artifact: bi('zip', 'zip'),
    signing: bi('无', 'none'),
    verify: bi('Snap 沙箱自检', 'Snap sandbox self-check'),
    publish: bi('Snap 平台', 'Snap platform'),
    notes: bi(
      '已关闭 · 季度复审（下次 2027-01-20）：Snap Minis 官方文档入口（docs.snap.com/minis/getting-started/home/）已 404，developers.snap.com 站点地图无 Minis 文档；npm @snapchat/minis-sdk（3.0.5，2022）已标注「deprecating minis」——官方 zip / manifest / 打包契约不可核验，不声明 TargetSpec。',
      'Closed · quarterly review (next 2027-01-20): the official Snap Minis docs entry (docs.snap.com/minis/getting-started/home/) returns 404 and the developers.snap.com sitemap has no Minis docs; npm @snapchat/minis-sdk (3.0.5, 2022) is marked "deprecating minis" — no verifiable official zip / manifest / packaging contract, so no TargetSpec.',
    ),
  }, 'closed'),
  plannedRow(2, 'LINE Mini App', 'LINE Mini App', {
    runtime: bi('LIFF（web）', 'LIFF (web)'),
    bridge: bi('HTTPS', 'HTTPS'),
    artifact: bi('静态包 + LIFF 配置', 'static bundle + LIFF config'),
    signing: bi('HTTPS / 平台 TLS', 'HTTPS / platform TLS'),
    verify: bi('LIFF 配置检查', 'LIFF config check'),
    publish: bi('LINE 平台', 'LINE platform'),
    notes: bi(
      '条件支持（部署模式，不声明 TargetSpec）：LIFF 应用由 LINE Developers Console 托管 URL 注册（LINE Login 渠道 → LIFF；Endpoint URL 须 HTTPS 且不得含 fragment）；`poly deploy line-liff` 只校验配置并输出 Console 人工步骤，不生成平台包、不触网，官方无本地上传包格式或 CI 上传 API。',
      'Conditional (deploy mode, no TargetSpec): LIFF apps are registered as hosted URLs in the LINE Developers Console (LINE Login channel → LIFF; the endpoint URL must be HTTPS without fragments); `poly deploy line-liff` only validates config and prints manual Console steps — no platform package, no network, and no official upload format or CI upload API.',
    ),
  }, 'conditional'),

  // ── L3 电视与大屏（9）──
  declaredRow(3, 'Android TV', 'Android TV', 'android-tv'),
  declaredRow(3, 'Apple tvOS', 'Apple tvOS', 'tvos'),
  declaredRow(3, 'HarmonyOS TV', 'HarmonyOS TV', 'harmonyos-tv'),
  declaredRow(3, 'Samsung Tizen（TV）', 'Samsung Tizen (TV)', 'tizen-tv'),
  declaredRow(3, 'LG webOS', 'LG webOS', 'webos-tv'),
  declaredRow(3, 'Roku OS', 'Roku OS', 'roku'),
  declaredRow(3, 'Amazon Fire TV', 'Amazon Fire TV', 'fire-tv'),
  declaredRow(3, 'Vizio SmartCast', 'Vizio SmartCast', 'smartcast-tv'),
  declaredRow(3, 'Hisense VIDAA', 'Hisense VIDAA', 'vidaa-tv'),

  // ── L4 移动系统变体（4）：GrapheneOS / KaiOS 已声明，其余二为规划平台 ──
  declaredRow(4, 'GrapheneOS', 'GrapheneOS', 'grapheneos'),
  declaredRow(4, 'KaiOS', 'KaiOS', 'kaios'),
  plannedRow(4, 'Sailfish OS', 'Sailfish OS', {
    runtime: bi('native', 'native'),
    bridge: bi('cffi', 'cffi'),
    artifact: bi('rpm', 'rpm'),
    signing: bi('RPM GPG', 'RPM GPG'),
    verify: bi('rpmlint', 'rpmlint'),
    publish: bi('Jolla Store', 'Jolla Store'),
    notes: bi(
      '规划中：现有 linux-rpm 链只出桌面 Tauri RPM（裸产物 + .desktop），缺 Qt / Silica QML 宿主、Sailfish SDK 交叉工具链（armv7hl / aarch64）与 harbour 打包 / 签名；尚未声明 TargetSpec。',
      'Planned: the existing linux-rpm chain only emits a desktop Tauri RPM (bare product + .desktop); missing the Qt / Silica QML host, the Sailfish SDK cross toolchain (armv7hl / aarch64), and harbour packaging / signing; no TargetSpec yet.',
    ),
  }),
  plannedRow(4, 'Ubuntu Touch', 'Ubuntu Touch', {
    runtime: bi('native', 'native'),
    bridge: bi('cffi', 'cffi'),
    artifact: bi('.click（Clickable）', '.click (Clickable)'),
    signing: bi('Clickable 打包', 'Clickable packaging'),
    verify: bi('Clickable 构建检查', 'Clickable build check'),
    publish: bi('OpenStore', 'OpenStore'),
    notes: bi(
      '规划中：现有 Linux deb 链不能产出 .click（格式 / AppArmor / click 清单均不同），缺 Clickable / Ubuntu Touch SDK 与 WebView 壳；尚未声明 TargetSpec。',
      'Planned: the existing Linux deb chain cannot produce .click (different format / AppArmor / click manifest); missing Clickable / Ubuntu Touch SDK and the WebView shell; no TargetSpec yet.',
    ),
  }),

  // ── L5 桌面发行版与 BSD（7）：Chrome OS 合并两个已声明目标 ──
  declaredRow(5, 'Ubuntu', 'Ubuntu', 'ubuntu'),
  declaredRow(5, 'Fedora', 'Fedora', 'fedora'),
  declaredRow(5, 'Linux Mint', 'Linux Mint', 'mint'),
  declaredRow(5, 'Chrome OS', 'Chrome OS', 'chromeos', 'chromeos-pwa'),
  declaredRow(5, 'FreeBSD', 'FreeBSD', 'freebsd'),
  declaredRow(5, 'OpenBSD', 'OpenBSD', 'openbsd'),
  declaredRow(5, 'BSD 家族（NetBSD / DragonFly 等）', 'BSD family (NetBSD / DragonFly, …)', 'netbsd'),

  // ── L6 穿戴 / 车机 / 嵌入式（10）：RTOS 行合并两个已声明目标 ──
  declaredRow(6, 'Apple watchOS', 'Apple watchOS', 'watchos'),
  declaredRow(6, 'Wear OS', 'Wear OS', 'wearos'),
  declaredRow(6, 'Android Automotive', 'Android Automotive', 'android-automotive'),
  declaredRow(6, 'QNX', 'QNX', 'qnx'),
  declaredRow(6, 'AUTOSAR（Adaptive）', 'AUTOSAR (Adaptive)', 'autosar'),
  declaredRow(6, 'VxWorks', 'VxWorks', 'vxworks'),
  declaredRow(6, 'RTOS（Zephyr / FreeRTOS）', 'RTOS (Zephyr / FreeRTOS)', 'zephyr', 'freertos'),
  declaredRow(6, 'Fitbit OS', 'Fitbit OS', 'fitbit'),
  declaredRow(6, 'Garmin（Connect IQ）', 'Garmin (Connect IQ)', 'garmin'),
  declaredRow(6, 'Zepp OS', 'Zepp OS', 'zepp'),

  // ── L7 主机平台（3）：均为规划平台 ──
  plannedRow(7, 'PlayStation', 'PlayStation', {
    runtime: bi('官方 SDK 插件', 'official SDK plugin'),
    bridge: bi('—', '—'),
    artifact: bi('平台包', 'platform bundle'),
    signing: bi('厂商工具', 'vendor tools'),
    verify: bi('厂商工具链', 'vendor toolchain'),
    publish: bi('需 Sony 授权', 'Sony license required'),
    notes: bi(
      '规划中：官方 SDK 插件（需 Sony 授权），SDK 不入库、本地路径探测；尚未声明 TargetSpec。',
      'Planned: official SDK plugin (Sony license required); SDK never in-repo, local path probing; no TargetSpec yet.',
    ),
  }),
  plannedRow(7, 'Xbox', 'Xbox', {
    runtime: bi('GDK 插件', 'GDK plugin'),
    bridge: bi('—', '—'),
    artifact: bi('平台包', 'platform bundle'),
    signing: bi('厂商工具', 'vendor tools'),
    verify: bi('厂商工具链', 'vendor toolchain'),
    publish: bi('需微软授权', 'Microsoft license required'),
    notes: bi(
      '规划中：GDK 插件（需微软授权），SDK 不入库、本地路径探测；尚未声明 TargetSpec。',
      'Planned: GDK plugin (Microsoft license required); SDK never in-repo, local path probing; no TargetSpec yet.',
    ),
  }),
  plannedRow(7, 'Nintendo Switch', 'Nintendo Switch', {
    runtime: bi('NintendoSDK 插件', 'NintendoSDK plugin'),
    bridge: bi('—', '—'),
    artifact: bi('平台包', 'platform bundle'),
    signing: bi('厂商工具', 'vendor tools'),
    verify: bi('厂商工具链', 'vendor toolchain'),
    publish: bi('需任天堂授权', 'Nintendo license required'),
    notes: bi(
      '规划中：NintendoSDK 插件（需任天堂授权），SDK 不入库、本地路径探测；尚未声明 TargetSpec。',
      'Planned: NintendoSDK plugin (Nintendo license required); SDK never in-repo, local path probing; no TargetSpec yet.',
    ),
  }),
];

export const DOCS = [
  { file: 'README.md', lang: 'zh' },
  { file: 'README.en.md', lang: 'en' },
  { file: 'docs/ROADMAP.md', lang: 'zh' },
  { file: 'docs/ROADMAP.en.md', lang: 'en' },
];

const HEADER = {
  zh: ['等级', '平台', '运行时', '桥', '打包器与产物', '签名', '产物校验', '发布渠道', 'experimental', 'notes'],
  en: ['Tier', 'Platform', 'Runtime', 'Bridge', 'Packager & artifact', 'Signing', 'Artifact verification', 'Publishing channel', 'experimental', 'notes'],
};
const uniq = (items) => [...new Set(items)];
/** 49 行完整性 + 分级计数 + TargetSpec 覆盖（每个已声明目标恰好出现一次）。 */
function validateManifest(specs) {
  if (PLATFORMS.length !== TOTAL) {
    throw new Error(`PLATFORMS 共 ${PLATFORMS.length} 行，期望 ${TOTAL} 行。${SECTION_HINT}`);
  }
  const counts = {};
  for (const row of PLATFORMS) counts[row.tier] = (counts[row.tier] ?? 0) + 1;
  for (let tier = 1; tier <= 7; tier += 1) {
    if (counts[tier] !== TIER_COUNTS[tier]) {
      throw new Error(
        `L${tier} 计数 ${counts[tier] ?? 0} ≠ README 现有总览 ${TIER_COUNTS[tier]}。${SECTION_HINT}`,
      );
    }
  }
  const declared = PLATFORMS.flatMap((row) => row.targets ?? []);
  const dupes = declared.filter((id, i) => declared.indexOf(id) !== i);
  if (dupes.length) throw new Error(`声明清单重复引用 TargetSpec: ${uniq(dupes).join(', ')}`);
  const specIds = specs.map((spec) => spec.id);
  const missing = specIds.filter((id) => !declared.includes(id));
  const unknown = declared.filter((id) => !specIds.includes(id));
  if (missing.length || unknown.length) {
    throw new Error(
      `TargetSpec 覆盖不完整（未入表: ${missing.join(', ') || '无'}；清单中不存在: ${unknown.join(', ') || '无'}）。${SECTION_HINT}`,
    );
  }
}

/** 已声明行：逐字段取自 TargetSpec；多目标行做确定性合并。 */
function declaredFields(row, specsById) {
  const specs = row.targets.map((id) => {
    const spec = specsById.get(id);
    if (!spec) throw new Error(`声明清单引用了不存在的 TargetSpec: ${id}`);
    return spec;
  });
  const tiers = uniq(specs.map((spec) => spec.tier));
  if (tiers.length !== 1 || tiers[0] !== row.tier) {
    throw new Error(`${row.zh}: 清单 tier=${row.tier} 与 TargetSpec tier=${tiers.join('/')} 不符`);
  }
  const notes = specs
    .filter((spec) => spec.notes)
    .map((spec) => (specs.length > 1 ? `${spec.id}：${spec.notes}` : spec.notes));
  return {
    tier: `L${tiers[0]}`,
    runtime: uniq(specs.map((spec) => spec.runtime)).join(' / '),
    bridge: uniq(specs.map((spec) => spec.bridge)).join(' / '),
    artifact: specs.map((spec) => `${code(spec.format)} · ${code(spec.builder)}`).join('; '),
    signing: uniq(specs.map((spec) => spec.signing).filter(Boolean)).join(' / '),
    verify: uniq(specs.flatMap((spec) => spec.verify)),
    publish: uniq(specs.flatMap((spec) => spec.publish)),
    experimental: String(specs.some((spec) => spec.experimental)),
    notes: notes.join('；'),
  };
}

/** 渲染一行；planned 行的字段来自声明清单，并在平台名后显式标注。 */
function renderRow(row, specsById, lang) {
  const none = lang === 'zh' ? '—' : 'none';
  const paren = (text) => (lang === 'zh' ? `（${text}）` : ` (${text})`);
  if (row.planned) {
    const fields = row.planned;
    const label = PLANNED_LABELS[row.status];
    if (!label) throw new Error(`${row.zh}: 未知状态 ${row.status}`);
    return [
      `L${row.tier}`,
      `${cell(row[lang])}${paren(label[lang])}`,
      cell(fields.runtime[lang]),
      cell(fields.bridge[lang]),
      cell(fields.artifact[lang]),
      cell(fields.signing[lang]),
      cell(fields.verify[lang]),
      cell(fields.publish[lang]),
      none,
      cell(fields.notes[lang]),
    ];
  }
  const fields = declaredFields(row, specsById);
  return [
    fields.tier,
    `${cell(row[lang])}${paren(row.targets.map(code).join(' + '))}`,
    code(fields.runtime),
    code(fields.bridge),
    fields.artifact,
    fields.signing ? code(fields.signing) : none,
    fields.verify.length ? codeList(fields.verify) : none,
    fields.publish.length ? codeList(fields.publish) : none,
    code(fields.experimental),
    fields.notes ? cell(fields.notes) : none,
  ];
}

/** 渲染 49 行现状表（lang = zh | en）。 */
export function renderStatusTable(lang = 'zh', specs = parseTargetSpecs()) {
  validateManifest(specs);
  const specsById = new Map(specs.map((spec) => [spec.id, spec]));
  const header = HEADER[lang];
  const lines = [
    `| ${header.join(' | ')} |`,
    `| ${header.map(() => '---').join(' | ')} |`,
  ];
  for (const row of PLATFORMS) lines.push(`| ${renderRow(row, specsById, lang).join(' | ')} |`);
  return lines.join('\n');
}

/** 校验四份文档的标记块是否与 TargetSpec / 清单一致。返回 { failed, lines }。 */
export function checkStatusDocs() {
  const tableByLang = { zh: renderStatusTable('zh'), en: renderStatusTable('en') };
  const lines = [];
  let failed = 0;
  for (const { file, lang } of DOCS) {
    const full = path.join(ROOT, file);
    try {
      const text = fs.readFileSync(full, 'utf8');
      const actual = readBlock(text, file, BEGIN, END).trim();
      const expected = tableByLang[lang].trim();
      if (actual === expected) {
        lines.push({ ok: true, text: `ok   ${file} · 49 行现状表 ↔ TargetSpec / 清单` });
      } else {
        failed += 1;
        lines.push({ ok: false, text: `DRIFT ${file} · 49 行现状表 ≠ TargetSpec / 清单（${firstDiff(actual, expected)}）` });
      }
    } catch (err) {
      failed += 1;
      lines.push({ ok: false, text: `DRIFT ${file} · ${err.message}` });
    }
  }
  return { failed, lines };
}

/** 生成 / 更新四份文档的标记块。返回变更文件列表。 */
export function writeStatusDocs() {
  const tableByLang = { zh: renderStatusTable('zh'), en: renderStatusTable('en') };
  const changed = [];
  for (const { file, lang } of DOCS) {
    const full = path.join(ROOT, file);
    const text = fs.readFileSync(full, 'utf8');
    if (!text.includes(BEGIN) || !text.includes(END)) {
      throw new Error(`${file}: 缺少现状表标记块 ${BEGIN} / ${END}`);
    }
    readBlock(text, file, BEGIN, END); // 同时校验标记块唯一
    const re = new RegExp(`${BEGIN.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}[\\s\\S]*?${END.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`);
    const next = text.replace(re, `${BEGIN}\n${tableByLang[lang]}\n${END}`);
    if (next !== text) {
      fs.writeFileSync(full, next);
      changed.push(file);
    }
  }
  return changed;
}

/** 读取 README 平台矩阵（标记块之外的手写分级表），返回 [{ tier, name }]。 */
function readMatrix(file, heading) {
  const text = fs.readFileSync(path.join(ROOT, file), 'utf8');
  const start = text.indexOf(heading);
  if (start < 0) throw new Error(`${file}: 未找到平台矩阵标题「${heading}」`);
  const rest = text.slice(start + heading.length);
  const end = rest.search(/\n#{2,3} /); // 矩阵段落到下一个 H2 / H3 为止
  const section = end < 0 ? rest : rest.slice(0, end);
  const rows = [];
  for (const line of section.split('\n')) {
    const m = /^\|\s*(L[1-7])\s*\|\s*([^|]+?)\s*\|/.exec(line);
    if (m) rows.push({ tier: m[1], name: m[2].trim() });
  }
  return rows;
}

/**
 * README 平台矩阵 ↔ PLATFORMS 清单对账（L1–L7 计数取自矩阵本身）。
 * 矩阵增删而未同步清单时失败，并给出 §11 第 4 条的待更新项。
 */
export function checkReadmeMatrix() {
  const targets = [
    ['README.md', 'zh', '## 支持平台矩阵'],
    ['README.en.md', 'en', '## Platform matrix'],
  ];
  const lines = [];
  let failed = 0;
  for (const [file, lang, heading] of targets) {
    try {
      const rows = readMatrix(file, heading);
      const counts = {};
      for (const row of rows) counts[row.tier] = (counts[row.tier] ?? 0) + 1;
      if (rows.length !== TOTAL) {
        throw new Error(`矩阵 ${rows.length} 行，期望 ${TOTAL} 行。${SECTION_HINT}`);
      }
      for (let tier = 1; tier <= 7; tier += 1) {
        const key = `L${tier}`;
        if (counts[key] !== TIER_COUNTS[tier]) {
          throw new Error(`${key} 计数 ${counts[key] ?? 0} ≠ 现有总览 ${TIER_COUNTS[tier]}。${SECTION_HINT}`);
        }
      }
      const bad = PLATFORMS.findIndex(
        (row, i) => rows[i].tier !== `L${row.tier}` || rows[i].name !== row[lang],
      );
      if (bad >= 0) {
        throw new Error(
          `第 ${bad + 1} 行矩阵=${rows[bad].tier} ${rows[bad].name}，清单=L${PLATFORMS[bad].tier} ${PLATFORMS[bad][lang]}。${SECTION_HINT}`,
        );
      }
      lines.push({ ok: true, text: `ok   ${file} · 平台矩阵 ${TOTAL} 行 ↔ 声明清单（L1–L7 计数一致）` });
    } catch (err) {
      failed += 1;
      lines.push({ ok: false, text: `DRIFT ${file} · 平台矩阵 ↔ 声明清单（${err.message}）` });
    }
  }
  return { failed, lines };
}

function main() {
  const check = process.argv.includes('--check');
  const print = process.argv.includes('--print');
  try {
    const specs = parseTargetSpecs();
    validateManifest(specs);
    if (print) {
      console.log(`## zh\n${renderStatusTable('zh', specs)}\n\n## en\n${renderStatusTable('en', specs)}`);
      return;
    }
    if (check) {
      const block = checkStatusDocs();
      const matrix = checkReadmeMatrix();
      for (const line of [...block.lines, ...matrix.lines]) (line.ok ? console.log : console.error)(line.text);
      const failed = block.failed + matrix.failed;
      console.log(
        failed
          ? `\n${failed} 项现状表漂移`
          : `\n四份文档 49 行现状表与 TargetSpec / 清单一致；README 平台矩阵与 L1–L7 分级计数一致`,
      );
      process.exit(failed ? 1 : 0);
    }
    const changed = writeStatusDocs();
    console.log(
      changed.length
        ? `已更新: ${changed.join(', ')}（${TOTAL} 行，${PLATFORMS.filter((row) => row.targets).length} 行已声明 / ${PLATFORMS.filter((row) => row.planned).length} 行未声明 TargetSpec）`
        : `无变化（${TOTAL} 行）`,
    );
  } catch (err) {
    console.error(`gen-status-table: ${err.message}`);
    process.exit(1);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
