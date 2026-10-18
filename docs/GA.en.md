# PolyPack Ecosystem GA Documentation

> Applies to: M8 ecosystem GA (closeout baseline `52968b4`). 中文: [GA.md](GA.md)
> Companion reading: [README](../README.md) · [ROADMAP](ROADMAP.md) · [TASKS](TASKS.md) · [FARM-DEPLOY](FARM-DEPLOY.md) · [EXPERIMENTAL-TARGETS](EXPERIMENTAL-TARGETS.md)

## 1. GA overview and stability commitments

M8 ecosystem GA delivers four things: the WASM plugin SDK (sandbox, SRI/signature, template/scaffold), remote farm v1, the store publish pipeline GA, and the generated 49-target status table. The plugin interface is versioned as `target-spec/v1`; this GA documentation set ships together with the "one-day new target" drill record.

### 1.1 Delivery scope

| M8 delivery area | Status | Evidence / entry point |
|---|---|---|
| WASM plugin sandbox | Delivered | `poly-plugin`: default zero capabilities, explicit grants, fuel/epoch/memory caps (T-M8-01) |
| Plugin interface `target-spec/v1` | Delivered | WIT `polypack-target`; non-v1 → `E_PLUGIN_0005` (T-M8-02) |
| SRI and signature chain | Delivered | `blake3-<hex>` before verification, verification before instantiation (T-M8-03) |
| Template and scaffold | Delivered | `poly plugin new` / `poly plugin check` / `poly plugin digest` + `templates/plugin-target` (T-M8-04) |
| Unified builtin/plugin interface | Delivered | `TargetProvider` + `ProviderRegistry`, no builtin special-casing (T-M8-05) |
| Remote farm v1 | Delivered | Protocol v1, concurrency / worker pool / timeout retry, v0 worker compatibility (T-M8-06..09) |
| Store publish GA | Delivered | Exponential backoff, idempotency key, SubmitJournal, receipt v2, `poly retry --from publish` (T-M8-10/11) |
| 49-target status table | Delivered | Generated table wired into docs-check (T-M8-12) |

### 1.2 Version policy and compatibility

- `target-spec/v1` is the only plugin interface version currently accepted. Breaking changes (field additions or removals, semantic changes, WIT world changes) must be published under a new version (for example `target-spec/v2`); v1 must not be modified in place.
- The host rejects non-v1 plugins with `poly::E_PLUGIN_0005` and a migration hint; the current host requires `target-spec/v1`.
- `polypack.toml` has a top-level `schema = 1`; config sections such as `[[plugins]]` use `deny_unknown_fields`, so unknown keys are rejected rather than silently dropped.
- The plugin artifact content digest (SRI) is `blake3-<hex>`; SRI and signature must be updated together with the artifact. After tampering, even a synchronized SRI update still fails signature verification.

| Object | Policy |
|---|---|
| Plugin interface | `target-spec/v1`; breaking changes go to a new version |
| Plugin artifact | SRI (`blake3-<hex>`) + optional GPG detached signature |
| Configuration | `schema = 1`; unknown keys rejected |
| This repository | `vX.Y.Z` semantic tags |

## 2. Plugin author guide

### 2.1 Prerequisites

- Rust 1.85+ (declared by `templates/plugin-target/README.md`) and the `wasm32-unknown-unknown` target:

  ```bash
  rustup target add wasm32-unknown-unknown
  ```

- A `poly` built from the same commit (`poly-cli` has `publish = false` and is not yet on crates.io).
- The build farm / publish chain is not required for plugin development; a local `poly build` is enough to produce a plugin-target package.

### 2.2 From scaffold to package

| Step | Command | Expected |
|---|---|---|
| 1. Generate project | `poly plugin new pocket-notes` | Template files such as `plugin/`, `componentize/`, `contract/`, `build.sh` |
| 2. Build wasm | `cd pocket-notes && ./build.sh` | Artifact `dist/plugin.wasm` |
| 3. Check project | `poly plugin check .` | Scaffold files present and WIT in sync with the host; a missing `dist/plugin.wasm` is a warning only (build first) |
| 4. Contract test | `cargo test -- --nocapture` | Loads the component and calls `describe` / `package`; failures usually mean stale contract assertions |
| 5. Compute SRI | `poly plugin digest dist/plugin.wasm` | Prints `blake3-<hex>` (same basis as host loading) |
| 6. Register plugin | Edit `[[plugins]]` in the app's `polypack.toml` | See §2.3 |
| 7. Real package | `poly build --config <app>/polypack.toml --target pocket-notes` | The plugin provider produces the target artifact |

Note: `poly plugin new` only scaffolds the plugin project; it does not write into the app. After building, copy `dist/plugin.wasm` into the app's `plugins/` directory and reference it from `path` in `polypack.toml`.

### 2.3 Register in the app

```toml
[[plugins]]
id = "pocket-notes"
path = "plugins/pocket-notes.wasm"
sri = "blake3-<hex>"
# Optional: detached signature (relative to the project root) and public-key trust root
# signature = "plugins/pocket-notes.wasm.sig"
# trust_root = { name = "pocket-notes-pub", source = { env = { variable = "POLYPACK_POCKET_NOTES_TRUST_ROOT" } } }
# Default zero capabilities; grant the minimum explicitly only when needed:
# fs_read = ["data"]
# fs_write = ["out"]
# net = ["api.example.com"]
```

- When `sri` is set, the host enforces verification before loading; a mismatch returns `poly::E_PLUGIN_0006`.
- `signature` and `trust_root` must be configured as a pair; `trust_root` holds only a public key whose payload is `base64(JSON) {"public_key":"<base64>"}` (keyring / Vault / env are all acceptable carriers). A verification failure returns `poly::E_PLUGIN_0009`; an unavailable trust root returns `poly::E_PLUGIN_0010`.
- Capabilities are empty by default: no filesystem, no network. `fs_read` / `fs_write` paths are relative to the project root; `net` entries are `host` or `host:port`. The grant list enters `JobSpec` and appears as `plugin_granted` events in `poly build --json` output.

### 2.4 Minimal host project and artifact copy

A plugin target still requires the app's `polypack.toml` to contain `[ui]` and `[core]` sections (the plugin may not use them, but the schema requires them):

```toml
[ui]
framework = "react"
entry = "index.html"

[core]
crate = "core"
```

```bash
mkdir -p <app>/plugins
cp dist/plugin.wasm <app>/plugins/pocket-notes.wasm
poly build --config <app>/polypack.toml --target pocket-notes
```

> Local verification note: `poly plugin digest` only prints the digest; `poly build --json` provides structured events. The human-readable `poly build` output currently does not show the plugin's full `describe` target declaration, so use `--json` events or the artifact result to confirm (see the known gap in §7).

### 2.5 Single source of template contract assertions

The template contract test keeps only two assertion constants at the top of `contract/src/lib.rs`: `EXPECTED_FORMAT` and `EXPECTED_PACKAGE`. After changing the plugin's `describe` / `package`, update these two constants; forgetting to do so makes `cargo test` fail with exit code 101 (the actual blocker in this drill, see §6.4). The template WIT is a copy of the host's `crates/poly-plugin/wit/polypack-target.wit`: a byte-for-byte comparison gate in `tools/check-docs.mjs` guards the repository side, and `poly plugin check` verifies a project's copy.

## 3. Security model

### 3.1 Default zero capabilities and explicit grants

| Capability | Default | Grant mechanism |
|---|---|---|
| Filesystem read | Denied | `fs_read = ["<relative directory>"]` |
| Filesystem write | Denied | `fs_write = ["<relative directory>"]` |
| Network egress | Denied | `net = ["<host or host:port>"]` |

Granted paths are resolved against the project root and canonicalized; escapes, path traversal and symlink escapes all return `poly::E_PLUGIN_0001` (writes additionally require write permission). Egress only allows explicit allowlist entries; redirects are never followed (`max_redirects = 0`) and non-2xx responses return `E_PLUGIN_0007`. Unauthorized calls never receive host filesystem or network handles.

### 3.2 Artifact integrity and signature chain

The load order is fixed: **SRI verification → signature verification → component parsing → instantiation**.

1. When `sri` is present, the host computes `blake3-<hex>` and compares first; a mismatch returns `E_PLUGIN_0006` and does not proceed to signature verification.
2. When `signature` is present, the host performs GPG detached verification with the public key resolved from `trust_root`; a missing or mismatched signature returns `E_PLUGIN_0009`, and an unavailable trust root returns `E_PLUGIN_0010`. Verification completes before `Component::from_binary`.
3. Only after verification does the host parse and instantiate the component; failure returns `E_PLUGIN_0007`.

GPG operations use a one-shot 0700 `GNUPGHOME`; the trust root accepts only a public key and never inlines a private key. Signature and SRI must be updated together; updating only the SRI cannot bypass signature verification.

### 3.3 Resource limits

| Resource | Default | Hard cap |
|---|---|---|
| fuel | 1,000,000,000 | 10,000,000,000 |
| epoch | 500 ticks (tick = 10ms, about 5s) | 60,000 ticks (about 10 minutes) |
| Linear memory | 64 MiB | 1 GiB |

Configuration can only tighten, never loosen: zero or anything above the hard cap is rejected (fuel / epoch → `E_PLUGIN_0003`, memory → `E_PLUGIN_0004`). Exceeding a limit aborts the plugin call without affecting the host process.

### 3.4 Error codes and artifact scanning

| Error code | Meaning |
|---|---|
| `E_PLUGIN_0001` | Capability denied: path outside the granted directories (traversal / symlink escape / no write permission) |
| `E_PLUGIN_0002` | Egress domain not granted |
| `E_PLUGIN_0003` | Resource limit exhausted (fuel / epoch timeout) |
| `E_PLUGIN_0004` | Memory limit exceeded (`memory.grow` rejected by the host cap) |
| `E_PLUGIN_0005` | Unsupported plugin interface version (not `target-spec/v1`) |
| `E_PLUGIN_0006` | Plugin artifact content digest (SRI) mismatch |
| `E_PLUGIN_0007` | Plugin execution failure (trap / plugin returned error / egress transfer failure) |
| `E_PLUGIN_0008` | Invalid grant list (missing directory / invalid domain or port format) |
| `E_PLUGIN_0009` | Missing signature or verification failure (signature does not match artifact / trust root) |
| `E_PLUGIN_0010` | Signature trust root unavailable (CredentialRef resolution failure / public-key import failure / missing verification tool) |

`poly scan` applies additional host-path, environment-trace and debug-section rules to `.wasm` artifacts: `/home/`, `C:\Users\`, `C:/Users/`, `/var/folders/`, `/tmp/` (host paths); `CARGO_MANIFEST_DIR`, `USERPROFILE=`, `TMPDIR=`, `$HOME` (environment traces); `.debug_info`, `.debug_abbrev` (debug sections). A hit reports only the rule, location and a redacted description (offset) and never echoes the matched path itself; the remediation hint is a release build with debug sections stripped.

## 4. Farm and publish GA

### 4.1 Farm v1

Farm protocol v1 (`farm/v1`) adds concurrent scheduling, a worker pool, and task timeout/retry on top of the v0 "origin site / build site" model; a v1 master still accepts `farm/v0` workers (once version negotiation succeeds). Commands and deployment steps are in [FARM-DEPLOY](FARM-DEPLOY.md) and the README.

| Capability | Behavior |
|---|---|
| Concurrency | The master splits work by target; idle workers in the pool each take one buildable target and run concurrently |
| Worker pool | One mailbox per process; `--jobs` / `--idle` / `--target` control the session and accepted targets |
| Timeout / disconnect | `E_FARM_0004` / `E_FARM_0005` come with remediation hints; `--retries` switches workers and retries |
| v0 compatibility | A v1 master accepts v0 workers; `JobSpec` serialization stays backward compatible |

### 4.2 Isolation, credentials and shared cache

- Each task runs inside a worker process; under `--work-dir` every task gets a one-shot 0700 directory whose name contains only pid + in-process sequence, removed with `remove_dir_all` when the task ends, and the work root is deleted when the worker exits; failure returns `E_FARM_0006`.
- Credentials are injected into memory only via `--credential <profile>=env:<variable>` and cleared with the `CredentialScope` when the task ends; neither arguments nor values enter argv, logs or disk. Zero plaintext on disk / logs is asserted by the drill script (Round 3).
- `--cache <shared directory>` uses the `DirSharedStore` shared-directory CAS: the same input produces the same cache key on different workers, with a 100% second-run hit; object writes use a same-directory temporary file plus atomic rename. When the shared directory is unavailable, the worker warns and degrades to the project-local CAS while the build continues.
- Real cloud object storage (S3 / OSS) is not implemented yet; the extension point is implementing the `ObjectStore` trait and replacing the `CacheBackend` backend (see FARM-DEPLOY §6.2).

### 4.3 Publish pipeline

| Mechanism | Behavior |
|---|---|
| Exponential backoff | 5xx / timeout / 429 / platform throttling codes retry under `RetryPolicy` |
| Idempotency key | `Idempotency-Key` = blake3 of channel + appid + version + content digest (stable across processes) |
| SubmitJournal | Persists the submission intent before sending; records `Accepted` after acceptance; a crashed rerun resends with the same key |
| Platform deduplication | Native duplicate codes such as WeChat 85009 (version already under review) are treated as accepted and produce no second submission |
| Receipt | Receipt schema v2; contains submission id and timestamp for traceability |
| Rerun | `poly retry --from publish` reuses the same receipt and submission deduplication state |

Automation stops at "submission": it does not promise automatic approval; review results are tracked by polling and archived receipts.

## 5. Technical review report

### 5.1 Dependency direction and acyclicity

| Crate | Internal crates depended on |
|---|---|
| `poly-targets` | None (leaf of the dependency graph) |
| `poly-cache` | None |
| `poly-toolchain` | None |
| `poly-core` | `poly-cache`, `poly-targets` (optional `poly-toolchain`) |
| `poly-sign` | `poly-core` |
| `poly-publish` | `poly-core` |
| `poly-bundle` | `poly-targets` |
| `poly-plugin` | `poly-core`, `poly-sign`, `poly-targets` |
| `poly-cli` | All of the above |

Dependency direction is one-way: `poly-plugin` depends on core / sign / targets, while core, targets and sign do not depend on `poly-plugin`; there is no cycle. Cargo itself rejects cyclic dependencies, and this table was checked against each crate's `Cargo.toml`.

### 5.2 no_std boundary

- `poly-targets` defaults to `std`; with `--no-default-features` it builds as `#![no_std] + alloc`, and `TargetSpec` parsing / validation and the `TargetProvider` contract remain available (`PackageInput` / `PackageArtifact` are alloc-only types with no std-specific types).
- `poly-core` likewise keeps the engine layer and `JobSpec` under `--no-default-features` (`no_std + alloc`); std-only capabilities (toolchain, cache std surface, bridge, farm, pipeline, etc.) are gated behind the feature.
- `poly-plugin` depends on wasmtime and is always std; it does not enter bare-metal targets and exists only as a host-side provider.
- The builtin provider (`poly-bundle`) is std; the existing packagers it delegates to were not changed by T-M8-05.

### 5.3 wasmtime version and features

- The version is pinned to `=49.0.3`; the component model / C API evolve across minor releases, and the sandbox boundary does not accept drift.
- `default-features = false` enables only `component-model`, `cranelift`, `runtime` and `std`; async / GC / pooling / cache / profiling / WAT and other non-essential features are disabled to reduce size and compile surface.
- Egress reuses `ureq` (same version and features as `poly-sign` / `poly-publish`), disables the default rustls provider and uses the system trust store; redirects are disabled by the caller to prevent bypassing the domain allowlist.

### 5.4 Provider unification

- The `TargetProvider` contract lives in `poly-targets` (no_std): `describe` returns a `TargetSpec`, and `package` takes a common `PackageInput` and returns a `PackageArtifact`.
- Builtin targets are wrapped by `poly-bundle::BuiltinProvider` around the existing packagers; third-party plugins are bridged from wasm by `poly-plugin::PluginProvider`, with `describe` fixed at load time.
- `ProviderRegistry` registers and looks up by (target id, artifact format); CLI build / verify / package use the same path with no `if builtin` special-casing.
- One contract test suite runs against both the builtin Tizen `.wgt` and the wasm reference plugin; the unified Tizen entry produces bytes identical to the direct packager, with sha256 baseline `626f4a5988183c434aeeb32b1e28cbb9a300d5fe49eccf0b4bae07c1a14f1532`.

### 5.5 CI gates and review conclusion

| Workflow | Coverage |
|---|---|
| `rust-ci` | ubuntu / macos-14 / windows-2022: fmt, clippy `-D warnings`, workspace tests, UI adapter tests |
| `secret-scan` | Full-history scan on three hosts; any hit exits with code 1 |
| `docs-check` | Pax assets, diagram drift (when archify is available), bilingual structure, SVG XML |

Review conclusion: the M8 delivery surface matches the M8 tasks in [TASKS](TASKS.md); the plugin boundary (zero capabilities, SRI, signature, resource caps) has negative tests; provider unification did not change L1–L5 packager sources; real-run evidence for the farm and publish chains comes from the local Linux drill and fault-injection tests, while macOS IPA R3 and real cloud object storage remain environment slots / follow-ups (see §7).

## 6. "One-day new target" drill record

### 6.1 Execution conditions

- Date: 2026-10-18.
- Executor: an independent-context developer agent (not a human external developer).
- Target: `pocket-notes` (a custom plugin target).
- Environment: Linux host; Rust + `wasm32-unknown-unknown`; this repository's M8 closeout window; the first build included a cold compile.
- Total wall clock about 38 minutes: about 30 minutes of cold compile and about 2 minutes of actual plugin development.

### 6.2 Timeline

| Stage | Result | Evidence |
|---|---|---|
| Scaffold → template wasm | Success | 3m20s |
| Scaffold → custom contract PASS | Success | 11m32s |
| Scaffold → real CLI package | Success | 12m23s |
| Total wall clock | Success | about 38min (about 30min cold compile, about 2min development) |

### 6.3 Exit-code evidence

| Command | Exit code | Duration / artifact |
|---|---|---|
| CLI build (cargo build -p poly-cli) | 0 | 1203s |
| `poly plugin new pocket-notes` | 0 | — |
| `./build.sh` | 0 | 197s |
| `cargo test` (contract) | 0 | 379s |
| `poly plugin digest` | 0 | — |
| `poly build --target pocket-notes` | 0 | 3s, 92B |
| SRI negative (digest mismatch) | 1 | `E_PLUGIN_0006` |

### 6.4 Blockers and resolutions

| Blocker | Resolution | Status |
|---|---|---|
| Hardcoded template contract | Reduced to the single constant source `EXPECTED_FORMAT` / `EXPECTED_PACKAGE` in `contract/src/lib.rs`, and documented in the template README and this document | Fixed in this change |
| No `poly plugin check` command | Added `poly plugin check <dir>`: validates scaffold files, WIT sync with the host, and the artifact digest (a missing artifact is a warning only) | Fixed in this change |
| Registration example missing `sri` | SRI example added to the template README and `poly plugin new` output | Fixed (`52968b4`) |
| Minimal host project undocumented | Written into the template README and §2.4 of this document | Fixed (`52968b4` / this document) |
| digest banner / artifact copy / build output does not show describe | Recorded as a known gap; use `poly build --json` events or the artifact result to confirm | Known gap |

### 6.5 Conclusion

The drill completed in ≤ 1 day, meeting the M8 exit criterion. Actual plugin development took about 2 minutes, while total time was dominated by the cold compile (about 30 minutes); scaffold → custom contract PASS took 11m32s, and scaffold → real CLI package took 12m23s. Boundary of this conclusion: the executor was an independent-context agent rather than a human external developer, and no human usability study was performed; the SRI negative case covered only the digest-mismatch path, while the signature / trust-root chain is covered end to end by the supplementary step in §6.6 (2026-10-19, `tools/plugin-signature-drill.sh`, 8/8).

### 6.6 Signature / trust-root end-to-end step (rerun 2026-10-19)

The original drill covered only the SRI negative case. This step brings the signature / trust-root chain into the drill and is reproducible:

```bash
tools/plugin-signature-drill.sh
# equivalent direct command:
CARGO_TARGET_DIR=target cargo test -p poly-plugin --test signature
```

The cases use a real gpg keypair generated inside the test process, a one-shot `GNUPGHOME`, and an in-memory credential backend:

| Case | What it proves |
|---|---|
| `valid_signature_with_matching_sri_loads_and_runs` | A valid signature + SRI pass and the plugin instantiates and runs `describe` |
| `tampered_bytes_with_updated_sri_fail_signature` | Tampered bytes with a synchronized SRI update are still rejected by signature verification (`E_PLUGIN_0009`) |
| `missing_signature_is_rejected` | Signature required by config but absent → `E_PLUGIN_0009` |
| `wrong_public_key_is_rejected` | Trust root holds a different public key → `E_PLUGIN_0009` |
| `sri_mismatch_is_rejected_even_with_valid_signature` | SRI first: digest mismatch → `E_PLUGIN_0006` without entering verification |
| `unavailable_trust_root_is_reported_without_host_paths` | Unavailable trust root → `E_PLUGIN_0010`, with no host paths in the error |
| `errors_and_artifacts_leak_no_host_paths_or_private_material` | Errors and signature artifacts leak no host paths or private-key material |
| `config_references_trust_root_without_inline_secrets` | `polypack.toml` references the trust root only via `CredentialRef` |

Expected: `test result: ok. 8 passed; 0 failed`. Measured locally on 2026-10-19: 8/8 (test body 5.05s, excluding compilation). On a host without gpg or on Windows MSYS the tests print SKIP, and the wrapper prints SKIP instead of PASS.

Boundary: this step verifies the host load chain (SRI → signature verification → component parsing → instantiation); the CLI path that resolves the trust root from keyring / env and loads a real built artifact is not part of this step.

## 7. Known gaps and follow-ups

### 7.1 Gap list

| Gap | Impact | Handling / follow-up |
|---|---|---|
| No standalone `poly plugin check` | Authors must manually chain build / test / digest / build | Fixed: `poly plugin check <dir>` validates scaffold files / WIT sync / artifact digest in one command (a missing artifact is a warning only) |
| Template WIT and host WIT are not auto-compared | Interface upgrades must sync the template copy by hand | Fixed: `tools/check-docs.mjs` now byte-compares template WIT against host WIT; `poly plugin check` applies the same check to a project |
| Signature / trust root not exercised end to end in the drill | The drill covered only the SRI negative case | Fixed (2026-10-19): §6.6 `tools/plugin-signature-drill.sh` runs the chain for real, 8/8 (valid / tampered / missing / wrong key / SRI first / unavailable root / no leaks / config reference). Remaining: the CLI resolving the trust root from keyring / env and loading a real built artifact is not part of this step |
| macOS farm IPA R3 is an environment-slot SKIP | The Linux environment has no Xcode / codesign | Kept open per FARM-DEPLOY §9; replace after running on a macOS host |
| Real cloud object storage (S3 / OSS) not implemented | Shared-directory CAS only | `ObjectStore` trait extension point exists; local fallback when no object store |
| Garmin has no builtin iq packager | `poly build --target garmin` SKIP-with-print at the package stage | Recorded experimental gap; conditional task |
| 13 planned platforms have no TargetSpec yet | Marked "planned" in the 49-target table | Advance per ROADMAP; the generated table follows the registry |

### 7.2 Follow-up actions

| Action | Trigger | Acceptance |
|---|---|---|
| Human external developer trial | After GA publication | Record blockers and write them back to the docs |
| CLI trust-root carrier end to end | Next drill involving signed releases | keyring / env trust-root resolution + `poly build` against a real plugin artifact, pass and fail paths |
| macOS worker + real IPA run | A macOS host is available | FARM-DEPLOY §9 logs and artifacts archived |
| Cloud object storage backend | Large cross-machine cache needed | `ObjectStore` implementation + second-run hit |
| Garmin iq packager | Conditional task triggered | Artifact structure check and real-device evidence |

References: [README](../README.md) · [ROADMAP](ROADMAP.md) · [TASKS](TASKS.md) · [FARM-DEPLOY](FARM-DEPLOY.md) · [EXPERIMENTAL-TARGETS](EXPERIMENTAL-TARGETS.md) · 中文: [GA.md](GA.md)
