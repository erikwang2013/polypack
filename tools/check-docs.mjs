#!/usr/bin/env node
/**
 * check-docs.mjs — 结构级双文档校验（T-M0-10 第 3 部分的前置交付）
 *
 * 对五对中英文档做结构对账（不做全文翻译比对）：
 *   1) H2 / H3 标题数一致
 *   2) Markdown 表格行数（'^| ' 行）一致
 *   3) 对 TASKS 档对：任务 ID 集合逐一相同
 *   4) L6 嵌入式 / 车机状态表与 TargetSpec 逐字一致（T-M6-12，见 gen-target-table.mjs）
 *
 * 零依赖。用法: node tools/check-docs.mjs   (一致 exit 0, 漂移 exit 1)
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { checkGeneratedDocs } from './gen-target-table.mjs';
import { checkExperimentalConsistency } from './check-experimental.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const PAIRS = [
  ['README.md', 'README.en.md'],
  ['docs/ROADMAP.md', 'docs/ROADMAP.en.md'],
  ['docs/TASKS.md', 'docs/TASKS.en.md'],
  ['docs/FARM.md', 'docs/FARM.en.md'],
  ['docs/EXPERIMENTAL-TARGETS.md', 'docs/EXPERIMENTAL-TARGETS.en.md'],
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

// T-M6-13：experimental 三处一致（TargetSpec / 生成文档表 / CI 强度）。
try {
  const experimental = checkExperimentalConsistency();
  for (const line of experimental.lines) (line.ok ? console.log : console.error)(line.text);
  failed += experimental.failed;
} catch (err) {
  failed += 1;
  console.error(`DRIFT experimental 三处一致校验: ${err.message}`);
}

console.log(
  failed
    ? `\n${failed} 项漂移`
    : '\n五对文档结构一致（含 TargetSpec 状态表与 experimental 三处一致）',
);
process.exit(failed ? 1 : 0);
