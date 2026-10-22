#!/usr/bin/env node
/**
 * check-site.mjs — static site gate for site/.
 *
 * Checks local asset references, bilingual data pairs, required SEO/OG files,
 * local vendor scripts (three.js), and JS syntax. Read-only; exit 1 on drift.
 */
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SITE = path.join(ROOT, 'site');
const INDEX = path.join(SITE, 'index.html');

let failed = 0;
const ok = (message) => console.log(`ok   ${message}`);
const bad = (message) => { failed += 1; console.error(`ERROR ${message}`); };
const read = (rel) => fs.readFileSync(path.join(SITE, rel), 'utf8');

const html = fs.readFileSync(INDEX, 'utf8');

// 1) Local references exist (src/href/url() in inline style are covered separately).
const refs = new Set([...html.matchAll(/(?:src|href)="([^"]+)"/g)].map((m) => m[1]));
for (const ref of refs) {
  if (/^(https?:|#|mailto:|data:|\/\/)/.test(ref)) continue;
  const clean = ref.split(/[?#]/)[0];
  if (!fs.existsSync(path.join(SITE, clean))) bad(`index.html references missing local asset: ${ref}`);
}
ok('local asset references resolve');

// 2) Bilingual attributes are paired.
const zh = [...html.matchAll(/data-zh="([^"]*)"/g)].length;
const en = [...html.matchAll(/data-en="([^"]*)"/g)].length;
const zhHtml = [...html.matchAll(/data-zh-html="([^"]*)"/g)].length;
const enHtml = [...html.matchAll(/data-en-html="([^"]*)"/g)].length;
if (zh !== en) bad(`data-zh (${zh}) and data-en (${en}) counts differ`);
if (zhHtml !== enHtml) bad(`data-zh-html (${zhHtml}) and data-en-html (${enHtml}) counts differ`);
if (zh + zhHtml === 0) bad('no bilingual text attributes found');
else ok(`bilingual attributes paired (${zh + zhHtml} entries)`);

// 3) Required SEO / PWA-ish files and metadata.
for (const file of ['robots.txt', 'sitemap.xml', 'manifest.webmanifest', 'assets/og/cover.png', 'vendor/three.min.js', 'vendor/three.LICENSE']) {
  if (!fs.existsSync(path.join(SITE, file))) bad(`missing ${file}`);
}
for (const needle of ['<link rel="canonical"', 'application/ld+json', 'og:image', 'hreflang="en"', 'perf-output']) {
  // perf-output is created by JS; the marker lives in site.js, not index.html.
  const hay = needle === 'perf-output' ? read('js/site.js') : html;
  if (!hay.includes(needle)) bad(`missing SEO/perf marker: ${needle}`);
}
ok('SEO, sitemap, OG and perf markers present');

// 4) No remote runtime assets except the requested Busuanzi stats script.
const remoteScripts = [...html.matchAll(/<script[^>]+src="([^"]+)"/g)].map((m) => m[1]).filter((u) => /^https?:/.test(u));
const remoteCss = [...html.matchAll(/<link[^>]+href="(https?:[^"]+\.css[^"]*)"/g)].map((m) => m[1]);
if (remoteScripts.length || remoteCss.length) bad(`remote runtime assets found: ${[...remoteScripts, ...remoteCss].join(', ')}`);
else ok('no remote runtime assets (three.js is vendored)');

// 5) JS syntax.
for (const file of ['js/site.js', 'js/three-effects.js']) {
  const result = spawnSync(process.execPath, ['--check', path.join(SITE, file)], { encoding: 'utf8' });
  if (result.status !== 0) bad(`${file}: ${result.stderr.trim() || 'syntax error'}`);
}
ok('site JavaScript parses');

// 6) Donation QR images are real local PNGs provided by the maintainer.
for (const file of ['assets/img/weixinpay.png', 'assets/img/alipay.png']) {
  const full = path.join(SITE, file);
  if (!fs.existsSync(full)) bad(`missing donation QR ${file}`);
  else if (fs.statSync(full).size < 10_000) bad(`${file} looks like a placeholder (too small)`);
}
ok('donation QR images present');

// 7) Size budget (local assets only; keeps first paint predictable).
const budget = 4 * 1024 * 1024;
const size = (dir) => fs.readdirSync(dir, { withFileTypes: true }).reduce((sum, entry) => {
  const p = path.join(dir, entry.name);
  return sum + (entry.isDirectory() ? size(p) : fs.statSync(p).size);
}, 0);
const bytes = size(SITE);
if (bytes > budget) bad(`site assets ${(bytes / 1048576).toFixed(2)} MiB exceed budget ${(budget / 1048576).toFixed(2)} MiB`);
else ok(`site assets ${(bytes / 1048576).toFixed(2)} MiB (budget ${(budget / 1048576).toFixed(2)} MiB)`);

if (failed) {
  console.error(`\n${failed} site check error(s)`);
  process.exit(1);
}
console.log('\nstatic site clean');
