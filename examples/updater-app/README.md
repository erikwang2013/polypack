# Updater drill sample (T-M9-12)

A minimal Tauri v2 app used to prove a **real** self-hosted update and rollback
cycle on Windows, driven by the repository's own `poly update manifest` output.
It is an example and a test harness, not a production app.

The CI job `windows-updater-real` in `.github/workflows/rust-ci.yml` runs
[`scripts/drill.ps1`](scripts/drill.ps1) on `windows-2022`. This directory was
developed and statically checked on Linux; the first Windows run (run
39395255929) passed and is the end-to-end evidence. The job fails unless the
drill prints `POLY_UPDATER_DRILL_RESULT: PASS`, so a SKIP is never treated as
success.

## What the drill proves

1. A throwaway Tauri updater key pair is generated at run time; the public key
   is baked into both builds, the private key stays outside the evidence tree.
2. `0.1.0` and `0.1.1` NSIS installers are built twice with that key
   (`createUpdaterArtifacts`), so each installer has a real `.sig`; on Windows
   the signed installer itself is the updater artifact.
3. `poly update manifest` turns each signed installer into a deterministic,
   GPG-signed Tauri v2 `latest.json`; `poly update check` verifies the manifest
   signature.
4. The installed `0.1.0` app reads a local HTTP `latest.json`, verifies the
   `0.1.1` installer's Tauri signature, installs it, and relaunches as `0.1.1`.
5. Restoring the retained `0.1.0` manifest makes the installed `0.1.1` app
   install `0.1.0` again; the rolled-back executable must hash-match the
   original `0.1.0` install.

## App modes

- Normal launch: shows a static window.
- `--smoke-update --marker <path>`: runs headless update logic and appends
  `start` / `detected` / `downloaded` / `up-to-date` / `error` lines to the
  marker. Release builds use the Windows GUI subsystem, so the marker file is
  the evidence channel. Exit code `0` means the check finished, `2` means the
  updater reported an error.

The sample's updater builder uses
`version_comparator(|current, remote| remote.version != current)` **only** so
the rollback leg can install an older version from a static manifest. A normal
app should keep Tauri's default "remote must be newer" check; downgrade is not
a production default.

## Running the drill

Prerequisites on Windows 10/11 or Windows Server 2022: WebView2 Runtime,
Rust stable, Tauri CLI 2.x (`npm install --global @tauri-apps/cli@^2`), GnuPG,
and Python 3 on `PATH`.

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File examples/updater-app/scripts/drill.ps1 `
  -WorkDir "$env:RUNNER_TEMP\poly-updater-drill"
```

`-WorkDir` must stay outside the repository. Generated keys, the GPG home, and
credential payloads go to the sibling `<WorkDir>-secrets` directory and are
never part of the evidence upload. Use `-Port` to avoid a busy local port.

## Evidence

`<WorkDir>/evidence/` contains `summary.json`, `artifacts.sha256`, the installed
executable hashes for `0.1.0` / `0.1.1` / rollback, both markers, a copy of both
signed manifests, the Tauri public key, the GPG public key, and tool versions.
Build and HTTP logs stay under `<WorkDir>`.

Failure codes are stable and include the failing step:
`POLY_UPDATER_DRILL_0001` prerequisites/keys, `0002` Tauri build, `0003`
artifact discovery, `0004` manifest generation, `0005` manifest verification,
`0006` local HTTP server, `0007` initial NSIS install, `0008` update leg,
`0009` rollback leg.

## Local checks (non-Windows)

```bash
cargo test --manifest-path examples/updater-app/src-tauri/Cargo.toml
actionlint .github/workflows/rust-ci.yml
```

These validate the sample's Rust code and the workflow syntax; they do not
replace the Windows run.
