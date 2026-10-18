# Remote Farm: Origin and Build Site

> Companion reading: [ROADMAP.en.md](ROADMAP.en.md) §7 (security and secrets), §8 (test and CI matrix), §9 M2 (risk R3: Apple-family targets require a macOS host).
> Deployment guide: [FARM-DEPLOY.en.md](FARM-DEPLOY.en.md) (topology / worker pool / isolation / cache / credentials / drill).
> 中文版: [FARM.md](FARM.md)

## Why a farm is needed

Apple-family targets (iOS / macOS / tvOS) can only produce real artifacts on a macOS host: `xcodebuild`, `lipo` and `codesign` do not ship with Linux or Windows. Windows and Linux users therefore need a **build site**: a macOS machine that runs the packaging chain on their behalf and hands the artifacts back. Protocol v0 (T-M2-13) splits this into two roles and makes both sides share one DAG implementation and one cache-key formula — local and remote builds never grow a second set of semantics.

## Origin / build site

| Role | Responsibility | What it needs |
|---|---|---|
| **Origin** | Declares what to build (`polypack.toml` + `--target`), submits the job, collects the artifact list | The `poly` binary, project source, a farm directory shared with the build site; **no** target-platform toolchain |
| **Build site** | Runs the job: compile, package, pull signing material on demand | The `poly` binary, platform toolchains (Xcode / Android SDK, …), credential backends; **no** developer-local repo state |

In one sentence: the **origin** decides *what* to build, the **build site** decides *how* and owns the credentials exclusively. Credentials never cross that boundary (see "Credential model" below).

## Protocol v0

The wire format is JSON Lines (one UTF-8 JSON object per line), version string `farm/v0`; a version mismatch fails with `poly::E_FARM_0001`.

| Frame | Direction | Payload |
|---|---|---|
| `hello` | worker → master | `protocol` + `worker`: `WorkerCaps { worker_id, triple, host, targets }` |
| `hello_ack` | master → worker | `protocol` + `accepted` + `reason` (present when rejected) |
| `job` | master → worker | `job_id` + `project_dir` (build-site path) + `spec` (the `JobSpec` wire form) |
| `result` | worker → master | `job_id` + `ok` + `error` (`RemoteDiagnostic`: code + message + fix hint) + `artifacts` (`ArtifactRecord`: name / blake3 digest / bytes) |
| `goodbye` | master → worker | Ends the session (the worker exits its pull loop) |

The frame order is fixed: `hello` → `hello_ack` → `job` → `result` → `goodbye`. `JobSpec` carries `id / profile / targets / excluded / secrets / budget` — `secrets` holds references only (see "Credential model").

Transport in v0 is a **shared-directory double mailbox** (`DirTransport`): the master writes `<dir>/to-worker` and reads `<dir>/to-master`; the worker does the opposite. Frame files are named `<tag>-<8-digit seq>` (fixed width, so lexicographic order equals send order), written through an atomic rename from a temp file; dot-prefixed files are never consumed. Remote backends such as HTTP / SSH plug into the same `FarmTransport` trait in M3+, so the command line will not change.

## Deployment and worker registration

1. **Prepare the shared directory**: a directory both sites can reach (NFS / SMB / rsync sync / same path on the same machine), e.g. `/srv/polypack-farm`. The protocol only carries paths and the `JobSpec`; frames are ordinary files.
2. **Project visibility**: the `project_dir` carried by the `job` frame is resolved locally by the build site, and the worker reads `polypack.toml` from that directory to run the pipeline. v0 therefore requires the **project to be visible at the same path on both sides** (shared mount, or synced to the same path); automatic cross-machine project sync is out of scope for v0.
3. **Start the build site**: on the macOS machine, install Xcode and the cross-compilation targets (`rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios`), then register and serve:

```bash
poly farm worker --dir /srv/polypack-farm --id mac-mini-1 --target ios --jobs 1 --idle 60
```

`--target` declares the targets this worker can build (repeatable; omitted = every declared target reachable from this host); `--jobs` is how many jobs to handle before exiting, `--idle` the idle wait limit (timing out is a normal exit — having nothing to do is not a failure).

4. **Submit from the origin**: on Linux, point at the same directory:

```bash
poly farm submit --dir /srv/polypack-farm --config /srv/proj/polypack.toml --target ios --wait 600
```

An explicit `--target <id>` is never filtered by origin-side reachability — it is resolved against the hosts each target declares in the registry, so Linux can submit `ios` and wait for a macOS worker; the default `--target all` matches `poly build` and takes the origin-reachable subset only (list target ids explicitly to submit across hosts). The master waits for `hello` and admits the worker against the **declared hosts** of the job's targets (a host outside the declared hosts, or a worker that cannot build every target of the job → `hello_ack accepted=false` plus `poly::E_FARM_0002`); once admitted it sends `job` and waits for `result` (timeout → `poly::E_FARM_0003`). Worker-side failures are never swallowed: the remote code and fix hint come back through `RemoteDiagnostic`, and the master re-raises them wrapped in `poly::E_FARM_0003` with exit code 1.

5. **Cross-machine cache**: `--cache local` uses the project-local CAS (`<project root>/.polypack/cache`, the default); `--cache <shared dir>` switches to the shared object-store backend. The cache-key formula excludes absolute paths (ROADMAP §5.2), so the same project hits the cache across build sites; hit counts are reported truthfully on job completion (`cache hits N/M`).

## Credential model (pulled at the build site, on demand)

- `JobSpec.secrets` contains only `CredentialRef { name, source }`: `name` is the target's declared signing profile (`apple-codesign` / `android-keystore`), and `source` is one of `keyring` (system keystore) / `vault` / `env { variable }` (the name of a CI-injected environment variable).
- **Plaintext never enters a protocol frame, a command line, a plan or a log**: the worker resolves the reference against its own backends at execution time. Android keystore passwords are referenced only as `env:POLYPACK_KS_PASS` / `env:POLYPACK_KEY_PASS`; the iOS notarization password only as `env:POLYPACK_APPLE_APP_PASSWORD`, with the signing identity left in the build site's login keychain (Apple's recommended `xcrun notarytool store-credentials <profile>` profile form).
- Plaintext credentials in the repository make `poly scan` (secret-scan, run both on commit and during builds) fail; CI additionally has a dedicated secret-scan job.

## End-to-end example: Linux origin → macOS worker produces an IPA

Prerequisites: the macOS worker has Xcode and the three Apple triples; the project sits at the same path `/srv/proj` on both sides; the shared directory is `/srv/polypack-farm`.

```bash
# Build site (macOS): register as a worker, handle one job, then exit
poly farm worker --dir /srv/polypack-farm --id mac-mini-1 --target ios --jobs 1 --idle 60

# Origin (Linux): submit the ios target and wait for the result
poly farm submit --dir /srv/polypack-farm --config /srv/proj/polypack.toml --target ios --wait 600
```

Expected output (origin side; with `--json`, stdout is a single JSON line):

```
worker 已注册：mac-mini-1（aarch64-apple-darwin，宿主 Macos）
任务 <job-id> 完成：1 个产物
  .polypack/packages/ios/Pax Notes.ipa（1234567 字节，<blake3>）
```

Artifacts land in the build site's project directory (`.polypack/packages/ios/`); once moved back to the origin over the shared directory, verify them with the next section. If the worker fails, the origin prints something like `[poly::E_FARM_0003] worker 执行失败：[<remote code>] <message>` together with the fix hint — reproduce on the build site to see the same code.

## Signature verification guide

| Platform | Command | Pass criteria |
|---|---|---|
| iOS (app bundle) | `codesign --verify --deep --strict --verbose=2 "Payload/Pax Notes.app"` | On success prints `valid on disk` and `satisfies its Designated Requirement`; failures name the exact broken requirement |
| iOS (distribution assessment) | `spctl --assess --type execute --verbose=4 "Payload/Pax Notes.app"` (`.pkg` / `.dmg` use `--type install`) | `accepted` (unsigned or unnotarized gives `rejected`; `source=` names the origin) |
| iOS (notarization ticket) | `xcrun stapler validate <artifact>` | On success `The validate action worked!`; an unstappled artifact is reported as such |
| iOS (IPA) | `unzip -o app.ipa -d /tmp/ipa`, then run the two commands above against `Payload/*.app` | The IPA is a zip container; the `.app` inside is what gets verified |
| Android (APK) | `apksigner verify --print-certs --verbose app-release.apk` | `Verifies` plus `Signer #1 certificate DN:` (these are exactly the lines poly-sign keeps as verification evidence) |
| Android (AAB) | `jarsigner -verify -verbose -certs app-release.aab` + `bundletool validate --bundle=app-release.aab` | The AAB is a signed JAR (jarsigner verifies the signature) and bundletool checks the bundle structure |

These commands correspond one-to-one with the verifiers declared in the target registry: iOS uses `codesign-verify` / `spctl`, Android uses `apksigner-verify` / `bundletool-validate` (goldens in `crates/poly-cli/tests/fixtures/m2/*.artifact.json`). Contract tests for the mobile targets live in [`.github/workflows/mobile-contract.yml`](../.github/workflows/mobile-contract.yml): mock mode on every commit, real mode nightly with `POLY_CONTRACT_REAL=1` and artifact retention (the two tiers of ROADMAP §8).

## Known limits and schedule

- Transport is the shared-directory double mailbox only (HTTP / SSH land in M3+ behind the same trait); protocol v0 assumes the project is visible at the same path on the build site.
- The signing chain (`poly-sign`: codesign / notarytool / stapler, apksigner) is not wired into the package stage yet — the farm path currently produces **unsigned** artifacts (the iOS archive step uses `CODE_SIGNING_ALLOWED=NO`); signing and notarization follow with T-M2-10. Until then, treat the table above as the artifact acceptance checklist.
- Farm v1 (deployment guide + concurrency / isolation / shared cache) is scheduled for M8 (ROADMAP §9).
