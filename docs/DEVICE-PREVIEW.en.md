# Pre-release device preview matrix (M9-E7)

> Status: T-M9-19 (device-farm research matrix) and T-M9-20 (`DevicePool` / `DeviceLease` protocol + `polypack preview --dry-run`) are delivered; **T-M9-21 real-device install / launch / screenshot is external environment work, not executed and not marked ✅**.
> Chinese: [DEVICE-PREVIEW.md](DEVICE-PREVIEW.md)

## 1. Status and scope

This document answers three questions: buy or build a device farm; what the device-pool and lease protocol looks like; which commands and evidence a real-device drill requires. `polypack preview --dry-run` only generates a deterministic plan: it does not lease devices, read credentials, or access the network. Real execution is left to T-M9-21's external devices and manually archived evidence.

Non-goals: a full device-farm product, unattended review, or promising real-device coverage for all 49 targets. Device availability (especially Android TV / Wear OS) changes with each provider catalog and must be checked at lease time.

## 2. Research method and evidence status

| Item | Value |
| --- | --- |
| Retrieval date | 2026-10-20 |
| Page fetch status | The fetch layer blocked all four official pages in this environment; they are marked **not fetched**. Details below come only from search-result snippets and are **unverified**; they are not procurement commitments. |
| Pricing basis | Billing units and mechanisms only. No exact amount is asserted; open the official pricing page and obtain a quote before budgeting. |
| Self-hosted basis | TCO lists cost items only (hardware, host, power, operator hours, consumables); no invented currency estimates. |

## 3. Vendor comparison

| Option | Supported platforms / devices (unverified) | API model | Pricing model (units) | Evidence (retrieved 2026-10-20) |
| --- | --- | --- | --- | --- |
| Firebase Test Lab | Android physical + virtual devices, iOS physical devices; Robo / instrumentation / Game Loop / XCTest. TV / Wear models depend on the catalog and were not verified. | `gcloud firebase test android\|ios run` and Testing API test matrices; results in the Firebase console / Cloud Storage. | Physical and virtual devices are billed per **device-hour** (different rates), rounded up per minute; the plan includes a daily no-cost allowance. No exact rate asserted. | https://firebase.google.com/docs/test-lab and https://firebase.google.com/docs/test-lab/usage-quotas-pricing — **not fetched**, search snippets only. |
| BrowserStack App Automate | Real Android / iOS devices; Appium, Espresso, XCUITest, Flutter, Detox (Android only). TV / Wear catalog not verified. | REST API (`api-cloud.browserstack.com`): upload the app, then start a framework session; CI integrations available. | Priced per **parallel session** (monthly / annual plans); the parallel count sets concurrency; test minutes are unmetered on the standard plan. No exact rate asserted. | https://www.browserstack.com/app-automate and https://www.browserstack.com/pricing — **not fetched**, search snippets only. |
| AWS Device Farm | Android / iOS physical devices; web apps via Appium / Selenium; private dedicated device fleet. TV / Wear catalog not verified. | AWS API / CLI: `ScheduleRun`, projects and device pools; private devices optionally in a VPC. | Metered per **device-minute**; or unmetered per **device slot-month** (one slot = one concurrent device). No exact rate asserted. | https://aws.amazon.com/device-farm/ and https://aws.amazon.com/device-farm/pricing/ — **not fetched**, search snippets only. |
| Self-hosted pool | Android via adb / STF / Appium; iOS needs a macOS host + Xcode / WebDriverAgent; TV / Wear depends on purchased hardware. | Directory or HTTP pool manager: lease → adb / Appium session → install / launch / screenshot / logcat → release. | Device purchase + USB hub / host + rack power + operator hours + spares; iOS adds a macOS host. Cost items only. | https://github.com/DeviceFarmer/stf and https://developer.android.com/tools/adb — **not fetched**, search snippets only. |

## 4. Build-vs-buy recommendation

1. **Buy on demand first by default**: the T-M9-21 drill leases once; do not buy a standing cloud plan or build a rack before real screenshots and logs exist.
2. **Self-host when Android / TV hardware already exists**: no per-minute fee, complete adb screenshot / logcat evidence, and full control over uninstall and release; the cost is operator time and spares.
3. **Buy for iOS and device breadth**: choose the cloud when iOS or high concurrency is required. Firebase for GCP-centric teams; BrowserStack for framework breadth and parallel sessions; Device Farm for AWS-native and private-fleet needs. Integrate one first; keep the others as fallbacks.
4. **Keep the protocol reversible**: the `preview/v1` `provider` field isolates vendor differences, and the dry-run plan is vendor-neutral; switching providers does not change the task model.
5. **Revisit with evidence**: count preview runs per month, queue time, and operator hours, then compare against cloud quotes; do not state a numeric break-even without real quotes.

## 5. DevicePool / DeviceLease protocol (preview/v1)

| Type | Field | Meaning |
| --- | --- | --- |
| DeviceSpec | provider | Device source: `firebase-test-lab` / `browserstack-app-automate` / `aws-device-farm` / `self-hosted` |
| DeviceSpec | device_id | Provider-specific device id (model, serial, or `host:port`) |
| DeviceSpec | os | Device OS (`android` / `ios` / `tvos`, etc.) |
| DeviceSpec | target | PolyPack target id; defaults to `os` |
| DevicePool | devices | Devices in declaration order; empty pools and duplicate provider+device are rejected |
| DeviceLease | lease_id | Stable plan-local id: `lease-0001`, `lease-0002`, ... |
| DeviceLease | provider | Device source |
| DeviceLease | device_id | Provider-specific device id |
| DeviceLease | os | Device OS |
| DeviceLease | target | PolyPack target id |
| DeviceLease | acquired_at | Lease start (Unix seconds) |
| DeviceLease | expires_at | Lease expiry (Unix seconds = acquired_at + timeout_secs) |
| DeviceLease | timeout_secs | Lease duration (seconds) |
| DeviceLease | cleanup | Cleanup semantics (below) |
| CleanupPolicy | on_success | Success path: `release` |
| CleanupPolicy | on_failure | Failure path: `release_and_archive` |
| CleanupPolicy | on_timeout | Timeout path: `force_release` |
| CleanupPolicy | uninstall | Whether to uninstall the fixture app before release |
| CleanupPolicy | evidence_dir | Evidence directory: `.polypack/preview/<target>/<device>/` |
| PreviewPlan | protocol | `preview/v1` |
| PreviewPlan | farm_protocol | `farm/v1` (the device pool extends farm protocol v1) |
| PreviewPlan | job_kind | `preview` (the task type for a future farm job frame) |
| PreviewPlan | dry_run | Always `true`: the plan never touches a device or the network |
| PreviewPlan | leases | Leases generated in device declaration order |

## 6. Dry-run command and plan

```bash
polypack preview --dry-run --json --at 0 --timeout 900 \
  --device "firebase-test-lab/Pixel 7/android" \
  --device "self-hosted/tv-lab-01/android/android-tv"
```

`--at 0` makes the output reproducible; `--timeout` is the duration of each lease in seconds; a device expression is `<provider>/<device-id>/<os>[/<target>]`, with target defaulting to os. `--json` prints one JSON line; without `--json` the same content is pretty-printed.

```json
{
  "protocol": "preview/v1",
  "farm_protocol": "farm/v1",
  "job_kind": "preview",
  "dry_run": true,
  "leases": [
    {
      "lease_id": "lease-0001",
      "provider": "firebase-test-lab",
      "device_id": "Pixel 7",
      "os": "android",
      "target": "android",
      "acquired_at": 0,
      "expires_at": 900,
      "timeout_secs": 900,
      "cleanup": {
        "on_success": "release",
        "on_failure": "release_and_archive",
        "on_timeout": "force_release",
        "uninstall": true,
        "evidence_dir": ".polypack/preview/android/Pixel 7/"
      }
    }
  ]
}
```

## 7. Lease, timeout and cleanup semantics

- Leases are numbered in `--device` declaration order; the same input and `--at` produce byte-identical JSON, so reruns can be diffed.
- `expires_at = acquired_at + timeout_secs` with an overflow check; `--timeout 0` and overflow are rejected with `polypack::E_PLAN_0001`.
- Success releases; failure releases and archives; timeout force-releases. All three paths uninstall the fixture app and release the device from the provider.
- The evidence directory is fixed at `.polypack/preview/<target>/<device>/`; device / os / target values containing `/`, `\`, control characters, or equal to `.` / `..` are rejected to prevent evidence-path escape.
- Dry-run is a pure function path: no credentials, no child processes, no sockets. Real execution is not wired; omitting `--dry-run` fails with `polypack::E_FARM_0005` instead of faking a result.

| Condition | Stable error code | Status |
| --- | --- | --- |
| Invalid provider / device expression / empty list / duplicate device | `polypack::E_FARM_0002` | Implemented |
| Invalid or overflowing lease duration | `polypack::E_PLAN_0001` | Implemented |
| Real execution without `--dry-run` | `polypack::E_FARM_0005` | Implemented (explicit refusal) |
| Lease expires before completion | `polypack::E_FARM_0004` | Planned, not implemented |
| Remote install / launch failure | `polypack::E_FARM_0003` | Planned, not implemented |

## 8. T-M9-21 external device drill (not delivered)

Status: **EXTERNAL / OPEN, not executed, not marked ✅**. It requires one real Android device and one real Android TV device (or cloud-farm equivalents), adb, a fixture APK, and a writable evidence directory. The commands below use a self-hosted pool; cloud farms perform the same install / launch / screenshot / log steps through their APIs.

### 8.1 Commands (self-hosted Android / Android TV)

```bash
# 0. Deterministic lease plan (archive first to prove devices and time window)
polypack preview --dry-run --json --at 0 --timeout 900 \
  --device "self-hosted/<android-serial>/android" \
  --device "self-hosted/<android-tv-serial>/android/android-tv"

# 1. Devices and fixture
adb devices -l
adb -s <serial> shell getprop ro.build.version.sdk
adb -s <serial> install -r -t .polypack/packages/android/app-release.apk

# 2. Launch and screenshot (same for Android and Android TV; TV may use D-pad to confirm focus)
adb -s <serial> shell am start -W -n <applicationId>/<activity>
adb -s <serial> exec-out screencap -p > .polypack/preview/<target>/<device>/launch.png
adb -s <serial> shell dumpsys window | grep -E 'mCurrentFocus|mFocusedApp' \
  > .polypack/preview/<target>/<device>/focus.txt

# 3. Logs
adb -s <serial> logcat -d > .polypack/preview/<target>/<device>/logcat.txt
adb -s <serial> logcat -d -b crash > .polypack/preview/<target>/<device>/crash.txt

# 4. Cleanup
adb -s <serial> shell pm uninstall <applicationId>
adb devices -l
```

### 8.2 Required evidence and error codes

| Evidence | Requirement |
| --- | --- |
| Plan | Raw `polypack preview --dry-run --json` output with lease_ids and the time window |
| Artifact | APK path + blake3 digest from the build receipt (`polypack build --json`) |
| Device | `adb devices -l` output, model, and API level (`getprop ro.build.version.sdk`) |
| Install | Exact `adb install` command and exit code |
| Launch | `am start -W` output with `Status: ok`, and the focused activity equal to the target activity |
| Screenshot | One PNG each for Android and Android TV, with path and SHA-256 |
| Logs | Full logcat and crash buffer archived in the evidence directory |
| Cleanup | Uninstall exit code, `pm list packages` confirming removal, and post-drill `adb devices -l` |

Failures must preserve the raw commands, exit codes, and logs, and map to the stable error codes in section 7. Emulator or web-page screenshots must never be presented as real-device evidence.

## 9. Risks and limits

- None of the four official pages was fetched this time, so matrix details are not verified against the source; confirm pricing pages and device catalogs before procurement.
- Cloud catalogs change constantly and TV / Wear devices are not guaranteed; a generated lease does not mean a device is available.
- A self-hosted pool has hidden costs in operations, power, USB stability, and spares; it is not free.
- Screenshots may contain user data: run fixture apps only and expire evidence directories on a retention schedule.
- Until T-M9-21 is complete, this matrix is not evidence that pre-release real-device regression is covered.
