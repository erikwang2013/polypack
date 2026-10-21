// env-provide.mjs tests: fake gh in PATH proves names-only reporting, no value
// echo, base64 file streaming, confirmation, and graceful gh-missing fallback.
// Run: node --test tools/test/env-provide.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const CLI = path.join(ROOT, 'tools', 'env-provide.mjs');
const CHECK = path.join(ROOT, 'tools', 'check-env-providers.mjs');
const POSIX = process.platform !== 'win32';
const it = POSIX ? test : test.skip;

const FAKE_GH = `#!/bin/sh
set -eu
echo "$*" >> "$GH_CAPTURE_DIR/args.log"
case "$1 $2" in
  "secret list")
    printf 'POLYPACK_APPLE_APP_PASSWORD\\t2026-10-01T00:00:00Z\\n'
    ;;
  "variable list")
    printf 'POLYPACK_IOS_TEAM_ID\\tTEAMID-NOT-A-SECRET\\t2026-10-01T00:00:00Z\\n'
    ;;
  "secret set")
    cat > "$GH_CAPTURE_DIR/secret-$3.stdin"
    echo "gh: set secret $3"
    ;;
  "variable set")
    printf '%s' "$7" > "$GH_CAPTURE_DIR/variable-$3.stdin"
    echo "gh: set variable $3"
    ;;
  *)
    echo "unexpected gh call: $*" >&2
    exit 1
    ;;
esac
`;

function fixture(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'env-provide-test-'));
  const bin = path.join(dir, 'bin');
  const capture = path.join(dir, 'capture');
  fs.mkdirSync(bin);
  fs.mkdirSync(capture);
  const gh = path.join(bin, 'gh');
  fs.writeFileSync(gh, FAKE_GH, { mode: 0o755 });
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  return {
    dir,
    capture,
    env: { ...process.env, PATH: `${bin}${path.delimiter}${process.env.PATH}`, GH_CAPTURE_DIR: capture },
  };
}

function run(args, env, input) {
  return spawnSync(process.execPath, [CLI, ...args], { cwd: ROOT, encoding: 'utf8', env, input });
}

test('manifest gate validates coverage, names, https URLs and anchors', () => {
  const result = spawnSync(process.execPath, [CHECK], { cwd: ROOT, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /12 non-CLOSED slots/);
});

it('--list --json reports configured/missing names only', (t) => {
  const fx = fixture(t);
  const result = run(['--list', '--json'], fx.env);
  assert.equal(result.status, 0, result.stderr);
  const doc = JSON.parse(result.stdout);
  assert.equal(doc.schema, 1);
  assert.equal(doc.gh.ok, true);
  const slot = doc.slots.find((entry) => entry.id === 'ENV-02');
  assert.equal(slot.secrets.find((entry) => entry.name === 'POLYPACK_APPLE_APP_PASSWORD').status, 'configured');
  assert.equal(slot.variables.find((entry) => entry.name === 'POLYPACK_IOS_TEAM_ID').status, 'configured');
  assert.equal(slot.variables.find((entry) => entry.name === 'POLYPACK_IOS_IDENTITY').status, 'missing');
  // gh variable list includes a value column; the tool must not print it.
  assert.doesNotMatch(result.stdout, /TEAMID-NOT-A-SECRET/);
});

it('--check --strict reports names and exits nonzero on missing', (t) => {
  const fx = fixture(t);
  const result = run(['--check', '--strict'], fx.env);
  assert.equal(result.status, 1);
  assert.match(result.stdout, /ENV-02 secret POLYPACK_APPLE_APP_PASSWORD configured/);
  assert.match(result.stdout, /ENV-02 variable POLYPACK_IOS_IDENTITY missing/);
  assert.doesNotMatch(result.stdout, /POLYPACK_IOS_TEAM_ID missing/);
  assert.doesNotMatch(result.stdout, /TEAMID-NOT-A-SECRET/);
});

it('--check falls back to manual Settings links when gh is unavailable', (t) => {
  const fx = fixture(t);
  const empty = path.join(fx.dir, 'empty-bin');
  fs.mkdirSync(empty);
  const result = run(['--check', '--slot', 'ENV-02'], { ...fx.env, PATH: empty });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /gh unavailable/);
  assert.match(result.stderr, /settings\/secrets\/actions/);
  assert.match(result.stderr, /settings\/variables\/actions/);
});

it('--slot prints Settings links, safe gh commands and portal links', (t) => {
  const fx = fixture(t);
  const result = run(['--slot', 'ENV-02'], fx.env);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /https:\/\/github\.com\/erikwang2013\/polypack\/settings\/secrets\/actions/);
  assert.match(result.stdout, /gh secret set POLYPACK_APPLE_APP_PASSWORD --repo erikwang2013\/polypack/);
  assert.match(result.stdout, /https:\/\/developer\.apple\.com\/account/);
  assert.match(result.stdout, /MISSING/);
});

it('--set-secret --dry-run output is stable and never reads a value', (t) => {
  const fx = fixture(t);
  const first = run(['--set-secret', 'POLYPACK_APPLE_APP_PASSWORD', '--dry-run'], fx.env);
  const second = run(['--set-secret', 'POLYPACK_APPLE_APP_PASSWORD', '--dry-run'], fx.env);
  assert.equal(first.status, 0, first.stderr);
  assert.equal(first.stdout, second.stdout);
  assert.equal(first.stderr, second.stderr);
  assert.match(first.stdout, /dry-run: would stream secret POLYPACK_APPLE_APP_PASSWORD/);
  assert.match(first.stdout, /source: hidden TTY prompt/);
  assert.ok(!fs.existsSync(path.join(fx.capture, 'args.log')), 'dry-run must not call gh');
});

it('--set-secret --file streams base64 over stdin, not argv or output', (t) => {
  const fx = fixture(t);
  const file = path.join(fx.dir, 'developer_key.der');
  const raw = Buffer.from('devkey-line-1\nline-2-é\n', 'utf8');
  fs.writeFileSync(file, raw);
  const expected = raw.toString('base64');
  const result = run(['--set-secret', 'POLYPACK_GARMIN_DEVKEY_B64', '--file', file], fx.env, 'yes\n');
  assert.equal(result.status, 0, result.stderr);
  const captured = fs.readFileSync(path.join(fx.capture, 'secret-POLYPACK_GARMIN_DEVKEY_B64.stdin'), 'utf8');
  assert.equal(captured.trimEnd(), expected);
  const output = `${result.stdout}${result.stderr}`;
  assert.ok(!output.includes(expected), 'base64 value must not be echoed');
  assert.ok(!output.includes('devkey-line-1'), 'raw file content must not be echoed');
  const args = fs.readFileSync(path.join(fx.capture, 'args.log'), 'utf8');
  assert.match(args, /secret set POLYPACK_GARMIN_DEVKEY_B64 --repo erikwang2013\/polypack/);
  assert.ok(!args.includes(expected), 'base64 value must not be on argv');
  assert.ok(!args.includes('devkey-line-1'), 'raw file content must not be on argv');
});

it('--set-secret requires explicit confirmation and refuses non-TTY hidden input', (t) => {
  const fx = fixture(t);
  const file = path.join(fx.dir, 'key.der');
  fs.writeFileSync(file, 'secret-bytes');
  const refused = run(['--set-secret', 'POLYPACK_APPLE_APP_PASSWORD', '--file', file], fx.env, 'no\n');
  assert.equal(refused.status, 1);
  assert.match(refused.stderr, /aborted/);
  assert.ok(!fs.existsSync(path.join(fx.capture, 'secret-POLYPACK_APPLE_APP_PASSWORD.stdin')));

  const noTty = run(['--set-secret', 'POLYPACK_APPLE_APP_PASSWORD'], fx.env, '');
  assert.equal(noTty.status, 1);
  assert.match(noTty.stderr, /hidden input needs a TTY/);
});

it('--set-var confirms and passes only the non-secret value', (t) => {
  const fx = fixture(t);
  const result = run(['--set-var', 'POLYPACK_QNX_QEMU=/usr/bin/qemu-system-x86_64'], fx.env, 'yes\n');
  assert.equal(result.status, 0, result.stderr);
  const captured = fs.readFileSync(path.join(fx.capture, 'variable-POLYPACK_QNX_QEMU.stdin'), 'utf8');
  assert.equal(captured, '/usr/bin/qemu-system-x86_64');
  assert.match(result.stdout, /set variable POLYPACK_QNX_QEMU/);
});

it('--check validates slot IDs and secret names', (t) => {
  const fx = fixture(t);
  const slot = run(['--slot', 'ENV-99'], fx.env);
  assert.equal(slot.status, 1);
  assert.match(slot.stderr, /unknown slot "ENV-99"/);
  const name = run(['--set-secret', 'lower-case', '--dry-run'], fx.env);
  assert.equal(name.status, 1);
  assert.match(name.stderr, /invalid secret name/);
});
