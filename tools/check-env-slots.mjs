#!/usr/bin/env node
/**
 * check-env-slots.mjs — ENV-SLOTS ledger gate (M9-G3 / T-M9-52 in-repo part).
 *
 * Validates the slot summary table in docs/ENV-SLOTS.md and
 * docs/ENV-SLOTS.en.md:
 *   1) exactly 19 slots (ENV-01..ENV-19) with unique IDs
 *   2) every row has owner / evidence / status; status and mode are valid
 *   3) owner tokens use only the five assigned roles
 *   4) every linked T-Mx-yy task ID (where present) exists in docs/TASKS.md
 *   5) Chinese and English rows agree on ID / status / mode / owner / evidence
 *
 * Read-only. Usage: node tools/check-env-slots.mjs (exit 0 clean, 1 drift)
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const TASKS_FILE = 'docs/TASKS.md';
const LEDGERS = ['docs/ENV-SLOTS.md', 'docs/ENV-SLOTS.en.md'];
const EXPECTED_IDS = Array.from({ length: 19 }, (_, i) => `ENV-${String(i + 1).padStart(2, '0')}`);
const VALID_STATUS = new Set(['OPEN', 'PARTIAL', 'CODE-GAP', 'HUMAN']);
const VALID_MODE = new Set(['CI', 'HOST', 'HUMAN']);
const VALID_ROLES = new Set(['release-sec', 'core', 'docs', 'infra', 'maintainer']);
const TASK_ID_RE = /T-M\d-\d{2}/g;

let failed = 0;
const ok = (msg) => console.log(`ok   ${msg}`);
const bad = (msg) => {
  failed += 1;
  console.error(`ERROR ${msg}`);
};

/** Split a Markdown table row into trimmed cells. */
const cells = (line) => line.split('|').slice(1, -1).map((cell) => cell.trim());

/** Read the `## 2.` section and return its header plus ENV-* data rows. */
function parseLedger(relPath) {
  const text = fs.readFileSync(path.join(ROOT, relPath), 'utf8');
  const lines = text.split(/\r?\n/);
  const start = lines.findIndex((line) => /^## 2\./.test(line));
  if (start < 0) throw new Error(`${relPath}: missing "## 2." summary section`);
  const end = lines.findIndex((line, i) => i > start && /^## /.test(line));
  const block = lines.slice(start + 1, end < 0 ? lines.length : end);
  const table = block.filter((line) => /^\| /.test(line));
  if (table.length < 3) throw new Error(`${relPath}: missing slot summary table`);
  const header = cells(table[0]);
  const rows = table
    .slice(2)
    .filter((line) => /^\|\s*ENV-\d{2}\s*\|/.test(line))
    .map(cells);
  return { header, rows };
}

function columnIndex(header, names, label) {
  const index = header.findIndex((cell) => names.some((name) => cell.includes(name)));
  if (index < 0) throw new Error(`missing "${label}" column in summary table`);
  return index;
}

function readTasks() {
  const text = fs.readFileSync(path.join(ROOT, TASKS_FILE), 'utf8');
  return new Set(text.match(TASK_ID_RE) ?? []);
}

function checkLedger(relPath, taskIds) {
  const { header, rows } = parseLedger(relPath);
  const col = {
    id: columnIndex(header, ['ID'], 'ID'),
    task: columnIndex(header, ['关联任务', 'Linked task'], 'linked task'),
    status: columnIndex(header, ['状态', 'Status'], 'status'),
    mode: columnIndex(header, ['模式', 'Mode'], 'mode'),
    owner: columnIndex(header, ['负责人', 'Owner'], 'owner'),
    evidence: columnIndex(header, ['证据', 'Evidence'], 'evidence'),
  };
  const errs = [];
  const map = new Map();

  if (rows.length !== EXPECTED_IDS.length) {
    errs.push(`${relPath}: expected ${EXPECTED_IDS.length} slot rows, found ${rows.length}`);
  }
  for (const row of rows) {
    const id = row[col.id];
    if (!/^ENV-\d{2}$/.test(id)) {
      errs.push(`${relPath}: invalid slot id "${id}"`);
      continue;
    }
    if (map.has(id)) errs.push(`${relPath}: duplicate slot id ${id}`);
    const linked = row[col.task] ?? '';
    const linkedIds = [...new Set(linked.match(TASK_ID_RE) ?? [])];
    for (const taskId of linkedIds) {
      if (!taskIds.has(taskId)) {
        errs.push(`${relPath}: ${id} links ${taskId}, which does not exist in ${TASKS_FILE}`);
      }
    }
    const status = row[col.status] ?? '';
    if (!VALID_STATUS.has(status)) errs.push(`${relPath}: ${id} has invalid status "${status}"`);
    const mode = row[col.mode] ?? '';
    if (!VALID_MODE.has(mode)) errs.push(`${relPath}: ${id} has invalid mode "${mode}"`);
    const owner = row[col.owner] ?? '';
    if (!owner) {
      errs.push(`${relPath}: ${id} is missing an owner`);
    } else {
      const roles = owner.split(/\s*\+\s*/);
      const invalid = roles.filter((role) => !VALID_ROLES.has(role));
      if (invalid.length > 0) {
        errs.push(
          `${relPath}: ${id} owner "${owner}" must use ${[...VALID_ROLES].join(' / ')} joined by " + "`,
        );
      }
    }
    const evidence = row[col.evidence] ?? '';
    if (!evidence) errs.push(`${relPath}: ${id} is missing an evidence reference`);
    map.set(id, { status, mode, owner, evidence, linkedIds: linkedIds.join(',') });
  }
  for (const id of EXPECTED_IDS) {
    if (!map.has(id)) errs.push(`${relPath}: missing slot ${id}`);
  }
  for (const id of map.keys()) {
    if (!EXPECTED_IDS.includes(id)) errs.push(`${relPath}: unexpected slot ${id}`);
  }

  for (const message of errs) bad(message);
  if (errs.length === 0) {
    ok(`${relPath} · ${EXPECTED_IDS.length} slots, unique IDs, owner/evidence/status complete`);
    ok(`${relPath} · linked task IDs exist in ${TASKS_FILE}`);
  }
  return map;
}

function crossCheck(zh, en) {
  const errs = [];
  for (const id of EXPECTED_IDS) {
    const a = zh.get(id);
    const b = en.get(id);
    if (!a || !b) continue;
    for (const field of ['status', 'mode', 'owner', 'evidence', 'linkedIds']) {
      if (a[field] !== b[field]) {
        errs.push(`zh/en mismatch ${id} ${field}: zh="${a[field]}" en="${b[field]}"`);
      }
    }
  }
  for (const message of errs) bad(message);
  if (errs.length === 0) ok('docs/ENV-SLOTS.md ↔ docs/ENV-SLOTS.en.md · slot fields aligned');
}

try {
  const taskIds = readTasks();
  const zh = checkLedger(LEDGERS[0], taskIds);
  const en = checkLedger(LEDGERS[1], taskIds);
  crossCheck(zh, en);
} catch (err) {
  bad(err.message);
}

if (failed > 0) {
  console.error(`\n${failed} ENV-SLOTS ledger error(s)`);
  process.exit(1);
}
console.log('\nENV-SLOTS ledger clean (19 slots; no ownerless or unlinked SKIP)');
