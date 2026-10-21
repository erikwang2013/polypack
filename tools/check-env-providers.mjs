#!/usr/bin/env node
/**
 * check-env-providers.mjs — validate tools/env-providers.json (schema v1).
 *
 * Checks:
 *  1) schema v1, repo owner/name, unique ENV slot IDs
 *  2) every non-CLOSED slot in docs/ENV-SLOTS.md has exactly one entry
 *  3) each entry carries bilingual title/request, a valid category, a docs
 *     anchor that resolves to the slot's H3 heading, and https portal URLs
 *  4) secret/variable names match [A-Z][A-Z0-9_]*; pilot names have a description
 *  5) entries only carry name/description/pilot — no secret values in the manifest
 *
 * Read-only, zero deps. Usage: node tools/check-env-providers.mjs (exit 0 clean, 1 drift)
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const MANIFEST = 'tools/env-providers.json';
const LEDGER = 'docs/ENV-SLOTS.md';
const ENV_RE = /^ENV-\d{2}$/;
const NAME_RE = /^[A-Z][A-Z0-9_]*$/;
const REPO_RE = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;

export const CATEGORIES = new Set(['credentials', 'authorization', 'hardware', 'human', 'mixed']);
const NAME_KEYS = new Set(['name', 'description', 'pilot']);

const slug = (text) => text.toLowerCase().replace(/[^\p{L}\p{N} -]/gu, '').replace(/ /g, '-');
const cells = (line) => line.split('|').slice(1, -1).map((cell) => cell.trim());
const bilingual = (value) => value && typeof value.zh === 'string' && value.zh.trim() && typeof value.en === 'string' && value.en.trim();

/** Read `## 2.` from the Chinese ledger and return `ENV-XX -> status`. */
export function ledgerStatuses(root = ROOT) {
  const text = fs.readFileSync(path.join(root, LEDGER), 'utf8');
  const lines = text.split(/\r?\n/);
  const start = lines.findIndex((line) => /^## 2\./.test(line));
  if (start < 0) throw new Error(`${LEDGER}: missing "## 2." summary section`);
  const end = lines.findIndex((line, i) => i > start && /^## /.test(line));
  const table = lines.slice(start + 1, end < 0 ? lines.length : end).filter((line) => /^\| /.test(line));
  if (table.length < 3) throw new Error(`${LEDGER}: missing slot summary table`);
  const header = cells(table[0]);
  const idIndex = header.findIndex((cell) => cell === 'ID');
  const statusIndex = header.findIndex((cell) => cell === '状态' || cell === 'Status');
  if (idIndex < 0 || statusIndex < 0) throw new Error(`${LEDGER}: missing ID/status columns`);
  const statuses = new Map();
  for (const line of table.slice(2)) {
    const row = cells(line);
    if (!ENV_RE.test(row[idIndex] ?? '')) continue;
    statuses.set(row[idIndex], row[statusIndex] ?? '');
  }
  return statuses;
}

/** GitHub-style anchors for the `### ENV-XX ...` headings in the ledger. */
export function headingAnchors(root = ROOT) {
  const text = fs.readFileSync(path.join(root, LEDGER), 'utf8');
  const anchors = new Map();
  for (const match of text.matchAll(/^### (ENV-\d{2})\s+.*$/gm)) {
    anchors.set(match[1], `#${slug(match[0].slice(4))}`);
  }
  return anchors;
}

export function checkEnvProviders({ root = ROOT } = {}) {
  const errors = [];
  const lines = [];
  const fail = (message) => {
    errors.push(message);
    lines.push({ ok: false, text: `FAIL ${message}` });
  };
  const pass = (message) => lines.push({ ok: true, text: `ok   ${message}` });

  let doc;
  try {
    doc = JSON.parse(fs.readFileSync(path.join(root, MANIFEST), 'utf8'));
  } catch (error) {
    fail(`${MANIFEST}: ${error.message}`);
    return { failed: errors.length, errors, lines };
  }
  if (doc.schema !== 1) fail(`${MANIFEST}: schema must be 1`);
  if (!REPO_RE.test(doc.repo ?? '')) fail(`${MANIFEST}: repo must be OWNER/REPO`);
  if (!Array.isArray(doc.slots)) {
    fail(`${MANIFEST}: slots must be an array`);
    return { failed: errors.length, errors, lines };
  }

  const statuses = ledgerStatuses(root);
  const anchors = headingAnchors(root);
  const seen = new Set();
  let secretCount = 0;
  let variableCount = 0;

  for (const [index, slot] of doc.slots.entries()) {
    const where = `${MANIFEST}: slots[${index}]`;
    if (!slot || typeof slot !== 'object' || Array.isArray(slot)) {
      fail(`${where}: must be an object`);
      continue;
    }
    const id = slot.id;
    if (!ENV_RE.test(id ?? '')) {
      fail(`${where}: invalid slot id "${id}"`);
      continue;
    }
    if (seen.has(id)) fail(`${where}: duplicate slot id ${id}`);
    seen.add(id);
    if (!statuses.has(id)) fail(`${MANIFEST}: ${id} is not in ${LEDGER}`);
    if (!bilingual(slot.title)) fail(`${MANIFEST}: ${id} needs title.zh and title.en`);
    if (!CATEGORIES.has(slot.category)) {
      fail(`${MANIFEST}: ${id} category "${slot.category}" must be ${[...CATEGORIES].join(' / ')}`);
    }
    if (!bilingual(slot.request)) fail(`${MANIFEST}: ${id} needs request.zh and request.en`);
    if (typeof slot.docs !== 'string' || !slot.docs.startsWith(`${LEDGER}#`)) {
      fail(`${MANIFEST}: ${id} docs must be ${LEDGER}#<anchor>`);
    } else if (anchors.has(id)) {
      const expected = `${LEDGER}${anchors.get(id)}`;
      if (slot.docs !== expected) fail(`${MANIFEST}: ${id} docs anchor is "${slot.docs}", expected "${expected}"`);
    }

    for (const field of ['secrets', 'variables']) {
      const entries = slot[field];
      if (!Array.isArray(entries)) {
        fail(`${MANIFEST}: ${id} ${field} must be an array`);
        continue;
      }
      for (const [entryIndex, entry] of entries.entries()) {
        const label = `${id} ${field}[${entryIndex}]`;
        if (!entry || typeof entry !== 'object' || Array.isArray(entry)) {
          fail(`${MANIFEST}: ${label} must be an object`);
          continue;
        }
        for (const key of Object.keys(entry)) {
          if (!NAME_KEYS.has(key)) fail(`${MANIFEST}: ${label} has unexpected key "${key}" (values are never allowed)`);
        }
        if (!NAME_RE.test(entry.name ?? '')) {
          fail(`${MANIFEST}: ${label} name "${entry.name}" must match [A-Z][A-Z0-9_]*`);
        }
        if (entry.description !== undefined && typeof entry.description !== 'string') {
          fail(`${MANIFEST}: ${label} description must be a string`);
        }
        if (entry.pilot === true && !(typeof entry.description === 'string' && entry.description.trim())) {
          fail(`${MANIFEST}: ${label} is a pilot name and needs a description`);
        }
        if (field === 'secrets') secretCount += 1;
        else variableCount += 1;
      }
    }

    if (!Array.isArray(slot.portals)) {
      fail(`${MANIFEST}: ${id} portals must be an array`);
    } else {
      for (const [portalIndex, portal] of slot.portals.entries()) {
        const label = `${id} portals[${portalIndex}]`;
        if (!portal || typeof portal !== 'object' || !bilingual(portal.label)) {
          fail(`${MANIFEST}: ${label} needs label.zh and label.en`);
        }
        if (!/^https:\/\/\S+$/.test(portal?.url ?? '')) {
          fail(`${MANIFEST}: ${label} url "${portal?.url}" must be an https URL`);
        }
      }
    }
  }

  for (const [id, status] of statuses) {
    if (status === 'CLOSED') {
      if (seen.has(id)) fail(`${MANIFEST}: ${id} is CLOSED in ${LEDGER}; remove it or reopen the slot`);
    } else if (!seen.has(id)) {
      fail(`${MANIFEST}: missing entry for non-CLOSED slot ${id}`);
    }
  }

  if (errors.length === 0) {
    pass(`${MANIFEST} · schema v1, ${doc.slots.length} non-CLOSED slots, ${secretCount} secret names, ${variableCount} variable names`);
    pass(`${MANIFEST} · names, https portals, docs anchors and ledger coverage clean`);
  }
  return { failed: errors.length, errors, lines };
}

function main() {
  try {
    const { failed, lines, errors } = checkEnvProviders();
    for (const line of lines) (line.ok ? console.log : console.error)(line.text);
    if (failed > 0) {
      console.error(`\n${errors.length} env-provider manifest error(s)`);
      process.exit(1);
    }
    console.log('\nenv-provider manifest clean');
  } catch (error) {
    console.error(`check-env-providers: ${error.message}`);
    process.exit(1);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
