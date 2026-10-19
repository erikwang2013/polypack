# PolyPack Initiation and Planning: Incubation Items, Planned Platforms, and Gap Projects

> Status: everything below is **candidate / unscheduled** and is not a scheduling commitment. This document only proposes and plans; it does not modify `ROADMAP` / `TASKS`. Integration points are listed in §8.
>
> Sources: `docs/ROADMAP.md` §13 (incubation pool E1–E8), §14 (open questions); `docs/GA.md` §6 (drill record), §7 (known gaps); `README.md` platform matrix and the 49-target status table.

## 0. Conventions

- Task IDs and status: `T-M9-xx` does not collide with existing `T-M0`–`T-M8`, and each ID appears once; every item is candidate / unscheduled and enters a formal milestone only after its trigger is met and maintainers review it.
- Person-days are rough estimates (implementation, tests, docs) and exclude external waits such as NDAs, review, and device procurement.
- Evidence boundary: unverified capabilities are written as "unverified / environment slot"; SKIP is never written as PASS.
- Bilingual mirror: `docs/INITIATIVES.md` and `docs/INITIATIVES.en.md` keep identical H2 / H3 / table row counts.

## 1. Overview and priority

This document covers 22 candidates: 7 incubation items, 8 planned platforms, 2 open decisions, and 5 GA gaps; 55 candidate tasks, roughly 226 person-days.

| Batch | Meaning | Items |
| --- | --- | --- |
| P0 | Do now: zero-code or low-ambiguity work that directly closes GA gaps | M9-G5, M9-G1, M9-G2, M9-G3 |
| P1 | Clear prerequisites and value; can start within one iteration after the trigger | M9-E1, M9-E3, M9-E5, M9-G4 |
| P2 | Needs a design decision, external demand, or cost validation | M9-E2, M9-E4, M9-E6, M9-E7, M9-D1, M9-D2 |
| P3 | Close / conditional / NDA-gated | M9-P1–M9-P8 |

## 2. Incubation items E1–E7

> Source: `docs/ROADMAP.md` §13. Each item keeps its original promotion trigger and adds the full initiation elements; the incubation pool still does not occupy M0–M8 scheduling.

### M9-E1 · SBOM / build provenance

- Status: candidate · unscheduled.
- Trigger: the first compliance or supply-chain audit request; or a target channel requires SBOM / provenance.
- Why: `poly verify` only checks artifact structure and signatures; the `poly release` receipt only carries `manifest_digest` (`crates/poly-cli/src/release.rs`), with no dependency list or build origin; T-M0-11 only covers baseline scanning, so an audit cannot answer "which inputs and toolchain produced this artifact".
- Goals: generate SPDX 2.3 JSON, CycloneDX 1.5 JSON, and in-toto / SLSA-style provenance next to every artifact, bound to the five cache-key inputs (`CacheInputs`, `crates/poly-cache/src/digest.rs`) and builder identity; verifiable offline. Non-goals: SLSA L3 certification, Sigstore / Rekor integration, VEX generation.
- Design sketch: add a leaf crate (working name `poly-sbom`) or a `poly-core` submodule that reuses `CacheInputs` sources / lockfile / target_spec / toolchain / profile; add `--sbom` to `crates/poly-cli/src/verify.rs` and an optional `sbom_digest` to the `poly release` receipt; data model is `<artifact>.cdx.json`, `<artifact>.spdx.json`, `<artifact>.intoto.jsonl`; the provenance predicate records builder, invocation, materials, and subject. Alternatives: external `cargo-cyclonedx` + `syft` (non-hermetic, extra install), or SBOM only without provenance.
- Deliverables: design note; SBOM / provenance generator; `poly verify --sbom` and receipt field; bilingual usage docs and one real artifact sample.
- Risks and mitigations: format drift in supply-chain tools → pin schema versions and keep golden samples; manifest bloat → direct dependencies only with a size cap; mistaking generation for compliance certification → say so explicitly in docs.
- Decisions: SPDX or CycloneDX first; whether to add Sigstore; whether SBOMs ship with artifacts.
- First step: map the `CacheInputs` fields in `crates/poly-cache/src/digest.rs` to provenance materials and write a one-page design draft.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-01 | SBOM generator (SPDX + CycloneDX) | coder | — | 4 | Byte-identical across two runs; dependency set matches the lockfile for a real artifact |
| T-M9-02 | SLSA-style provenance | coder | T-M9-01 | 4 | Subject digest equals artifact digest; materials cover all five CacheInputs; offline verifier passes |
| T-M9-03 | verify / release / publish wiring | coder + tester | T-M9-01, T-M9-02 | 3 | `poly verify --sbom` emits all three files; receipt contains `sbom_digest`; missing artifact fails clearly |

### M9-E2 · monorepo / multi-app workspace

- Status: candidate · delivered (T-M9-04/05/06; code, tests, ADR, bilingual docs, and demo script).
- Trigger: the first multi-app repository user appears and shares one Rust core plus release process.
- Why: `PolypackConfig` (`crates/poly-core/src/config.rs`) is single-app (`[package]` / `[ui]` / `[core]` / `[targets.*]` / `[retention]`) with `deny_unknown_fields`; `Project` (`crates/poly-core/src/plan.rs`) has one root and no workspace members, app selection, or shared-cache semantics.
- Goals: one root config declares multiple app members, resolves JobSpec and artifact paths per app, and shares core source-tree cache keys. Non-goals: cross-app dependency graphs, Nx / Turborepo-level orchestration, changing single-app config meaning.
- Design sketch: root `[workspace] members` is implemented (each member owns its `polypack.toml`, id = final directory segment, unique, no root escape); `JobSpec.app_id` is optional and omitted for single-app jobs; `poly build --workspace` builds all members in declaration order and `--app <id>` builds one; the shared core compile key excludes the app id while the UI bundle key includes it; outputs/snapshots land in `.polypack/packages/<app>/<target>/` and `.polypack/artifacts/<app>/<target>/`; new `E_WS_0001..0004`. Integration points: `crates/poly-core/src/workspace.rs`, `plan.rs`, `job.rs`, `crates/poly-cli/src/build.rs`, `crates/poly-cache/src/digest.rs`. Alternatives: an external loop over invocations, or one repository per app (shared core cost is high).
- Deliverables: `[workspace]` schema and [ADR 0008](../decisions/0008-monorepo-workspace.md); member discovery and composite JobSpec semantics; CLI selector; two-app example (`crates/poly-cli/tests/workspace.rs` / `scripts/demo-workspace.sh`) and cache-hit tests; bilingual [WORKSPACE](WORKSPACE.en.md) usage docs.
- Risks and mitigations: schema breaks single-app compatibility → behavior is byte-identical when `[workspace]` is absent; config drift between members → shared inheritance plus explicit override table; cache key collisions → force app_id and member path into the key.
- Decisions: whether the root or per-app config is the source of truth; whether members may use different `core` crates.
- First step: draft the `[workspace]` schema and a minimal two-app example, and land config parsing before touching the pipeline.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-04 | `[workspace]` schema and ADR | architect | — | 3 | Single-app config unchanged; two-app example parses; ADR records alternatives and impact |
| T-M9-05 | Member discovery and composite JobSpec | coder | T-M9-04 | 8 | `--app` builds only that member; shared core has identical cache keys for identical input; artifact roots are isolated |
| T-M9-06 | CLI selector, example, and docs | coder + tester | T-M9-05 | 5 | `poly build --workspace` builds all members; two-app second run hits cache; bilingual docs reproduce |

### M9-E3 · Incremental release `poly release --changed`

- Status: candidate · partially delivered (T-M9-07/08 delivered 2026-10-20; the T-M9-09 CI example and measured savings are pending).
- Trigger: multi-target CI release time becomes a bottleneck, or a repository with ≥ 5 targets releases at least weekly.
- Why: `poly release` only accepts target / repo / credential / dry-run (`Args` in `crates/poly-cli/src/release.rs`) with no change detection; the existing no-op only compares a receipt `manifest_digest` inside one target, so CI still builds every target.
- Goals: skip unchanged targets based on cache keys or receipt digests and emit stable skipped events and exit codes, cutting multi-target CI time. Non-goals: incremental compilation, partial artifact upload inside one target, changing release receipt semantics.
- Design sketch: `--changed` is implemented: after `prepare`, compare the receipt `cache_key` (currently equal to `manifest_digest`) with a `manifest_digest` fallback, plus repo root and repo kind; a hit emits a distinct `release-skipped` event, does not write the repo, does not rewrite the receipt, and does not increment `runs`; `key_source` names the fallback path. `--since <ref>` returns `E_PLAN_0002` because release has no per-target source/package mapping, with a hint to use `--changed` or pre-filter `--target`. Integration points: `crates/poly-cli/src/release.rs`, `release/report.rs`, `release/tests.rs`, `poly-cli/tests/release.rs`. Alternatives: `git diff` detection (fragile across repos/generated files), mtime (unreliable).
- Deliverables: `--changed` and `--since`; receipt `cache_key` field; skipped JSON contract; CI example and measured savings.
- Risks and mitigations: missing cache-key input causes a missed release → single-source key construction plus change-input table tests; old receipts lack `cache_key` → explicit warning and digest fallback; skipped mistaken for a successful release → strictly separate skipped and published events.
- Decisions: use the cache key or the receipt digest as the primary key; whether `--changed` is on by default.
- First step: write a read-only prototype in `crates/poly-cli/src/release.rs` that prints the current key and receipt key per target.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-07 | Change detection and `--changed` | coder | — | 4 | One of two targets changes and only it releases; unchanged target emits skipped with exit code 0 |
| T-M9-08 | skipped event and receipt field | coder + tester | T-M9-07 | 2 | JSON contract stable; old receipts warn on fallback; published and skipped never mix |
| T-M9-09 | CI example and measured savings | tester + docs | T-M9-07, T-M9-08 | 2 | Second release on a 5-target repo saves ≥ 50% time; bilingual docs reproduce |

### M9-E4 · Production hot update / OTA channel

- Status: candidate · partially delivered (T-M9-10/11 delivered; the T-M9-12 real Tauri install / rollback drill is environment-gated).
- Trigger: the first users ask for online updates and the target platform allows a self-managed update channel.
- Why: the pipeline ends at publish; there is no manifest, signature, staged rollout, or rollback semantics after release. Mechanisms differ per platform (Tauri updater, Play In-App Updates, App Store phased release, mini-program version management), so one package cannot cover all.
- Goals: define a signed update manifest with channel / rollback semantics, first for self-hosted Tauri targets, plus a per-platform mechanism matrix. Non-goals: generic binary deltas, bypassing store review, promising self-update for every target.
- Design sketch: `poly update manifest --channel <name>` is implemented: validate channel/version/platform/base URL/pub_date, select a unique artifact and embed its Tauri `.sig`, emit a deterministic Tauri v2 `latest.json` (sorted keys, two spaces, trailing newline), produce a GPG detached signature with `GpgSigner`, and retain `<version>.json` / `.sig` for rollback; `poly update check` validates the schema and optionally verifies with a trust root. The mechanism matrix and source dates are in [OTA.en.md](OTA.en.md). Integration points: `crates/poly-cli/src/update.rs`, `update/manifest.rs`, `crates/poly-sign`. Alternatives: documentation only, or leave everything to vendor toolchains.
- Deliverables: mechanism matrix (delivered); update manifest schema and signature (delivered); real Tauri self-hosted install / rollback drill (environment-gated, not run); rollback runbook (delivered, see [OTA.en.md](OTA.en.md)).
- Risks and mitigations: signing key leakage → reuse the reference model and one-shot signing environment; uncontrolled rollout → manifest supports percentage/allowlist but only one channel first; store policy changes → matrix records source dates and does not hard-code.
- Decisions: which update channels GA supports; whether update signing reuses the release key; whether to offer a hosted manifest service.
- First step: research each platform mechanism and write the matrix with three categories: self-updatable / store-only / not updatable.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-10 | Per-platform update mechanism matrix | researcher | — | 3 | Covers Tauri / Play / App Store / mini programs / self-hosted; each row has an official source and date |
| T-M9-11 | Manifest schema, signing, and rollback design | architect | T-M9-10 | 4 | Versioned schema; signing and verification path explicit; rollback steps drillable |
| T-M9-12 | Tauri self-hosted channel prototype | coder | T-M9-11 | 6 | One real drill each for update detection, signature verification, update, and rollback |

### M9-E5 · Store metadata pipeline

- Status: candidate · implemented (T-M9-13/14/15; author guide in [STORE-METADATA.en.md](STORE-METADATA.en.md)).
- Trigger: after the first M2 / M3 store release works, listing copy or screenshots need multi-language maintenance.
- Why: `poly publish --materials` accepts one JSON file (`crates/poly-cli/src/publish.rs`, `PublishRequest::with_materials`) with no versioned directory, field-length validation, screenshot specs, or locale completeness checks; every release hand-assembles JSON.
- Goals: a versioned metadata directory in the repository, validated per platform / locale and deterministically packed into submission materials. Non-goals: generating screenshots, calling store listing APIs, replacing human review.
- Design sketch: `store/metadata/<platform>/<locale>.toml` is implemented (`schema = 1`; WeChat and Alipay rule sets, CJK counts as 2); validation reports every violation with stable `metadata.*` codes; `--metadata-dir` / `--check-metadata` / `--metadata-locale` are wired into `poly publish`, with `--materials` and `--metadata-dir` mutually exclusive; materials are key-sorted and their blake3(canonical JSON) digest is recorded in the receipt as `materials_digest`. Integration points: `crates/poly-publish/src/metadata.rs`, `flow.rs`, `archive.rs`, `crates/poly-cli/src/publish.rs`. Alternatives: external store CLIs, per-platform ad hoc JSON.
- Deliverables: metadata schema and validator (delivered); `poly publish` wiring (delivered); locale completeness report (delivered); bilingual author guide (delivered, see [STORE-METADATA.en.md](STORE-METADATA.en.md)).
- Risks and mitigations: platform field rules change → version the rule table and record sources; screenshot asset bloat → store paths and digests, not binaries; missing locales → fall back to the default locale with an explicit warning.
- Decisions: metadata source of truth in the repository or a CMS; whether screenshots may be linked externally.
- First step: pick 2 supported platforms, list their required listing fields and length limits, and land the schema first.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-13 | Metadata schema and validator | architect + coder | — | 3 | Required fields and length checks for 2 platforms; missing locale and overflow produce stable errors |
| T-M9-14 | publish wiring and deterministic packing | coder | T-M9-13 | 4 | `--check-metadata` failure blocks packaging; identical input yields the same material digest; receipt records it |
| T-M9-15 | Locale report and author docs | tester + docs | T-M9-14 | 2 | Report lists missing locales/fields; bilingual guide has a complete example |

### M9-E6 · GUI target matrix view / build history

- Status: candidate · partially delivered (T-M9-16/17 delivered 2026-10-20; the T-M9-18 GUI is deferred because `app/` is not tracked, [ADR 0007](../decisions/0007-external-and-ui-decisions.md)).
- Trigger: the M1 Tauri pipeline works and the `app/` console skeleton lands; or users explicitly ask for a visual build history.
- Why: README describes `app/` as the target shape of the poly-gui console, but the directory is not tracked in the repository; `BuildEvent` (`crates/poly-core/src/events.rs`) only goes to stdout / JSON Lines with no persistent history, so "when did this target last build successfully" is unanswerable.
- Goals: persist build events locally and show a target × recent-build matrix plus per-build details. Non-goals: multi-user server, cloud sync, remote farm scheduling UI.
- Design sketch: `HistoryRecord` (schema_version, build_id, ts, target, event, status, duration, artifact digest, error code) is written to `.polypack/history/events.jsonl`, rotating at 5 MiB / 10,000 records while keeping the newest records; paths are redacted to project-relative form before writing. `poly history [--target ID] [--limit N] [--json]` provides per-target aggregation; the GUI matrix/detail view (T-M9-18) waits for `app/`. Integration points: `crates/poly-core/src/events.rs`, `crates/poly-cli/src/build.rs`, `crates/poly-cli/src/history.rs`. Alternatives: parse CI logs (unreliable), SQLite (strong queries but too heavy for v1).
- Deliverables: history write and rotation (delivered); query / aggregation command (delivered); matrix and detail views (deferred); privacy note (path redaction, delivered).
- Risks and mitigations: event stream contains sensitive paths → redact before writing and keep relative paths only; unbounded history → count and size caps; GUI and CLI drift → one schema and golden samples.
- Decisions: JSONL or SQLite; default retention; whether absolute host paths are recorded.
- First step: define a persistable `HistoryRecord` schema in `crates/poly-core/src/events.rs` and share it between `poly build --json` and file writes.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-16 | Event persistence and rotation | coder | — | 3 | Every build appends one parseable record; rotation at the cap; path-redaction assertion |
| T-M9-17 | History query and matrix aggregation | coder | T-M9-16 | 5 | Returns the latest N statuses/durations/digests per target; empty history is not an error |
| T-M9-18 | poly-gui matrix and detail view | frontend | T-M9-17 | 8 | Matrix shows all built targets; clicking opens details; guidance shown when empty |

### M9-E7 · Pre-release device preview matrix

- Status: candidate · unscheduled; decision recorded: build-vs-buy research first (T-M9-19); no device-farm code at this stage ([ADR 0007](../decisions/0007-external-and-ui-decisions.md)).
- Trigger: M4 TV or M7 wearable smoke hits a device bottleneck, or store preview / real-device regression blocks releases.
- Why: `poly smoke` and platform structure checks can SKIP without devices, but there is no device pool, lease, install/launch screenshot, or log archive; the pre-release device matrix is fully manual.
- Goals: define a device pool and preview job model, then install, launch, screenshot, and collect logs on real devices / simulators with archived evidence. Non-goals: a full device-farm product, unattended review, promising devices for every target.
- Design sketch: extend the farm protocol (`crates/poly-cli/src/farm.rs`, `docs/FARM.md`) with `DevicePool` / `DeviceLease` and job kind `preview`; providers may be Firebase Test Lab, BrowserStack, AWS Device Farm, or self-hosted; artifacts go to `.polypack/preview/<target>/<device>/`. Integration points: `crates/poly-cli/src/smoke.rs`, `.github/workflows/tv-smoke.yml`, `.github/workflows/wear-smoke.yml`. Alternatives: pure manual QA, or cloud device farms without protocol integration.
- Deliverables: device pool model and protocol extension; preview CLI; real preview drill for 2 targets; cost and retention policy.
- Risks and mitigations: rental cost → on-demand single leases first, no standing pool; toolchain differences → isolate behind a provider adapter; screenshots may contain user data → run fixture apps only and clean up automatically.
- Decisions: self-host or buy a cloud device farm; evidence retention and access control.
- First step: research 3 cloud device farms' APIs, prices, and coverage, and produce a build-vs-buy conclusion.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-19 | Device farm research and cost model | researcher | — | 3 | API/price/coverage comparison for 3 vendors; includes self-hosted TCO |
| T-M9-20 | DevicePool / DeviceLease protocol extension | architect | T-M9-19 | 4 | Lease, timeout, and reclamation semantics explicit; compatible with farm protocol v1 |
| T-M9-21 | Two-target preview drill | coder | T-M9-20 | 6 | One Android and one TV device install/launch/screenshot; failures have logs and stable error codes |

## 3. Planned platforms (8 without a TargetSpec)

> Source: the 8 "planned" rows in the `README.md` 49-target status table; the single source of truth is the `planned` rows in the `PLATFORMS` list of `tools/gen-status-table.mjs`. Note: `docs/GA.md` §7.1 says "13 planned platforms", which disagrees with the 8 in README / the generator; reconcile before integration.
>
> Verdicts: **close** = remove from the planned list or mark retired; **conditional** = start only under an explicit trigger; **NDA-gated** = no code and no SDK in the repository without authorization.

### M9-P1 · Google Play Instant

- Status: candidate · unscheduled; verdict: close (retired, keep a row); see [ADR 0006](../decisions/0006-platform-decisions.md).
- Trigger: Google officially restores Instant Apps intake and the Instant API (no sign of this today).
- Current state: Google stopped accepting Instant Apps submissions in Dec 2025 and shut down Play services Instant APIs; README and `tools/gen-status-table.mjs` already say "discontinued", and there is no TargetSpec.
- Feasibility: not feasible; there is no submission channel.
- Blockers and prerequisites: service shutdown; no developer console entry; no review path.
- Recommendation: close. Keep historical links and invest no further packager work.
- Goals: keep the matrix honest and remove a dead plan; non-goals: implementing `instant-aab` for a discontinued platform.
- Design sketch: do not touch `crates/poly-targets/src/builtin.rs`; remove the planned row from `tools/gen-status-table.mjs` or mark it retired, and update the README 49-row count and tier counts (ROADMAP §11 item 4). Alternatives: keep "planned" without implementation (status quo), or move to a PWA / Web demo.
- Deliverables: closure decision record; synchronized generator and README count changes; CHANGELOG entry.
- Risks and mitigations: removing the row drifts the 49-target table and docs-check → regenerate once from the generator and run `node tools/check-docs.mjs`; users still search for the platform → keep a retired row with an explanation.
- Decisions: remove from the 49-row table or mark retired (affects TOTAL and tier counts).
- First step: archive the official shutdown notice as evidence and open an ADR deciding "remove vs retired".

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-22 | Shutdown evidence archive and closure ADR | researcher + maintainer | — | 1 | Official notice traceable; ADR concludes remove or retired |
| T-M9-23 | Conditional task: instant-aab if intake resumes | coder | T-M9-22 | 6 | Starts only after official restoration; AAB contains instant manifest and passes bundletool validation |

### M9-P2 · Snapchat Mini

- Status: candidate · unscheduled; verdict: close with quarterly review (next 2027-01-20); see [ADR 0006](../decisions/0006-platform-decisions.md).
- Trigger: Snap officially republishes verifiable Minis docs, SDK, and upload contract.
- Current state: the official docs entry returns 404, developers.snap.com has no Minis docs; npm `@snapchat/minis-sdk` 3.0.5 (2022) is marked deprecating minis; no zip / manifest / packaging contract can be verified.
- Feasibility: not feasible today; without a contract any packager is guesswork.
- Blockers and prerequisites: official docs, developer account, upload endpoint, and review process; all four are required.
- Recommendation: close; recheck the official entry quarterly and reopen only with a complete contract.
- Goals: avoid fabricating platform artifacts; non-goals: implementing a zip format based on community guesses.
- Design sketch: keep the planned row in `tools/gen-status-table.mjs` with a "quarterly recheck" note; if the contract returns, reuse the HTML5 static bundle path (see `facebook-instant-games` / `tiktok-minis` packagers) and add the Snap manifest. Alternative: emit a generic HTML5 bundle for self-hosting (without claiming Snap compatibility).
- Deliverables: quarterly recheck record; close-or-reopen decision record; trigger evidence checklist for the conditional task.
- Risks and mitigations: treating a 404 as permanent → set a recheck date; community packages mislead → docs state they are unverifiable; developer account unavailable → do not start the conditional task.
- Decisions: whether to keep it in the 49-row table (recommend keeping it marked as a closure candidate).
- First step: create the quarterly checklist (docs URL, SDK version, upload entry, review terms) and record today's 404 evidence.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-24 | Official contract recheck and evidence archive | researcher | — | 1 | Docs/SDK/upload/review each have a conclusion and date |
| T-M9-25 | Conditional task: HTML5 zip + manifest packager | coder | T-M9-24 | 5 | Starts only after all four contract items verify; structure self-check + deterministic zip |

### M9-P3 · LINE Mini App

- Status: candidate · unscheduled; verdict: conditional (deploy mode only, no TargetSpec); see [ADR 0006](../decisions/0006-platform-decisions.md).
- Trigger: a user clearly needs the LINE channel and accepts "hosted URL only, no local upload package".
- Current state: LIFF apps are registered by hosted URL in the LINE Developers Console (LINE Login channel → LIFF; the endpoint URL must be HTTPS without fragments); there is no official local upload format or LIFF config file.
- Feasibility: feasible as "deploy + config validation"; not feasible as platform artifact packaging.
- Blockers and prerequisites: LINE Login channel, LIFF app, HTTPS static hosting; no official CI upload API.
- Recommendation: conditionally support `poly deploy --target line-liff`, validating config and guiding deployment, without generating a "platform package".
- Goals: let LINE users reuse the Web build and config validation; non-goals: fabricate a LIFF config file or claim store acceptance.
- Design sketch: add `line-liff.toml` (liffId, endpoint, channelId, scopes) and `poly deploy line-liff --dir dist`; validate HTTPS / no fragment / required fields and print the manual Console steps; do not call an upload API. Integration points: a new subcommand in `crates/poly-cli/src/main.rs`, the Web static packager `crates/poly-bundle/src/webapp.rs`. Alternatives: documentation only, or generic static hosting.
- Deliverables: deploy mode design; config validator; end-to-end hosting drill (manual Console steps); docs.
- Risks and mitigations: deploy mode mistaken for a TargetSpec → separate naming in docs and commands; endpoint rules change → version the validation rules; no upload API → explicitly SKIP the manual step.
- Decisions: whether a non-TargetSpec deploy mode is accepted into the official support surface.
- First step: manually register a LIFF app with the existing Web build and record every Console step and failure point.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-26 | line-liff deploy mode design | architect | — | 2 | Explicitly no TargetSpec; config fields and manual steps complete |
| T-M9-27 | Validator and deploy dry-run | coder | T-M9-26 | 4 | Non-HTTPS / fragment / missing fields fail; dry-run is offline and prints steps |

### M9-P4 · Sailfish OS

- Status: candidate · unscheduled; verdict: conditional (needs sponsorship / device commitment); see [ADR 0006](../decisions/0006-platform-decisions.md).
- Trigger: a Sailfish user or sponsor commits test devices and a Jolla account.
- Current state: the existing `linux-rpm` chain only emits a desktop Tauri RPM (`crates/poly-bundle/src/packagers/linux.rs`); it lacks a Qt / Silica QML host, the Sailfish SDK cross toolchain (armv7hl / aarch64), and harbour packaging / signing.
- Feasibility: medium-high technically (RPM and Linux cross-compilation experience is reusable); market is niche.
- Blockers and prerequisites: Sailfish SDK, QML / Silica host template, harbour account and signing, a real device or emulator.
- Recommendation: conditional support; do not build without sponsorship.
- Goals: produce an installable harbour RPM that passes structure checks; non-goals: reuse the Tauri desktop shell and call it a Sailfish app.
- Design sketch: add a `ui/adapters/native/host/sailfish` QML / Silica shell; add a `sailfish-harbour` packager in `crates/poly-bundle` that reuses RPM writing but uses the harbour spec and desktop file; probe the Sailfish SDK in `poly-toolchain`; declare the TargetSpec only after packaging and validation pass. Alternatives: a standalone Qt app outside PolyPack, or pure Web.
- Deliverables: toolchain probe; QML host template; harbour RPM packager; contract tests and TargetSpec draft.
- Risks and mitigations: SDK access and licensing → probe locally only, never commit; device scarcity → the conditional task requires a device; maintenance cost → mark experimental until stable.
- Decisions: whether to accept an experimental, community-maintained target.
- First step: install the Sailfish SDK on Linux, cross-compile a minimal QML app, and record the toolchain paths.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-28 | Sailfish SDK / harbour toolchain probe | researcher | — | 3 | SDK path, cross triple, harbour commands, and licensing conclusion reproducible |
| T-M9-29 | QML / Silica host and RPM packager | coder | T-M9-28 | 8 | Minimal app produces a harbour RPM; structure assertions pass; SKIP-with-print without SDK |
| T-M9-30 | Contract tests and TargetSpec draft | tester + architect | T-M9-29 | 4 | Golden sample identical across two runs; rpmlint passes; TargetSpec fields backed by evidence |

### M9-P5 · Ubuntu Touch

- Status: candidate · unscheduled; verdict: conditional (needs sponsorship / device commitment); see [ADR 0006](../decisions/0006-platform-decisions.md).
- Trigger: an Ubuntu Touch user or sponsor commits test devices and an OpenStore release.
- Current state: the existing Linux deb chain cannot produce a `.click` (format, AppArmor, and click manifest all differ); it lacks Clickable / the Ubuntu Touch SDK and a WebView shell.
- Feasibility: medium technically; ecosystem and device base are small.
- Blockers and prerequisites: Clickable, UBports SDK, AppArmor profile, OpenStore submission process.
- Recommendation: conditional support; do not build without sponsorship.
- Goals: produce an installable `.click` that passes structure checks; non-goals: rename a deb and call it a `.click`.
- Design sketch: add a `ui/adapters/native/host/ubuntutouch` WebView shell; add a `clickable` packager in `crates/poly-bundle` (invoke Clickable or generate the click manifest + AppArmor); probe Clickable in `poly-toolchain`; declare the TargetSpec after a real install passes. Alternatives: PWA, Flatpak, Ubuntu deb.
- Deliverables: toolchain probe; WebView shell template; `.click` packager; contract tests and TargetSpec draft.
- Risks and mitigations: Clickable version drift → pin the version and keep golden samples; overly broad AppArmor policy → least-privilege template; missing devices → the conditional task requires a device.
- Decisions: whether to accept an experimental, community-maintained target.
- First step: install Clickable, build an empty `.click` from the official template, and record the manifest and AppArmor structure.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-31 | Clickable / UBports toolchain probe | researcher | — | 3 | Official template builds; manifest, AppArmor, and architecture conclusions reproducible |
| T-M9-32 | WebView shell and `.click` packager | coder | T-M9-31 | 8 | Minimal app produces a `.click`; structure assertions pass; SKIP-with-print without tools |
| T-M9-33 | Contract tests and TargetSpec draft | tester + architect | T-M9-32 | 4 | One real-device install and launch; golden sample identical across two runs; OpenStore steps documented |

### M9-P6 · PlayStation

- Status: candidate · unscheduled; verdict: NDA-gated (no code without authorization); see [ADR 0006](../decisions/0006-platform-decisions.md).
- Trigger: Sony grants authorized developer status and an NDA is signed, with the official SDK installable locally.
- Current state: L7 planned target; the official SDK requires authorization, is never committed, and is only probed at a local path; there is no TargetSpec.
- Feasibility: not feasible publicly; only implementable as a plugin inside an authorized environment.
- Blockers and prerequisites: NDA, devkit, SDK, certification process; any missing item means no start.
- Recommendation: NDA-gated; the public repository keeps only the plugin interface and a mock, never the SDK or platform package format.
- Goals: integrate packaging as a `target-spec/v1` plugin after authorization; non-goals: implement or distribute the vendor SDK in the public repository.
- Design sketch: the plugin implements `describe` / `package` (`crates/poly-plugin/wit/polypack-target.wit`), and the host probes a local `$SONY_SDK` path; SKIP-with-print without the SDK; the artifact format is defined by authorized documentation. Alternatives: no console platform, ship on PC / Web only.
- Deliverables: NDA prerequisite checklist; conditional plugin skeleton; mock contract tests; local path probe.
- Risks and mitigations: SDK leakage → `.gitignore` and secret-scan; public docs overreach → publish only already-public information; maintainers lack authorization → do not start.
- Decisions: whether to invest in the NDA and devkit cost.
- First step: assemble the NDA / developer application prerequisite checklist and state "no code without authorization".

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-34 | NDA checklist and SDK path probe | maintainer | — | 1 | Checklist covers authorization, devkit, SDK, certification; zero SDK files in the repository |
| T-M9-35 | Conditional task: target-spec plugin skeleton | coder | T-M9-34 | 8 | Starts only after authorization; mock contract passes; SKIP-with-print without SDK |

### M9-P7 · Xbox

- Status: candidate · unscheduled; verdict: NDA-gated (no code without authorization); see [ADR 0006](../decisions/0006-platform-decisions.md).
- Trigger: Microsoft grants authorized developer status and an NDA is signed, with the GDK installable locally.
- Current state: L7 planned target; the GDK requires authorization, is never committed, and is only probed at a local path; there is no TargetSpec.
- Feasibility: not feasible publicly; only implementable as a plugin inside an authorized environment.
- Blockers and prerequisites: NDA, devkit, GDK, certification process.
- Recommendation: NDA-gated; the public repository keeps only the plugin interface and a mock.
- Goals: integrate packaging as a `target-spec/v1` plugin after authorization; non-goals: implement or distribute the GDK in the public repository.
- Design sketch: the plugin implements `describe` / `package`, and the host probes a local `$GDK` path; SKIP-with-print without the GDK; the artifact format is defined by authorized documentation. Alternatives: no console platform, ship on PC / Web only.
- Deliverables: NDA prerequisite checklist; conditional plugin skeleton; mock contract tests; local path probe.
- Risks and mitigations: SDK leakage → secret-scan; public docs overreach → publish only already-public information; no authorization → do not start.
- Decisions: whether to invest in the NDA and devkit cost.
- First step: assemble the Microsoft authorization / NDA prerequisite checklist and state "no code without authorization".

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-36 | NDA checklist and GDK path probe | maintainer | — | 1 | Checklist covers authorization, devkit, GDK, certification; zero SDK files in the repository |
| T-M9-37 | Conditional task: GDK plugin skeleton | coder | T-M9-36 | 8 | Starts only after authorization; mock contract passes; SKIP-with-print without GDK |

### M9-P8 · Nintendo Switch

- Status: candidate · unscheduled; verdict: NDA-gated (no code without authorization); see [ADR 0006](../decisions/0006-platform-decisions.md).
- Trigger: Nintendo grants authorized developer status and an NDA is signed, with NintendoSDK installable locally.
- Current state: L7 planned target; NintendoSDK requires authorization, is never committed, and is only probed at a local path; there is no TargetSpec.
- Feasibility: not feasible publicly; only implementable as a plugin inside an authorized environment.
- Blockers and prerequisites: NDA, devkit, NintendoSDK, certification process.
- Recommendation: NDA-gated; the public repository keeps only the plugin interface and a mock.
- Goals: integrate packaging as a `target-spec/v1` plugin after authorization; non-goals: implement or distribute NintendoSDK in the public repository.
- Design sketch: the plugin implements `describe` / `package`, and the host probes a local `$NINTENDO_SDK` path; SKIP-with-print without the SDK; the artifact format is defined by authorized documentation. Alternatives: no console platform, ship on PC / Web only.
- Deliverables: NDA prerequisite checklist; conditional plugin skeleton; mock contract tests; local path probe.
- Risks and mitigations: SDK leakage → secret-scan; public docs overreach → publish only already-public information; no authorization → do not start.
- Decisions: whether to invest in the NDA and devkit cost.
- First step: assemble the Nintendo authorization / NDA prerequisite checklist and state "no code without authorization".

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-38 | NDA checklist and NintendoSDK path probe | maintainer | — | 1 | Checklist covers authorization, devkit, SDK, certification; zero SDK files in the repository |
| T-M9-39 | Conditional task: NintendoSDK plugin skeleton | coder | T-M9-38 | 8 | Starts only after authorization; mock contract passes; SKIP-with-print without SDK |

## 4. Open decisions

> Source: `docs/ROADMAP.md` §14. Both are decisions, not scheduled features; write an ADR first, then decide whether to invest in implementation.

### M9-D1 · Plugin marketplace distribution: official registry vs decentralized index

- Status: candidate · unscheduled; decision recorded: signed decentralized index ([ADR 0004](../decisions/0004-plugin-marketplace.md)).
- Trigger: at least 3 third-party plugins, or the first external request for "how to discover / install plugins".
- Why: ADR 0003 fixed the WASM sandbox and `target-spec/v1`, but plugins are distributed only as files (`crates/poly-plugin`, `crates/poly-cli/src/plugin_new.rs`) with no index, version resolution, revocation, or provenance display.
- Goals: choose a distribution model and define a verifiable index format. Non-goals: a paid marketplace, hosted plugin execution, or changing the SRI / signature trust chain.
- Design sketch: prefer a "signed decentralized index first" model: `index.json` contains schema and plugins[] (id, version, wit_version, sri, signature, url, revoked), and the index itself is signed with GPG/minisign; `poly plugin search/add` reuses `poly-sign`; the official registry is an optional curated mirror. Alternatives: an official registry (high governance and availability cost), a Git repository index (no signature semantics), an OCI registry (heavy ecosystem).
- Deliverables: ADR; index schema; `poly plugin add/search` prototype; revocation list and trust-root docs.
- Risks and mitigations: supply-chain poisoning → SRI + signature + revocation; index outage → local cache and multiple mirrors; unclear governance → the ADR names who can revoke.
- Decisions: official registry or decentralized index; who owns revocation; whether private indexes are allowed.
- First step: draft ADR 0004 comparing threats and operational cost, and settle signing and revocation semantics first.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-40 | Distribution model ADR and threat model | architect | — | 3 | Two-model comparison; signing, revocation, trust root, and governance conclusions explicit |
| T-M9-41 | Signed index and `poly plugin add/search` | coder | T-M9-40 | 6 | Index signature failure rejects; install reuses SRI + GPG; revoked entries refuse to load |
| T-M9-42 | Conditional task: official registry / mirror | coder + maintainer | T-M9-41 | 8 | Starts only if an official service is chosen; availability and revocation drills pass |

### M9-D2 · Remote farm: self-hosted only vs hosted service

- Status: candidate · unscheduled; decision recorded: self-hosted only for GA ([ADR 0005](../decisions/0005-farm-hosting.md)).
- Trigger: external users lack macOS / Windows build hosts and explicitly ask for hosting; or self-hosted deployment support becomes a burden.
- Why: farm v1 already has a protocol, isolation, shared cache, and deployment guide (`crates/poly-cli/src/farm.rs`, `docs/FARM.md`, `docs/FARM-DEPLOY.md`), but the ROADMAP principle says user code and keys are not hosted; hosting changes the trust boundary and business model.
- Goals: make a clear "self-hosted only" or "hosted service" decision with boundaries. Non-goals: this document does not commit to implementing hosting or change current self-hosted capability.
- Design sketch: recommend self-hosted only for GA; hosting becomes a separate project requiring tenant isolation, quotas, key custody, audit, data residency, and SLAs. If hosted, reuse farm protocol v1 and the worker pool and add a control plane; otherwise harden the self-hosted reference deployment. Alternatives: partner hosting, bring-your-own-cloud (BYOC).
- Deliverables: ADR; cost and threat model; one-page production deployment checklist if self-hosted; control-plane requirements if hosted.
- Risks and mitigations: hosting violates an existing principle → explicitly revise the ROADMAP principle and review it; key custody risk → store references only or use user-provided KMS; operational cost → do BYOC first.
- Decisions: whether to offer hosting; if so, who owns keys and compliance.
- First step: draft ADR 0005 listing the minimum trusted boundary for hosting and the reasons not to do it.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-43 | Farm model ADR and cost model | architect | — | 3 | Self-hosted / hosted / BYOC comparison; trust boundary and costs auditable |
| T-M9-44 | Conditional task: hosted control plane or self-hosted hardening | coder | T-M9-43 | 8 | Control plane only if hosting is chosen; otherwise a hardening checklist and drill |

## 5. GA §7 gap projects

> Source: `docs/GA.md` §7.1 / §7.2. These projects turn known gaps into acceptance-ready candidate tasks without changing GA's completion claims.

### M9-G1 · CLI distribution to crates.io / GitHub Releases

- Status: candidate · partially landed (GitHub Releases, the install script, optional secret-gated signing, and install-smoke have landed; the crates.io release audit, T-M9-45, is still open).
- Trigger: GA release; external users want `poly` without a Rust toolchain.
- Why: the root `Cargo.toml` version is `0.0.0`, `crates/poly-cli/Cargo.toml` declares the bin name `poly`, and the repository has `repository` metadata; previously there was no release workflow, so users could only build from source. `.github/workflows/release.yml` now builds all three hosts on a `v*` tag and attaches `SHA256SUMS.txt`; `scripts/install.sh` (mandatory SHA-256 + optional strict minisign verification), bilingual `docs/INSTALL`, secret-gated signing, and an install-smoke job have landed. The crates.io release audit (T-M9-45) is still open.
- Goals: provide verifiable crates.io installation and GitHub Releases binaries with checksums and signatures. Non-goals: Homebrew / Scoop / apt repositories in v1.
- Design sketch: first audit all publishable crates in dependency order with `cargo package` / `cargo publish --dry-run`; GitHub Actions builds linux x86_64 / aarch64, macOS x86_64 / aarch64, and Windows x86_64; publish `SHA256SUMS` and a GPG/minisign signature; the installer verifies before writing. The GitHub Releases leg has landed in minimal form (three-host build + `SHA256SUMS`); signing and the installer are still pending. Integration points: root `Cargo.toml`, `crates/poly-cli/Cargo.toml`, `.github/workflows/`. Alternatives: GitHub Releases only (partially landed), crates.io only (requires Rust).
- Deliverables: release inventory and crate order; cross-platform build workflow (landed); checksums (landed) / signatures (pending); installer and bilingual docs.
- Risks and mitigations: crate name / binary name `poly` collision → check ownership first and prepare a fallback name; version 0.0.0 cannot publish → bump to 1.0.0 and tag before the first release; supply chain → signatures plus reproducible-build notes.
- Decisions: publish to both crates.io and Releases; whether to rename on a binary collision.
- First step: run `cargo package --list` and `cargo publish --dry-run` to list crate metadata that blocks release.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-45 | Crate release audit and version policy | release | — | 2 | All publishable crates pass `--dry-run`; version and tag policy documented |
| T-M9-46 | Cross-platform Release workflow and signing | release + coder | T-M9-45 | 5 | Artifacts downloadable on all three hosts; SHA256SUMS and signature verify |
| T-M9-47 | Installer and docs | docs + tester | T-M9-45, T-M9-46 | 2 | Clean container install succeeds and runs `poly --version`; bilingual docs reproduce |

### M9-G2 · Human external developer trial protocol

- Status: candidate · protocol landed (T-M9-48); the ≥3 human trials in T-M9-49 are external.
- Trigger: after GA release; the first external developer is willing to participate.
- Why: the GA §6 "add a target in one day" drill was run by an independent-context AI agent and explicitly did not study human usability; there is no recruitment, informed consent, task script, observation record, or issue triage process. The protocol and templates now live in `docs/HUMAN-TRIAL.md` / `.en.md`; running them with humans cannot be done here.
- Goals: establish a repeatable human trial protocol and complete trials with ≥ 3 external developers. Non-goals: a formal usability lab, collection of unrelated personal data.
- Design sketch: the protocol covers recruitment criteria, informed consent, a 90-minute task script (scaffold → custom target → package), think-aloud notes, issue severity (blocker / friction / suggestion), and an anonymized report template; findings feed back into GA §7.2 and issues. Integration points: `docs/GA.md` §6 / §7.2, `.github/ISSUE_TEMPLATE`. Alternatives: a questionnaire only, or agent drills only (status quo).
- Deliverables: trial protocol; consent and record templates; ≥ 3 anonymized reports; fix list.
- Risks and mitigations: privacy → minimize collection and anonymize; sampling bias → cover different Rust / mobile backgrounds; blockers drowned out → set a fix SLA by severity.
- Decisions: whether trials are paid; whether reports are public.
- First step: write a one-page protocol and task script, run an internal rehearsal, and revise.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-48 | Human trial protocol and templates | docs | — | 2 | Includes consent, script, records, severity, anonymization; internal rehearsal passes |
| T-M9-49 | Run ≥ 3 trials and feed back | researcher + docs | T-M9-48 | 5 | 3 anonymized reports; blockers filed as issues; GA §7.2 update proposal written |

### M9-G3 · Environment-slot closure program

- Status: candidate · ledger / owner / validator landed (T-M9-50, T-M9-52); the real per-slot closure in T-M9-51 remains external.
- Trigger: now; close slots before any external claim of "verified".
- Why: GA §7.1 and FARM-DEPLOY §9 still leave environment slots open for macOS IPA, real S3 / OSS, keyring / Vault, and Garmin iq; without a unified ledger, SKIP-with-print is easily misread as passing. `docs/ENV-SLOTS.md` has landed (ENV-01..ENV-19, with Owner/Evidence columns), and `tools/check-env-slots.mjs` is wired into docs-check; real per-slot closure still needs external environments and evidence.
- Goals: every slot has an owner, missing capability, evidence path, closure criterion, and status; a SKIP without an owner may not enter release notes. Non-goals: fabricating environments or writing mock verification as real-environment verification.
- Design sketch: the ledger is built (slot id, target, missing capability, owner, evidence path, status) from GA §7.1, FARM-DEPLOY §9, and target notes; closure evidence is real logs plus artifact digests; optionally add `tools/check-env-slots.mjs` to validate the ledger and doc references. Integration points: `docs/GA.md` §7.1, `docs/FARM-DEPLOY.md` §9, `.github/workflows/`. Alternatives: issue tracking only (not auditable).
- Deliverables: environment-slot ledger (landed at `docs/ENV-SLOTS.md`); closure evidence for four slot classes; release checklist with no unexplained SKIP.
- Risks and mitigations: environment unavailable → name an owner and alternative, never fake PASS; ledger and docs drift → machine check; cost → order by impact, macOS IPA and real object storage first.
- Decisions: who owns each slot; whether to accept long-lived open slots with disclosure.
- First step: extract every slot from GA §7.1 and FARM-DEPLOY §9 into an owner / evidence table.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-50 | Slot ledger and owner assignment | maintainer | — | 1 | Every slot has owner, evidence path, closure criterion; none missing |
| T-M9-51 | Close slots (macOS IPA / S3 / keyring / Garmin iq) | coder + maintainer | T-M9-50 | 10 | Each slot has real logs and artifact digests; open slots disclosed |
| T-M9-52 | Ledger check and release gate | coder | T-M9-51 | 2 | SKIP without an owner fails the check; doc references agree |

### M9-G4 · MSI real-sample launch test

- Status: candidate · delivered (the first `windows-2022` run passed on 2026-10-20: `windows-msi-real` in 6m30s).
- Progress (2026-10-20): the real Tauri v2 sample `examples/windows-msi-app/`, the gated real-run test `crates/poly-bundle/tests/windows_msi_real.rs` (`POLY_MSI_REAL=1`, `POLY_TAURI_CORE_BIN`, `POLY_MSI_REAL_OUT`, codes `POLY_MSI_REAL_0001..0006`), and the rust-ci `windows-msi-real` job have landed; run 39308849058 passed, with MSI / install / launch / uninstall / residue evidence archived as CI artifacts. ENV-01 is now `CLOSED`.
- Trigger: Windows L1 claims installability; or the first Windows user reports an install problem.
- Why: `crates/poly-bundle/src/packagers/windows.rs` generates a WiX `.wxs` and calls candle / light, and comments suggest an `msiexec /i` smoke, but there is no evidence of install, launch, uninstall, and residue checks on a real sample.
- Goals: use a real Tauri sample on a clean Windows environment to install → launch → uninstall → check residue and archive evidence. Non-goals: MSIX / Store packaging, a multi-language installer matrix.
- Design sketch: add a sample project (suggest `tests/fixtures/windows-msi-app` or `examples/`), have a Windows runner build the MSI; `msiexec /i /qn` install, launch the process and check the window / exit code, `msiexec /x /qn` uninstall, and check install-directory and registry residue; archive logs and screenshots. Integration points: `crates/poly-bundle/src/packagers/windows.rs`, `.github/workflows/rust-ci.yml`. Alternatives: script structure checks only (status quo), manual testing.
- Deliverables: real sample project; Windows CI job; install / launch / uninstall / residue evidence; known-limitations doc.
- Risks and mitigations: CI runner has no desktop → use an interactive Windows runner session or VM; WiX version drift → pin the toolchain version; false residue positives → explicit allowlist and snapshot diff.
- Decisions: put the sample under `tests/fixtures` or `examples/`; whether the job becomes a required check.
- First step: run `candle` / `light` on a minimal Tauri app on a Windows runner and record artifacts and install logs.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-53 | Real Tauri sample and MSI build | coder | — | 3 | Windows runner produces an MSI; digest archived; build log traceable |
| T-M9-54 | Install / launch / uninstall / residue smoke | tester | T-M9-53 | 3 | All three steps pass; residue check concluded; failure paths have stable error codes |

### M9-G5 · Branch protection admin bypass (external process)

- Status: candidate · unscheduled; external action: repository owner must execute ([ADR 0007](../decisions/0007-external-and-ui-decisions.md)).
- Trigger: now; T-M2-12 already records `enforce_admins=false`.
- Why: required checks are configured, but admins can push directly and bypass secret-scan and other required checks; this is a process gap, not a code gap, and cannot be fixed inside the repository.
- Goals: make admins subject to the same rules (or an org-level ruleset) and leave verifiable evidence. Non-goals: changing CI code or the required-check set.
- Design sketch: enable a GitHub ruleset requiring PRs, required checks, no force push, and `enforce_admins=true`; the repository owner performs it; archive the screenshot / link in the T-M2-12 record. Alternatives: process convention only (status quo), org-level policy.
- Deliverables: rule screenshot / link; T-M2-12 status update proposal; a verification record where an admin direct push is rejected.
- Risks and mitigations: blocking emergency fixes → keep an audited break-glass process; insufficient permission → owner performs it; misconfiguration → rehearse in a test repository first.
- Decisions: whether to allow emergency break-glass; who holds the exemption.
- First step: the repository owner checks the current ruleset and `enforce_admins` in settings and captures evidence.

| Task | Title | Role | Depends | Days | Acceptance |
| --- | --- | --- | --- | --- | --- |
| T-M9-55 | Enforce admin constraints and archive evidence | maintainer | — | 1 | Admin direct push rejected; required checks enforced; evidence linked into T-M2-12 |

## 6. Recommended order

Ordered by "repair GA credibility first, then high-leverage engineering, then external dependencies"; everything remains candidate / unscheduled.

1. M9-G5 branch protection: zero code, immediately closes the T-M2-12 process gap.
2. M9-G1 CLI distribution: without an installable `poly`, external trials and GA lack an entry point.
3. M9-G2 human trial: calibrate priorities with real users instead of assumptions.
4. M9-G3 environment-slot closure: establish a trustworthy boundary for "what is verified".
5. M9-E3 `release --changed`: reuses the existing cache and yields the fastest CI payoff.
6. M9-G4 MSI real sample: closes the L1 Windows install evidence gap.
7. M9-E1 SBOM: clear compliance trigger and reuses cache keys.
8. M9-E5 store metadata: reduces manual multi-language release work.
9. M9-E2 monorepo: wait for the first user; write the schema / ADR first.
10. M9-E6 GUI history: depends on the `app/` skeleton; start with event persistence.
11. M9-E4 OTA: research and manifest design first; end-to-end after user demand.
12. M9-E7 device preview: high cost; do build vs buy first.
13. M9-D1 plugin marketplace ADR: settle governance before any index code.
14. M9-D2 farm model ADR: decision only, no hosting commitment.
15. M9-P3 LINE: support as a deploy mode when a LINE user appears.
16. M9-P4 Sailfish: only with sponsorship / device commitment.
17. M9-P5 Ubuntu Touch: only with sponsorship / device commitment.
18. M9-P2 Snapchat: quarterly recheck; close without a contract.
19. M9-P1 Google Play Instant: close immediately.
20. M9-P6 PlayStation: NDA-gated, no action without authorization.
21. M9-P7 Xbox: NDA-gated, no action without authorization.
22. M9-P8 Nintendo Switch: NDA-gated, no action without authorization.

## 7. Not now

- **M9-E2 monorepo**: no multi-app user; introducing `[workspace]` early expands the config compatibility surface.
- **M9-E4 OTA**: mechanisms differ widely and support burden is high; research only until user demand exists.
- **M9-E6 GUI / M9-E7 device preview**: `app/` does not exist yet and CLI `--json` is enough for automation; devices and cloud farms are costly, so keep structure checks and a manual matrix for now.
- **M9-D1 official registry**: too few plugins; even a decentralized index waits for demand.
- **M9-D2 hosted farm**: conflicts with "do not host user code and keys"; needs a separate business and compliance review.
- **M9-P1 Google Play Instant**: the platform is discontinued; no return on investment.
- **M9-P2 Snapchat Mini**: the official contract is unverifiable; any implementation is guesswork.
- **M9-P4 / M9-P5 Sailfish / Ubuntu Touch**: small ecosystems and high toolchain/device cost; require sponsorship.
- **M9-P6 / M9-P7 / M9-P8 console platforms**: high NDA and devkit barriers; no code without authorization.

## 8. Later integration points (no existing repository files modified)

- `docs/ROADMAP.md` / `ROADMAP.en.md` §13: add a "candidate / unscheduled, see INITIATIVES §2" link to the E1–E7 rows; keep the original promotion triggers.
- `docs/ROADMAP.md` / `ROADMAP.en.md` §14: add ADR links and recommended conclusions for M9-D1 / M9-D2 (without replacing the decision).
- `docs/GA.md` / `GA.en.md` §7.1 / §7.2: reconcile "13 planned platforms" with the 8 in README / `tools/gen-status-table.mjs`, and link each platform row to M9-P1–M9-P8; link the human trial to M9-G2, keyring / Vault, S3 / OSS, macOS IPA, and Garmin iq to M9-G3, and MSI to M9-G4.
- `README.md` / `README.en.md`: add this document's link to the notes of the 8 planned rows in `tools/gen-status-table.mjs`, then regenerate the 49-target status table; the platform matrix counts stay unchanged.
- `docs/TASKS.md` / `TASKS.en.md`: after T-M8-13, add an M9 candidate section containing T-M9-01–T-M9-55; mark everything candidate / unscheduled, totaling 226 person-days.
- `tools/check-docs.mjs` `PAIRS`: add `['docs/INITIATIVES.md', 'docs/INITIATIVES.en.md']`; if M9-P1 decides removal or retired, `tools/gen-status-table.mjs` must update `PLATFORMS`, `TOTAL`, `TIER_COUNTS`, and the README tier table (ROADMAP §11 item 4); this document does not change them.
- `docs/decisions/`: M9-D1 produces ADR 0004, M9-D2 produces ADR 0005, and M9-G1's release policy may add an ADR; `docs/ENV-SLOTS.md` does not exist when this document is written, and if another workstream lands it, M9-G3 uses it as the single source of truth and cross-links it; this document does not create that file.
