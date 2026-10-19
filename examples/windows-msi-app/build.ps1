#Requires -Version 5.1
<#
.SYNOPSIS
    Builds the minimal Tauri v2 sample used by PolyPack's real-MSI test.

.DESCRIPTION
    Runs a plain cargo release build. That is enough for the MSI test: it feeds
    the resulting exe into PolyPack's existing WiX packager (crate
    `poly-bundle`), which produces the MSI that gets installed, launched and
    uninstalled. The Tauri CLI is optional (see README).

    Prerequisites: Rust (MSVC toolchain), Visual Studio C++ build tools,
    WebView2 Runtime (present on Windows 11 / recent Windows 10).
#>
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$manifest = Join-Path $root "src-tauri\Cargo.toml"
$exe = Join-Path $root "src-tauri\target\release\poly-msi-sample.exe"

Push-Location $root
try {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "cargo not found: install Rust from https://rustup.rs first"
    }

    cargo build --release --manifest-path $manifest
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }

    if (-not (Test-Path $exe)) {
        throw "build finished but $exe is missing"
    }

    Write-Host "Built: $exe"
    Write-Host "Smoke: & `"$exe`" --smoke"
    Write-Host "       (writes %TEMP%\poly-msi-sample-smoke.txt and exits automatically)"
} finally {
    Pop-Location
}
