#!/usr/bin/env node
/**
 * env-provide.mjs — safe local entry helper for ENV-SLOTS credentials.
 *
 * Only names and non-secret configuration live in tools/env-providers.json.
 * Secret values are read from a hidden TTY prompt or a local file (base64)
 * and streamed to `gh secret set` over stdin. Values never touch argv, shell
 * history, or this tool's output, and are never written to disk.
 *
 * Usage:
 *   node tools/env-provide.mjs --list [--json]
 *   node tools/env-provide.mjs --check [--slot ENV-XX] [--strict]
 *   node tools/env-provide.mjs --slot ENV-XX
 *   node tools/env-provide.mjs --open ENV-XX [--dry-run]
 *   node tools/env-provide.mjs --set-secret NAME [--file PATH] [--dry-run]
 *   node tools/env-provide.mjs --set-var NAME=VALUE [--dry-run]
 *
 * Global: --repo OWNER/REPO (default erikwang2013/polypack).
 *
 * gh requires repo admin rights ("Secrets: read/write" and "Variables:
 * read/write" for fine-grained tokens). Hidden input uses TTY raw mode; on a
 * terminal without raw-mode support use --file with a local file instead.
 */
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const MANIFEST = path.join(ROOT, 'tools', 'env-providers.json');
const NAME_RE = /^[A-Z][A-Z0-9_]*$/;
const REPO_RE = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;
const VALUE_FLAGS = new Set(['slot', 'repo', 'file', 'set-secret', 'set-var', 'open']);

const HELP = `env-provide — local helper for missing ENV slot credentials

  --list [--json]                 slots and configured/missing names
  --check [--slot ENV-XX] [--strict]
                                  names only; --strict exits 1 on any missing
  --slot ENV-XX                   missing names, Settings links, gh commands,
                                  authorization / hardware / human links
  --open ENV-XX [--dry-run]       open authorization portals
  --set-secret NAME [--file PATH] [--dry-run]
                                  hidden TTY prompt, or base64 of PATH; value is
                                  streamed to "gh secret set" via stdin
  --set-var NAME=VALUE [--dry-run]
                                  non-secret repository variable, with confirmation

Global: --repo OWNER/REPO (default from tools/env-providers.json)
Exit: nonzero on invalid input or gh failure; --strict also on missing names.
`;

function parseArgv(argv) {
  const opts = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '-h' || arg === '--help') {
      opts.help = true;
    } else if (/^--(list|check|strict|json|dry-run)$/.test(arg)) {
      opts[arg.slice(2)] = true;
    } else if (arg.startsWith('--') && VALUE_FLAGS.has(arg.slice(2))) {
      const value = argv[i + 1];
      if (value === undefined || value.startsWith('--')) {
        throw new Error(`${arg} requires a value`);
      }
      opts[arg.slice(2)] = value;
      i += 1;
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  return opts;
}

function loadManifest() {
  const doc = JSON.parse(fs.readFileSync(MANIFEST, 'utf8'));
  if (doc.schema !== 1 || !Array.isArray(doc.slots)) {
    throw new Error('tools/env-providers.json: expected schema 1 with a slots array');
  }
  return doc;
}

function pickCommand(opts) {
  const commands = ['set-secret', 'set-var', 'open', 'check', 'list'].filter((key) => opts[key] !== undefined);
  if (opts.slot !== undefined) {
    if (opts.check || opts.list) return opts.check ? 'check' : 'list';
    commands.push('slot');
  }
  if (commands.length !== 1) {
    throw new Error(commands.length === 0 ? 'no command given (try --list or --help)' : `choose one command, got: ${commands.join(', ')}`);
  }
  if (opts.strict && commands[0] !== 'check') throw new Error('--strict only applies to --check');
  if (opts.json && commands[0] !== 'list') throw new Error('--json only applies to --list');
  if (opts.file !== undefined && commands[0] !== 'set-secret') throw new Error('--file only applies to --set-secret');
  return commands[0];
}

function repoOf(opts, manifest) {
  const repo = opts.repo ?? manifest.repo;
  if (!REPO_RE.test(repo)) throw new Error(`invalid repo "${repo}" (expected OWNER/REPO)`);
  return repo;
}

function findSlot(manifest, id) {
  const slot = manifest.slots.find((entry) => entry.id === id);
  if (!slot) throw new Error(`unknown slot "${id}"`);
  return slot;
}

function settingsLinks(repo) {
  return {
    secrets: `https://github.com/${repo}/settings/secrets/actions`,
    variables: `https://github.com/${repo}/settings/variables/actions`,
  };
}

/** List only names from gh; values are deliberately never read or printed. */
function ghNames(kind, repo) {
  const result = spawnSync('gh', [kind, 'list', '--repo', repo], { encoding: 'utf8' });
  if (result.error) {
    return { ok: false, error: result.error.code === 'ENOENT' ? 'gh CLI not found in PATH' : result.error.message };
  }
  if (result.status !== 0) {
    const line = (result.stderr || '').trim().split(/\r?\n/)[0];
    return { ok: false, error: line || `gh ${kind} list exited ${result.status}` };
  }
  const names = new Set();
  for (const line of result.stdout.split(/\r?\n/)) {
    const trimmed = line.trim();
    // gh tables are tab-separated; do not mistake a warning line for a name.
    const name = trimmed.split(/\s+/)[0];
    if (NAME_RE.test(name) && (trimmed === name || trimmed.includes('\t'))) names.add(name);
  }
  return { ok: true, names };
}

function ghStatus(repo) {
  const secrets = ghNames('secret', repo);
  const variables = ghNames('variable', repo);
  if (!secrets.ok || !variables.ok) {
    return { ok: false, error: secrets.error || variables.error, secrets: null, variables: null };
  }
  return { ok: true, secrets: secrets.names, variables: variables.names };
}

function statusOf(kind, name, gh) {
  if (!gh.ok) return 'unknown';
  return (kind === 'secret' ? gh.secrets : gh.variables).has(name) ? 'configured' : 'missing';
}

function statusLabel(slot, kind, gh) {
  const entries = kind === 'secret' ? slot.secrets : slot.variables;
  if (entries.length === 0) return '-';
  if (!gh.ok) return `?/${entries.length}`;
  const configured = entries.filter((entry) => statusOf(kind, entry.name, gh) === 'configured').length;
  return `${configured}/${entries.length}`;
}

function printSlotLines(slot, gh) {
  const groups = [
    ['secret', slot.secrets],
    ['variable', slot.variables],
  ];
  for (const [kind, entries] of groups) {
    for (const entry of entries) {
      const status = statusOf(kind, entry.name, gh).toUpperCase();
      console.log(`  ${status.padEnd(10)} ${kind.padEnd(8)} ${entry.name}${entry.description ? `  # ${entry.description}` : ''}`);
    }
  }
}

function cmdList(manifest, opts, repo) {
  const gh = ghStatus(repo);
  if (opts.json) {
    const payload = {
      schema: 1,
      repo,
      gh: gh.ok ? { ok: true } : { ok: false, error: gh.error },
      slots: manifest.slots.map((slot) => ({
        id: slot.id,
        title: slot.title,
        category: slot.category,
        docs: slot.docs,
        secrets: slot.secrets.map((entry) => ({ name: entry.name, status: statusOf('secret', entry.name, gh) })),
        variables: slot.variables.map((entry) => ({ name: entry.name, status: statusOf('variable', entry.name, gh) })),
      })),
    };
    console.log(JSON.stringify(payload, null, 2));
  } else {
    console.log(`repo: ${repo}${gh.ok ? '' : `  (gh unavailable: ${gh.error})`}`);
    for (const slot of manifest.slots) {
      console.log(`\n${slot.id}  ${slot.category.padEnd(13)} secrets ${statusLabel(slot, 'secret', gh).padEnd(6)} variables ${statusLabel(slot, 'variable', gh)}`);
      console.log(`  ${slot.title.zh}`);
      printSlotLines(slot, gh);
    }
    if (!gh.ok) {
      const links = settingsLinks(repo);
      console.log(`\nManual check: ${links.secrets}`);
      console.log(`Manual check: ${links.variables}`);
    }
  }
  return gh.ok ? 0 : 1;
}

function cmdCheck(manifest, opts, repo) {
  const slots = opts.slot === undefined ? manifest.slots : [findSlot(manifest, opts.slot)];
  const gh = ghStatus(repo);
  let missing = 0;
  let required = 0;
  if (!gh.ok) {
    console.error(`gh unavailable: ${gh.error}`);
    const links = settingsLinks(repo);
    console.error(`Manual check secrets:   ${links.secrets}`);
    console.error(`Manual check variables: ${links.variables}`);
    return 1;
  }
  for (const slot of slots) {
    const groups = [
      ['secret', slot.secrets],
      ['variable', slot.variables],
    ];
    for (const [kind, entries] of groups) {
      for (const entry of entries) {
        required += 1;
        const status = statusOf(kind, entry.name, gh);
        if (status === 'missing') missing += 1;
        console.log(`${slot.id} ${kind} ${entry.name} ${status}`);
      }
    }
  }
  console.log(`\n${required} required name(s), ${missing} missing (repo ${repo})`);
  return opts.strict && missing > 0 ? 1 : 0;
}

function cmdSlot(manifest, opts, repo) {
  const slot = findSlot(manifest, opts.slot);
  const gh = ghStatus(repo);
  const links = settingsLinks(repo);
  console.log(`${slot.id} · ${slot.title.zh}`);
  console.log(`${slot.title.en}`);
  console.log(`category: ${slot.category}`);
  console.log(`docs: ${slot.docs}`);
  console.log(`gh: ${gh.ok ? 'names read' : `unavailable (${gh.error})`}`);
  console.log('\nRequired secrets:');
  if (slot.secrets.length === 0) console.log('  (none)');
  for (const entry of slot.secrets) {
    console.log(`  ${statusOf('secret', entry.name, gh).toUpperCase().padEnd(10)} ${entry.name}${entry.description ? `  # ${entry.description}` : ''}`);
  }
  console.log('\nRequired variables:');
  if (slot.variables.length === 0) console.log('  (none)');
  for (const entry of slot.variables) {
    console.log(`  ${statusOf('variable', entry.name, gh).toUpperCase().padEnd(10)} ${entry.name}${entry.description ? `  # ${entry.description}` : ''}`);
  }
  console.log('\nGitHub Settings:');
  console.log(`  secrets:   ${links.secrets}`);
  console.log(`  variables: ${links.variables}`);
  console.log('\nCommands (values are prompted/streamed, never placed on argv):');
  for (const entry of slot.secrets) {
    console.log(`  gh secret set ${entry.name} --repo ${repo}`);
    console.log(`  node tools/env-provide.mjs --set-secret ${entry.name} --repo ${repo}`);
  }
  for (const entry of slot.variables) {
    console.log(`  gh variable set ${entry.name} --repo ${repo}`);
    console.log(`  node tools/env-provide.mjs --set-var ${entry.name}=<value> --repo ${repo}`);
  }
  console.log('\nAuthorization / hardware / human entry points:');
  if (slot.portals.length === 0) console.log('  (no portal; manual request required)');
  for (const portal of slot.portals) console.log(`  ${portal.label.zh} / ${portal.label.en}: ${portal.url}`);
  console.log(`\nRequest note:\n  ${slot.request.zh}\n  ${slot.request.en}`);
  return gh.ok ? 0 : 1;
}

function openUrl(url) {
  const command = process.platform === 'win32'
    ? ['cmd', ['/c', 'start', '', url]]
    : process.platform === 'darwin'
      ? ['open', [url]]
      : ['xdg-open', [url]];
  const result = spawnSync(command[0], command[1], { stdio: 'ignore' });
  return !result.error && result.status === 0;
}

function cmdOpen(manifest, opts) {
  const slot = findSlot(manifest, opts.open);
  if (slot.portals.length === 0) {
    console.log(`${slot.id} has no authorization portal; manual request required.`);
    console.log(slot.request.zh);
    console.log(slot.request.en);
    return 0;
  }
  for (const portal of slot.portals) {
    if (opts['dry-run']) {
      console.log(`dry-run: would open ${portal.label.en}: ${portal.url}`);
      continue;
    }
    if (openUrl(portal.url)) {
      console.log(`opened ${portal.label.en}: ${portal.url}`);
    } else {
      console.log(`manual: ${portal.url}`);
    }
  }
  return 0;
}

function ask(question) {
  return new Promise((resolve) => {
    const rl = readline.createInterface({ input: process.stdin, output: process.stderr, terminal: false });
    let settled = false;
    const finish = (answer) => {
      if (settled) return;
      settled = true;
      rl.close();
      resolve(answer);
    };
    rl.question(question, (answer) => finish(answer.trim().toLowerCase()));
    rl.on('close', () => finish(''));
  });
}

/** Read a secret from a TTY with echo off; rejects on non-TTY or cancellation. */
function readHidden(prompt) {
  return new Promise((resolve, reject) => {
    const stdin = process.stdin;
    if (!stdin.isTTY) {
      reject(new Error('no TTY for hidden input; rerun with --file PATH on a local file'));
      return;
    }
    let value = '';
    const finish = (error) => {
      stdin.removeListener('data', onData);
      try {
        stdin.setRawMode(false);
      } catch {
        // terminal already gone; nothing to restore
      }
      stdin.pause();
      process.stderr.write('\n');
      if (error) reject(error);
      else resolve(value);
    };
    const onData = (chunk) => {
      for (const char of chunk) {
        if (char.codePointAt(0) === 3 || char.codePointAt(0) === 4) {
          finish(new Error('cancelled'));
          return;
        }
        if (char === '\r' || char === '\n') {
          finish();
          return;
        }
        if (char.codePointAt(0) === 127 || char.codePointAt(0) === 8) {
          value = value.slice(0, -1);
        } else if (char >= ' ') {
          value += char;
        }
      }
    };
    try {
      stdin.setRawMode(true);
    } catch {
      reject(new Error('terminal does not support hidden input; rerun with --file PATH'));
      return;
    }
    process.stderr.write(prompt);
    stdin.setEncoding('utf8');
    stdin.resume();
    stdin.on('data', onData);
  });
}

/** Stream the value over stdin; the value is never an argv or env entry. */
function ghSet(kind, name, value, repo) {
  return new Promise((resolve, reject) => {
    const args = kind === 'secret'
      ? ['secret', 'set', name, '--repo', repo]
      : ['variable', 'set', name, '--repo', repo, '--body', value];
    const child = spawn('gh', args, { stdio: ['pipe', 'inherit', 'inherit'] });
    child.stdin.on('error', () => {}); // gh may exit before reading; the exit code decides
    child.on('error', (error) => {
      reject(new Error(`cannot run gh: ${error.code === 'ENOENT' ? 'not found in PATH' : error.message}`));
    });
    child.on('exit', (code) => {
      if (code === 0) resolve();
      else reject(new Error(`gh ${kind} set failed (exit ${code})`));
    });
    if (kind === 'secret') child.stdin.end(`${value}\n`);
    else child.stdin.end();
  });
}

function validateName(name, label) {
  if (!NAME_RE.test(name)) throw new Error(`invalid ${label} name "${name}" (must match [A-Z][A-Z0-9_]*)`);
}

async function cmdSetSecret(opts, repo) {
  const name = opts['set-secret'];
  validateName(name, 'secret');
  const file = opts.file;
  if (file !== undefined && !fs.existsSync(file)) throw new Error(`file not found: ${file}`);
  const source = file === undefined ? 'hidden TTY prompt' : 'file base64';
  if (opts['dry-run']) {
    console.log(`dry-run: would stream secret ${name} to repo ${repo} via "gh secret set" (source: ${source})`);
    return 0;
  }
  if (file === undefined && !process.stdin.isTTY) {
    throw new Error('hidden input needs a TTY; rerun with --file PATH on a local file');
  }
  const answer = await ask(`Store secret ${name} in GitHub Actions secrets for ${repo}? Type "yes": `);
  if (answer !== 'yes') throw new Error('aborted: confirmation was not "yes"');
  let value;
  if (file === undefined) {
    value = await readHidden(`Secret value for ${name} (input hidden): `);
  } else {
    const bytes = fs.readFileSync(file);
    if (bytes.length === 0) throw new Error(`file is empty: ${file}`);
    value = bytes.toString('base64');
  }
  if (value.length === 0) throw new Error('secret value is empty');
  await ghSet('secret', name, value, repo);
  console.log(`set secret ${name} in ${repo}`);
  return 0;
}

async function cmdSetVar(opts, repo) {
  const raw = opts['set-var'];
  const eq = raw.indexOf('=');
  if (eq <= 0) throw new Error('--set-var expects NAME=VALUE');
  const name = raw.slice(0, eq);
  const value = raw.slice(eq + 1);
  validateName(name, 'variable');
  if (opts['dry-run']) {
    console.log(`dry-run: would set variable ${name} in repo ${repo} via "gh variable set" (value not printed)`);
    return 0;
  }
  const answer = await ask(`Set non-secret variable ${name} in ${repo}? Type "yes": `);
  if (answer !== 'yes') throw new Error('aborted: confirmation was not "yes"');
  await ghSet('variable', name, value, repo);
  console.log(`set variable ${name} in ${repo}`);
  return 0;
}

async function main() {
  try {
    const opts = parseArgv(process.argv.slice(2));
    if (opts.help) {
      process.stdout.write(HELP);
      return 0;
    }
    const command = pickCommand(opts);
    const manifest = loadManifest();
    const repo = repoOf(opts, manifest);
    switch (command) {
      case 'list':
        return cmdList(manifest, opts, repo);
      case 'check':
        return cmdCheck(manifest, opts, repo);
      case 'slot':
        return cmdSlot(manifest, opts, repo);
      case 'open':
        return cmdOpen(manifest, opts);
      case 'set-secret':
        return await cmdSetSecret(opts, repo);
      case 'set-var':
        return await cmdSetVar(opts, repo);
      default:
        throw new Error(`unknown command ${command}`);
    }
  } catch (error) {
    console.error(`env-provide: ${error.message}`);
    return 1;
  }
}

main().then((code) => {
  process.exitCode = code;
});
