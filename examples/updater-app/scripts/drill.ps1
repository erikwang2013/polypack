#Requires -Version 5.1
<#
.SYNOPSIS
    Real Tauri v2 self-hosted update + rollback drill (T-M9-12).

.DESCRIPTION
    Builds the sample app twice (0.1.0 and 0.1.1) with a throwaway Tauri updater
    key pair, signs the NSIS installers (the Windows updater artifact), generates the static `latest.json`
    with the repository's own `polypack update manifest`, verifies the manifest's GPG
    signature with `polypack update check`, serves the channel over local HTTP,
    installs 0.1.0, lets the installed app update itself to 0.1.1, then restores
    the 0.1.0 manifest and lets it roll back.

    Artifacts and evidence live under -WorkDir (default:
    $env:RUNNER_TEMP\polypack-updater-drill). The throwaway private key and GPG home
    live in "${WorkDir}-secrets", outside the evidence tree; they must never be
    committed or uploaded.

    Prerequisites (the CI job installs them before calling this script):
    cargo, the Tauri v2 CLI (`tauri`), GnuPG (`gpg`) and Python 3 (`python`).
    Tauri brings its own NSIS toolchain; no system NSIS/WiX is required.

.PARAMETER Port
    Local HTTP port used for the static updater channel (default 8765).

.PARAMETER WorkDir
    Evidence/scratch directory. Must be outside the repository.

.PARAMETER KeyPassword
    Password for the throwaway Tauri updater private key. Test-only value.
#>
[CmdletBinding()]
param(
    [int]$Port = 8765,
    [string]$WorkDir = '',
    [string]$KeyPassword = 'polypack-updater-drill'
)

if ([string]::IsNullOrWhiteSpace($WorkDir)) {
    $WorkDir = Join-Path ([IO.Path]::GetTempPath()) 'polypack-updater-drill'
}

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (Get-Variable PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$v1 = '0.1.0'
$v2 = '0.1.1'
$channel = 'stable'
$pubDate = '2026-10-20T00:00:00Z'

$appDir = Split-Path -Parent $PSScriptRoot
$repoRoot = Split-Path -Parent (Split-Path -Parent $appDir)
$polypack = Join-Path $repoRoot 'target\release\polypack.exe'
$srcTauri = Join-Path $appDir 'src-tauri'

# Secrets live in a sibling directory so the evidence upload (WorkDir) can never
# pick up the throwaway Tauri/GPG private keys.
$secretsDir = "$WorkDir-secrets"
$keysDir = Join-Path $secretsDir 'keys'
$gpgHome = Join-Path $secretsDir 'gnupg'
$credentialsDir = Join-Path $secretsDir 'credentials'
$packagesDir = Join-Path $WorkDir 'packages'
$manifestsDir = Join-Path $WorkDir 'manifests'
$serveDir = Join-Path $WorkDir 'serve'
$markersDir = Join-Path $WorkDir 'markers'
$evidenceDir = Join-Path $WorkDir 'evidence'
$overlaysDir = Join-Path $WorkDir 'overlays'

$installDir = Join-Path $env:LOCALAPPDATA 'PolyUpdaterSample'
$installedExe = Join-Path $installDir 'polypack-updater-sample.exe'
$uninstaller = Join-Path $installDir 'uninstall.exe'

$server = $null

# Git for Windows ships an MSYS gpg that only understands POSIX paths. Detect it
# and convert every path argument; a native Gpg4win install keeps Windows paths.
$gpgCommand = Get-Command gpg -ErrorAction SilentlyContinue
$gpgNeedsPosix = $null -ne $gpgCommand -and $gpgCommand.Source -match '\\Git\\'
function ConvertTo-GpgPath([string]$Path) {
    if ($gpgNeedsPosix) {
        $converted = & cygpath -u $Path
        if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($converted)) {
            throw "POLYPACK_UPDATER_DRILL_0001 cygpath -u failed for $Path"
        }
        return $converted.Trim()
    }
    return $Path
}

function Step([string]$Message) {
    Write-Host ""
    Write-Host "==> $Message"
}

function New-Directory([string]$Path) {
    New-Item -ItemType Directory -Force -Path $Path | Out-Null
}

function Invoke-Checked([string]$Description, [string]$Code, [scriptblock]$Command) {
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Code $Description exited $LASTEXITCODE"
    }
}

function Wait-ForMarker {
    param(
        [string]$Path,
        [string]$Pattern,
        [int]$TimeoutSeconds,
        [string]$Description,
        [string]$Code
    )
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path -LiteralPath $Path) {
            try {
                $text = [IO.File]::ReadAllText($Path)
                if ($text -match [regex]::Escape($Pattern)) {
                    return $text
                }
            } catch {
                # The app appends and closes per line; a transient sharing error is fine.
            }
        }
        Start-Sleep -Milliseconds 500
    }
    $seen = if (Test-Path -LiteralPath $Path) {
        [IO.File]::ReadAllText($Path)
    } else {
        '<marker missing>'
    }
    throw "$Code timed out waiting for $Description (${TimeoutSeconds}s). Marker:`n$seen"
}

function Get-BuildArtifacts {
    param([string]$Version)
    $bundleDir = Join-Path $srcTauri 'target\release\bundle\nsis'
    if (-not (Test-Path -LiteralPath $bundleDir)) {
        throw "POLYPACK_UPDATER_DRILL_0003 bundle directory missing: $bundleDir"
    }
    $installer = Get-ChildItem -LiteralPath $bundleDir -File -Filter '*.exe' |
        Where-Object { $_.Name -like "*_${Version}_x64-setup.exe" } |
        Select-Object -First 1
    if (-not $installer) {
        throw "POLYPACK_UPDATER_DRILL_0003 NSIS installer for $Version not found in $bundleDir"
    }
    # Tauri v2 uses the signed NSIS installer itself as the Windows updater
    # artifact (the plugin can install an .exe directly); the CLI writes
    # `<installer>.exe.sig` when createUpdaterArtifacts is true.
    $signature = "$($installer.FullName).sig"
    if (-not (Test-Path -LiteralPath $signature)) {
        throw "POLYPACK_UPDATER_DRILL_0003 updater signature missing: $signature"
    }
    [pscustomobject]@{
        Installer = $installer.FullName
        Signature = $signature
    }
}

function Build-Version {
    param([string]$Version)
    $overlayPath = Join-Path $overlaysDir "tauri.$Version.json"
    $overlay = [ordered]@{
        version = $Version
        bundle  = [ordered]@{
            active                 = $true
            targets                = @('nsis')
            createUpdaterArtifacts = $true
        }
        plugins = [ordered]@{
            updater = [ordered]@{
                pubkey                              = $script:pubkey
                endpoints                           = @("http://127.0.0.1:$Port/latest.json")
                dangerousInsecureTransportProtocol  = $true
                windows                             = [ordered]@{
                    installMode = 'quiet'
                }
            }
        }
    }
    $json = $overlay | ConvertTo-Json -Depth 10
    [IO.File]::WriteAllText($overlayPath, $json, (New-Object Text.UTF8Encoding($false)))

    Push-Location $appDir
    try {
        # Force the app crate (and tauri-codegen's TAURI_CONFIG merge) to rebuild
        # so the second version embeds its own version/pubkey config; deps stay cached.
        & cargo clean -p polypack-updater-sample --manifest-path (Join-Path $srcTauri 'Cargo.toml') |
            Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "POLYPACK_UPDATER_DRILL_0002 cargo clean for $Version exited $LASTEXITCODE"
        }
        $env:TAURI_SIGNING_PRIVATE_KEY = Join-Path $keysDir 'updater.key'
        $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $KeyPassword
        & tauri build --config $overlayPath --bundles nsis 2>&1 |
            Tee-Object -FilePath (Join-Path $WorkDir "build-$Version.log")
        if ($LASTEXITCODE -ne 0) {
            throw "POLYPACK_UPDATER_DRILL_0002 tauri build $Version exited $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
}

function New-Manifest {
    param(
        [string]$Version,
        [string]$PackageDir,
        [string]$ManifestDir,
        [string]$ArtifactName
    )
    New-Directory $ManifestDir
    & $polypack update manifest `
        --channel $channel `
        --version $Version `
        --notes "PolyPack updater drill $Version" `
        --base-url "http://127.0.0.1:$Port" `
        --platform windows `
        --arch x86_64 `
        --package-dir $PackageDir `
        --artifact $ArtifactName `
        --output (Join-Path $ManifestDir 'latest.json') `
        --credential env:POLYPACK_UPDATE_KEY `
        --pub-date $pubDate
    if ($LASTEXITCODE -ne 0) {
        throw "POLYPACK_UPDATER_DRILL_0004 polypack update manifest $Version exited $LASTEXITCODE"
    }
    & $polypack update check `
        --manifest (Join-Path $ManifestDir 'latest.json') `
        --signature (Join-Path $ManifestDir 'latest.json.sig') `
        --trust-root env:POLYPACK_UPDATE_TRUST
    if ($LASTEXITCODE -ne 0) {
        throw "POLYPACK_UPDATER_DRILL_0005 polypack update check $Version failed (manifest GPG signature)"
    }
}

try {
    New-Directory $WorkDir
    New-Directory $secretsDir
    New-Directory $keysDir
    New-Directory $credentialsDir
    New-Directory $packagesDir
    New-Directory $manifestsDir
    New-Directory $serveDir
    New-Directory $markersDir
    New-Directory $evidenceDir
    New-Directory $overlaysDir

    Step 'Checking prerequisites'
    $missing = @()
    foreach ($tool in @('cargo', 'tauri', 'gpg', 'python')) {
        if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
            $missing += $tool
        }
    }
    if ($missing.Count -gt 0) {
        throw "POLYPACK_UPDATER_DRILL_0001 missing prerequisites: $($missing -join ', ')"
    }
    $wv2Keys = @(
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
        'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
        'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    )
    if (@($wv2Keys | Where-Object { Test-Path $_ }).Count -eq 0) {
        throw 'POLYPACK_UPDATER_DRILL_0001 WebView2 Runtime missing (windows-2022 should preinstall it)'
    }
    $versions = & {
        cargo --version
        tauri --version
        gpg --version | Select-Object -First 1
        python --version
    }
    $versions | Tee-Object -FilePath (Join-Path $evidenceDir 'tool-versions.txt') | Out-Host

    if (-not (Test-Path -LiteralPath $polypack)) {
        Step 'Building polypack CLI (release)'
        Push-Location $repoRoot
        try {
            & cargo build -p polypack-cli --release 2>&1 |
                Tee-Object -FilePath (Join-Path $WorkDir 'polypack-build.log')
            if ($LASTEXITCODE -ne 0) {
                throw "POLYPACK_UPDATER_DRILL_0001 cargo build -p polypack-cli exited $LASTEXITCODE"
            }
        } finally {
            Pop-Location
        }
    }

    Step 'Generating throwaway Tauri updater key pair'
    Invoke-Checked 'tauri signer generate' 'POLYPACK_UPDATER_DRILL_0001' {
        tauri signer generate --ci --force --password $KeyPassword `
            --write-keys (Join-Path $keysDir 'updater.key')
    }
    $pubkey = (Get-Content -LiteralPath (Join-Path $keysDir 'updater.key.pub') -Raw).Trim()
    if ([string]::IsNullOrWhiteSpace($pubkey)) {
        throw 'POLYPACK_UPDATER_DRILL_0001 generated public key is empty'
    }
    Write-Host "Tauri updater public key: $pubkey"
    Set-Content -LiteralPath (Join-Path $evidenceDir 'tauri-updater-pubkey.txt') -Value $pubkey

    Step 'Generating throwaway GPG key for the static manifest'
    New-Directory $gpgHome
    $gpgBatch = Join-Path $credentialsDir 'gpg-keygen.txt'
    $batch = @"
%no-protection
Key-Type: RSA
Key-Length: 3072
Key-Usage: sign
Name-Real: PolyPack Updater Drill
Name-Email: updater-drill@polypack.invalid
Expire-Date: 1d
%commit
"@
    [IO.File]::WriteAllText($gpgBatch, $batch)
    Invoke-Checked 'gpg --generate-key' 'POLYPACK_UPDATER_DRILL_0001' {
        gpg --homedir (ConvertTo-GpgPath $gpgHome) --batch --pinentry-mode loopback `
            --generate-key (ConvertTo-GpgPath $gpgBatch)
    }
    $fingerprintLine = & gpg --homedir (ConvertTo-GpgPath $gpgHome) --batch --with-colons --list-secret-keys |
        Select-String '^fpr:' |
        Select-Object -First 1
    if (-not $fingerprintLine) {
        throw 'POLYPACK_UPDATER_DRILL_0001 could not read the GPG fingerprint'
    }
    $fingerprint = $fingerprintLine.Line.Split(':')[9]
    if ([string]::IsNullOrWhiteSpace($fingerprint)) {
        throw 'POLYPACK_UPDATER_DRILL_0001 generated GPG fingerprint is empty'
    }
    $secretAsc = Join-Path $credentialsDir 'manifest-secret.asc'
    $publicAsc = Join-Path $evidenceDir 'manifest-public.asc'
    Invoke-Checked 'gpg --export-secret-keys' 'POLYPACK_UPDATER_DRILL_0001' {
        gpg --homedir (ConvertTo-GpgPath $gpgHome) --batch --armor `
            --output (ConvertTo-GpgPath $secretAsc) --export-secret-keys $fingerprint
    }
    Invoke-Checked 'gpg --export' 'POLYPACK_UPDATER_DRILL_0001' {
        gpg --homedir (ConvertTo-GpgPath $gpgHome) --batch --armor `
            --output (ConvertTo-GpgPath $publicAsc) --export $fingerprint
    }
    $keystore = [Convert]::ToBase64String([IO.File]::ReadAllBytes($secretAsc))
    $credentialPayload = @{
        keystore       = $keystore
        store_password = [Guid]::NewGuid().ToString()
    } | ConvertTo-Json -Compress
    $env:POLYPACK_UPDATE_KEY = [Convert]::ToBase64String(
        [Text.Encoding]::UTF8.GetBytes($credentialPayload)
    )
    $trustPayload = @{
        public_key = [Convert]::ToBase64String([IO.File]::ReadAllBytes($publicAsc))
    } | ConvertTo-Json -Compress
    $env:POLYPACK_UPDATE_TRUST = [Convert]::ToBase64String(
        [Text.Encoding]::UTF8.GetBytes($trustPayload)
    )

    Step "Building app $v1 with updater artifacts"
    Build-Version -Version $v1
    $artifactsV1 = Get-BuildArtifacts -Version $v1

    Step "Packaging updater artifact $v1"
    $pkgV1 = Join-Path $packagesDir $v1
    New-Directory $pkgV1
    $artifactV1Name = Split-Path -Leaf $artifactsV1.Installer
    Copy-Item -LiteralPath $artifactsV1.Installer -Destination (Join-Path $pkgV1 $artifactV1Name)
    Copy-Item -LiteralPath $artifactsV1.Signature -Destination (Join-Path $pkgV1 "$artifactV1Name.sig")

    Step "Building app $v2 with updater artifacts"
    Build-Version -Version $v2
    $artifactsV2 = Get-BuildArtifacts -Version $v2

    Step "Packaging updater artifact $v2"
    $pkgV2 = Join-Path $packagesDir $v2
    New-Directory $pkgV2
    $artifactV2Name = Split-Path -Leaf $artifactsV2.Installer
    Copy-Item -LiteralPath $artifactsV2.Installer -Destination (Join-Path $pkgV2 $artifactV2Name)
    Copy-Item -LiteralPath $artifactsV2.Signature -Destination (Join-Path $pkgV2 "$artifactV2Name.sig")

    Step "Generating and verifying static manifests with polypack update"
    $manifestV1 = Join-Path $manifestsDir $v1
    $manifestV2 = Join-Path $manifestsDir $v2
    New-Manifest -Version $v1 -PackageDir $pkgV1 -ManifestDir $manifestV1 -ArtifactName $artifactV1Name
    New-Manifest -Version $v2 -PackageDir $pkgV2 -ManifestDir $manifestV2 -ArtifactName $artifactV2Name

    Step 'Preparing local HTTP channel'
    Copy-Item -LiteralPath (Join-Path $pkgV1 $artifactV1Name) -Destination $serveDir
    Copy-Item -LiteralPath (Join-Path $pkgV1 "$artifactV1Name.sig") -Destination $serveDir
    Copy-Item -LiteralPath (Join-Path $pkgV2 $artifactV2Name) -Destination $serveDir
    Copy-Item -LiteralPath (Join-Path $pkgV2 "$artifactV2Name.sig") -Destination $serveDir
    Copy-Item -LiteralPath (Join-Path $manifestV2 'latest.json') -Destination $serveDir
    Copy-Item -LiteralPath (Join-Path $manifestV2 'latest.json.sig') -Destination $serveDir

    $python = (Get-Command python -ErrorAction SilentlyContinue).Source
    $serverArgs = "-m http.server $Port --bind 127.0.0.1 --directory `"$serveDir`""
    $server = Start-Process -FilePath $python `
        -ArgumentList $serverArgs `
        -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $WorkDir 'http-server.out.log') `
        -RedirectStandardError (Join-Path $WorkDir 'http-server.err.log')
    $serverReady = $false
    $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline) {
        try {
            Invoke-WebRequest -UseBasicParsing -TimeoutSec 2 "http://127.0.0.1:$Port/latest.json" |
                Out-Null
            $serverReady = $true
            break
        } catch {
            Start-Sleep -Milliseconds 300
        }
    }
    if (-not $serverReady) {
        throw "POLYPACK_UPDATER_DRILL_0006 local HTTP server did not serve latest.json on port $Port"
    }

    Step "Installing $v1 with the NSIS installer"
    $install = Start-Process -FilePath $artifactsV1.Installer -ArgumentList '/S' -PassThru -Wait
    if ($install.ExitCode -ne 0) {
        throw "POLYPACK_UPDATER_DRILL_0007 NSIS install exited $($install.ExitCode)"
    }
    if (-not (Test-Path -LiteralPath $installedExe)) {
        throw "POLYPACK_UPDATER_DRILL_0007 installed executable missing: $installedExe"
    }
    $installedV1Hash = (Get-FileHash -LiteralPath $installedExe -Algorithm SHA256).Hash
    Write-Host "Installed $v1 exe SHA256: $installedV1Hash"

    Step "Launching $v1 with --smoke-update (expected update to $v2)"
    $updateMarker = Join-Path $markersDir 'update.marker'
    Remove-Item -LiteralPath $updateMarker -Force -ErrorAction SilentlyContinue
    $updateArgs = "--smoke-update --marker `"$updateMarker`""
    $null = Start-Process -FilePath $installedExe -ArgumentList $updateArgs -PassThru
    $updateLog = Wait-ForMarker -Path $updateMarker -Pattern "up-to-date version=$v2" `
        -TimeoutSeconds 300 -Description "update to $v2" -Code 'POLYPACK_UPDATER_DRILL_0008'
    if ($updateLog -notmatch [regex]::Escape("detected current=$v1 remote=$v2")) {
        throw "POLYPACK_UPDATER_DRILL_0008 update did not report the expected detection: $updateLog"
    }
    Start-Sleep -Seconds 3
    $installedV2Hash = (Get-FileHash -LiteralPath $installedExe -Algorithm SHA256).Hash
    if ($installedV2Hash -eq $installedV1Hash) {
        throw 'POLYPACK_UPDATER_DRILL_0008 installed binary hash did not change after the update'
    }
    Write-Host "Updated $v2 exe SHA256: $installedV2Hash"

    Step "Restoring the $v1 manifest and launching $v2 with --smoke-update (expected rollback to $v1)"
    Copy-Item -LiteralPath (Join-Path $manifestV1 "$v1.json") -Destination (Join-Path $serveDir 'latest.json') -Force
    Copy-Item -LiteralPath (Join-Path $manifestV1 "$v1.json.sig") -Destination (Join-Path $serveDir 'latest.json.sig') -Force
    $rollbackMarker = Join-Path $markersDir 'rollback.marker'
    Remove-Item -LiteralPath $rollbackMarker -Force -ErrorAction SilentlyContinue
    $rollbackArgs = "--smoke-update --marker `"$rollbackMarker`""
    $null = Start-Process -FilePath $installedExe -ArgumentList $rollbackArgs -PassThru
    $rollbackLog = Wait-ForMarker -Path $rollbackMarker -Pattern "up-to-date version=$v1" `
        -TimeoutSeconds 300 -Description "rollback to $v1" -Code 'POLYPACK_UPDATER_DRILL_0009'
    if ($rollbackLog -notmatch [regex]::Escape("detected current=$v2 remote=$v1")) {
        throw "POLYPACK_UPDATER_DRILL_0009 rollback did not report the expected detection: $rollbackLog"
    }
    Start-Sleep -Seconds 3
    $installedRollbackHash = (Get-FileHash -LiteralPath $installedExe -Algorithm SHA256).Hash
    if ($installedRollbackHash -ne $installedV1Hash) {
        throw "POLYPACK_UPDATER_DRILL_0009 rolled-back binary hash differs from the original $v1 install"
    }
    Write-Host "Rolled back $v1 exe SHA256: $installedRollbackHash"

    Step 'Writing evidence'
    $evidenceManifests = Join-Path $evidenceDir 'manifests'
    New-Directory $evidenceManifests
    Copy-Item -LiteralPath $manifestV1 -Destination $evidenceManifests -Recurse
    Copy-Item -LiteralPath $manifestV2 -Destination $evidenceManifests -Recurse
    Copy-Item -LiteralPath $updateMarker -Destination (Join-Path $evidenceDir 'update.marker')
    Copy-Item -LiteralPath $rollbackMarker -Destination (Join-Path $evidenceDir 'rollback.marker')
    $artifactLines = foreach ($entry in @(
            @{ Version = $v1; Path = $artifactsV1.Installer },
            @{ Version = $v2; Path = $artifactsV2.Installer }
        )) {
        $hash = (Get-FileHash -LiteralPath $entry.Path -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  [$($entry.Version)]  $(Split-Path -Leaf $entry.Path)"
    }
    $artifactLines | Set-Content -LiteralPath (Join-Path $evidenceDir 'artifacts.sha256')
    $summary = [ordered]@{
        status                        = 'PASS'
        channel                       = $channel
        base_url                      = "http://127.0.0.1:$Port"
        update                        = "$v1 -> $v2"
        rollback                      = "$v2 -> $v1"
        installed_exe_v1_sha256       = $installedV1Hash.ToLowerInvariant()
        installed_exe_v2_sha256       = $installedV2Hash.ToLowerInvariant()
        installed_exe_rollback_sha256 = $installedRollbackHash.ToLowerInvariant()
        update_marker                 = $updateMarker
        rollback_marker               = $rollbackMarker
    }
    $summary | ConvertTo-Json -Depth 4 |
        Set-Content -LiteralPath (Join-Path $evidenceDir 'summary.json')

    Step 'Drill completed'
    Get-Content -LiteralPath (Join-Path $evidenceDir 'summary.json') | Out-Host
    Write-Host 'POLYPACK_UPDATER_DRILL_RESULT: PASS'
    exit 0
} catch {
    Write-Host 'POLYPACK_UPDATER_DRILL_RESULT: FAIL'
    Write-Host $_
    exit 1
} finally {
    if ($null -ne $server -and -not $server.HasExited) {
        Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $uninstaller) {
        Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -ErrorAction SilentlyContinue |
            Out-Null
    }
}
