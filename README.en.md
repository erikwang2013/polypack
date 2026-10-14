# PolyPack

**Package once, deliver everywhere** — a Rust core × React UI adapters × 50+ target platforms

[简体中文](README.md) · [Roadmap](docs/ROADMAP.en.md) · [Architecture](#architecture) · [Platforms](#platform-matrix)

> **Project status: M1 Desktop & Web delivered** (2026-10-15). Available today: `poly doctor` / `poly init` / `poly build` (real deb / AppDir / web static bundles on Linux, with Pax icon derivation) / `poly dev` (full-reload hot preview) / `poly clean`; the Windows / macOS packagers and their contracts are in place, awaiting the three-host CI (post-billing) for real-machine runs. Later milestones follow [docs/ROADMAP.en.md](docs/ROADMAP.en.md).
>
> **Note on diagram labels:** the embedded diagrams are labeled in Chinese (the project's primary language). English-labeled variants are scheduled for milestone M1.

---

## Meet Pax

![Pax's animated expression loop: happy → blinking → puzzled → crying → shocked](docs/assets/pax.svg)

**Pax** is the PolyPack project pet — a postcrab. The rust-orange shell is a nod to Ferris; the postal cap on its head is its job: it packs your app into a parcel, sticks a React-orbit stamp on it, and delivers it claw by claw to phones, watches, and TVs. The parcel it carries is your one codebase, compiled into dozens of artifacts.

Pax is animated and multi-form: it idly bobs, blinks on a timer, and cycles through five expressions — **happy / blinking / puzzled / crying / shocked**; every individual expression carries its own motion too — crying drips tears, shock shivers and sweats, puzzled floats a question mark, blinking blinks on a timer, happy bobs lightly. All pure CSS, no scripts; with "reduce motion" enabled every form settles into its static pose. All five at a glance (every one in the grid is moving):

![Pax's five expressions, each animated: happy / crying / shocked / puzzled / blinking](docs/assets/pax-moods.svg)

Individual expression files (each with its own motion): [happy](docs/assets/pax-happy.svg) · [crying](docs/assets/pax-cry.svg) · [shocked](docs/assets/pax-shock.svg) · [puzzled](docs/assets/pax-confused.svg) · [blinking](docs/assets/pax-blink.svg); open [docs/pax.html](docs/pax.html) locally to switch expressions by hand.

**Brand rule**: Pax *is* the PolyPack project icon — the docs favicon ([pax-mark.svg](docs/assets/pax-mark.svg)), the mini badge in the bottom-right corner of generated diagrams, and future CLI / GUI / installer icons all use it; the five expressions map onto build states (success = happy · failure = crying · exception = shocked · needs-confirmation = puzzled · idle = blinking). Every form is generated from a single source by `tools/build-pax.mjs`.

Pax shares the same color variables as every diagram in this repository — you will meet its rust orange and orbit cyan again in the architecture diagram.

## What PolyPack is

PolyPack is a **write-once, package-everywhere build tool**: a single Rust core uses a declarative target registry (TargetSpec) to compile, sign, package, and publish one **Rust business core + React interface** to **50+ target platforms** — from Android / iOS / HarmonyOS / Windows / macOS / Linux / Web, all the way to mini programs, quick apps, TVs, car systems, wearables, embedded RTOS, and (with vendor licensing) game consoles.

### Why it exists

- **Fragmented targets.** Multi-platform builds today are separate fiefdoms — Gradle for Android, Xcode for iOS, a different vendor IDE per mini-program platform, bespoke packaging scripts per desktop OS. PolyPack collapses them into one `poly` command.
- **One stack.** Business logic in Rust, written once, reused everywhere; UI in React, written once, re-skinned per platform by the adapter layer.
- **One chain.** Toolchain probe → cross-compile → UI bundle → sign / notarize / harden → package → store submission, orchestrated, cached, and reproducible end to end.
- **Extensible.** A new platform is a TargetSpec plus a WASM plugin — no waiting for an official release.

### How it works (the 90-second version)

```bash
poly release --target android,ios,wechat,tizen --publish
```

1. Load `polypack.toml` and resolve a **TargetSpec** for every requested target (runtime, package format, signing requirements, distribution channel);
2. **Probe toolchains** (Android SDK/NDK, Xcode, emsdk, …) and reuse the content-addressed build cache incrementally;
3. **Cross-compile Rust** and **bundle the React adapter layer** in parallel; artifacts flow into the packager matrix;
4. **Sign → notarize → harden → verify**, then package per target: APK / IPA / HAP / MSI / DMG / AppImage / wgt / ipk / mini-program bundles… and emit a build report.

The full 13-step journey is in [Request lifecycle](#request-lifecycle) below.

---

## Platform matrix

| Tier | Platform | UI runtime | Packaging / distribution |
|---|---|---|---|
| L1 | Android | React Native · Material 3 | AAB / APK → Google Play |
| L1 | iOS | React Native · HIG | IPA → App Store |
| L1 | HarmonyOS | RN for OpenHarmony · HarmonyOS Design | HAP → AppGallery |
| L1 | Windows | react-dom (Tauri) | MSI / NSIS → MS Store |
| L1 | macOS | react-dom (Tauri) | DMG / PKG + notarization |
| L1 | Linux | react-dom (Tauri) | AppImage · deb · rpm · Flatpak · Snap |
| L1 | Web | react-dom | static bundle + PWA |
| L2 | WeChat Mini Program | Taro (React syntax) | mini-program bundle → WeChat review |
| L2 | Alipay Mini Program | Taro | mini-program bundle → Alipay review |
| L2 | Douyin Mini Program | Taro | mini-program bundle → Douyin review |
| L2 | Quick App (快应用) | Taro / Quick App framework | .rpk → alliance stores |
| L2 | TikTok Minis | HTML5 + Minis bridge | zip → TikTok platform |
| L2 | Google Play Instant | React Native | instant-enabled AAB |
| L2 | Facebook Instant Games | HTML5 | zip → Meta platform |
| L2 | Snapchat Mini | HTML5 | zip → Snap platform |
| L2 | LINE Mini App | LIFF (web) | LIFF app → LINE platform |
| L3 | Android TV | RN · Leanback | APK → Play (TV track) |
| L3 | Apple tvOS | react-native-tvos | TV bundle → App Store |
| L3 | HarmonyOS TV | RNOH | HAP → Huawei video store |
| L3 | Samsung Tizen (TV) | react-dom (web bundle) | .wgt → Seller Office |
| L3 | LG webOS | react-dom (web bundle) | .ipk → LG Content Store |
| L3 | Roku OS | constrained renderer (SceneGraph subset) ※experimental | channel bundle → Roku channel store |
| L3 | Amazon Fire TV | React Native (Android) | APK → Amazon Appstore |
| L3 | Vizio SmartCast | react-dom (web bundle) | app bundle → Vizio platform |
| L3 | Hisense VIDAA | react-dom (web bundle) | app bundle → VIDAA store |
| L4 | GrapheneOS | React Native (no-GMS build) | APK → self-signed / F-Droid |
| L4 | KaiOS | react-dom (web stack) | zip → KaiStore |
| L4 | Sailfish OS | Qt / Silica shell + WebView | RPM → Jolla Store |
| L4 | Ubuntu Touch | WebView shell (Clickable) | .click → OpenStore |
| L5 | Ubuntu | react-dom / Tauri | deb · Snap · Flatpak |
| L5 | Fedora | react-dom / Tauri | rpm · Flatpak |
| L5 | Linux Mint | react-dom / Tauri | deb · Flatpak |
| L5 | Chrome OS | Linux (Crostini) + PWA | deb + PWA |
| L5 | FreeBSD | native build | pkg |
| L5 | OpenBSD | native build | ports |
| L5 | BSD family (NetBSD / DragonFly, …) | native build | pkgsrc, etc. |
| L6 | Apple watchOS | companion app (SwiftUI template) ※experimental | watch bundle |
| L6 | Wear OS | RN / lightweight runtime | APK → Play |
| L6 | Android Automotive | React Native (Android) | APK → OEM channels |
| L6 | QNX | Rust core + optional HMI layer | image / static library |
| L6 | AUTOSAR (Adaptive) | headless (core library) | ARXML / static library |
| L6 | VxWorks | headless (core library) | static library / image |
| L6 | RTOS (Zephyr / FreeRTOS) | headless (no_std core) | static library |
| L6 | Fitbit OS | Fitbit JS SDK | Fitbit app package |
| L6 | Garmin (Connect IQ) | Monkey C transpile ※experimental | .iq package |
| L6 | Zepp OS | Zepp JS API | Zepp mini app |
| L7 | PlayStation | official SDK plugin (license required) | platform bundle |
| L7 | Xbox | GDK plugin (license required) | platform bundle |
| L7 | Nintendo Switch | NintendoSDK plugin (license required) | platform bundle |

> Tier legend: **L1** first-class, full pipeline support; **L2** mini programs and instant apps via the Taro / HTML5 adapters; **L3** TVs and large screens; **L4** mobile OS variants; **L5** desktop distributions and BSD; **L6** wearables / automotive / embedded (several are headless core libraries); **L7** consoles, gated by vendor licensing and shipped only as official SDK plugins.
>
> The list above contains **49 explicit platforms**; counting the BSD, RTOS, and Linux-distribution families it reaches **50+ targets**. Targets marked ※experimental have constrained runtimes — see the risk register in [docs/ROADMAP.en.md](docs/ROADMAP.en.md).

---

## Architecture

![PolyPack architecture overview](docs/assets/en/architecture.svg)

Five layers, top to bottom:

1. **Entry layer** — the `poly` CLI (single binary), the `poly-gui` console (Tauri + React), and CI pipelines all submit build requests to the core;
2. **Rust core workspace** — `poly-core` pipeline orchestration, `poly-targets` target registry (50+ TargetSpecs), `poly-toolchain` toolchain probing, `poly-bundle` the packager matrix, `poly-sign` signing / notarization / hardening, `poly-publish` store submission;
3. **Side resources** — the build cache (content-addressed, incremental), the remote build farm (Apple targets require a macOS host; a macOS worker pool covers them), and the key vault (Keychain / Vault references — keys never enter the repository);
4. **React UI adapter layer** — the `@polypack/ui` design system (tokens + headless primitives) plus platform render adapters (react-dom / react-native / Taro / TV / wearable) that re-skin one interface per platform;
5. **Artifacts and recipients** — APK / IPA / HAP / MSI / DMG / AppImage / wgt / ipk / mini-program bundles… delivered to 50+ stores and channels.

Rust and React communicate across four bridges: **napi-rs** (Node / Taro side), **uniffi** (Kotlin / Swift), **wasm-bindgen** (Web / WebView side), and **cffi** (QNX / RTOS and other embedded sides).

## Request lifecycle

![Request lifecycle of one poly release](docs/assets/en/request-cycle.svg)

The full journey of one `poly release`: 13 steps in 5 phases.

| Phase | Steps |
|---|---|
| 01 resolve & probe | command enters the CLI → `polypack.toml` loaded into a JobSpec → SDK / NDK probed → toolchain ready (cache hits are reused outright) |
| 02 compile & UI | Rust cross-compiles per-target libraries (.a / .so / .wasm) → React is bundled with adapter transforms into bundle.js + assets |
| 03 signing | credentials referenced from the key vault → signing / notarization / hardening complete |
| 04 package & verify | per-target packaging (APK / IPA / HAP / wgt…) → artifact manifest verified |
| 05 delivery | build report + artifact paths returned; with `--publish`, submission to stores continues |

## Features

![PolyPack capability map](docs/assets/en/features.svg)

Five capability families:

- **Targets & coverage** — declarative target registry (50+ TargetSpecs), one-command multi-target releases, OS-variant support (GrapheneOS / KaiOS / HarmonyOS family);
- **Build & cache** — automatic toolchain probing and installation, content-addressed caching (incremental, reproducible), remote build farm (macOS worker pool);
- **Cross-platform UI** — the `@polypack/ui` design system, platform component adaptation (Material / HIG / HarmonyOS styles), `poly dev` hot preview;
- **Signing & publishing** — vault integration (Keychain / Vault references), signing · notarization · hardening (notarize / AVB), store pipelines (Play / ASC / AGC / mini-program platforms);
- **Ecosystem & operations** — WASM plugins (custom targets, sandboxed), CI integrations, diagnostics (build explanations, failure localization).

## Lifecycle

![Project and artifact lifecycle](docs/assets/en/lifecycle.svg)

A state machine from bootstrap to archival, with two **recovery loops**:

- **Main path**: init → dev iteration → build → sign & verify → release;
- **Waiting states**: signing blocked on vault unlock; store and mini-program human reviews (waiting consumes no build resources);
- **Recovery loops**: a failed build retries back into build after a fix; a rejected review loops back to dev iteration — every failure has a defined way home;
- **Terminal**: archive / EOL is the only exit, retiring the artifact while keeping it traceable.

## UI component strategy

The interface is written once (React) and mapped by the adapter layer onto **each platform's own component system and plugins**:

| Platform family | React runtime | Component system |
|---|---|---|
| Web / desktop (Win / macOS / Linux) | react-dom (Tauri WebView) | `@polypack/ui` desktop tokens |
| Android family (incl. GrapheneOS / Wear / Auto / Fire TV / Android TV) | React Native (Fabric + JSI) | Material 3 variants |
| Apple family (iOS / macOS / tvOS) | React Native / react-native-tvos | HIG variants |
| HarmonyOS family (incl. TV) | React Native for OpenHarmony | HarmonyOS Design |
| Mini-program family (WeChat / Alipay / Douyin / Quick App) | Taro (React syntax) | each platform's native components |
| TV web runtime (Tizen / webOS / VIDAA / Vizio / KaiOS) | react-dom (packaged as wgt / ipk / app bundle) | 10-foot UI + focus navigation |
| Wearables (Wear OS / watchOS / Fitbit / Zepp) | RN / lightweight JS runtime (experimental) | compact tokens |
| Embedded / automotive (QNX / AUTOSAR / RTOS) | headless core (optional HMI layer) | platform HMI specs |

## Quick start (planned API)

```bash
# available today (M1): init / build / dev / clean / doctor (see below); release / publish are target shape
cargo install polypack      # install the poly CLI
poly init                   # generate polypack.toml
poly dev                    # hot preview
poly release --target android,ios,wechat
poly release --target all --publish
```

> `--target all` = every **buildable** target in the current environment; exclusions (license-gated, unreachable host/credentials) and their reasons are listed during the plan stage.
>
> Available today (M1): `cargo run -p poly-cli -- init|build|dev|clean|doctor` (toolchain probes, sample-app scaffolding, cache-addressed multi-target build & packaging, full-reload hot preview, cache/artifact cleanup).

Sketch of `polypack.toml`:

```toml
schema = 1

[package]
name = "my-app"
version = "1.2.0"

[ui]
framework = "react"
entry = "ui/src/main.tsx"

[core]
crate = "crates/app-core"    # Rust business core, compiled per target

[targets.android]
min-sdk = 24
signing = "keystore"          # references a vault entry; keys never in-repo

[targets.ios]
bundle-id = "dev.example.polypack"
team = "ABCDE12345"

[targets.wechat]
appid = "wx0000000000000000"

[publish]
stores = ["play", "appstore", "appgallery"]
```

## Repository layout

```text
polypack/
├── crates/
│   ├── poly-cli/          # command line entry (clap)
│   ├── poly-core/         # pipeline orchestration, JobSpec, event bus
│   ├── poly-targets/      # TargetSpec registry (50+ target declarations)
│   ├── poly-toolchain/    # SDK / NDK / Xcode / emsdk probe & install
│   ├── poly-bundle/       # packager matrix (apk / ipa / hap / msi / dmg / deb / rpm / wgt / …)
│   ├── poly-sign/         # signing, notarization, hardening
│   ├── poly-publish/      # store and mini-program platform submission
│   ├── poly-cache/        # content-addressed build cache
│   └── poly-plugin/       # WASM plugin host (custom targets)
├── ui/
│   ├── polypack-ui/       # @polypack/ui design system (tokens + headless primitives)
│   └── adapters/          # react-dom / react-native / taro / tv / wear adapters
├── app/                   # poly-gui console (Tauri + React)
├── docs/
│   ├── ROADMAP.en.md      # detailed plan (milestones, risks, support matrix)
│   ├── TASKS.en.md        # task plan (116 tasks / 283 person-days)
│   ├── FARM.en.md         # remote farm: origin / build site (deployment, credentials, signature checks)
│   ├── assets/            # every SVG embedded in this document (incl. Pax)
│   └── diagrams/          # diagram sources (JSON IR) + rebuild pipeline
└── tools/
    ├── render-diagrams.mjs  # diagram pipeline: JSON IR → SVG (with the Pax corner badge)
    └── build-pax.mjs        # Pax generator: expressions + icon + contact sheet + playground
```

(M0 delivered: the 9-crate workspace skeleton plus `poly-cli` / `poly-core` / `poly-targets` / `poly-toolchain`; since M2 `ui/` has landed — `ui/adapters/native` native adapter v0 + RN host skeleton (T-M2-04/05); `app/` remains target shape.)

## Documentation sync convention

Docs are part of the product; diagrams and prose must stay single-sourced:

1. **`docs/diagrams/*.json` is the single source of truth for the diagrams** — change the JSON first, then run:

   ```bash
   node tools/render-diagrams.mjs   # validate → render → extract to docs/assets/*.svg
   ```

   CI catches drift automatically via `render-diagrams.mjs --check`, `build-pax.mjs --check`, and `tools/check-docs.mjs` (see `.github/workflows/docs-check.yml`).

2. `README.md` and [`README.en.md`](README.en.md) must be updated **in the same commit** (bilingual mirrors);
3. Milestone and platform-matrix changes sync to `docs/ROADMAP.md` / [`ROADMAP.en.md`](docs/ROADMAP.en.md);
4. When Pax's art or copy changes, update `docs/assets/pax.svg` to stay consistent with the design system.

## Roadmap

Releases: since v1.0.0, tags and GitHub Release names follow `vX.Y.Z` semantic increments (v1.0.0 = this documentation and planning baseline).

Incubated extension directions (SBOM / monorepo / incremental releases / OTA / store metadata / GUI dashboard / device previews / ADR) live in [docs/ROADMAP.en.md](docs/ROADMAP.en.md) §13.

| Milestone | Scope |
|---|---|
| M0 | repo skeleton, `polypack.toml` schema, TargetSpec DSL, `poly doctor` |
| M1 | Windows / macOS / Linux / Web end-to-end + content-addressed cache + **English-labeled diagrams** |
| M2 | Android / iOS (RN bridge + signing pipeline) |
| M3 | HarmonyOS (RNOH) + WeChat / Alipay / Douyin mini programs (Taro) |
| M4 | TV family (Android TV / Fire TV / tvOS / Tizen / webOS / VIDAA / Vizio; Roku constrained adapter) |
| M5 | desktop distributions & BSD (deb / rpm / Flatpak / Snap / AppImage / pkg / ports / ChromeOS) |
| M6 | embedded & automotive (headless core: QNX / AUTOSAR / VxWorks / RTOS / Android Automotive) |
| M7 | wearables (Wear OS / watchOS companion / Fitbit / Garmin / Zepp, experimental) |
| M8 | WASM plugin SDK, remote build farm, store publishing pipelines, ecosystem GA |

The complete plan (per-platform support detail, risks, and mitigations) lives in **[docs/ROADMAP.en.md](docs/ROADMAP.en.md)**; the task-level breakdown (116 tasks / 283 person-days, with roles, dependencies, acceptance criteria, and scheduling notes) is in **[docs/TASKS.en.md](docs/TASKS.en.md)**.

Cross-host builds (Linux origin → macOS worker produces an IPA) — deployment, credential model, and signature verification — are documented in **[docs/FARM.en.md](docs/FARM.en.md)**.

## License

[Apache-2.0](LICENSE) (the `LICENSE` file is in place; contribution rules formalize with the code).
