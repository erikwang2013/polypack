# Incremental releases: `polypack release --changed`

`polypack release --changed` republishes only the targets that changed in a multi-target
repository. The decision key comes from the artifact set's manifest digest
(`manifest_digest` / `cache_key`); a target that matches an existing receipt emits a
distinct `release-skipped` event and exits 0 without touching the repository backend,
rewriting the receipt, or incrementing `runs`.

## Decision and events

- Manifest digest: artifacts sorted by name, mixing name, byte count and SHA256
  (order-independent, content-bound).
- Receipt key: `<project root>/.polypack/release/<target>/<version>-<digest16>.json`.
- Hit condition: the receipt's `cache_key` (old receipts fall back to
  `manifest_digest`), repository root and repository format all match.
- Misses (new artifacts / targets / repository) publish normally and record
  `cache_key` on the `--changed` path.
- Receipts written by a plain `polypack release` first run have no `cache_key`: the next
  `--changed` run falls back to `manifest_digest` (`key_source: "manifest_digest"`,
  still skipped); later `--changed` receipts carry `cache_key`
  (`key_source: "cache_key"`).

## Five-target fixture and command

The fixture uses five targets (`linux`, `web`, `windows`, `macos`, `android`) with
their format overridden to `deb`, each holding a valid deb under
`.polypack/packages/<target>/`:

```toml
[package]
name = "release-bench"
version = "0.1.0"

[targets.linux]
format = "deb"
# web / windows / macos / android likewise
```

Both runs use the same command; the first run has no receipts, so all five targets
publish, and the second run publishes only the changed target:

```bash
polypack release --changed \
  --target linux,web,windows,macos,android \
  --repo .polypack/repo \
  --credential env:RELEASE_GPG_KEY \
  --json
```

After changing one artifact (for example rebuilding the `android` deb), running the
same command again prints five JSON events on stdout: one `published` plus four
`release-skipped`, exit code 0.

## JSON `release-skipped` contract

| Field | Type | Meaning |
| --- | --- | --- |
| `event` | string | Always `"release-skipped"`, strictly distinct from `"published"` |
| `target` | string | Target id |
| `version` | string | Project version |
| `repo` | string | Absolute path of the repository root |
| `repo_kind` | string | `deb` / `rpm` / `pkg` / `pkgsrc` |
| `manifest_digest` | string | Manifest digest of the current artifact set |
| `cache_key` | string | Current change key (same source as the manifest digest today) |
| `receipt` | string | Path of the matched receipt |
| `artifact_count` | number | Number of artifacts |
| `runs` | number | Existing run count in the receipt (skips do not increment) |
| `key_source` | string | `cache_key`, or `manifest_digest` for the old-receipt fallback |
| `reason` | string | Always `"unchanged"` |
| `note` | string | Human-readable note (not part of the machine decision) |

## Measured savings

`scripts/bench-release-changed.sh` generates the fixture and runs both rounds
(Linux, debug build, one measurement):

| Round | published (= backend calls) | release-skipped | Wall clock |
| --- | --- | --- | --- |
| First run (all five publish) | 5 | 0 | 8366 ms |
| Second run (one artifact changed) | 1 | 4 | 1549 ms |
| Saving | 80% | — | 81% |

Backend calls are the deterministic gate (each `published` line corresponds to one
repository publish; `release-skipped` never touches the backend), and the script
asserts 5 → 1. Wall clock depends on gpg and filesystem caches, so it is reported
but not asserted; another measurement of the same fixture in the Rust integration
test was 11743 ms → 3163 ms (73%).

## CI wiring

The existing `rust-ci` Linux leg gets one focused step (no separate job; it reuses
the already-built test binary):

```yaml
- name: release --changed 5-target measurement (Linux focused)
  if: runner.os == 'Linux'
  run: |
    cargo test -p polypack-cli --test release five_target_changed -- --nocapture 2>&1 \
      | tee /tmp/release-changed.log
    grep -q "first_published=5 second_published=1" /tmp/release-changed.log
```

Without gpg the test prints `SKIP` and returns; the `grep` ensures this Linux leg
actually reached the measurement instead of silently skipping. `cargo test
--workspace` already covers the test as well.

## `--since` limitation

`polypack release` does not build, and receipts carry no per-target source/package path
mapping, so git diff cannot robustly decide whether a target changed. `--since <REF>`
therefore returns `polypack::E_PLAN_0002` in this mode instead of being silently ignored;
use `--changed` (receipt-based) or pre-filter `--target` from a git diff.
