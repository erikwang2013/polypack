# PolyPack experimental targets: capability / gap / alternatives

> Applies to `watchos` / `garmin` (`experimental = true`). Wear OS is platform-level (`experimental = false`); see §4 for its boundary.
> Single source of truth: `TargetSpec.experimental` in `crates/polypack-targets/src/builtin.rs`. The generated documentation status tables (README ×2 / ROADMAP ×2) and CI strength are checked for three-way consistency by `node tools/check-experimental.mjs`; this file and its Chinese counterpart are reconciled by `node tools/check-docs.mjs` (heading and table-row counts).
> Decision: both experimental targets **continue in constrained mode** — watchOS ships a SwiftUI companion template with the Apple packaging/signing chain; Garmin ships a build-time Monkey C subset emitter with a constrained packaging surface. Trigger prerequisites (compiler / on-device closure) are listed per section.

## 1. Classification and decisions

| Target | experimental | Decision | Trigger prerequisite |
|---|---|---|---|
| `watchos` | `true` | Continue (constrained): companion template + Apple packaging / signing chain | `xcodebuild` build + device walkthrough (macOS host) |
| `garmin` | `true` | Continue (constrained): Monkey C subset transpile spike | Connect IQ SDK compile + device / emulator walkthrough |

## 2. watchOS (companion template)

### 2.1 Capabilities

| Capability | Description | Evidence |
|---|---|---|
| Template generation | `polypack init --target watchos` emits a minimal buildable SwiftUI watch app: `@main` entry, ContentView, CoreBridge stub, WatchConnectivity skeleton, Info.plist (WKApplication), Xcode project + shared scheme | `crates/polypack-cli/templates/watchos/`; TargetSpec `watchos` |
| Packaging | `watchos-xcodebuild`: `xcodebuild archive` → `ditto` → `export/Payload/PolyPackWatch.app`; `plan()` is pure assembly and runs on any host | `crates/polypack-bundle/src/packagers/watchos.rs` |
| Signing / verification | Reuses the M2 Apple chain (`apple-codesign`, codesign hardened runtime + timestamp); `codesign --verify --deep --strict` + `spctl --assess`; credentials appear only as CredentialRef | TargetSpec verify; template README |
| Companion messaging | WatchConnectivity skeleton with three channels (`sendMessage` / `transferUserInfo` / `updateApplicationContext`); core calls are proxied by the iOS companion app | `WatchConnectivity.swift`; `CoreBridge.swift` |
| Cross-host submission | On Linux / Windows the package stage reports `ToolMissing` (`polypack::E_BND_0001`) and points to `polypack farm submit --target watchos` (R3 macOS worker) | `packagers/watchos.rs`; template README |

### 2.2 Gaps and alternatives

| # | Gap | Impact | Alternative (or "no alternative") |
|---|---|---|---|
| W1 | Watch side has `runtime = none`; the core does not run on the watch | Cannot call the Rust core directly; watch UI logic cannot reuse RN | Proxy via the iOS companion app + WatchConnectivity; "run the core directly on the watch" has no alternative |
| W2 | The template's CoreBridge is a stub (returns placeholder status); real core calls are not wired | Out-of-the-box end-to-end calls are incomplete | Follow the three-channel notes in `WatchConnectivity.swift` and integrate the iOS companion app; auto-generating real calls has no alternative (the app must provide an iOS core) |
| W3 | Build / signing toolchain ships only with Xcode (macOS) | Linux / Windows cannot produce a local bundle | Remote farm macOS worker (R3); local non-macOS packaging has no alternative |
| W4 | The template is watch-only and does not include an iOS companion app project | Pairing requires manual integration into an iOS project | Remove `WKWatchOnly` and add `WKCompanionAppBundleIdentifier` (template README); generate the iOS project via the M2 chain and pair manually; auto-generating the companion project has no alternative |
| W5 | This repository's CI (Linux) has no `xcodebuild` / `codesign` evidence | Packaging / signing / device behavior are not closed in this repository | Run on a macOS host or the remote farm; closing it inside this repository has no alternative (requires macOS) |
| W6 | The template UI is SwiftUI; watchOS has no native React runtime | React pages cannot run as-is | Native SwiftUI subset + core API proxy; "React on watchOS" has no alternative |
| W7 | Certificates / provisioning profiles must come from the system keychain / Vault / CI injection | Signing is impossible without credentials | CredentialRef references (`POLYPACK_IOS_IDENTITY` / `POLYPACK_IOS_TEAM_ID` are non-secret configuration); plaintext credentials have no alternative (never committed) |

### 2.3 Conclusion

Continue (constrained). `experimental = true` stays; the capability boundary ships with the template README and this section. Trigger prerequisite: complete an `xcodebuild` build and one device walkthrough on a macOS host (or the remote farm); do not claim "device-verified" before that closure.

## 3. Garmin (Monkey C subset transpile)

### 3.1 Capabilities

| Capability | Description | Evidence |
|---|---|---|
| Build-time transpile | Wear page model (tokens / layout boxes / focus order) → Monkey C source; `runtime = none`, no JS runtime | `ui/adapters/garmin/src/page.js`, `emit.js`; capability-boundary draft §1 |
| UI subset | View / card / text / Pressable composite (background + focus ring + label); colors / spacing / safe area share adapter-wear tokens; round 454×454 and square 320×320 reference viewports | Draft §2 tables A/C |
| Interaction subset | Three keys UP / DOWN / SELECT / BACK; focus order = the shared focus kernel's `engine.order` linear cycle; Back history mirrors the kernel | Draft §2 table B |
| Connect IQ project | `manifest.xml` + `monkey.jungle` + `resources/strings/strings.xml` + `source/*.mc`; reference devices `venu2` / `venusq`; app id = first 32 hex of `sha256(identifier)` | Draft §2 table D |
| Verification | 29/29 Node tests plus the built-in Rust `.iq` structure check (root `manifest.xml` + ≥1 per-device `.prg`); the `iq-structure-check` verifier is wired into `polypack verify`; `npm run build` really runs the structure self-check | Draft §5; `ui/adapters/garmin/test/`; `crates/polypack-bundle/tests/packagers_garmin.rs` |
| Compiler leg (when environment is ready) | The Rust packager `crates/polypack-bundle/src/packagers/garmin.rs` resolves `<project>/garmin` (or the host root) and runs `monkeyc -e -w -r -o out.iq -y <devkey> -f monkey.jungle` → `.iq` (`manifest.xml` + per-device `.prg`); devkey path only (`CIQ_DEVKEY`) | Draft §2 table D, §5; `crates/polypack-bundle/src/packagers/garmin.rs` |

### 3.2 Gaps and alternatives

| # | Gap | Impact | Alternative (or "no alternative") |
|---|---|---|---|
| G1 | No JS runtime: React component bodies cannot be transpiled | Page logic cannot be reused; the `RendererAdapter` contract does not apply | Build-time subset emitter + Monkey C rewrite; "run JS as-is" has no alternative |
| G2 | Monkey C subset limits (no exceptions / enums / switch / for / while / modules / visibility modifiers) | Complex logic must be refactored; the generator vocabulary is closed | `src/subset.js` allowlist rejects on violation; the example uses only if/else + assignment |
| G3 | No Connect IQ SDK / `monkeyc` / developer key on this machine | Real `.iq` compilation and device behavior cannot be closed locally | Structure / subset / determinism run for real; the compiler leg is SKIP-with-print (manual command printed once `CIQ_MONKEYC` / `CIQ_HOME` / `CIQ_DEVKEY` are ready); "compiler evidence" has no alternative (needs SDK + key) |
| G4 | No flex / text measurement; layout is fixed at build time | Dynamic layout and text auto-sizing are not possible | Reuse wear `layoutPage` build-time layout + runtime linear scaling; long text uses a fixed box (overflow to be observed on device) |
| G5 | `Dc` has no rounded-rectangle primitive; fonts are built-in bitmaps | Visuals are not pixel-identical to wear | Square corners replace rounded corners; `FONT_SMALL` / `FONT_XTINY` approximate 16/14 px |
| G6 | Pointer / gestures absent; three-key devices have no left/right; the right column of a 2×2 grid is unreachable | Interaction semantics differ from four-way traversal | `focusNav=true` + `engine.order` linear cycle (all four buttons reachable); `traversal` snapshot kept for future four-way devices; no new navigation is written |
| G7 | Assets / icons / launch images are missing | Manifest omits icon fields (development sideload form) | Asset pipeline to follow; this spike emits only the `AppName` string resource |
| G8 | Only one reference device per form (`venu2` / `venusq`) | No multi-device product matrix | `iq:products` can be extended as needed; the scaling formula based on `dc.getWidth()/getHeight()` is generic |
| G9 | Minimal lifecycle: no `onStart` / `onStop` persistence | State resets after exit / restart | The example page is transient by design; `Application.Storage` is not in the allowlist and is a follow-up extension |
| G10 | No real Connect IQ SDK / device evidence: the built-in packager performs structure self-checks only | Real `.iq` compilation and device behavior still require the environment slots; the structure can be checked for real by the Rust verifier | The built-in `iq` packager + `check_garmin_iq_structure` have landed; set `CIQ_MONKEYC` / `CIQ_HOME` / `CONNECTIQ_HOME` + `CIQ_DEVKEY` to run the compiler; "compiler / device evidence" has no alternative (needs SDK + key + device) |

### 3.3 Conclusion

Continue (constrained). `experimental = true` stays. Trigger prerequisite: one Connect IQ SDK compile + a device / emulator walkthrough (this machine has no `monkeyc` / SDK / devkey, so the compiler leg is SKIP-with-print). Degrade option (if not undertaken): keep the L6 TargetSpec + `experimental` + capability boundary + the built-in packager / structure check, but do not promise compiler / device evidence.

## 4. Non-experimental target: Wear OS boundary

- `wearos` has `TargetSpec.experimental = false`; all four generated documentation tables show `false`, and no ※experimental marker may appear for it in documentation (`check-experimental.mjs` enforces three-way consistency).
- Platform-level support: `runtime = native`, `bridge = uniffi`, APK (`android-aapt2`), `android-keystore` signing, `apksigner-verify` + `bundletool-validate`; Wear manifest `android.hardware.type.watch`, minSdkVersion ≥ 26.
- Boundary note: **the UI runtime (RN on Wear) is beta** — beta affects UI runtime confidence only, not the packaging and signing chain; packaging and signing use the Android base (reusing M2, with no new Android packaging implementation).
- Publishing and acceptance: `google-play-wear` channel; on-device smoke (install → launch → forms → uninstall) is the release gate (T-M7-04); contract tests and goldens are T-M7-11 (in parallel).

## 5. Maintenance convention

| Change | Required |
|---|---|
| Change experimental / target capabilities | Edit TargetSpec first (single source of truth) → `node tools/gen-target-table.mjs` → update this file in both languages |
| Gap / alternative changes | Update this file and the README L6 notes in the same commit; every gap must keep an alternative or explicitly say "no alternative" |
| CI checks | `node tools/gen-target-table.mjs --check`; `node tools/check-docs.mjs`; `node tools/check-experimental.mjs` |

Sources: `ui/adapters/garmin/CAPABILITY-BOUNDARY-DRAFT.md` (T-M7-09), `crates/polypack-bundle/src/packagers/garmin.rs`, `crates/polypack-cli/templates/watchos/README.md`, `crates/polypack-bundle/src/packagers/watchos.rs`, TargetSpec notes.
