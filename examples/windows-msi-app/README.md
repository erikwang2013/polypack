# windows-msi-app — minimal real Tauri v2 MSI sample

Windows-only sample used by `crates/polypack-bundle/tests/windows_msi_real.rs`
(T-M9-53/54). It is intentionally tiny:

- one Tauri v2 window, one static `web/index.html` + `web/app.js` asset;
- no frontend toolchain, no dev server, no network calls, no external services;
- release builds use the Windows GUI subsystem; `--smoke` starts the normal
  Tauri runtime, writes a marker file and exits automatically.

The MSI is produced by PolyPack's existing WiX packager
(`crates/polypack-bundle/src/packagers/windows.rs`, `candle` + `light`), not by
`tauri build --bundles msi`. That is deliberate: the test must exercise the
packager that ships in this repository.

## Prerequisites (Windows)

- Rust stable with the MSVC target (`rustup default stable-x86_64-pc-windows-msvc`).
- Visual Studio 2022 Build Tools with the "Desktop development with C++" workload.
- WebView2 Runtime (preinstalled on Windows 11 and current Windows 10; the
  Evergreen installer is available from Microsoft).
- WiX Toolset v3.14 (`candle.exe` / `light.exe` on `PATH`). GitHub's
  `windows-2022` image already includes it at
  `%ProgramFiles(x86)%\WiX Toolset v3.14\bin`.
- An **elevated** shell when running the real-MSI test: the packager's MSI uses
  `InstallScope="perMachine"` and installs under `%ProgramFiles%`, so
  `msiexec` needs elevation.

Tauri CLI is **not** required for the smoke test. It is optional for humans:

```powershell
cargo install tauri-cli --version "^2" --locked
```

## Build

```powershell
# From the repository root:
powershell -ExecutionPolicy Bypass -File examples/windows-msi-app/build.ps1

# Or directly:
cargo build --release --manifest-path examples/windows-msi-app/src-tauri/Cargo.toml
```

Output: `examples/windows-msi-app/src-tauri/target/release/polypack-msi-sample.exe`.

Optional Tauri CLI path (also produces the exe, no bundle):

```powershell
cd examples/windows-msi-app/src-tauri
cargo tauri build --no-bundle
```

## Smoke without installing

```powershell
$exe = "examples\windows-msi-app\src-tauri\target\release\polypack-msi-sample.exe"
& $exe --smoke
Get-Content "$env:TEMP\polypack-msi-sample-smoke.txt"   # ok
```

A normal launch (`& $exe`) opens the single sample window.

## Real MSI install / launch / uninstall test

Run from the repository root in an elevated PowerShell:

```powershell
$env:POLYPACK_MSI_REAL = "1"
$env:POLYPACK_TAURI_CORE_BIN = (Resolve-Path "examples\windows-msi-app\src-tauri\target\release\polypack-msi-sample.exe").Path
$env:POLYPACK_MSI_REAL_OUT = Join-Path $env:TEMP "polypack-msi-real"
cargo test -p polypack-bundle --test windows_msi_real -- --nocapture
```

The test builds the MSI with the PolyPack WiX packager, runs
`msiexec /i /qn`, launches the installed copy from
`%ProgramFiles%\Polypack MSI Sample` with `--smoke`, runs `msiexec /x /qn`, and
checks that the install directory, Start Menu folder, `HKCU` marker value and
ARP entry are gone. Evidence (`.wxs`, `.msi`, install/uninstall logs, smoke
marker, SRI digest) is written to `POLYPACK_MSI_REAL_OUT` (default: a per-process
directory under `%TEMP%`).

`POLYPACK_MSI_REAL=1` is required so an ordinary `cargo test` on Windows never
silently performs a machine-wide install. When the gate is absent, the sample
was not built, or WiX/msiexec are unavailable, the test prints `SKIP` instead
of pretending it ran.

In CI, `.github/workflows/rust-ci.yml` job `windows-msi-real` performs the same
steps and archives the logs, MSI and digest as build artifacts.

## Failure codes

The test prints stable codes for stage failures:

| Code | Stage |
|---|---|
| `POLYPACK_MSI_REAL_0001` | sample / `POLYPACK_TAURI_CORE_BIN` input missing or invalid |
| `POLYPACK_MSI_REAL_0002` | bare-product layout or `candle` / `light` MSI build |
| `POLYPACK_MSI_REAL_0003` | `msiexec /i /qn` install |
| `POLYPACK_MSI_REAL_0004` | installed exe launch / `--smoke` exit |
| `POLYPACK_MSI_REAL_0005` | `msiexec /x /qn` uninstall |
| `POLYPACK_MSI_REAL_0006` | residue check (install dir / Start Menu / HKCU / ARP) |
