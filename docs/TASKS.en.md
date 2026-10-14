# PolyPack Task Plan

> Derived from the milestone breakdown in [ROADMAP.en.md](ROADMAP.en.md) §9; produced in parallel by a four-person planning group and merged after cross-reconciliation: **116 tasks**, all cross-milestone references verified in two passes (existence + description match, zero dangling), Chinese/English task IDs aligned one-to-one.
> Chinese original: [TASKS.md](TASKS.md). This file falls under the documentation sync convention (ROADMAP §11): task additions/removals must land in both files in the same commit, and drift is caught in CI by the structural checker delivered in T-M0-10.
> Extension directions (E1–E8) are incubated and intentionally absent from this task table — see [ROADMAP.en.md](ROADMAP.en.md) §13.
> Progress (2026-10-15): **M0 11/11** · **M1 delivered 15/15** (11 ✅; the matching CI legs for T-M1-02/05/06/07 are green — remaining tool-level notes live in the task entries) · **M2 in progress 3/15** (T-M2-04/05 ✅: adapter v0 + focus baseline, 36 tests; T-M2-15 ✅: bilingual FARM docs, checker extended to four pairs) · **CI green on all three hosts** (rust-ci#38654820649: ubuntu 201 / macos-14 194 / windows-2022 192 tests passed — the differences are exactly the per-host gating surface; docs-check and the ui test step are green too).

## Team roster

| Role | Responsibility |
|---|---|
| `core` | core and pipeline: poly-core / poly-targets / poly-toolchain / poly-cache |
| `packagers` | the packager matrix: poly-bundle formats (apk / ipa / hap / msi / dmg / wgt / ipk / deb / rpm / …) |
| `ui-adapters` | UI adapter layer: design system, platform render adapters, TV focus system |
| `platforms` | platform ports: SDK / toolchain integration, cross-compilation, native builds |
| `release-sec` | signing, publishing, and security: poly-sign / poly-publish / vault / plugin signing |
| `infra` | CI and infrastructure: host matrix, smoke tests, remote build farm |
| `docs` | documentation and diagrams: bilingual sync, diagram pipeline, usage docs |

## Totals, critical path, and scheduling notes

| Milestone | Tasks | Person-days | Notes |
|---|---|---|---|
| M0 Foundation | 11 | 18 | includes the doc-sync checker (delivered) and supply-chain checks |
| M1 Desktop & Web | 15 | 31.5 | no buffer (31.5d vs 30d capacity, 105%); artifacts unsigned until M2 |
| M2 Mobile | 15 | 38 | 101% loaded; includes the remote-farm and "initiate vs build" docs |
| M3 HarmonyOS & mini programs | 12 | 31.5 | 105% loaded; includes Quick App packaging and store verification |
| M4 TV family | 15 | 36 | includes the conditional T-M4-15 (Roku packager; 14 tasks / 33d if untriggered) |
| M5 Desktop distros & BSD | 11 | 25 | the BSD build-host decision lands in T-M5-09 and writes back to §8 |
| M6 Embedded & automotive | 13 | 36 | includes the no_std feature split (no new crate) |
| M7 Wearables | 11 | 29 | Wear OS is platform-level support (UI runtime beta); watchOS / Garmin are experimental |
| M8 Ecosystem GA | 13 | 38 | below the 45d target (bounded by task granularity), still inside ±20% |
| **Total** | **116** | **283** | 280d without the conditional task; ≈ 37.5 weeks at 1.5 FTE, matching the ROADMAP |

**Global longest dependency chain (16.5d)**: T-M2-01 → T-M2-02 → T-M2-06 → T-M2-07 → T-M2-14 → T-M3-11 (bridge generator → Android packaging chain → mobile contract goldens → mini-program contract tests).

**Scheduling notes (reconciliation output, recorded honestly):**

- M0–M3 are at or over capacity (18/15, 31.5/30, 38/37.5, 31.5/30 person-days) — accepted without compression; the critical path is 16.5d over a 25-working-day window, so parallelism is ample, but any slip directly squeezes an acceptance item (M2's "IPA via the farm", M3's "all three mini programs preview").
- M1 artifacts are unsigned until M2: macOS demos will show Gatekeeper / SmartScreen warnings (consistent with the signing chain arriving in M2).
- M4 hits the +20% estimate ceiling when the Roku conditional task triggers; T-M4-15 is the only schedulable-away item in M4/M5.
- Cross-milestone references passed two reconciliation passes: existence (113 tasks, zero dangling) and description match (the second pass fixed five guessed references in fragment-m4m5).

## How to use

- **Claiming**: filter by `Role`; order execution topologically by the `Deps` field; `T-<M#>-<NN>` works directly as an issue key (e.g. GitHub Issues).
- **Checking off**: append `✅` to the task heading when done, and update the milestone's trailing stats comment in the same change.
- **Changes**: task additions/removals and estimate adjustments must sync [TASKS.md](TASKS.md), per the ROADMAP §11 documentation sync convention.
## M0 Foundation (2 weeks)

> Prerequisite: none｜Exit criteria: `poly doctor` correctly reports missing local toolchains; the schema covers every L1 field; CI is green on all three hosts (ubuntu-latest / macos-14 / windows-2022); the doc-sync validator (diagrams / Pax / bilingual structural check) is enforced in CI.

### T-M0-01 · ✅ Cargo workspace skeleton and dependency-direction constraints

- Role: core
- Deps: —
- Estimate: 2d
- Deliverable: A Cargo workspace with 9 crate stubs (poly-cli / poly-core / poly-targets / poly-toolchain / poly-bundle / poly-sign / poly-publish / poly-cache / poly-plugin) plus a dependency-direction constraint check script.
- Acceptance: `cargo check --workspace` is green; introducing a reverse dependency by hand (e.g. poly-core depending on poly-bundle) makes the check fail and names the offending direction.

### T-M0-02 · Three-host CI with Rust build caching (workflow delivered; remote run pending billing)

- Role: infra
- Deps: T-M0-01
- Estimate: 2d
- Deliverable: CI workflows for ubuntu-latest / macos-14 / windows-2022 (fmt + clippy + test) with Rust build caching (cargo registry and target directories).
- Acceptance: all three hosts green; a second run restores the cache and its hit is recorded (build wall time clearly lower than a cold start).

### T-M0-03 · ✅ polypack.toml schema (serde + JSON Schema export)

- Role: core
- Deps: T-M0-01
- Estimate: 1.5d
- Deliverable: serde model for `polypack.toml` ([package] / [ui] / [core] / [targets.*] / [publish]), parse validation, and JSON Schema export.
- Acceptance: covers every key of the README example config; invalid config (unknown key, type error) fails with a stable error code; the exported JSON Schema validates the sample configs in the repo; round-trip serialization unit tests pass.

### T-M0-04 · ✅ TargetSpec DSL and target registry

- Role: core
- Deps: T-M0-03
- Estimate: 1.5d
- Deliverable: TargetSpec struct (all §4.3 fields: id / tier / hosts / runtime / bridge / ui / package / signing / verify / publish / experimental / notes) and the TargetRegistry::resolve interface.
- Acceptance: all fields parse and round-trip identically; an invalid tier (outside 1–7) or unknown runtime enum fails with an error code; resolving an unknown id returns a diagnostic instead of panicking.

### T-M0-05 · ✅ L1 seven-target TargetSpec declarations

- Role: platforms
- Deps: T-M0-04
- Estimate: 1.5d
- Deliverable: Complete declarations for the seven L1 targets (Android / iOS / HarmonyOS / Windows / macOS / Linux / Web): runtime, bridge, UI adapter, package format, signing reference, verifiers, publishing channels, notes — column-aligned with §3.2.
- Acceptance: all seven targets resolve through the registry; declarations match the §3.2 detail table column by column (comparison script or test); every target has at least one verifier and one publishing channel, and credentials appear only as references.

### T-M0-06 · ✅ JobSpec and the plan stage (DAG skeleton)

- Role: core
- Deps: T-M0-04
- Estimate: 1.5d
- Deliverable: JobSpec struct (§4.2: profile / targets / stages / cache_policy / secret references / artifact budget) and the plan stage (load polypack.toml → JobSpec), including the plan → probe → compile → bundle → package stage-graph skeleton.
- Acceptance: unknown targets, dependency cycles, and missing credential references all fail fast in the plan stage (before any compilation), each with a stable error code and fix hint.

### T-M0-07 · ✅ Diagnostics framework (error-code scheme + --json event stream)

- Role: core
- Deps: T-M0-06
- Estimate: 2d
- Deliverable: The `poly::E_<module>_<number>` error-code scheme (code + cause + fix hint), the §4.2 BuildEvent bus and its serialization, and the full `poly build --json` event-stream output.
- Acceptance: every failure path emits a stable error code and hint; `--json` output is consumable line by line by a JSON parser; event variant definitions cover the full §4.2 enum (M0 can at least emit Planned / Failed); error codes are locked by snapshot tests.

### T-M0-08 · ✅ poly doctor (probe report skeleton)

- Role: core
- Deps: T-M0-01
- Estimate: 1.5d
- Deliverable: The `poly doctor` command, the Probe trait (locate / install), and probe-report rendering (human-readable terminal + machine-readable).
- Acceptance: an environment item with no registered probe reports "not probed" instead of crashing; the report gives a ready / missing / version tri-state per toolchain; the exit code distinguishes "all ready" from "something missing".

### T-M0-09 · ✅ L1 host toolchain probes

- Role: platforms
- Deps: T-M0-08
- Estimate: 2d
- Deliverable: Probe implementations for L1-related tools: Android SDK/NDK, Xcode, HarmonyOS SDK, emsdk, WiX/NSIS, WebView2 runtime, Linux packaging toolchain (annotated by host applicability).
- Acceptance: `poly doctor` correctly reports missing/installed toolchains and versions on all three hosts (against a manual checklist); one failing probe does not abort the others; the all-missing scenario prints actionable suggestions.

### T-M0-10 · ✅ Doc-sync validator wired into CI (delivered)

- Role: docs
- Deps: T-M0-02
- Estimate: 1.5d
- Deliverable: A doc-sync validator wired into CI: 1) add a `--check` mode to `tools/render-diagrams.mjs` (re-render and compare against the on-disk SVGs, non-zero exit on drift, mirroring `tools/build-pax.mjs --check`); 2) run `tools/build-pax.mjs --check` (no Pax asset drift); 3) a new structural bilingual-doc check that, for each zh/en pair of README, ROADMAP, and the task-breakdown document (TASKS), verifies identical H2/H3 heading counts, identical task-ID sets, and identical Markdown table row counts (no full-text translation diffing).
- Acceptance: hand-editing `docs/assets/*.svg` without re-rendering makes CI fail and names the drifted files; changing a heading / task / table row on only one side of a doc pair makes the bilingual check fail and names the mismatch; the existing doc pairs (README, ROADMAP) pass as baseline; CI is green when every check passes.

### T-M0-11 · ✅ Supply-chain checks (cargo-deny / cargo-audit / lockfile policy)
- Role: release-sec
- Deps: T-M0-01, T-M0-02
- Estimate: 1d
- Deliverable: `cargo-deny` (licenses / vulnerabilities / duplicates) and `cargo-audit` wired into CI; committed `Cargo.lock` enforced; a first full-scan baseline archived as a report.
- Acceptance: CI fails when a violating dependency is introduced; the baseline report is archived as a build artifact; `cargo tree` shows no duplicate-version warnings.

## M1 Desktop & Web (4 weeks)

> Prerequisite: M0 (T-M0-03, T-M0-04, T-M0-05, T-M0-06, T-M0-07, T-M0-08, T-M0-09, T-M0-10)｜Exit criteria: the sample app packages for Windows / macOS / Linux / Web; second-run cache hit ≥ 90% on identical inputs; English-labeled diagrams merged and checked in CI (`poly dev` hot update is demo-only — the exit criteria do not depend on the advanced core hot-update injection form).

### T-M1-01 · ✅ Pipeline skeleton: Stage trait and DAG scheduling

- Role: core
- Deps: T-M0-05, T-M0-06, T-M0-07
- Estimate: 3d
- Deliverable: The Stage trait implementation framework (plan / probe / compile / bundle / package) and DAG scheduling (petgraph), with the compile and bundle(ui) branches running in parallel and converging at package.
- Acceptance: a mock target drives the full "two parallel branches → convergence" event sequence; when one branch fails, the other branches' statuses are summarized in the report; events and error codes follow the T-M0-07 conventions.

### T-M1-02 · Tauri pipeline: triple detection and core compile (Linux real-run; cross-host compile awaits the three-host CI)

- Role: core
- Deps: T-M0-09, T-M1-01
- Estimate: 3d
- Deliverable: Host → target-triple detection and the release compile stage for the core Rust crate (artifacts written to disk, events emitted, failures carry diagnostics).
- Acceptance: detected triples on all three hosts match rustc's actual targets; the sample core compiles into an inspectable artifact; a missing linker/SDK fails in the probe/compile stage with an error code and fix hint.

### T-M1-03 · ✅ Tauri bundling stage and bare artifacts (real Tauri binary accepted on Linux; Win/Mac layout contract-level)

- Role: platforms
- Deps: T-M1-02
- Estimate: 2d
- Deliverable: The bundling stage: inject the frontend dist into WebView resources and invoke Tauri bundling per host, producing format-agnostic bare artifacts (Windows executable + resources, macOS .app, Linux executable / AppDir input).
- Acceptance: the sample app yields a launchable bare artifact on all three hosts (prints the app version on start and exits cleanly; `--version` smoke substitutes where no GUI session exists); host runtime dependencies such as WebView2 are recorded in the artifact notes.

### T-M1-04 · ✅ Web pipeline (vite-driven + PWA manifest generation)

- Role: ui-adapters
- Deps: T-M1-01
- Estimate: 2.5d
- Deliverable: vite frontend build integration (invoke the build, produce dist), react-dom rendering baseline wiring, and PWA manifest generation (name / icons / theme color from polypack.toml; icons derived from Pax brand assets).
- Acceptance: the sample UI builds to dist; manifest structure and icon-size assertions pass; html-validate passes on the emitted HTML; build failures carry an error code and location info.

### T-M1-05 · poly-bundle: Packager trait and Windows packagers (MSI / NSIS) (implementation + contracts complete; real-machine run blocked: Windows host)

- Role: packagers
- Deps: T-M1-03
- Estimate: 2d
- Deliverable: The Packager trait (id / package(inputs, spec) → Artifact) plus the WiX MSI and NSIS Windows packagers.
- Acceptance: windows-2022 generates MSI and NSIS installers from the bare artifact; MSI table structure and installer-artifact structure assertions pass; the nightly install → launch → uninstall smoke passes.

### T-M1-06 · macOS DMG packager (implementation + contracts complete; real-machine run blocked: macOS host)

- Role: packagers
- Deps: T-M1-03, T-M1-05
- Estimate: 1.5d
- Deliverable: The DMG packager (.app → disk image with a drag-to-install layout).
- Acceptance: macos-14 generates a DMG that passes `hdiutil verify` and mounts/unmounts in a smoke check; non-macOS hosts run the contract assertions in mock mode.

### T-M1-07 · Linux packagers (AppImage / deb) (real deb with derived icons; AppImage single-file awaits appimagetool, lintian not installed)

- Role: packagers
- Deps: T-M1-03, T-M1-05
- Estimate: 2d
- Deliverable: The AppImage and deb packagers (desktop file, icons, dependency and metadata complete).
- Acceptance: on ubuntu-latest the AppImage passes a launch smoke; the deb passes `dpkg -i` install and uninstall smokes; the deb passes lintian (no error-level findings).

### T-M1-08 · ✅ Web static-site packager

- Role: packagers
- Deps: T-M1-04, T-M1-05
- Estimate: 1d
- Deliverable: The static-site packager (dist + manifest → a hostable static package including PWA assets), using the same Packager interface as built-in targets.
- Acceptance: serving the artifact from a plain static server makes it reachable; html-validate and manifest assertions pass; it matches the §3.2 Web row (static site + PWA manifest).

### T-M1-09 · ✅ poly-cache (blake3 CAS + local backend + object-storage trait)

- Role: core
- Deps: T-M1-01, T-M1-02
- Estimate: 3d
- Deliverable: The §5.2 cache key (source-tree digest ‖ dependency lockfile ‖ TargetSpec version ‖ toolchain version ‖ build parameters), content-addressed storage (blake3 + CAS), a local filesystem backend, an object-storage backend trait, and cache-hit statistics events.
- Acceptance: the cache key is sensitive to each of the five input classes (verified one by one in unit tests); a second run hits the cache (a Cached event with hit=true, build wall time clearly lower); an entry whose content does not match its digest is detected and treated as a miss.

### T-M1-10 · ✅ poly dev hot preview v0 (web host)

- Role: ui-adapters
- Deps: T-M1-01, T-M1-04
- Estimate: 2.5d
- Deliverable: The `poly dev` command: launch the web-host dev server; hot-update on UI changes; trigger recompilation and refresh injection on core (Rust) changes. Fallback plan: if the injection approach overruns, degrade to full-page reload and defer hot update (injection form) to M2+.
- Acceptance: editing a UI source file updates the browser without a manual refresh; editing core source recompiles automatically with the effect visible in the page (full-page reload is an acceptable degraded form); exiting leaves no orphaned child processes.

### T-M1-11 · ✅ poly init sample-app template

- Role: core
- Deps: T-M0-03
- Estimate: 2d
- Deliverable: `poly init` generating a minimal sample app: a Rust core crate + a React UI skeleton (using minimal @polypack/ui tokens) + polypack.toml (windows / macos / linux / web L1 target declarations) + an app icon asset based on Pax.
- Acceptance: right after `poly init` in an empty directory the config passes schema validation and `poly doctor` produces a toolchain report; signing is all references with no plaintext credentials; the README quick-start steps match what is generated.

### T-M1-12 · ✅ English-labeled diagrams (English-label JSON → docs/assets/en/*.svg)

- Role: docs
- Deps: T-M0-10
- Estimate: 2d
- Deliverable: English-label JSON (same structure as the existing Chinese JSON) + render-diagrams.mjs support for emitting `docs/assets/en/*.svg` + README.en.md / ROADMAP.en.md referencing the English diagrams.
- Acceptance: running `node tools/render-diagrams.mjs` generates all English SVGs and `--check` passes; Chinese and English diagrams share identical node structure (labels differ only by language); English diagrams keep the Pax badge in the bottom-right corner (`--no-brand` disables it).

### T-M1-13 · ✅ Four-target contract tests and M1 acceptance

- Role: infra
- Deps: T-M1-03, T-M1-04, T-M1-05, T-M1-06, T-M1-07, T-M1-08, T-M1-09, T-M1-11
- Estimate: 2.5d
- Deliverable: Contract tests for the sample app on all four targets (Windows / macOS / Linux / Web): packaging + verifiers + artifact structure assertions with golden comparison (mock mode every commit, real mode nightly), plus the second-run cache-hit-rate assertion.
- Acceptance: CI is green on all three hosts; artifact structure assertions pass on all four targets; second-run cache hit ≥ 90% on identical inputs (with statistics emitted in the build report).

### T-M1-14 · ✅ Pax branding rollout (CLI state expressions + app-icon derivation; real deb assets / MSI·DMG contract-level)
- Role: ui-adapters
- Deps: T-M1-05, T-M1-11
- Estimate: 1.5d
- Deliverable: (1) CLI banner and build-state expression mapping (success = happy / failure = crying / exception = shocked / needs-confirmation = puzzled / idle = blinking; text-and-color fallback on terminals without graphics); (2) icon derivation pipeline: `pax-mark.svg` → PNG / ICNS / ICO generated at build time, never committed; (3) poly-gui application and window icons.
- Acceptance: MSI / DMG / deb artifact icons are Pax-derived with size assertions passing; the CLI prints the matching expression on success and failure paths (text in fallback mode); every brand asset traces to the single source `tools/build-pax.mjs`.

### T-M1-15 · ✅ `poly clean` and retention policy
- Role: core
- Deps: T-M1-09
- Estimate: 1d
- Deliverable: `poly clean` (`--cache` / `--artifacts` / `--all`; dry-run by default listing reclaimable entries and space, `--yes` executes); `[retention]` config in `polypack.toml` (cache age/capacity caps, artifact build history); LRU auto-eviction on breach; a cleanup report (reclaimed-space stats).
- Acceptance: LRU eviction keeps builds reusing the remaining cache; `poly doctor` passes after `poly clean --all --yes`; dry-run performs no writes.

<!-- 统计: M0=11任务/18d, M1=15任务/31.5d -->
## M2 Mobile (5 weeks)

> Prerequisites: T-M0-01 (workspace), T-M0-02 (three-host CI), T-M0-04/T-M0-05 (TargetSpec DSL and L1 declarations), T-M0-06/T-M0-07 (JobSpec and diagnostics framework), T-M0-09 (toolchain probes), T-M1-05 (Packager trait), T-M1-09 (poly-cache), T-M1-11 (sample-app template) | Exit criteria: sample APK/IPA installs and signatures verify; zero keys in-repo (secret-scan green); a Linux host produces an IPA through the remote farm

### T-M2-01 · uniffi bridge generator (single interface source → Kotlin/Swift bindings)

- Role: core
- Deps: T-M0-01, T-M1-11
- Estimate: 3d
- Deliverable: workspace interface generator: from one Rust interface definition it emits Kotlin and Swift bindings (uniffi), versioned as `bridge/v1`; the generator structure keeps napi / wasm-bindgen / cffi outlets open (consumed by M3) so no bridge is ever hand-written four times.
- Acceptance: one command emits both binding sets for the sample core and both toolchains compile them; an interface change busts the library-level cache (cache key includes the interface digest); generated output matches the actual Rust export signatures (test assertion).

### T-M2-02 · Kotlin/JNI binding artifact (AAR)

- Role: core
- Deps: T-M2-01, T-M1-09
- Estimate: 2.5d
- Deliverable: the core compiled to JNI `.so` per ABI triple + generated Kotlin bindings, assembled into an AAR (minimal manifest/resources) as the input to Android packaging.
- Acceptance: the AAR loads in the Android contract test; Kotlin-side calls to the sample interface return the same results as the Rust unit tests; the AAR goes through the library-level cache and hits on the second run.

### T-M2-03 · Swift binding artifact (XCFramework)

- Role: platforms
- Deps: T-M2-01
- Estimate: 2.5d
- Deliverable: the core compiled to a device + simulator slice XCFramework (staticlib) + generated Swift bindings and modulemap.
- Acceptance: a minimal host project links it via xcodebuild on a macOS host; Swift-side calls to the sample interface match Rust output; slices are enumerable and verifiable (`lipo -info`).

### T-M2-04 · ✅ @polypack/adapter-native v0 (RN host + capabilities contract; 36 tests green)

- Role: ui-adapters
- Deps: T-M1-04, T-M1-11
- Estimate: 3d
- Deliverable: `@polypack/adapter-native` v0: the RN host assembly layer implementing the §6 RendererAdapter contract (`mount` / `platformStyle(tokens)` / `capabilities(): { focusNav, safeArea, gestures }`), with a native-module registration slot for bridge artifacts.
- Acceptance: the same page component tree renders in an Android RN host; `capabilities()` output matches TargetSpec/host capability; degradation paths are driven by the declared capabilities (no pointer → focus-ring branch enumerable; the full traversal graph waits for M4).

### T-M2-05 · ✅ adapter-native focus/navigation baseline (traversal graph + back-stack + key routing)

- Role: ui-adapters
- Deps: T-M2-04
- Estimate: 2.5d
- Deliverable: baseline focus model: minimal D-pad traversal graph, focus-ring rendering, Back-key focus-memory interface — a shared kernel for the M4 TV focus navigation system.
- Acceptance: the sample app completes one core task with the D-pad in an Android host (external keyboard/debug mode); focus order is stable (snapshot assertion); Back returns to the previous focus and no dead ends occur without a pointer.

### T-M2-06 · Android packager (AAPT2, Gradle-free)

- Role: packagers
- Deps: T-M2-02, T-M0-09, T-M1-05
- Estimate: 3d
- Deliverable: `poly-bundle` Android packager: AAPT2 compile/link for resources and manifest (ABI and locale trimming, reproducible flags) producing unsigned AAB/APK intermediates (the signing slot is wired by T-M2-09); no Gradle anywhere.
- Acceptance: `poly build --target android` emits both AAB and APK; AAPT2 link is warning-free; with `SOURCE_DATE_EPOCH` injected, a second run is byte-identical apart from signatures and timestamps (`poly verify --reproducible`).

### T-M2-07 · Android artifact assembly and verification (bundletool)

- Role: packagers
- Deps: T-M2-06
- Estimate: 2.5d
- Deliverable: bundletool emit-and-verify chain: `build-bundle` / `build-apks` assembly, `bundletool validate` verification, and a device install-smoke script.
- Acceptance: `bundletool validate` passes; the sample APK installs and cold-starts on an emulator/device; artifact structure assertions (manifest, ABIs, resource table) are emitted on the event stream.

### T-M2-08 · iOS packager (xcodebuild archive + exportArchive)

- Role: packagers
- Deps: T-M2-03, T-M2-04, T-M0-09, T-M1-05
- Estimate: 3d
- Deliverable: `poly-bundle` iOS packager: generates/reuses the Xcode project skeleton and produces an unsigned IPA via `xcodebuild archive` + `exportArchive` (export method configurable); macOS hosts only, host eligibility decided at the probe stage.
- Acceptance: the sample app emits an IPA on a macOS host; `plutil`/`xcrun` checks confirm Info.plist and architectures; provisioning profiles and certificates appear only as credential references (no plaintext).

### T-M2-09 · Android signing (apksigner + keystore reference)

- Role: release-sec
- Deps: T-M2-07, T-M2-11
- Estimate: 2d
- Deliverable: `poly-sign` Android signer: `CredentialRef` → keystore resolution → apksigner signing, with key material only in memory/temp dirs; `apksigner verify` runs automatically after signing.
- Acceptance: the sample APK passes `apksigner verify --print-certs`; a missing/wrong reference fails fast at the plan stage (stable error code + fix hint); a post-sign assertion shows no leftover key material.

### T-M2-10 · iOS signing and notarization (codesign + notarytool)

- Role: release-sec
- Deps: T-M2-08, T-M2-11
- Estimate: 2d
- Deliverable: `poly-sign` iOS chain: codesign (certificate/profile via credential references) + `notarytool submit --wait` + stapler; `codesign --verify` and `spctl` checks wired into the verify stage.
- Acceptance: the sample IPA notarizes and staples successfully; `codesign --verify --deep` and `spctl -a -vv` pass; the notarization receipt is archived into the submission-level cache record.

### T-M2-11 · Vault integration (keyring + Vault backend trait)

- Role: release-sec
- Deps: T-M0-06
- Estimate: 3d
- Deliverable: credential resolution layer: `CredentialRef` → backend chain (local keyring + Vault backend trait + CI memory injection); the sign stage pulls on demand and discards immediately; plaintext credentials in the repo fail the build.
- Acceptance: the same JobSpec resolves against both the keyring and Vault backends; credential values never appear in the event stream/logs (redaction assertion); `poly retry --from sign` reruns independently and re-pulls credentials.

### T-M2-12 · secret-scan (commit and build double-check)

- Role: release-sec
- Deps: T-M0-02
- Estimate: 2.5d
- Deliverable: built-in rule set (keystore/JKS, certificate private keys, API-key patterns) plus a commit hook and a CI job as double checks; a hit fails immediately with a fix hint (error-code scheme).
- Acceptance: crafted samples for every rule class are all caught; a full-history repo scan is green; secret-scan is a required check in CI (all three hosts).

### T-M2-13 · Remote-farm protocol v0 (JobSpec serialization + worker pull-and-execute)

- Role: core
- Deps: T-M2-08, T-M2-10, T-M1-09
- Estimate: 3d
- Deliverable: farm protocol v0: JobSpec serialization format + worker pull-execute-report loop (worker registration and capability declaration), with local and remote runs sharing one DAG implementation.
- Acceptance: a Linux host initiates an iOS-target build → a macOS worker produces the IPA and signature verification passes (the R3 acceptance path); worker-side failures return complete error codes and fix hints; cross-machine cache sharing is verified through the object-storage backend.

### T-M2-14 · Mobile CI integration and contract-test goldens

- Role: infra
- Deps: T-M2-07, T-M2-10, T-M1-13, T-M0-02
- Estimate: 2.5d
- Deliverable: CI contract-test jobs for the mobile targets (Android on ubuntu-latest, iOS on macos-14; mock mode every commit, real mode nightly) + golden artifacts committed with a comparison script.
- Acceptance: the CI matrix runs per §8 and mock mode is green; golden drift fails with a diff; nightly real mode passes and retains artifacts.

### T-M2-15 · ✅ Remote farm and build-origin documentation (bilingual FARM pair aligned 8/0/16, all internal links resolve; initiate-vs-build-site defined with the Linux→macOS IPA end-to-end example)

- Role: docs
- Deps: T-M2-13
- Estimate: 1d
- Deliverable: farm usage docs (R3 mitigation): an explicit "where you initiate / where it builds" split, farm deployment and worker registration, credentials pulled on demand only at the build site (same model as T-M2-11), and APK/IPA signature-verification guidance; bilingual README/ROADMAP mirror updates (§11).
- Acceptance: the Chinese and English versions are structurally aligned with working links; "initiate vs build" is defined with an end-to-end example in both (Linux initiates → macOS worker emits the IPA); the doc-sync check job (T-M0-10) stays green.

## M3 HarmonyOS & mini programs (4 weeks)

> Prerequisites: T-M0-09 (HarmonyOS SDK probe), T-M1-05 (Packager trait), T-M2-01 (bridge generator), T-M2-04/T-M2-05 (adapter-native and focus baseline), T-M2-11 (vault), T-M2-14 (contract-test golden infrastructure) | Exit criteria: all three mini programs preview in devtools; HarmonyOS HAP signs and installs; submission scripting is re-runnable

### T-M3-01 · RNOH host and NAPI bridge integration

- Role: platforms
- Deps: T-M2-01, T-M0-09
- Estimate: 3d
- Deliverable: HarmonyOS host project template (RNOH) + NAPI (C ABI) bridge integration: `.so` build chain (HarmonyOS triple) and napi binding registration, with bridge artifacts emitted by the T-M2-01 generator.
- Acceptance: the sample core loads as an NAPI module in the RNOH host; sample-interface calls match the Rust unit tests; `poly doctor` recognizes the HarmonyOS SDK and hints how to fix a missing one.

### T-M3-02 · adapter-native on the RNOH host (capability mapping)

- Role: ui-adapters
- Deps: T-M2-04, T-M2-05
- Estimate: 2.5d
- Deliverable: the RNOH host branch of adapter-native: component mapping plus HarmonyOS adaptation of `capabilities()`/token skinning; the page component tree stays untouched.
- Acceptance: the same sample page renders identically on the Android RN and RNOH hosts (structure-snapshot comparison); the focus/navigation baseline works under RNOH (external keyboard/debug mode).

### T-M3-03 · HAP packager (hvigor invocation chain)

- Role: packagers
- Deps: T-M3-01, T-M1-05
- Estimate: 2.5d
- Deliverable: `poly-bundle` HAP packager: project-config generation + hvigor build invocation + artifact collection, producing an unsigned HAP; build logs wired into the event stream.
- Acceptance: `poly build --target harmonyos` emits the HAP; the HAP structure check (manifest/module/resource-index entries) passes; the second run hits the cache.

### T-M3-04 · HAP signing and verification (hap-sign-tool)

- Role: release-sec
- Deps: T-M3-03, T-M2-11
- Estimate: 2.5d
- Deliverable: `poly-sign` HarmonyOS chain: hap-sign-tool signing (certificate/profile via credential references) + signature verification + HAP structure check wired into the verify stage.
- Acceptance: the sample HAP signs, installs and launches on a DevEco emulator/device; the verification command is green; credentials stay references end to end (no plaintext in-repo).

### T-M3-05 · @polypack/adapter-taro v0

- Role: ui-adapters
- Deps: T-M2-04 (capabilities contract and degradation pattern)
- Estimate: 3d
- Deliverable: `@polypack/adapter-taro` v0: the `mount` contract for React-syntax pages on the Taro renderer, mini-program-family capability degradation (no direct DOM access, lifecycle and routing mapping) and token skinning.
- Acceptance: the same page component tree renders in the Taro (WeChat target) devtools preview; degradation paths carry assertion tests; calls outside the host API allowlist are rejected at build time.

### T-M3-06 · Taro build pipeline integration (WeChat/Alipay/Douyin/Quick App targets)

- Role: packagers
- Deps: T-M3-05, T-M1-05
- Estimate: 2.5d
- Deliverable: `poly-bundle` mini-program packager: Taro compile configs for the four targets + bundle assembly, leaving the size/API-allowlist pre-check hook for T-M3-08.
- Acceptance: compile output exists for all four Taro targets (Quick App stops at the compile project; the `.rpk` is handled by the platform toolchain step); WeChat/Alipay/Douyin bundles open in their devtools; the second run hits the cache.

### T-M3-07 · wasm core into the mini-program sandbox (wasm-bindgen + API allowlist)

- Role: core
- Deps: T-M2-01
- Estimate: 3d
- Deliverable: wasm-bindgen output wired into the mini-program sandbox: an API allowlist (exposing only the sandbox-safe interface subset) and a load/init adapter shipped inside the bundle.
- Acceptance: the sample core executes the sample interface as wasm inside the mini-program sandbox; interfaces outside the allowlist are rejected at compile/load time (negative tests); the artifact has no native-module dependency.

### T-M3-08 · Size budget (budget.size) gate and wasm-opt trimming

- Role: core
- Deps: T-M3-06, T-M3-07
- Estimate: 2.5d
- Deliverable: the `budget.size` check wired into the DAG (configurable warn/fail policy) + wasm-opt optimization and per-target trimming (the R10 countermeasure).
- Acceptance: the sample app reports wasm and bundle sizes (event stream + terminal report); an artificial over-budget run warns with a locatable error code; before/after wasm-opt sizes are recorded in artifact metadata.

### T-M3-09 · WeChat mini-program publish client (upload + submit + status polling)

- Role: release-sec
- Deps: T-M3-06, T-M2-11
- Estimate: 3d
- Deliverable: the `poly-publish` WeChat Open Platform client: bundle upload → submit for review → review-status polling + receipt archiving (automation stops at submission, per R4).
- Acceptance: one full upload+submit cycle runs on a test account and the script is re-runnable (idempotent/deduplicated); polling states and receipt-archive format are fixed; credentials are injected via the reference model.

### T-M3-10 · Alipay / Douyin publish clients (shared-skeleton reuse)

- Role: release-sec
- Deps: T-M3-09
- Estimate: 2.5d
- Deliverable: the Alipay Open Platform and Douyin Open Platform publish clients: the WeChat skeleton reused, platform differences confined to client implementations.
- Acceptance: each platform's upload+submit+poll path runs; all three share a single Publisher interface (zero core forks); credentials are injected via the reference model.

### T-M3-11 · Mini-program contract tests (devtools CLI + golden comparison)

- Role: infra
- Deps: T-M3-06, T-M2-14
- Estimate: 3d
- Deliverable: one contract test per target: WeChat/Alipay/Douyin devtools CLI checks + bundle structure assertions + golden comparison, reusing the M2 contract-test and golden infrastructure; devtools login state is injected via credential references (same model as T-M2-11).
- Acceptance: all three contract tests are green; golden drift fails; CI runs the two §8 tiers (mock every commit, real CLI nightly); login state never reaches the logs (redaction assertion).

### T-M3-12 · Quick App packaging and store verification

- Role: packagers
- Deps: T-M3-06, T-M3-09
- Estimate: 1.5d
- Deliverable: Quick App chain closure: hap-toolkit build producing the `.rpk` + hap-toolkit lint wired into the verify stage; the alliance-store submission script reuses the T-M3-09 publish client skeleton.
- Acceptance: the sample app emits a `.rpk` and lint passes; the alliance-store submission script is re-runnable (idempotent, credentials via the reference model); consistent with the §3.3 Quick App row (`.rpk` → Quick App alliance stores).

<!-- 统计: M2=15任务/38d, M3=12任务/31.5d -->
## M4 TV Family (4 weeks)

> Prerequisite: M1 (packager matrix and content-addressed cache), M2 (Android/iOS packaging chains, uniffi bridge and keyring), M3 (wasm core and adapter baseline) | Exit criteria: focus traversal has no dead ends; wgt/ipk validators pass; Roku capability boundary reaches a conclusion (continue/downgrade)

### T-M4-01 · adapter-tv skeleton and 10-foot tokens

- Role: ui-adapters
- Deps: —
- Estimate: 2d
- Deliverable: New `@polypack/adapter-tv` package (`RendererAdapter` contract implementation: react-dom mount + 10-foot skin tokens — distance-readable type scale, spacing and safe area, tokens derived from the single source `@polypack/ui`).
- Acceptance: `capabilities()` reports `focusNav: true` and the TV safe area; the same example page component tree renders under both adapter-dom and adapter-tv, and token snapshot (golden) comparison passes.

### T-M4-02 · Focus traversal graph and focus ring

- Role: ui-adapters
- Deps: T-M4-01
- Estimate: 3d
- Deliverable: Directional-key traversal graph (focus neighbours derived from geometry, with explicit `focusOrder` overrides), focus ring rendering, and a static connectivity checker over the traversal graph.
- Acceptance: The connectivity checker reports 0 orphan nodes (no dead ends) for the example app; smoke passes for four-way movement and focus ring visibility.

### T-M4-03 · Focus memory, Back-key semantics and numeric-key shortcuts

- Role: ui-adapters
- Deps: T-M4-02
- Estimate: 2d
- Deliverable: Focus memory stack (return to previous focus), layered Back-key semantics (close overlay first → then pop focus memory → then exit app), and remote-control numeric-key shortcut mapping.
- Acceptance: Back pops sub-pages/overlays layer by layer and restores the previous focus; numeric shortcuts trigger their actions; contract tests cover all three Back layers and focus restoration.

### T-M4-04 · Android TV / Fire TV Leanback manifest and APK packaging

- Role: packagers
- Deps: T-M2-07, T-M2-09 (Android AAPT2 + bundletool + apksigner chain), T-M4-01
- Estimate: 3d
- Deliverable: New TV-variant Packager in `poly-bundle`: Leanback manifest (`android.software.leanback`, `android.hardware.touchscreen=false`, TV banner/icon) plus Android TV and Fire TV target declarations (Fire TV reuses the same APK chain with the Amazon Appstore channel).
- Acceptance: APK passes bundletool validate and apksigner verify; Leanback/TV-track manifest assertions pass in contract tests; size stays within `budget.size`.

### T-M4-05 · Android TV / Fire TV contract tests and focus-navigation smoke

- Role: infra
- Deps: T-M4-04, T-M4-03, T-M2-14 (contract-test and golden infrastructure)
- Estimate: 2d
- Deliverable: Contract tests for both targets (mock mode per commit, real mode nightly on emulators) plus focus-navigation smoke cases (launch → directional traversal → open detail → Back).
- Acceptance: Mock contract tests green in CI; nightly emulator smoke archives artifact structure assertions and logs; smoke includes a "complete the core task with no pointer" check.

### T-M4-06 · RN-tvOS host integration and focus wiring

- Role: ui-adapters
- Deps: T-M4-02, T-M2-04, T-M2-05 (RN host baseline)
- Estimate: 2d
- Deliverable: RN-tvOS branch of `@polypack/adapter-native`: tvOS remote event source wired into the focus-navigation system, with 10-foot tokens in effect under RN-tvOS.
- Acceptance: The example app launches on the tvOS simulator (macOS host), directional traversal and focus ring behave identically to adapter-tv; the core task is completed with no pointer.

### T-M4-07 · tvOS packaging chain (reusing M2 Xcode chain)

- Role: platforms
- Deps: T-M4-06, T-M2-08, T-M2-10 (iOS xcodebuild archive + exportArchive chain), T-M2-13 (remote-farm protocol v0)
- Estimate: 2.5d
- Deliverable: tvOS target declaration and packager: reuse the M2 Xcode packaging chain (tvOS SDK slice), merge the uniffi native staticlib with the RN-tvOS bundle, codesign via credential reference; Linux/Windows hosts package through the M2 remote farm.
- Acceptance: `poly build --target tvos` produces a TV package on a macOS host and `codesign --verify` passes; the same JobSpec initiated from a Linux host via the remote farm also packages successfully.

### T-M4-08 · Tizen wgt packager and wgt-lint verification

- Role: packagers
- Deps: T-M3-07 (wasm core build)
- Estimate: 3d
- Deliverable: Tizen target (web runtime): wasm core + react-dom UI packaged as `.wgt`, signed with the Tizen author certificate (credential reference), `wgt-lint` wired into the verify stage.
- Acceptance: `.wgt` passes `wgt-lint`; the signed artifact sideloads and installs on a Tizen emulator; size stays within `budget.size`.

### T-M4-09 · webOS ipk packager and ares-package verification

- Role: packagers
- Deps: T-M4-08
- Estimate: 2.5d
- Deliverable: webOS target: reuse the T-M4-08 wasm + react-dom UI artifact to produce `.ipk`, `ares-package` verification wired into verify; signing marked as platform-side (release signing).
- Acceptance: `ares-package` verification passes and the package installs on a webOS emulator; artifact digest matches the Tizen one from the same source (one UI artifact reused across targets).

### T-M4-10 · VIDAA / Vizio SmartCast app packages and sandbox self-checks

- Role: packagers
- Deps: T-M4-08
- Estimate: 2d
- Deliverable: Hisense VIDAA and Vizio SmartCast web-runtime target declarations and packagers (app packages), with sandbox self-check rules (entry file, manifest fields, size) wired into verify.
- Acceptance: Both targets package and pass the sandbox self-check; TargetSpec `notes` honestly state the boundaries such as no signing and no push.

### T-M4-11 · Roku SceneGraph subset restricted-renderer spike

- Role: ui-adapters
- Deps: T-M4-01
- Estimate: 3d
- Deliverable: SceneGraph subset renderer spike (the Roku core does not participate; UI subset plus native logic only): list of supported components and interactions, a minimal channel demo (zip sideload self-check), and a capability boundary document (supported / gaps / alternatives).
- Acceptance: The minimal demo sideloads on a Roku device or emulator and completes the core task subset; the capability boundary document lists gaps and downgrade paths and feeds directly into the continue/downgrade decision.

### T-M4-12 · Roku continue/downgrade decision and L3 capability-boundary doc sync

- Role: docs
- Deps: T-M4-11
- Estimate: 1.5d
- Deliverable: Decision record (continue building a packager or downgrade to a restricted target), plus §11-mandated sync of README.md / README.en.md and ROADMAP.md / ROADMAP.en.md L3 details and Roku's `experimental` / `notes` fields, with diagram JSON updated and re-rendered.
- Acceptance: The bilingual matrix and TargetSpec agree on Roku's tier, `experimental` flag and capability boundary description; `node tools/render-diagrams.mjs --check` passes.

### T-M4-13 · Remote-control smoke test suite (core task with no pointer)

- Role: infra
- Deps: T-M4-03, T-M4-04, T-M4-09 (web-family targets), T-M2-14 (contract-test and golden infrastructure)
- Estimate: 3d
- Deliverable: Reusable TV smoke framework: drive the example app through the core task using only directional keys + OK + Back + numeric keys (browse → detail → playback placeholder → return), covering Android TV emulators and the web family (Tizen / webOS emulators), emitting structured results with failure localization.
- Acceptance: The suite covers every landed TV target branch and passes in the nightly CI; any focus dead end on any target fails the run with localization details.

### T-M4-14 · HarmonyOS TV: HAP reuse and focus-adaptation smoke

- Role: platforms
- Deps: T-M3-01, T-M3-03, T-M3-04 (RNOH + HAP packaging and signing chain), T-M4-01
- Estimate: 1.5d
- Deliverable: HarmonyOS TV target adaptation check: the packaging chain reuses the M3 RNOH host and HAP packaging/signing (hap-sign-tool); this task only adds the TV side — TV resolution and safe-area verification plus wiring into the adapter-tv focus system.
- Acceptance: The HAP installs and launches on a HarmonyOS emulator or in the developer tools; directional focus traversal and the focus ring pass the smoke per adapter-tv behaviour; the TV safe area is unobstructed at the target resolutions.

### T-M4-15 · Roku channel package packager and verifier (conditional)

- Role: packagers
- Deps: T-M4-12
- Estimate: 3d
- Deliverable: Package the SceneGraph subset renderer output into a Roku channel package (zip) with sideload self-check, and ship the restricted-capability statement alongside the artifact; scheduled only after the T-M4-12 decision comes back "continue" (within the M5–M8 window).
- Acceptance: Executed only when the T-M4-12 spike decision is "continue"; when triggered, the channel package passes sideload self-check on a Roku device/emulator and the restricted-capability statement ships with the artifact and matches TargetSpec `experimental` / `notes`; when not triggered the task occupies no schedule and does not count toward milestone progress.

<!-- 统计: M4=15 tasks/36d (T-M4-15 is conditional at 3d; 14 tasks/33d if not triggered), M5=11 tasks/25d -->

## M5 Desktop Distributions and BSD (3 weeks)

> Prerequisite: M1 (first Linux desktop packagers, Web pipeline and PWA manifest), M2 (poly-sign and the credential-reference model) | Exit criteria: every format installs successfully on its host/container; BSD targets enter CI; `poly release --target fedora,freebsd` runs end to end and is re-runnable

### T-M5-01 · deb / rpm packager maturation

- Role: packagers
- Deps: T-M1-07 (Linux packager: AppImage/deb)
- Estimate: 2.5d
- Deliverable: Mature deb (control/Depends/maintainer scripts) and rpm (spec) packagers: per-distribution metadata (Ubuntu / Fedora / Linux Mint target declarations), lintian and rpmlint gates wired into the verify stage.
- Acceptance: All three distribution targets package and pass lintian / rpmlint (0 errors); `apt install` / `dnf install` succeed inside the matching containers.

### T-M5-02 · Flatpak packager

- Role: packagers
- Deps: T-M5-01
- Estimate: 2d
- Deliverable: Flatpak packager: manifest, AppStream metadata, least-privilege sandbox permission declarations, `.flatpak` artifact with verification wired into verify.
- Acceptance: The artifact passes `flatpak build` / lint and installs and runs in a container; declared permissions match `notes` (no redundant permissions).

### T-M5-03 · Snap packager

- Role: packagers
- Deps: T-M5-01
- Estimate: 2d
- Deliverable: Snap packager: `snapcraft.yaml`, confinement and interface declarations, packaging contract tests (artifact structure assertions).
- Acceptance: The artifact installs and runs via `snap install --dangerous` in a container; interface declarations match TargetSpec `notes`.

### T-M5-04 · AppImage packager maturation

- Role: packagers
- Deps: T-M1-07 (Linux packager: AppImage/deb)
- Estimate: 2d
- Deliverable: AppImage packager maturation: AppStream metadata, desktop entry and icon, reproducible builds via `SOURCE_DATE_EPOCH`.
- Acceptance: The artifact runs directly inside a container; AppStream metadata passes `appstreamcli validate`; `poly verify --reproducible` passes for the AppImage artifact (signature and timestamps excepted).

### T-M5-05 · Repository metadata generation and GPG signing

- Role: release-sec
- Deps: T-M5-01
- Estimate: 2.5d
- Deliverable: Generators for deb repository metadata (Packages / Release / InRelease) and rpm repository metadata (repomd), GPG signing through credential references (zero secrets in the repo); self-hosted repository layout and publishing notes.
- Acceptance: The generated repo is consumable by `apt update` / `dnf makecache` with signature verification passing; regeneration is idempotent (a second run is byte-identical except for signature timestamps); no plaintext keys in the repo (secret-scan green).

### T-M5-06 · FreeBSD pkg target (native build + pkg check + self-hosted repo signing)

- Role: platforms
- Deps: T-M5-05 (repository signing abstraction)
- Estimate: 2.5d
- Deliverable: FreeBSD target declaration and packager: native build (Tauri IPC, core as host), `pkg` artifact, `pkg check` wired into verify, self-hosted pkg repository signing (optional, reusing the T-M5-05 signing abstraction).
- Acceptance: `pkg install` succeeds inside a FreeBSD VM and the app launches; `pkg check` passes; the signed repo is consumable by `pkg update`.

### T-M5-07 · OpenBSD ports target (signify + portcheck)

- Role: platforms
- Deps: —
- Estimate: 2.5d
- Deliverable: OpenBSD target: ports directory skeleton generator (Makefile, PLIST, DESCR), signify signing, portcheck wired into verify.
- Acceptance: `make package` succeeds inside an OpenBSD VM and the result installs and runs; portcheck passes; signify signatures verify.

### T-M5-08 · BSD-family pkgsrc target (NetBSD / DragonFly)

- Role: packagers
- Deps: T-M5-06
- Estimate: 2d
- Deliverable: pkgsrc target declaration and packager (NetBSD / DragonFly and friends): `Makefile` + `PLIST` generation, repository signing and self-hosted repo layout reusing T-M5-06.
- Acceptance: `bmake package` succeeds inside a NetBSD VM and passes the packager self-check rules; the artifact lands in the self-hosted repo and installs.

### T-M5-09 · BSD CI approach finalized (VM/QEMU native builds and smoke)

- Role: infra
- Deps: T-M5-06, T-M5-07
- Estimate: 2d
- Deliverable: Finalized BSD CI approach (VM vs cross-compilation evaluation with a written conclusion plus implementation): native builds and install smoke inside VMs/QEMU in CI; §8 CI matrix and README / ROADMAP bilingual descriptions updated in the same change.
- Acceptance: FreeBSD / OpenBSD targets build and smoke-pass in the nightly CI; build logs and artifacts are archived and traceable; bilingual docs match the CI matrix.

### T-M5-10 · ChromeOS dual track (Crostini deb + PWA)

- Role: platforms
- Deps: T-M5-01, T-M1-04 (Web pipeline and PWA manifest), T-M1-08 (Web static-site packager)
- Estimate: 2.5d
- Deliverable: ChromeOS dual-track delivery: Crostini side reuses the deb packager (install smoke), PWA side reuses the Web pipeline + manifest (PWA audit rules wired into verify).
- Acceptance: The deb installs and launches inside a Crostini container; the PWA passes offline capability, manifest and installability audits; both tracks derive from the same JobSpec.

### T-M5-11 · `poly release --target fedora,freebsd` end to end

- Role: core
- Deps: T-M5-05, T-M5-06, T-M5-09
- Estimate: 2.5d
- Deliverable: Multi-target release orchestration: one command performs artifact upload, repository metadata refresh and signing, and receipt archival for fedora (rpm repos) and freebsd (pkg repos); shares the same DAG and event stream as the M1/M2 publish stage.
- Acceptance: End-to-end run is re-runnable (second run hits cache; repository metadata refresh is idempotent); the `--json` event stream contains Published events with receipt paths; failures emit stable error codes with fix hints.

<!-- 统计: M4=15 tasks/36d (T-M4-15 is conditional at 3d; 14 tasks/33d if not triggered), M5=11 tasks/25d -->
## M6 embedded & automotive (5 weeks)
> Prerequisites: M0 (TargetSpec DSL and crate dependency-direction constraints), M2 (Android packaging chain, reused by Automotive) | Exit criteria: every headless target (QNX / AUTOSAR / VxWorks / Zephyr / FreeRTOS) is in the CI compile matrix; QNX smoke passes

### T-M6-01 · Core layering: extract the no_std engine layer
- Role: core
- Deps: —
- Estimate: 3d
- Deliverable: a feature split inside `poly-core`, moving JobSpec validation, TargetSpec parsing and cache-key computation into an engine layer that compiles with std disabled (`no_std + alloc`) while host dependencies (tokio / petgraph / reqwest) stay behind the std feature; **no new crate is added**, and the shape decision is written back into ROADMAP §4.1 so the 9-crate table stays valid.
- Acceptance: `cargo build -p poly-core --no-default-features --target thumbv7em-none-eabihf` passes; in that build `cargo tree -p poly-core --no-default-features` shows no tokio / petgraph / reqwest; the M0 dependency-direction check stays green with the workspace still at nine crates; the cache key for one input is identical under std and no_std builds.

### T-M6-02 · Engine-layer C ABI and generated cffi headers
- Role: core
- Deps: T-M6-01
- Estimate: 2.5d
- Deliverable: a stable engine-layer C ABI (`poly_engine_*` prefix) plus C headers and static-library artifacts generated from a single interface definition (the §4.4 cffi bridge).
- Acceptance: headers compile under both `gcc -std=c11 -Wall -Werror` and `clang -std=c11`; `nm` shows no Rust-mangled symbols leaking from the exported surface; no second hand-written header exists in the repo (the generator is the only source).

### T-M6-03 · QNX cross-compile target and artifacts
- Role: platforms
- Deps: T-M6-02
- Estimate: 3d
- Deliverable: QNX TargetSpec (`runtime = "staticlib"`, `bridge = "cffi"`, `package = { format = "image" | "library" }`) with QNX SDP probing wired into `poly-toolchain`.
- Acceptance: `poly build --target qnx` produces both the static library and the image; `poly doctor` reports a missing local QNX SDP with an install hint; cross-compile smoke (via the T-M6-09 QEMU path) passes.

### T-M6-04 · AUTOSAR Adaptive target (ARXML + static library)
- Role: platforms
- Deps: T-M6-02
- Estimate: 3d
- Deliverable: AUTOSAR (Adaptive) TargetSpec (`runtime = "staticlib"`, headless, `no_std` optional) with an ARXML manifest plus static-library packager.
- Acceptance: the AUTOSAR target is green in the CI compile matrix; ARXML passes XML schema validation; service/port declarations map one-to-one onto exported C ABI symbols (asserted by script); `notes` states "headless, self-deployed".

### T-M6-05 · VxWorks target (library / image)
- Role: platforms
- Deps: T-M6-02
- Estimate: 2.5d
- Deliverable: VxWorks TargetSpec and a static-library/image packager (headless, cffi).
- Acceptance: the VxWorks target is green in the CI compile matrix; the artifact validates as a loadable module under the VxWorks host toolchain `ld` (no unresolved symbols); no absolute paths appear in the artifact (§5.3).

### T-M6-06 · Zephyr target (west + no_std)
- Role: platforms
- Deps: T-M6-02
- Estimate: 3d
- Deliverable: Zephyr TargetSpec (`runtime = "staticlib"`, `no_std`) plus a west project template (zephyr module with the engine-layer static library linked in).
- Acceptance: `west build` succeeds for both `qemu_x86` and `qemu_cortex_m3`; the engine layer links as a module and pulls in no std; the template's minimal `CONFIG_*` set is recorded by CI and versioned with the template.

### T-M6-07 · FreeRTOS target (static library + QEMU board)
- Role: platforms
- Deps: T-M6-06
- Estimate: 2.5d
- Deliverable: FreeRTOS TargetSpec and static-library packager plus a sample board project that runs in QEMU (reusing the T-M6-06 no_std integration).
- Acceptance: the FreeRTOS target is green in the CI compile matrix; the sample board boots in QEMU, successfully calls one engine-layer API (printing the version) and exits with code 0.

### T-M6-08 · CI compile matrix for headless targets
- Role: infra
- Deps: T-M6-03, T-M6-04, T-M6-05, T-M6-06, T-M6-07
- Estimate: 3d
- Deliverable: the §8 host × target-tier CI matrix extended so ubuntu-latest cross-compiles QNX / AUTOSAR / VxWorks / Zephyr / FreeRTOS on every run, with matrix entries derived from TargetSpec and toolchain images/probe caching in place (mitigating R9).
- Acceptance: all five targets are green on every commit; declaring a new target makes it appear in the matrix automatically (no CI-file edit); matrix cold start ≤ 15 min and warm start ≤ 5 min.

### T-M6-09 · QEMU smoke: QNX image
- Role: infra
- Deps: T-M6-03, T-M6-08
- Estimate: 3d
- Deliverable: a QEMU smoke script for the QNX image (boot → engine-layer self-check → exit-code assertion → serial log archival), wired into the nightly pipeline.
- Acceptance: the smoke passes in nightly CI; on failure it emits `Failed{stage, code, hint}` and the serial log pinpoints the failing self-check; `poly retry --from verify` re-runs that stage standalone.

### T-M6-10 · QEMU smoke: Zephyr
- Role: infra
- Deps: T-M6-06, T-M6-08
- Estimate: 2.5d
- Deliverable: a Zephyr smoke script (`west build -t run` plus exit-code assertion) sharing the log-archival format with T-M6-09.
- Acceptance: the smoke passes in nightly CI; the Zephyr and QNX smokes share one result-parsing implementation (no second decision path); failures map to named steps in the self-check script.

### T-M6-11 · Android Automotive target (Android chain reuse + OEM channel placeholder)
- Role: platforms
- Deps: —
- Estimate: 3d
- Deliverable: Android Automotive TargetSpec (`runtime = "native"`, `bridge = "uniffi"`, AAOS manifest variant) plus a placeholder Publisher for OEM channels.
- Acceptance: the M2 AAPT2 + bundletool + apksigner chain (no second Android packaging implementation) produces an AAOS APK that passes `bundletool validate`; the placeholder Publisher returns an explicit "OEM credential reference required" error instead of faking a submission; `notes` states "OEM channels, no unified public API".

### T-M6-12 · Embedded target status table (derived from TargetSpec)
- Role: docs
- Deps: T-M6-03, T-M6-04, T-M6-05, T-M6-06, T-M6-07, T-M6-11
- Estimate: 2.5d
- Deliverable: a status table for the L6 embedded / automotive targets (runtime, bridge, artifacts, signing, verification, channel, `experimental`, `notes`), generated from TargetSpec and synced into the bilingual README / ROADMAP (§11).
- Acceptance: every field in the table is reproducible from TargetSpec, with no second hand-written table in the repo; `node tools/render-diagrams.mjs --check` and the doc-sync check are green in CI.

### T-M6-13 · Artifact size budget and experimental-flag validation
- Role: core
- Deps: T-M6-08
- Estimate: 2.5d
- Deliverable: `budget.size` enforcement for headless targets (over-budget warning) plus an `experimental` field validator keeping TargetSpec, CI strength and the generated docs table in agreement.
- Acceptance: an over-budget build warns and never passes silently; an inconsistent flag fails CI and names the three conflicting sides; the check applies to all ten L6 targets.

## M7 wearables (4 weeks)
> Prerequisites: M2 (Android packaging chain, Apple signing chain and the "local or remote farm" decision) | Exit criteria: Wear OS on-device smoke passes; experimental targets (watchOS / Garmin) are fully documented (capability / gap / alternatives)

### T-M7-01 · `@polypack/adapter-wear` compact tokens
- Role: ui-adapters
- Deps: —
- Estimate: 3d
- Deliverable: `@polypack/adapter-wear` (compact tokens plus round/square safe areas) implementing `platformStyle(tokens) -> SkinnedTokens` and `capabilities()` (`safeArea` / `gestures`).
- Acceptance: the same page component tree stays inside the safe area at both round and square Wear sizes (assertions plus golden comparison); when `capabilities()` reports a missing capability the interaction degrades per contract (no pointer also means reusing the existing focus-ring path, not writing a new navigation system).

### T-M7-02 · Wear OS target declaration and Wear manifest
- Role: platforms
- Deps: T-M7-01
- Estimate: 2d
- Deliverable: Wear OS TargetSpec (`runtime = "native"`, `bridge = "uniffi"`, APK (Wear)) with a Wear manifest variant (`android.hardware.type.watch`); Wear OS is platform-level support, `experimental = false`, with the device-side boundary written into `notes`: "the UI runtime (RN on Wear) is beta; packaging and signing ride the Android base".
- Acceptance: the manifest verifier asserts the Wear feature and minimum API level; the M2 Android chain produces an APK that passes `bundletool validate`; `experimental = false` and the generated docs table shows that value as-is; the beta wording in `notes` reaches the wearable entries of the README / ROADMAP (no ※experimental mark on Wear OS).

### T-M7-03 · Wear OS packaging and form smoke suite
- Role: platforms
- Deps: T-M7-02
- Estimate: 3d
- Deliverable: a Wear packager (reusing the M2 Android chain, adding no second Android packaging implementation) plus the §8 "Wear form" smoke cases (input, swipe, back).
- Acceptance: the APK installs on a Wear emulator; the form cases are green in nightly CI; failures produce a report pinpointing the exact form step.

### T-M7-04 · Wear OS on-device smoke
- Role: infra
- Deps: T-M7-03
- Estimate: 2.5d
- Deliverable: an on-device smoke pipeline (install → launch → form cases → uninstall) with screenshot/log archival, required before releases.
- Acceptance: the smoke passes on a real Wear device or the official emulator; archived artifacts carry a timestamp and build digest for traceability; a failing smoke blocks the release (the release pipeline depends on this job).

### T-M7-05 · watchOS companion template generator (SwiftUI, experimental)
- Role: platforms
- Deps: —
- Estimate: 3d
- Deliverable: `poly init --target watchos` generating a SwiftUI companion template (core-API call stubs and WatchConnectivity notes), marked experimental per §3.7.
- Acceptance: the template builds via `xcodebuild` on a macOS host; it contains no plaintext credentials (secret-scan green); `experimental = true` reaches the generated docs table.

### T-M7-06 · watchOS packaging and signing chain
- Role: platforms
- Deps: T-M7-05
- Estimate: 2.5d
- Deliverable: watch bundle packaging (Xcode archive) and the codesign signing chain, reusing the M2 Apple host / remote-farm decision (the R3 path).
- Acceptance: the watch bundle is produced and `codesign --verify` passes; a Linux host can produce it through the remote farm; signing credentials appear only as references throughout (nothing plaintext in-repo).

### T-M7-07 · Fitbit OS packager (JS SDK)
- Role: platforms
- Deps: T-M7-01
- Estimate: 2.5d
- Deliverable: Fitbit OS TargetSpec (`runtime = "none"`, the core does not run on-device) with a Fitbit JS SDK app-package packager.
- Acceptance: the Fitbit CLI self-check passes; the artifact structure matches the golden; `notes` states "core does not run on-device, capability lives in JS" and reaches the docs table.

### T-M7-08 · Zepp OS packager (Zepp JS API)
- Role: platforms
- Deps: T-M7-01
- Estimate: 2.5d
- Deliverable: Zepp OS TargetSpec with a Zepp mini-app packager (Zepp JS API).
- Acceptance: Zepp devtools validation passes; the artifact installs on the emulator; its contract test (package + verify + structure assertions) runs in CI.

### T-M7-09 · Garmin Monkey C transpile spike (experimental)
- Role: platforms
- Deps: T-M7-01
- Estimate: 3d
- Deliverable: a Monkey C subset transpile spike (bounded language subset plus API allowlist) and a capability-boundary conclusion (continue / degrade), formatted like the M4 Roku conclusion.
- Acceptance: at least one sample within the subset produces a `.iq` package and passes the Connect IQ compiler; the conclusion is written into the experimental-target doc (capability / gap / alternatives); `experimental = true`.

### T-M7-10 · Experimental-target capability / gap / alternatives docs
- Role: docs
- Deps: T-M7-04, T-M7-05, T-M7-09
- Estimate: 2.5d
- Deliverable: three-part documentation (capability, gap, alternatives) for watchOS / Garmin, synced bilingually (§11); Wear OS is not an experimental target and is covered only by a boundary note in the same chapter: "the UI runtime (RN on Wear) is beta".
- Acceptance: every experimental target has all three sections, and each gap either has an alternative or an explicit "no alternative"; the `experimental` field agrees across docs and CI annotations (Wear OS reads `false`, and no ※experimental mark appears for it in the docs); the doc-sync check is green in CI.

### T-M7-11 · Wearable contract tests and goldens
- Role: infra
- Deps: T-M7-02, T-M7-07, T-M7-08
- Estimate: 2.5d
- Deliverable: one contract test per target for Wear / Fitbit / Zepp (package + verifier + artifact-structure assertions + golden comparison), running in mock mode on every commit per §8.
- Acceptance: all three contract tests are green on every commit; golden updates require an explicit `--update-golden` and show up in the diff; adding a wearable target means copying the same assertion skeleton.

## M8 ecosystem GA (6 weeks)
> Prerequisites: M0 (TargetSpec schema), M2 (remote-farm protocol v0, Apple signing chain), M6 / M7 (all L6 target declarations in place) | Exit criteria: a third-party developer adds a custom target and ships a package in under a day, following the docs

### T-M8-01 · `poly-plugin` sandbox capability model (wasmtime)
- Role: core
- Deps: —
- Estimate: 3d
- Deliverable: the wasmtime-host capability model in place: no network and no filesystem by default, with read/write directories and outbound domains granted explicitly per need (the §4.5 capability model).
- Acceptance: unauthorized network / filesystem access is denied with a stable error code; the grant list travels in the JobSpec and appears in the `poly build --json` event stream; the sandbox-escape negative test suite is green.

### T-M8-02 · Finalize the `polypack-target` WIT world and `target-spec/v1` versioning
- Role: core
- Deps: T-M8-01
- Estimate: 3d
- Deliverable: the finalized `world polypack-target` (`import inputs`, `export describe`, `export package`) plus the `target-spec/v1` versioning scheme (breaking changes go to a new version number) and the wit-bindgen-generated Rust skeleton.
- Acceptance: a reference plugin compiles against that world, instantiates and completes `describe` + `package`; non-v1 versions are rejected with a migration hint; the world definition matches §4.5 line by line (script-compared).

### T-M8-03 · Plugin SRI verification and signed artifacts
- Role: release-sec
- Deps: T-M8-02
- Estimate: 3d
- Deliverable: content-digest (SRI) verification for plugin artifacts plus a plugin release signing chain (keys as references), with secret-scan rules covering plugin artifacts.
- Acceptance: a digest mismatch refuses to load the plugin; a failed signature refuses instantiation; plugin artifacts carry no absolute paths, environment variables or debug symbols (§7 sanitization passes).

### T-M8-04 · Plugin template repo and scaffolding
- Role: core
- Deps: T-M8-02
- Estimate: 3d
- Deliverable: a `poly plugin new <id>` scaffolder plus a standalone template repository (Rust + WIT starter, `describe` / `package` examples, its own CI).
- Acceptance: from an empty repo to "plugin loaded by `poly` and completing one package run" takes ≤ 30 min (excluding dependency download); the template CI ships its own contract test and is green inside the template repo.

### T-M8-05 · Built-in targets and plugins share one interface (principle 6)
- Role: core
- Deps: T-M8-02
- Estimate: 3d
- Deliverable: built-in targets registered through the same `describe` / `package` interface (built-ins are official plugins), removing bypass branches from the registry.
- Acceptance: built-ins and third-party plugins go through one registration and invocation path (no `if builtin`-style branch in the code); the same contract-test suite is green for both classes; existing L1–L5 targets package byte-identically to before (signatures and timestamps excepted).

### T-M8-06 · Remote farm v1: concurrency and worker pool
- Role: core
- Deps: —
- Estimate: 3d
- Deliverable: the farm protocol v0 → v1 (concurrent scheduling, worker pool, task timeouts and retries) with JobSpec serialization keeping v0 workers interoperable.
- Acceptance: a multi-target concurrent build completes through the farm; a v0 worker can still join (version negotiation succeeds); task timeouts / disconnects emit a `Failed` event carrying a hint.

### T-M8-07 · Farm isolation and in-memory credential injection
- Role: release-sec
- Deps: T-M8-06
- Estimate: 3d
- Deliverable: worker-side isolation (container / process boundary plus a one-shot working directory) and a credential delivery channel that only ever touches memory (§7).
- Acceptance: no plaintext credentials exist on worker disk or in logs (scan assertions); credentials and the working directory are destroyed at task end; nothing survives across tasks (isolation negative suite green).

### T-M8-08 · Farm cache sharing (CAS object-storage backend)
- Role: infra
- Deps: T-M8-06
- Estimate: 3d
- Deliverable: the `poly-cache` object-storage backend shipped in its production shape, with multiple workers sharing one CAS and the §5.2 three-level cache reused across machines.
- Acceptance: second-run cache hit ≥ 90% inside the farm (matching the M1 acceptance wording); the same input yields the same cache key on different workers; the backend stays a trait and the local filesystem default still works (the farm degrades rather than fails without object storage).

### T-M8-09 · Remote farm deployment guide
- Role: docs
- Deps: T-M8-06, T-M8-07, T-M8-08
- Estimate: 2.5d
- Deliverable: a remote-farm deployment guide (topology, worker pool, isolation parameters, cache backend, vault integration), bilingual.
- Acceptance: following the guide, a macOS worker pool is deployed from scratch and one IPA is built from a Linux host (the R3 path); every command in the guide is copy-pasteable and appears in the CI rehearsal log.

### T-M8-10 · Store publishing pipeline GA: retries and backoff
- Role: core
- Deps: —
- Estimate: 3d
- Deliverable: the `publish` stage maturing: exponential-backoff retries, idempotent submission (re-submitting does not create a second submission) and platform rate-limit handling (mitigating R4).
- Acceptance: after injecting 5xx / timeout / rate-limit faults, the submission eventually succeeds with no duplicate submission record; unrecoverable failures emit a machine-readable `Failed{stage, code, hint}`; automation stops at "submit" and never promises automatic approval.

### T-M8-11 · Submission receipt archival and review-status polling
- Role: release-sec
- Deps: T-M8-10
- Estimate: 2.5d
- Deliverable: submission receipt archival (the §5.2 submission level of the cache) and review-status polling, landing the `Published{channel, submission}` event.
- Acceptance: once polling reaches a terminal state the receipt is persisted with submission ID and timestamp and is traceable; after an interruption `poly retry --from publish` resumes without creating a duplicate submission.

### T-M8-12 · 49-target status table (derived from TargetSpec)
- Role: docs
- Deps: —
- Estimate: 3d
- Deliverable: a status table for all 49 targets generated from TargetSpec (tier, runtime, bridge, artifacts, signing, verification, channel, `experimental`, `notes`), synced into the bilingual README / ROADMAP.
- Acceptance: the table matches TargetSpec field by field (generated, not hand-written twice); adding or removing a platform makes CI flag the pending updates per §11 item 4; all 49 rows are present and the L1–L7 tier counts match the README.

### T-M8-13 · GA docs, technical review and the "add a target in one day" rehearsal
- Role: docs
- Deps: T-M8-02, T-M8-04, T-M8-05, T-M8-09, T-M8-12
- Estimate: 3d
- Deliverable: the GA documentation set (bilingual) plus a technical review report and a rehearsal record from a developer outside the project adding a custom target and shipping a package.
- Acceptance: the rehearsal takes ≤ 1 day end to end (scaffolding, `describe` / `package` implementation, packaging and verification); every blocker hit is written back into the docs or fixed and re-tested; the GA doc-sync check is green and the 49-target table matches TargetSpec.

<!-- 统计: M6=13任务/36d, M7=11任务/29d, M8=13任务/38d -->
