#!/usr/bin/env node
/**
 * check-experimental.mjs — `experimental` 三处一致性校验（T-M6-13）
 *
 * 单一事实源仍是 `crates/poly-targets/src/builtin.rs` 的 `TargetSpec`。对每个已注册
 * `tier = 6` 目标核对三处：
 *
 *   (a) TargetSpec.experimental —— 声明源（本脚本直接解析 Rust 字面量，不编译）；
 *   (b) 生成的文档状态表 —— `tools/gen-target-table.mjs` 写入四份文档标记块的
 *       `experimental` 列（这里读**落盘文档**，不读生成器内存值：手改文档同样会炸）；
 *   (c) CI 强度 —— `.github/workflows/embedded-ci.yml` 的矩阵口径：
 *       - tier-6 无 UI 目标必须由 `poly targets --tier 6 --headless` 动态派生进矩阵
 *         （不得被 `matrix.id != ...` 显式排除）；
 *       - 带 UI 的 tier-6 目标不入嵌入式矩阵，由 rust-ci 的 workspace 测试覆盖；
 *       - 矩阵作业必须保留 `SKIP-with-print` 如实打印（环境槽跳过不得静默）。
 *
 * 任一不一致 → exit 1，并逐目标点名三处实际取值。目标集合由 builtin.rs 的 tier-6
 * 声明**动态**枚举（不写死 id），未来新增声明自动纳入校验；若新目标尚未进文档表，
 * 会以「docs 缺行」失败而不是被静默忽略。
 *
 * 零依赖。用法:
 *   node tools/check-experimental.mjs
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { BEGIN, DOCS, END, parseTargetSpecs } from './gen-target-table.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const WORKFLOW = '.github/workflows/embedded-ci.yml';
const RUST_CI = '.github/workflows/rust-ci.yml';

const escapeRe = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** 读取文档中的状态表标记块内容（不含标记行）；块数必须恰为 1。 */
function readBlock(text, file) {
  const re = new RegExp(`${escapeRe(BEGIN)}([\\s\\S]*?)${escapeRe(END)}`, 'g');
  const hits = [...text.matchAll(re)];
  if (hits.length !== 1) {
    throw new Error(`${file}: 期望恰好 1 个状态表标记块，实际 ${hits.length} 个`);
  }
  return hits[0][1];
}

/** 按未转义的 `|` 切单元格（notes 里的 `\|` 属于单元格内容）。 */
const cells = (line) => line.split(/(?<!\\)\|/).slice(1, -1).map((cell) => cell.trim());

/** 解析一张生成表的 `{ id → experimental }`；列序与 gen-target-table 的 renderTable 同源。 */
export function parseExperimentalColumn(block, file) {
  const values = new Map();
  for (const line of block.split('\n')) {
    if (!line.startsWith('| ')) continue;
    const row = cells(line);
    if (row.length < 9) continue; // 表头 / 分隔行（或结构损坏，交由缺失行报告）
    const id = /^`([^`]+)`$/.exec(row[0])?.[1];
    // slice(1, -1) 去掉了首尾空段：0=目标 … 7=experimental，8=notes。
    const value = /^`(true|false)`$/.exec(row[7])?.[1];
    if (!id || !value) continue;
    if (values.has(id)) throw new Error(`${file}: 目标 ${id} 在状态表中出现两次`);
    values.set(id, value === 'true');
  }
  return values;
}

/** 读取四份文档的 experimental 列；任一份缺行都记为 undefined（由调用方报冲突）。 */
function docsValues(ids) {
  const byDoc = [];
  for (const { file } of DOCS) {
    const full = path.join(ROOT, file);
    let column;
    try {
      const text = fs.readFileSync(full, 'utf8');
      column = parseExperimentalColumn(readBlock(text, file), file);
    } catch (err) {
      throw new Error(`${file}: ${err.message}`);
    }
    byDoc.push({ file, column });
  }
  return ids.map((id) => ({
    id,
    values: byDoc.map(({ file, column }) => ({ file, value: column.get(id) })),
  }));
}

/** CI 强度模型：从 workflow 文本读矩阵口径，而不是假设它没被改过。 */
export function ciModel() {
  const workflow = fs.readFileSync(path.join(ROOT, WORKFLOW), 'utf8');
  const rustCi = fs.readFileSync(path.join(ROOT, RUST_CI), 'utf8');
  return {
    // 矩阵条目必须由 TargetSpec 派生（新增目标自动进矩阵的唯一机制）。
    derivesMatrix: /poly targets\s+--tier\s+6\s+--headless\s+--json/.test(workflow),
    // 显式排除（`matrix.id != 'qnx'`）会把目标从矩阵里摘掉——不允许对已注册目标做。
    exclusions: [...workflow.matchAll(/matrix\.id\s*!=\s*'([^']+)'/g)].map((m) => m[1]),
    // 环境槽跳过必须如实打印（job summary 收集 warning 事件 + SKIP-with-print 语义）。
    printsSkip:
      workflow.includes('SKIP-with-print') &&
      /select\(\.event == "warning"\)/.test(workflow),
    rustWorkspace: /cargo test --workspace/.test(rustCi),
  };
}

/** 单个 tier-6 目标的 CI 强度：matrix / external / absent（absent 即不一致）。 */
function strength(spec, ci) {
  if (spec.ui.length === 0) {
    if (!ci.derivesMatrix) {
      return { kind: 'absent', detail: 'embedded-ci 未从 `poly targets --tier 6 --headless` 派生矩阵' };
    }
    if (ci.exclusions.includes(spec.id)) {
      return { kind: 'absent', detail: `embedded-ci 用 matrix.id != '${spec.id}' 显式排除` };
    }
    if (!ci.printsSkip) {
      return { kind: 'absent', detail: 'embedded-ci 缺少 SKIP-with-print 如实打印（静默跳过）' };
    }
    return { kind: 'matrix', detail: 'embedded-ci tier-6 headless 动态矩阵（SKIP-with-print 如实打印）' };
  }
  if (ci.rustWorkspace) {
    return { kind: 'external', detail: 'rust-ci cargo test --workspace（带 UI，不入嵌入式矩阵）' };
  }
  return { kind: 'absent', detail: '带 UI 目标不在嵌入式矩阵，且 rust-ci 未跑 workspace 测试' };
}

const describe = (kind) =>
  kind === 'matrix' ? 'matrix' : kind === 'external' ? 'external（rust-ci）' : 'absent';

/**
 * 校验全部已注册 tier-6 目标。返回 `{ failed, lines }`（与 check-docs 的既有形态一致）。
 */
export function checkExperimentalConsistency() {
  const specs = parseTargetSpecs().filter((spec) => spec.tier === 6);
  const ids = specs.map((spec) => spec.id).sort();
  const docs = docsValues(ids);
  const ci = ciModel();
  const lines = [];
  let failed = 0;

  for (const spec of specs.sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))) {
    const doc = docs.find((entry) => entry.id === spec.id);
    const docText = doc.values
      .map(({ file, value }) => `${file}:${value === undefined ? '<缺行>' : value}`)
      .join(', ');
    const ciStrength = strength(spec, ci);
    const specValue = String(spec.experimental);
    const docsAgree = doc.values.every(({ value }) => value === spec.experimental);
    const ciOk = ciStrength.kind !== 'absent';
    if (docsAgree && ciOk) {
      lines.push({
        ok: true,
        text: `ok   ${spec.id} · experimental=${specValue}（docs 四份一致；CI=${describe(ciStrength.kind)}）`,
      });
      continue;
    }
    failed += 1;
    lines.push({
      ok: false,
      text:
        `FAIL ${spec.id} · experimental 三处冲突：` +
        `TargetSpec=${specValue}；docs=${docText}；CI=${describe(ciStrength.kind)}（${ciStrength.detail}）`,
    });
  }
  return { failed, lines };
}

function main() {
  try {
    const { failed, lines } = checkExperimentalConsistency();
    for (const line of lines) (line.ok ? console.log : console.error)(line.text);
    console.log(
      failed
        ? `\n${failed} 个 tier-6 目标 experimental 三处不一致`
        : `\n${lines.length} 个 tier-6 目标 experimental 三处一致（TargetSpec ↔ 文档表 ↔ CI 强度）`,
    );
    process.exit(failed ? 1 : 0);
  } catch (err) {
    console.error(`check-experimental: ${err.message}`);
    process.exit(1);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
