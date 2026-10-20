# Environment slot close-out tracker (ENV-SLOTS)

> Status snapshot: 2026-10-20 · Baseline: `main` (same revision as the current `TASKS.md` / `GA.md`).
> This tracker records **environment slots only**: acceptance legs whose in-repo structure / mock / command-assembly parts already run, but whose real toolchain, physical device, real cloud, or human sign-off has not executed and therefore still prints SKIP-with-print.
> Rule: `SKIP-with-print` is not PASS. A slot may not be marked ✅ in TASKS / GA without raw evidence (command, exit code, tool versions, artifact digest). 中文版：[ENV-SLOTS.md](ENV-SLOTS.md)

## 1. Scope and status definitions

Environment slots are tracked separately from code gaps: a slot closes with environment / credentials / a host; a code gap must be wired in code first, and environment variables alone will not close it.

| Status | Meaning |
| --- | --- |
| `OPEN` | No real-run evidence; with the gate enabled the real leg still skips or fails |
| `PARTIAL` | Structure / mock / command-assembly legs run; the real leg is still missing |
| `CODE-GAP` | Code wiring is missing; environment variables alone cannot close it |
| `HUMAN` | Requires a human or management action; a runner cannot complete it |

| Mode | Meaning |
| --- | --- |
| `CI` | Completable on a hosted or self-hosted runner with credentials / toolchain / KVM |
| `HOST` | Requires a physical device, GUI, LAN, or interactive login |
| `HUMAN` | Requires an organizational process or human sign-off |
| `CLOSED` | Real evidence archived and reviewed; the slot is closed |

## 2. Slot index
| ID | Linked task | Slot | Status | Mode | Owner | Evidence | Existing CI |
| --- | --- | --- | --- | --- | --- | --- | --- |
| ENV-01 | T-M1-05 | Windows real Tauri sample + default Program Files launch | CLOSED | HOST | release-sec | run 39308849058 · windows-msi-real | rust-ci |
| ENV-02 | T-M2-10 | macOS `codesign` + `notarytool` real execution | PARTIAL | CI | release-sec | §3 ENV-02 | mobile-contract |
| ENV-03 | T-M2-13 / T-M8-09 | macOS farm worker → IPA | OPEN | HOST | infra + release-sec | §3 ENV-03 | — |
| ENV-04 | T-M3-05 / T-M3-06 | WeChat / Alipay / Douyin devtools preview | PARTIAL | HOST | core + release-sec | §3 ENV-04 | miniprogram-contract |
| ENV-05 | T-M4-15 | Roku real-device sideload | PARTIAL | HOST | core | §3 ENV-05 | — |
| ENV-06 | T-M5-07 | OpenBSD VM install smoke | PARTIAL | CI | infra | §3 ENV-06 | bsd-ci |
| ENV-07 | T-M5-08 | NetBSD `bmake` / pkgsrc | OPEN | CI | infra | §3 ENV-07 | — |
| ENV-08 | T-M6-03 / T-M6-09 | QNX SDP + QEMU smoke | PARTIAL | CI | infra | §3 ENV-08 | embedded-ci |
| ENV-09 | T-M6-05 | VxWorks `wr-cc` link leg | PARTIAL | CI | infra | §3 ENV-09 | embedded-ci |
| ENV-10 | T-M7-04 | Wear OS device / emulator smoke | PARTIAL | CI | infra | §3 ENV-10 | wear-release-smoke |
| ENV-11 | T-M7-05 / T-M7-06 | watchOS real `xcodebuild` | PARTIAL | CI | core + release-sec | §3 ENV-11 | — |
| ENV-12 | T-M7-07 | Fitbit `fitbit-build` self-check | PARTIAL | CI | infra | §3 ENV-12 | — |
| ENV-13 | T-M7-08 | Zepp `zeus` + simulator install | PARTIAL | HOST | core | §3 ENV-13 | — |
| ENV-14 | T-M7-09 | Garmin Connect IQ SDK / devkey compile | PARTIAL | CI | core | §3 ENV-14 | — |
| ENV-15 | T-M8-03 | keyring trust-root carrier | OPEN | CI | release-sec | §3 ENV-15 | — |
| ENV-16 | T-M8-03 | Vault trust-root carrier | OPEN | CI | release-sec + core | §3 ENV-16 | — |
| ENV-17 | T-M8-09 | Real S3 / OSS round-trip + farm second run | OPEN | CI | infra + release-sec | §3 ENV-17 | — |
| ENV-18 | T-M2-12 | Branch protection admin bypass | CLOSED | HUMAN | maintainer | §3 ENV-18 | secret-scan |
| ENV-19 | GA §7.2 | Human external developer trial | OPEN | HUMAN | docs | §3 ENV-19 | — |

Owners use only five roles: `release-sec` / `core` / `docs` / `infra` / `maintainer`; combinations use ` + `. The evidence column points at the §3 run card; the real evidence bundle is still archived per §4.2 and this table never fabricates evidence. Real per-slot close-out remains external work; unclosed slots keep their current status.
## 3. Slot run cards

### ENV-01 · Windows real Tauri sample + default Program Files launch

- Linked task: T-M1-05 (Windows packagers), T-M1-03 (Tauri layout). Gap: the NSIS smoke uses a rustc fixture PE and a `/D=` temp directory; a real Tauri sample, the default `$PROGRAMFILES64\Pax Notes`, and Start-menu launch are unverified. `real_tauri_core_binary_resolves_injected_resources` currently pins `Host::Linux` and cannot produce Windows evidence.
- Environment / toolchain / credentials: windows-2022 or a physical Windows 10/11 host, WebView2 Runtime, NSIS, WiX v3.14, Tauri CLI, a real Tauri sample project; no cloud credentials.
- Gates: `POLY_TAURI_CORE_BIN=<real Tauri executable>`; the real MSI run also needs `POLY_MSI_REAL=1` and `POLY_MSI_REAL_OUT=<evidence dir>`; the NSIS tests need no extra gate.
- Commands:
```powershell
cargo test -p poly-bundle --test packagers_windows nsis_real_build_produces_pe_installer -- --nocapture
cargo test -p poly-bundle --test packagers_windows nsis_install_launch_uninstall_smoke -- --nocapture
$env:POLY_TAURI_CORE_BIN = "C:\work\pax-notes\target\release\pax-notes.exe"
cargo test -p poly-bundle --test tauri_layout real_tauri_core_binary_resolves_injected_resources -- --nocapture
$env:POLY_MSI_REAL = "1"; $env:POLY_MSI_REAL_OUT = "C:\work\msi-evidence"
cargo test -p poly-bundle --test windows_msi_real -- --nocapture
poly build --target windows
msiexec /i ".polypack\packages\windows\Pax Notes_1.2.3_x64_en-US.msi" /qn
& "$env:ProgramFiles\Pax Notes\pax-notes.exe" --version
```
- Progress (2026-10-20): the real Tauri v2 sample `examples/windows-msi-app/`, `crates/poly-bundle/tests/windows_msi_real.rs`, and the rust-ci `windows-msi-real` job have landed; real MSI / install / launch / uninstall / residue evidence awaits the first `windows-2022` run, so the status stays `PARTIAL`.
- Evidence: output of both NSIS tests; `--version` output and a window screenshot after installing the real sample into the default path; SHA-256 of `Pax Notes_1.2.3_x64-setup.exe`; the install directory is gone after uninstall. The current `tauri_layout` assertion is Linux-only; the Windows real-sample leg needs a new test or external script, and Linux output must not be presented as Windows evidence.
- Status: `CLOSED` (2026-10-20). Run 39308849058's `windows-msi-real` job passed on windows-2022: build MSI → `msiexec /i /qn` → launch the installed copy with `--smoke` → `msiexec /x /qn` → check install-directory / Start-menu / HKCU / ARP residue; logs, MSI, `.wxs`, `smoke-marker.txt`, and digests are archived as CI artifacts. Owner: release-sec.
- Next action: none (the slot is closed); re-run only when Windows / WebView2 / WiX versions change.
- Blocker: none; visual GUI rendering still needs an interactive desktop, but install / default-path launch / exit / uninstall / residue are covered by CI.

### ENV-02 · macOS `codesign` + `notarytool` real execution

- Linked task: T-M2-10 (iOS signing and notarization). Gap: `ios_sign` only asserts command assembly and zero plaintext credentials; `ios_real_run` produces an unsigned IPA; real `codesign`, `notarytool submit`, `stapler`, and `spctl` have not run with Apple credentials.
- Environment / toolchain / credentials: macOS 14 + Xcode, Apple Distribution certificate, Team ID, Apple ID + app-specific password (or a notarytool keychain profile), `xcrun` / `codesign` / `ditto` / `spctl`.
- Gates: `POLYPACK_IOS_IDENTITY`, `POLYPACK_IOS_TEAM_ID`, `POLYPACK_APPLE_ID`, `POLYPACK_APPLE_APP_PASSWORD`; or store the `apple-codesign` profile first.
- Commands:
```sh
xcrun notarytool store-credentials apple-codesign \
  --apple-id "$POLYPACK_APPLE_ID" --team-id "$POLYPACK_IOS_TEAM_ID" \
  --password "$POLYPACK_APPLE_APP_PASSWORD"
poly build --target ios --config /srv/proj/polypack.toml
APP="/srv/proj/.polypack/packages/ios/export/Payload/Pax Notes.app"
codesign --verify --deep --strict --verbose=2 "$APP"
ditto -c -k --keepParent "$APP" /tmp/pax-ios.zip
xcrun notarytool submit /tmp/pax-ios.zip --keychain-profile apple-codesign --wait
xcrun stapler staple "$APP" && xcrun stapler validate "$APP"
spctl --assess --type execute --verbose=4 "$APP"
```
- Evidence: notarytool `status: Accepted` and submission id; green `codesign --verify`, `stapler validate`, and `spctl` output; SHA-256 of the IPA / `.app`; certificate fingerprint (no private key). Logs must never contain the app-specific password.
- Status: `PARTIAL`. Command chain and credential references are tested; real signing / notarization waits on the Apple gate. Owner: release-sec.
- Next action: configure Apple secrets on macos-14, run the commands once, and record the submission id in TASKS T-M2-10.
- Blocker: the Apple developer account, certificate, and notarization credentials are held by the organization; without them evidence cannot be faked. Execution mode: CI (macos-14 + secrets) can close it; no physical device required.

### ENV-03 · macOS farm worker → IPA

- Linked task: T-M2-13 (remote farm v0), T-M8-09 (farm deployment guide). Gap: the Linux two-worker drill is green; macOS worker registration, cross-host submit, IPA landing in the shared farm directory, and macOS-side verification have not run.
- Environment / toolchain / credentials: macOS worker (full Xcode, `xcode-select`, `rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios`), Linux submit host, a farm directory / shared cache visible at the same path on both sides, Apple signing credentials.
- Gates: `POLYPACK_IOS_IDENTITY`, `POLYPACK_IOS_TEAM_ID`; farm-side `--cache` / `--work-dir`.
- Commands:
```sh
# macOS worker
poly farm worker --dir /srv/polypack-farm --id mac-01 --target ios --jobs 0 --idle 120 \
  --cache /srv/polypack-cache --work-dir /var/tmp/polypack-work
# Linux submit (project and farm directory visible at the same path)
poly farm submit --dir /srv/polypack-farm --config /srv/proj/polypack.toml \
  --target ios --wait 1800 --retries 1 --project-dir /srv/proj \
  --cache /srv/polypack-cache
APP=$(find .polypack/packages/ios -path '*/export/Payload/*.app' -print -quit)
codesign --verify --deep --strict --verbose=2 "$APP"
spctl --assess --type execute --verbose=4 "$APP"
```
- Evidence: both logs contain `worker 已注册：mac-01（aarch64-apple-darwin，宿主 Macos）` and `农场任务完成：1 个任务 / 1 个 worker`; IPA path and SHA-256; `codesign` / `spctl` output; side-by-side `ls -l` of the farm directory on both hosts.
- Status: `OPEN`. FARM-DEPLOY §9 is still SKIP-with-print; no macOS worker is in the nightly. Owner: infra + release-sec.
- Next action: prepare a macOS host, execute FARM-DEPLOY §9, archive both transcripts, then remove this row from GA §7.1.
- Blocker: a macOS host and a cross-host shared mount are required; a hosted Linux runner cannot run Xcode / codesign. Execution mode: physical macOS host + Linux submit host; not inside this repo's CI.

### ENV-04 · WeChat / Alipay / Douyin devtools preview

- Linked task: T-M3-05 (Taro adapter), T-M3-06 (mini-program build pipeline). Gap: `contract_devtools_real_preview` calls the three CLIs under `POLY_CONTRACT_REAL=1`; when a tool is absent or logged out it only prints NOTE / SKIP. No real project preview QR code / device preview has been produced.
- Environment / toolchain / credentials: macOS (best official devtools support), WeChat DevTools CLI, Alipay Mini Program Studio `minidev`, Douyin DevTools `tma`, logged-in accounts on all three (QR / 2FA); the Douyin token can be supplied to CI.
- Gates: `POLY_CONTRACT_REAL=1`, `POLYPACK_WECHAT_DEVTOOLS_CLI`, `POLYPACK_ALIPAY_DEVTOOLS_CLI`, `POLYPACK_DOUYIN_DEVTOOLS_CLI`, `POLYPACK_WECHAT_DEVTOOLS_LOGIN`, `POLYPACK_ALIPAY_DEVTOOLS_LOGIN`, `POLYPACK_DOUYIN_DEVTOOLS_LOGIN`, `POLYPACK_DEVTOOLS_LOGIN_FILE`.
- Commands:
```sh
POLY_CONTRACT_REAL=1 POLYPACK_WECHAT_DEVTOOLS_CLI=/Applications/wechatwebdevtools.app/Contents/MacOS/cli \
POLYPACK_ALIPAY_DEVTOOLS_CLI=/Applications/minidev POLYPACK_DOUYIN_DEVTOOLS_CLI=/Applications/tma \
POLYPACK_WECHAT_DEVTOOLS_LOGIN="$POLYPACK_WECHAT_DEVTOOLS_LOGIN" \
POLYPACK_ALIPAY_DEVTOOLS_LOGIN="$POLYPACK_ALIPAY_DEVTOOLS_LOGIN" \
cargo test -p poly-cli --test contract_m3_devtools contract_devtools_real_preview -- --nocapture
```
- Evidence: all three preview commands exit 0; WeChat / Alipay preview QR screenshot or URL; Douyin remote preview response; tool versions and logged-in account (redacted); full `contract_devtools_real_preview` output with no SKIP.
- Status: `PARTIAL`. Mock and command assembly run; real preview depends on GUI login, and the miniprogram-contract `real` job only prints NOTE when vars / secrets are absent. Owner: core + release-sec.
- Next action: log in to all three tools on macOS, set CLI paths and login state, run the module test, and save QR / URLs; prefer the passwordless Douyin token for CI.
- Blocker: WeChat / Alipay login involves QR or 2FA and cannot be stored in the public repo; Douyin accounts also require qualification review. Execution mode: GUI login needs physical macOS; once login state exists the command leg can run in CI.

### ENV-05 · Roku real-device sideload

- Linked task: T-M4-15 (Roku channel packager and verifier). Gap: 19 structure checks run; the `POLY_ROKU_REAL=1` ECP probe and `plugin_install` have not executed against a real Roku.
- Environment / toolchain / credentials: Roku device (developer mode), a host on the same LAN, device IP, developer password, `curl`, `zip` / `unzip`.
- Gates: `POLY_ROKU_REAL=1`, `ROKU_DEV_TARGET=<ip>`, `ROKU_DEV_PASSWORD=<pwd>`.
- Commands:
```sh
cd ui/adapters/roku
POLY_ROKU_REAL=1 ROKU_DEV_TARGET=<ip> ROKU_DEV_PASSWORD=<pwd> node scripts/build-and-check.mjs
curl --connect-timeout 2 -s "http://<ip>:8060/"
```
- Evidence: script prints `ECP 设备: <name>` and `plugin_install 侧载成功`; screenshot of the channel launching on the Roku; SHA-256 of `ui/adapters/roku/dist/polypack-spike-channel.zip`; device model / firmware.
- Status: `PARTIAL`. Structure checks and zip layout run; real sideload is SKIP-with-print and has no CI job. Owner: core.
- Next action: run the commands on a developer-mode Roku, screenshot, and archive; if the device can stay online, add a self-hosted runner job.
- Blocker: a real Roku on the LAN plus its developer password; a hosted runner has no device network. Execution mode: physical device + same-LAN host; cannot run on hosted CI.

### ENV-06 · OpenBSD VM install smoke

- Linked task: T-M5-07 (OpenBSD ports target). Gap: bsd-ci has an `openbsd` job but it is `continue-on-error: true`; the OpenBSD VM fails to boot over TCG on hosted runners, so no green install evidence exists.
- Environment / toolchain / credentials: a KVM-capable self-hosted runner (or physical OpenBSD host), OpenBSD 7.x image, `pkg_add rust`, ports tree, `signify`.
- Gates: no dedicated gate; driven by the `vmactions/openbsd-vm` job. Remove `continue-on-error` after the first green run.
- Commands:
```sh
pkg_add rust
cargo build -p poly-cli --ignore-rust-version
./target/debug/poly init /tmp/poly-openbsd && cd /tmp/poly-openbsd
./target/debug/poly build --target openbsd
cd /usr/ports/x11/<port> && make package PACKAGES=/tmp/openbsd-packages
pkg_add /tmp/openbsd-packages/<pkg>.tgz && pkg_info && pkg_check
```
- Evidence: `make package` and `pkg_add` exit 0; `pkg_info` shows the package; `pkg_check` has no errors; green bsd-ci `openbsd` log and artifact.
- Status: `PARTIAL`. The job is wired but the host lacks virtualization; only SKIP / failure logs exist, with no install evidence. Owner: infra.
- Next action: add a KVM self-hosted runner label to bsd-ci, get one green run, then flip `continue-on-error` to false.
- Blocker: hosted ubuntu-latest has no KVM; OpenBSD boot over TCG times out. Execution mode: CI (KVM self-hosted) can close it.

### ENV-07 · NetBSD `bmake` / pkgsrc

- Linked task: T-M5-08 (BSD-family pkgsrc target). Gap: `bmake package` and the NetBSD VM run are absent; the repo only asserts pkgsrc file structure / quoting. bsd-ci has no NetBSD job.
- Environment / toolchain / credentials: NetBSD 10 VM / host, `pkgin install rust bmake`, pkgsrc tree, `pkg_add`; no signing credentials (a self-built repo needs separate config).
- Gates: no dedicated gate; `poly build --target netbsd` emits a pkgsrc project.
- Commands:
```sh
pkgin install rust bmake
cargo build -p poly-cli --ignore-rust-version
./target/debug/poly init /tmp/poly-netbsd && cd /tmp/poly-netbsd
./target/debug/poly build --target netbsd
cd <generated pkgsrc dir> && bmake package
pkg_add <artifact>.tgz && pkg_info && pkg_info -L <pkg>
```
- Evidence: `bmake package` exit 0; `pkg_add` / `pkg_info` output; `.tgz` SHA-256; NetBSD version and `bmake -V MAKE_VERSION`.
- Status: `OPEN`. pkgsrc generation and structure assertions run; the `bmake` leg is SKIP-with-print and has no CI job. Owner: infra.
- Next action: add a `netbsd` job to bsd-ci (vmactions/netbsd-vm or self-hosted), run structure first, then `bmake package`.
- Blocker: no NetBSD host; the first pkgsrc bootstrap and image bandwidth are large. Execution mode: CI (NetBSD VM / self-hosted) can close it.

### ENV-08 · QNX SDP + QEMU smoke

- Linked task: T-M6-03 (QNX target), T-M6-09 (QNX QEMU smoke). Gap: the embedded-ci QNX entry SKIPs when `vars.POLYPACK_QNX_SDP` is absent; real SDP cross-compile, `.ifs` packaging, and QEMU serial self-check have not run.
- Environment / toolchain / credentials: QNX SDP 7.1 (`qcc` / `nto*-gcc`, `mkifs`), QNX license, `source qnxsdp-env.sh`, `qemu-system-x86_64` (or board QEMU); optional `POLYPACK_QNX_QEMU_ARGS`.
- Gates: `POLYPACK_QNX_SDP`, `POLYPACK_QNX_QEMU`, `POLYPACK_QNX_QEMU_ARGS`; CI-side repository variable `vars.POLYPACK_QNX_SDP`.
- Commands:
```sh
export POLYPACK_QNX_SDP=/opt/qnx710
source "$POLYPACK_QNX_SDP/qnxsdp-env.sh"
poly doctor --json
poly init /tmp/poly-qnx && cd /tmp/poly-qnx
poly build --target qnx --json
POLYPACK_QNX_QEMU=/usr/bin/qemu-system-x86_64 poly verify --target qnx --json
```
- Evidence: `doctor` finds `qcc` / `nto*-gcc`; build JSONL contains a `verified` event for `qnx-image-structure`; QEMU serial log and `result.json`; `.ifs` / static library SHA-256.
- Status: `PARTIAL`. SDP probing, structure verification, and the CI matrix are wired; SDP is absent, so real compile / QEMU smoke SKIP. Owner: infra.
- Next action: install SDP 7.1 on a self-hosted runner, set `vars.POLYPACK_QNX_SDP`, and remove QNX from the job-summary SKIP list after the first green run.
- Blocker: QNX SDP requires a commercial license and cannot be downloaded into a hosted runner. Execution mode: CI (self-hosted runner + license) can close it.

### ENV-09 · VxWorks `wr-cc` link leg

- Linked task: T-M6-05 (VxWorks target). Gap: library / image packaging and the `nm` allowlist run; `wr-cc` is absent, so the link self-check is SKIP-with-print.
- Environment / toolchain / credentials: Wind River Workbench / VxWorks 7 SDK, `wr-cc`, target triple (default `x86_64-wrs-vxworks`), license; a nightly toolchain for `-Z build-std`.
- Gates: `WR_CC`, `POLYPACK_VXWORKS_TRIPLE`.
- Commands:
```sh
export WR_CC=/opt/windriver/wr-cc
export POLYPACK_VXWORKS_TRIPLE=x86_64-wrs-vxworks
poly doctor --json
poly init /tmp/poly-vx && cd /tmp/poly-vx
poly build --target vxworks --json
```
- Evidence: build JSONL contains a `verified` event for `vxworks-ld-check`; `check.sh` `wr-cc -c` and link commands exit 0; `nm` shows no unresolved symbols outside the allowlist; `.vxe` / library SHA-256; `wr-cc --version`.
- Status: `PARTIAL`. Packager and structure assertions run; the `wr-cc` link leg SKIPs. Owner: infra.
- Next action: install the Workbench SDK on a self-hosted runner, set `WR_CC`, run once, and archive `check.sh` output.
- Blocker: the VxWorks SDK and license require a commercial purchase. Execution mode: CI (self-hosted runner) can close it.

### ENV-10 · Wear OS device / emulator smoke

- Linked task: T-M7-04 (Wear OS dual smoke). Gap: the mock leg runs; install → launch → cases → uninstall on a real AVD or watch has not executed. `wear-release-smoke` hard-blocks when `/dev/kvm` or the prebuilt APK is absent.
- Environment / toolchain / credentials: Linux KVM (`/dev/kvm`) or a physical Wear OS device with ADB authorization, Android SDK / `avdmanager`, Wear OS 5 (API 34) x86_64 image or prebuilt APK; `POLYPACK_WEAR_FORM_APK`.
- Gates: `POLYPACK_WEAR_FORM_APK`, `POLYPACK_WEAR_FORM_PACKAGE` (default `com.polypack.weatform`), `POLYPACK_WEAR_SMOKE_LOGS`.
- Commands:
```sh
adb devices
POLYPACK_WEAR_FORM_APK=/path/to/weatform.apk \
POLYPACK_WEAR_FORM_PACKAGE=com.polypack.weatform \
POLYPACK_WEAR_SMOKE_LOGS=/tmp/wear-logs \
node ui/smoke/wear/run.mjs --leg android --serial emulator-5554 \
  --apk "$POLYPACK_WEAR_FORM_APK" --logs /tmp/wear-logs
```
- Evidence: `result.json` (schema `polypack/wear-smoke/v1`), `logcat.txt`, screenshots, UIAutomator dump, build summary with commit and APK SHA-256; green `wear-release-smoke` job with a 90-day artifact.
- Status: `PARTIAL`. Mock / nightly jobs are ready; KVM and the APK are absent, so the real device leg has not run. Owner: infra.
- Next action: enable KVM on the runner, build the form APK and set `vars.POLYPACK_WEAR_FORM_APK`; or attach a permanently online watch.
- Blocker: hosted runners lack `/dev/kvm`; a physical watch needs ADB authorization and a stable connection. Execution mode: CI (KVM self-hosted) or physical device.

### ENV-11 · watchOS real `xcodebuild`

- Linked task: T-M7-05 (watchOS template), T-M7-06 (watchOS packaging and signing chain). Gap: on Linux `xcodebuild` returns `ToolMissing`; template build and `.xcarchive` → `Payload/*.app` are asserted only at the mock layer; a real macOS build has not run.
- Environment / toolchain / credentials: macOS 14 + Xcode (watchOS SDK / simulator), `xcodebuild`, `ditto`, `plutil`; signing additionally needs Apple credentials.
- Gates: `POLY_CONTRACT_REAL=1` (shared Apple chain); the watchOS packager gates on host automatically, with no dedicated gate.
- Commands:
```sh
poly init --target watchos /tmp/pax-watch && cd /tmp/pax-watch
xcodebuild -project PolyPackWatch.xcodeproj -scheme PolyPackWatch \
  -destination 'generic/platform=watchOS' build CODE_SIGNING_ALLOWED=NO
poly build --target watchos
xcodebuild archive -project PolyPackWatch.xcodeproj -scheme PolyPackWatch \
  -configuration Release -destination 'generic/platform=watchOS' \
  -archivePath /tmp/PolyPackWatch.xcarchive \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY=
```
- Evidence: `xcodebuild` `BUILD SUCCEEDED`; `.polypack/packages/watchos/export/Payload/<product>.app` exists; `Info.plist` / `PkgInfo` structure assertions; `.app` SHA-256; Xcode version.
- Status: `PARTIAL`. The 10-file template and packager mock assertions run; the macOS real build is missing and has no CI job. Owner: core.
- Next action: add a watchOS job on macos-14 (install Xcode, run both `xcodebuild` commands), or execute manually and archive.
- Blocker: requires macOS + Xcode; a Linux host can only return `poly::E_BND_0001` with the R3 farm hint. Execution mode: CI (macos-14) can close it.

### ENV-12 · Fitbit `fitbit-build` self-check

- Linked task: T-M7-07 (Fitbit OS packager). Gap: `.fba` three-layer structure checks and goldens run; `fitbit-build` is not on PATH, so the CLI self-check is SKIP-with-print.
- Environment / toolchain / credentials: Node 18+, `@fitbit/sdk-cli` (provides `fitbit-build`), npm network; no hardware.
- Gates: no dedicated gate; the tool on PATH makes it real.
- Commands:
```sh
npm install -g @fitbit/sdk-cli
fitbit-build --help
cargo test -p poly-cli --test contract_m7_wear_js -- --nocapture
```
- Evidence: `fitbit-build --help` exit 0; `poly verify --target fitbit --json` notes no longer contain the `fitbit-build` SKIP-with-print; `.fba` SHA-256 and structure assertion output.
- Status: `PARTIAL`. Packager and structure checks run; the official CLI self-check is absent. Owner: infra.
- Next action: install `@fitbit/sdk-cli` in CI and turn the Fitbit leg of `contract_m7_wear_js` from SKIP to REAL; the official CLI only checks presence, it does not rebuild this project (which does no TS / rollup).
- Blocker: npm package and account availability; no hardware requirement. Execution mode: CI (ubuntu / macOS) can close it.

### ENV-13 · Zepp `zeus` + simulator install

- Linked task: T-M7-08 (Zepp OS packager). Gap: `.zab` structure and md5 naming run; `zeus --version` SKIPs because the tool is absent; the simulator install leg always SKIP-with-print in verify (the code explicitly does not automate it).
- Environment / toolchain / credentials: Node + Zepp OS `zeus` CLI (`@zeppos/zeus-cli`, per official docs), Zepp OS simulator (GUI), developer account (install / preview as officially required).
- Gates: no dedicated gate; `zeus` on PATH makes the self-check real. The simulator requires manual operation.
- Commands:
```sh
npm install -g @zeppos/zeus-cli
zeus --version
cargo test -p poly-cli --test contract_m7_wear_js -- --nocapture
# Simulator: manually install .polypack/packages/zepp/*.zab into the Zepp OS simulator and screenshot
```
- Evidence: `zeus --version` output; verify notes no longer contain the `zeus` SKIP; simulator install / launch screenshot and `.zab` SHA-256; simulator version.
- Status: `PARTIAL`. Packager and structure assertions run; both the `zeus` and simulator legs are absent. Owner: core.
- Next action: install zeus in CI to close the self-check leg; run the simulator leg manually and attach screenshots to the ENV-13 issue.
- Blocker: the simulator is a GUI app and cannot be installed automatically on a headless runner; the `zeus` package name and licensing follow official docs. Execution mode: the `zeus` self-check can run in CI; simulator install needs a physical GUI host.

### ENV-14 · Garmin Connect IQ SDK / devkey compile

- Linked task: T-M7-09 (Garmin Monkey C transpile spike). Gap: 29/29 Node tests and structure checks run; `monkeyc` / `CIQ_DEVKEY` are absent, so the Monkey C compile leg SKIPs (PASS 4 / FAIL 0 / SKIP 3).
- Environment / toolchain / credentials: Connect IQ SDK (`bin/monkeyc`), developer key `.der`, Java runtime; SDK download may be restricted by 403 / account.
- Gates: `CIQ_MONKEYC`, `CIQ_HOME`, `CONNECTIQ_HOME`, `CIQ_DEVKEY`.
- Commands:
```sh
CIQ_HOME=/opt/connectiq-sdk CIQ_MONKEYC=/opt/connectiq-sdk/bin/monkeyc \
CIQ_DEVKEY=/secure/developer_key.der npm --prefix ui/adapters/garmin run build
cd <generated dir>/round && monkeyc -e -w -r -o ../garmin-round.iq -y "$CIQ_DEVKEY" -f monkey.jungle
cd <generated dir>/square && monkeyc -e -w -r -o ../garmin-square.iq -y "$CIQ_DEVKEY" -f monkey.jungle
```
- Evidence: `npm run build` prints PASS 7 / FAIL 0 / SKIP 0; round / square `.iq` files and entry assertions; `monkeyc --version`; devkey fingerprint (no private key); compile log.
- Status: `PARTIAL`. Transpile and structure checks run; the compiler leg SKIPs; `poly build --target garmin` still has no built-in iq packager (G10 conditional task). Owner: core.
- Next action: install the Connect IQ SDK in CI and put the devkey in a secret; close the compiler leg. The iq packager is a separate conditional task and is not faked inside this slot.
- Blocker: SDK access may require a Garmin account; the devkey is developer-generated and must not enter the repo. Execution mode: CI (ubuntu / macOS + SDK) can close the compiler leg.

### ENV-15 · keyring trust-root carrier

- Linked task: T-M8-03 (plugin trust root). Gap: the env carrier is automated by `plugin_trust_root_env`; the keyring backend test (`keyring_real`) has an isolated recipe but is not in CI; a CLI E2E that resolves a trust root via `source = "keyring"` and loads a real plugin has not run.
- Environment / toolchain / credentials: Linux Secret Service (dbus + gnome-keyring), macOS Keychain, or Windows Credential Manager; `secret-tool` or an equivalent writer; a GPG keypair (trust-root payload).
- Gates: `POLY_KEYRING_REAL=1`, `DBUS_SESSION_BUS_ADDRESS`; CLI config `trust_root = { name = "plugin-trust-root", source = "keyring" }`.
- Commands:
```sh
mkdir -p -m 700 /tmp/poly-keyring-rt /tmp/poly-keyring-home/keyrings
dbus-run-session -- bash -c '
  export HOME=/tmp/poly-keyring-home XDG_RUNTIME_DIR=/tmp/poly-keyring-rt \
         XDG_DATA_HOME=/tmp/poly-keyring-home/.local/share \
         GNOME_KEYRING_CUSTOM_LOCAL_PATH=/tmp/poly-keyring-home/keyrings
  printf "poly-it-pass\n" | gnome-keyring-daemon --unlock --components=secrets --foreground &
  sleep 3
  POLY_KEYRING_REAL=1 cargo test -p poly-sign --test keyring_real -- --nocapture'
printf "%s" "$TRUST_ROOT_B64" | secret-tool store --label=plugin-trust-root \
  service polypack username plugin-trust-root
poly build --target linux-reference --json
```
- Evidence: `keyring_real` real write / read / delete output; `gdbus` cross-check that the entry exists; `poly build` emits `plugin_granted` / `packaged`, with the error path returning `E_PLUGIN_0010`; no plaintext payload in logs.
- Status: `OPEN`. Backend and isolation recipe are verified; the CLI keyring carrier and CI job are not wired. Owner: release-sec.
- Next action: add a `plugin_trust_root_keyring` CLI test (reuse the env-test project fixture, change only to `source = "keyring"`), and run it on rust-ci ubuntu under dbus-run-session + gnome-keyring.
- Blocker: headless Secret Service needs the isolation recipe in the file header; macOS / Windows need their own real keychain hosts, and Linux results must not be presented as theirs. Execution mode: CI (ubuntu + dbus / gnome-keyring) can close the Linux leg; macOS / Windows are separate.

### ENV-16 · Vault trust-root carrier

- Linked task: T-M8-03 (plugin trust root). Gap: `vault_real` verifies `VaultBackend` KV v2 read/write; `default_backends()` now adds `VaultBackend<HttpVaultClient>` when `VAULT_ADDR` / `VAULT_TOKEN` are available (plugin trust roots, publishing, and signing share the same backend chain), so the code gap is closed. The remaining work is a real Vault service plus a real-component CLI E2E.
- Environment / toolchain / credentials: Vault service (dev or real cluster), `VAULT_ADDR`, `VAULT_TOKEN`, KV v2 path; plugin public-key payload `base64(JSON)`.
- Gates: `POLY_VAULT_REAL=1`, `VAULT_ADDR`, `VAULT_TOKEN`; CLI config `source = "vault"`.
- Commands:
```sh
vault server -dev -dev-root-token-id=root &
export VAULT_ADDR=http://127.0.0.1:8200 VAULT_TOKEN=root
POLY_VAULT_REAL=1 cargo test -p poly-sign --test vault_real -- --nocapture
# With VAULT_* set, the default backend chain includes VaultBackend; this runs the real plugin trust-root E2E
poly build --target linux-reference --json
```
- Evidence (required before closure): the `vault_real` round-trip alone is insufficient; need real CLI-side `plugin_granted` / `packaged` and `E_PLUGIN_0010` error-path output, with no token in logs.
- Status: `OPEN` (environment slot: needs a real Vault service and credentials); the code side is wired, and `vault_real` remains gated by `POLY_VAULT_REAL=1`. Owner: release-sec + core.
- Next action: run the real E2E on a host with a Vault service (`vault_real` + CLI trust-root / publishing), with no token in logs.
- Blocker: needs a Vault service and credentials; the code gap is closed. Execution mode: CI (Vault dev server) can close it; no service is available now.

### ENV-17 · Real S3 / OSS round-trip + farm second run

- Linked task: T-M8-09 (remote farm deployment guide). Gap: the S3-compatible backend and mock contract are verified; PUT / GET / LIST / DELETE on a real bucket and a two-worker second-run cache hit have not run.
- Environment / toolchain / credentials: S3-compatible bucket (AWS S3 / MinIO / Alibaba Cloud OSS S3-compatible), endpoint / region / access key / secret / optional session token, bucket write permission.
- Gates: `POLYPACK_S3_REAL=1`, `POLYPACK_S3_TEST_URI=s3://<bucket>[/<prefix>]`, `POLYPACK_S3_ENDPOINT`, `POLYPACK_S3_REGION`, `POLYPACK_S3_ACCESS_KEY_ID`, `POLYPACK_S3_SECRET_ACCESS_KEY`, `POLYPACK_S3_SESSION_TOKEN`, `POLYPACK_S3_PATH_STYLE`, `POLYPACK_S3_OSS_COMPAT=1` (OSS).
- Commands:
```sh
POLYPACK_S3_REAL=1 POLYPACK_S3_TEST_URI=s3://bucket/ci-prefix \
POLYPACK_S3_ENDPOINT=https://s3.example.com POLYPACK_S3_REGION=us-east-1 \
POLYPACK_S3_ACCESS_KEY_ID=... POLYPACK_S3_SECRET_ACCESS_KEY=... \
cargo test -p poly-cache --test s3_real -- --nocapture
# Two-worker second run (same bucket prefix as shared cache)
poly farm worker --dir /srv/farm --id w1 --target web --cache s3://bucket/ci-prefix
for i in 1 2; do poly farm submit --dir /srv/farm --config /srv/proj/polypack.toml \
  --target web --cache s3://bucket/ci-prefix --wait 600; done
```
- Evidence: `s3_real` PUT / GET / LIST / DELETE all green; bucket object listing screenshot or `aws s3 ls`; second run `cache hit 1/1`; object key / bytes match local digests; credentials absent from logs.
- Status: `OPEN`. Gate and test are ready; no real bucket and no CI job. Owner: infra + release-sec.
- Next action: prepare a dedicated test bucket and prefix, configure secrets, run the unit test, then the two-worker second run; write the result back to GA §7.2.
- Blocker: real cloud account / bucket / credentials; the test bucket must allow object deletion. Execution mode: CI (secrets + egress) can close it.

### ENV-18 · Branch protection admin bypass

- Linked task: T-M2-12 (secret-scan and required checks). Original gap: 3-host secret-scan is a required check, but `enforce_admins=false` let admins push directly and bypass required checks; the admin constraint was enabled on 2026-10-20.
- Environment / toolchain / credentials: GitHub repository admin permission, `gh` CLI or the web settings.
- Gates: no environment variable; a GitHub setting.
- Commands:
```sh
gh api repos/erikwang2013/polypack/branches/main/protection --jq '.enforce_admins.enabled'
gh api repos/erikwang2013/polypack/branches/main/protection/required_status_checks --jq '.contexts'
# Target: enforce_admins.enabled == true; or record a time-bounded exception and four-eyes review in GA
```
- Evidence: the GitHub branch protection API reports `enforce_admins.enabled = true` and 9 required contexts (including `secret-scan / ubuntu-latest`, `secret-scan / macos-14`, and `secret-scan / windows-2022`); verified 2026-10-20 and recorded in the T-M2-12 heading and this section.
- Status: `CLOSED` (2026-10-20). The 9 required contexts are enforced, and admin direct pushes are subject to the same required checks. Owner: maintainer.
- Next action: keep the rule in place; if an emergency break-glass path is enabled, record the exception conditions, approver, and four-eyes review in GA §7, and restore `enforce_admins=true` afterwards.
- Blocker: none (closed); later changes still require repository admin permission. Execution mode: HUMAN / GitHub settings, not inside the code repository.

### ENV-19 · Human external developer trial

- Linked task: GA §7.2 "Human external developer trial". Gap: no real external developer has been recruited to add a custom target and produce a package within one day per GA §2; blockers are unrecorded.
- Environment / toolchain / credentials: 3–5 external developers not involved in this repo, clean machines (at least one each of Linux / macOS / Windows), GA / README / plugin template, feedback template and timing sheet.
- Gates: none; a purely human process.
- Commands:
```sh
# Each tester starts from zero and follows only docs/GA.md §2; record commands, time, and blockers
poly plugin new pocket-notes
poly plugin check <dir>
poly build --config <app>/polypack.toml --target pocket-notes
```
- Evidence: a trial report per tester (environment, start / end time, commands completed, verbatim blockers, whether a package was produced within one day); a summary issue; links to documentation-fix PRs.
- Status: `OPEN`. No external trial record yet. Owner: docs.
- Next action: after GA release, recruit 3–5 people, collect reports with the template, and write blockers back into GA / README / templates.
- Blocker: requires real external people and time; an agent cannot stand in. Execution mode: HUMAN; not in CI.

## 4. Close-out protocol

### 4.1 Roles and responsibilities

| Role | Responsibility |
| --- | --- |
| Slot owner | Prepare the environment, run commands, produce the evidence bundle, draft the TASKS / GA write-back |
| Reviewer | Confirm the raw log shows a real invocation, no SKIP, exit code 0, and evidence bound to the commit |
| release-manager | Merge the write-back, maintain GA §7, and ensure SKIP is never presented as PASS |

### 4.2 Evidence capture steps

1. Open an issue titled `[ENV-XX] <slot>` with the linked task, planned commands, expected evidence, and owner.
2. Build the CLI from the target commit and record `git rev-parse HEAD`, `poly --version`, and host `uname -a` / `winver` / `sw_vers`.
3. Set the environment per the run card; read credentials only from secrets / the local keychain, never from shell history or the repo.
4. Capture stdout / stderr / exit code with `tee` or runner logs; record the command itself after redaction.
5. Produce the evidence bundle and compute artifact SHA-256; screenshots cover only the necessary area and hide accounts / tokens / device passwords.
6. Run `secret-scan` or inspect the evidence to confirm no keys, private keys, app-specific passwords, or Vault tokens.
7. Attach the bundle to the issue (small files) or a GitHub Actions artifact / release asset (large files); TASKS / GA carry only the link, never the log.
8. The reviewer checks against the 4.3 list; on success, write TASKS.md, TASKS.en.md, GA.md, and GA.en.md in one PR.

### 4.3 Evidence bundle and redaction

| File | Content |
| --- | --- |
| `cmd.txt` | Actual commands (tokens / passwords replaced with `<redacted>`) |
| `versions.txt` | Host, OS, toolchain, SDK, `poly --version`, commit |
| `stdout.log` / `stderr.log` | Raw output and exit code; must show a real tool invocation, not only SKIP |
| `result.json` | Machine-readable result (Wear / QEMU / farm slots with a schema) |
| `sha256.txt` | SHA-256 of artifacts and evidence files |
| `screenshot.png` | GUI / device / preview QR (when needed, redacted) |

Suggested evidence path: `env-slots/<ENV-XX>/<UTC timestamp>/`. The repository does not commit binaries, logs, or screenshots; use issue / artifact links. Evidence must be bound to a commit and re-run after a major toolchain upgrade.

### 4.4 TASKS / GA write-back

- In TASKS, change the task-title note from "environment slot SKIP-with-print" to "ENV-XX closed: <evidence link> (YYYY-MM-DD, <tool version>)"; add the same link to the acceptance line. Chinese and English in the same commit.
- In GA §7.1, delete the corresponding gap row (or mark "closed (ENV-XX)" and keep history); in §7.2, delete the action or mark it done. Chinese and English in the same commit.
- Never change docs without evidence; never use a SKIP log as closure evidence; never present another platform / toolchain's output as this slot's.
- Commit messages: `test(env): close ENV-XX <slot>`; write-back: `docs(ga): record ENV-XX closure`.

## 5. Slot → CI workflow matrix

| Slot | Workflow | Job | Coverage and gap |
| --- | --- | --- | --- |
| ENV-01 | `.github/workflows/rust-ci.yml` | `rust / windows-2022` | NSIS real compile / install / launch; no real Tauri sample, no default Program Files |
| ENV-02 | `.github/workflows/mobile-contract.yml` | `ios / macos-14` | Apple chain runs; no Apple signing / notary secrets |
| ENV-03 | — | — | No farm job; FARM-DEPLOY §9 is manual |
| ENV-04 | `.github/workflows/miniprogram-contract.yml` | `real / macos-14` | CLI preview test wired; NOTE when vars / secrets absent |
| ENV-05 | — | — | No Roku job |
| ENV-06 | `.github/workflows/bsd-ci.yml` | `openbsd` | VM job exists, `continue-on-error`, TCG boot fails |
| ENV-07 | — | — | No NetBSD job |
| ENV-08 | `.github/workflows/embedded-ci.yml` | `embedded` (matrix `qnx`) | Build / doctor / QEMU wired; SKIP when SDP absent |
| ENV-09 | `.github/workflows/embedded-ci.yml` | `embedded` (matrix `vxworks`) | Build wired; SKIP when `wr-cc` absent |
| ENV-10 | `.github/workflows/wear-release-smoke.yml` | `wear-device-smoke` | Hard release gate; blocks when KVM / APK absent |
| ENV-11 | — | — | No watchOS job; mobile-contract ios does not run watchOS xcodebuild |
| ENV-12 | — | — | No `fitbit-build` job |
| ENV-13 | — | — | No `zeus` job |
| ENV-14 | — | — | No Connect IQ job |
| ENV-15 | — | — | No keyring job |
| ENV-16 | — | — | No Vault job; the default backend chain is wired from the environment |
| ENV-17 | — | — | No S3 job |
| ENV-18 | `.github/workflows/secret-scan.yml` | `scan` (3 hosts) | Supplies required checks; admin bypass was closed on 2026-10-20 and is not a workflow setting |
| ENV-19 | — | — | Human trial, no workflow |

## 6. Not closable in this repo

| Slot | External resource | Why the repo cannot close it |
| --- | --- | --- |
| ENV-01 (GUI leg) | Physical Windows desktop | Start-menu / default-install-dir interactive launch cannot be observed in a service session |
| ENV-03 | macOS host + shared farm directory | Needs Xcode / codesign and a cross-host mount; a Linux runner cannot stand in |
| ENV-04 | Three developer accounts and GUI login | QR / 2FA / account qualification are outside repository control |
| ENV-05 | Roku device on the LAN | ECP sideload needs a real device IP and developer password |
| ENV-10 | KVM or physical Wear device | ADB authorization and device / emulator are host resources |
| ENV-13 (simulator leg) | GUI simulator | Install / launch is a desktop app; verify explicitly does not automate it |
| ENV-18 | GitHub admin setting | Branch protection is repository-platform config, not code |
| ENV-19 | External developers | A human trial cannot be replaced by an agent or CI |

## 7. Open decisions and code gaps

| Item | Type | Disposition |
| --- | --- | --- |
| Vault default backend chain | Closed (code) | `default_backends()` assembles Vault from the environment; ENV-16 is down to the real-service E2E |
| Garmin built-in iq packager | Delivered | `poly build --target garmin` now has a built-in packager; ENV-14 closes only the compiler / device leg |
| Missing CI jobs | Infrastructure | NetBSD / S3 / keyring / Vault / watchOS / Fitbit / Zepp / Garmin / Roku have run cards only, no workflow |

## 8. References

- [TASKS](TASKS.md) · [GA](GA.md) · [FARM-DEPLOY](FARM-DEPLOY.md) · [EXPERIMENTAL-TARGETS](EXPERIMENTAL-TARGETS.md) · Chinese: [ENV-SLOTS.md](ENV-SLOTS.md)
