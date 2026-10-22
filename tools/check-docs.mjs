#!/usr/bin/env node
/**
 * check-docs.mjs — 结构级双文档校验（T-M0-10 第 3 部分的前置交付）
 *
 * 对中英文档做结构对账（不做全文翻译比对）：
 *   1) H2 / H3 标题数一致
 *   2) Markdown 表格行数（'^| ' 行）一致
 *   3) 对 TASKS 档对：任务 ID 集合逐一相同
 *   4) L6 嵌入式 / 车机状态表与 TargetSpec 逐字一致（T-M6-12，见 gen-target-table.mjs）
 *   5) 49 目标现状表与 TargetSpec / 规划清单逐字一致，且 README 平台矩阵 49 行、
 *      L1–L7 分级计数与清单一致（T-M8-12，见 gen-status-table.mjs；矩阵增删时
 *      按 ROADMAP §11 第 4 条提示待更新项）
 *
 * 零依赖。用法: node tools/check-docs.mjs   (一致 exit 0, 漂移 exit 1)
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { checkGeneratedDocs } from './gen-target-table.mjs';
import { checkReadmeMatrix, checkStatusDocs } from './gen-status-table.mjs';
import { checkExperimentalConsistency } from './check-experimental.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const PAIRS = [
  ['README.md', 'README.en.md'],
  ['docs/ROADMAP.md', 'docs/ROADMAP.en.md'],
  ['docs/TASKS.md', 'docs/TASKS.en.md'],
  ['docs/FARM.md', 'docs/FARM.en.md'],
  ['docs/EXPERIMENTAL-TARGETS.md', 'docs/EXPERIMENTAL-TARGETS.en.md'],
  ['docs/FARM-DEPLOY.md', 'docs/FARM-DEPLOY.en.md'],
  ['docs/GA.md', 'docs/GA.en.md'],
  ['docs/INITIATIVES.md', 'docs/INITIATIVES.en.md'],
  ['docs/ENV-SLOTS.md', 'docs/ENV-SLOTS.en.md'],
  ['docs/ENV-PROVIDE.md', 'docs/ENV-PROVIDE.en.md'],
  ['docs/INSTALL.md', 'docs/INSTALL.en.md'],
  ['docs/HUMAN-TRIAL.md', 'docs/HUMAN-TRIAL.en.md'],
  ['docs/STORE-METADATA.md', 'docs/STORE-METADATA.en.md'],
  ['docs/OTA.md', 'docs/OTA.en.md'],
  ['docs/WORKSPACE.md', 'docs/WORKSPACE.en.md'],
  ['docs/CRATES-IO.md', 'docs/CRATES-IO.en.md'],
  ['docs/DEVICE-PREVIEW.md', 'docs/DEVICE-PREVIEW.en.md'],
  ['docs/GUI.md', 'docs/GUI.en.md'],
  ['docs/RELEASE-CHANGED.md', 'docs/RELEASE-CHANGED.en.md'],
  ['docs/SBOM.md', 'docs/SBOM.en.md'],
];

const count = (text, re) => (text.match(re) || []).length;
const taskIds = (text) => [...new Set(text.match(/^### (T-M\d-\d{2})/gm)?.map((m) => m.slice(4)) ?? [])].sort();

let failed = 0;
for (const [zhPath, enPath] of PAIRS) {
  const zh = fs.readFileSync(path.join(ROOT, zhPath), 'utf8');
  const en = fs.readFileSync(path.join(ROOT, enPath), 'utf8');
  const checks = [
    ['H2 标题数', count(zh, /^## /gm), count(en, /^## /gm)],
    ['H3 标题数', count(zh, /^### /gm), count(en, /^### /gm)],
    ['表格行数', count(zh, /^\| /gm), count(en, /^\| /gm)],
  ];
  const zhIds = taskIds(zh);
  const enIds = taskIds(en);
  if (zhIds.length || enIds.length) {
    checks.push(['任务 ID 集合', zhIds.length, zhIds.join(',') === enIds.join(',') ? zhIds.length : -1]);
  }
  for (const [label, a, b] of checks) {
    if (a === b) {
      console.log(`ok   ${zhPath} ↔ ${enPath} · ${label}: ${a}`);
    } else {
      failed += 1;
      console.error(`DRIFT ${zhPath} ↔ ${enPath} · ${label}: zh=${a} en=${b}`);
    }
  }
}
const generated = checkGeneratedDocs();
for (const line of generated.lines) (line.ok ? console.log : console.error)(line.text);
failed += generated.failed;

// T-M8-12：49 行现状表 + README 平台矩阵 ↔ 声明清单（含 §11 第 4 条提示）。
const status = checkStatusDocs();
for (const line of status.lines) (line.ok ? console.log : console.error)(line.text);
failed += status.failed;
const matrix = checkReadmeMatrix();
for (const line of matrix.lines) (line.ok ? console.log : console.error)(line.text);
failed += matrix.failed;

// T-M6-13：experimental 三处一致（TargetSpec / 生成文档表 / CI 强度）。
try {
  const experimental = checkExperimentalConsistency();
  for (const line of experimental.lines) (line.ok ? console.log : console.error)(line.text);
  failed += experimental.failed;
} catch (err) {
  failed += 1;
  console.error(`DRIFT experimental 三处一致校验: ${err.message}`);
}

// GA 缺口：模板 WIT 与宿主 WIT 逐字节一致（两处接口定义不得漂移）。
const HOST_WIT = 'crates/polypack-plugin/wit/polypack-target.wit';
const TEMPLATE_WIT = 'templates/plugin-target/plugin/wit/polypack-target.wit';
try {
  const host = fs.readFileSync(path.join(ROOT, HOST_WIT));
  const template = fs.readFileSync(path.join(ROOT, TEMPLATE_WIT));
  if (host.equals(template)) {
    console.log('ok   WIT 同步 · crates/polypack-plugin ↔ templates/plugin-target');
  } else {
    failed += 1;
    console.error(`DRIFT ${HOST_WIT} ↔ ${TEMPLATE_WIT} · WIT 字节不一致`);
  }
} catch (err) {
  failed += 1;
  console.error(`DRIFT ${HOST_WIT} ↔ ${TEMPLATE_WIT} · ${err.code === 'ENOENT' ? '文件缺失' : err.message}`);
}

console.log(
  failed
    ? `\n${failed} 项漂移`
    : `\n${PAIRS.length} 对文档结构一致（含 TargetSpec 状态表、49 目标现状表、平台矩阵与 experimental 三处一致）`,
);
process.exit(failed ? 1 : 0);
