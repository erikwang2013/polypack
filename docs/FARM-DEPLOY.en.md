# Remote farm deployment guide

> Applies to: farm protocol v1 (`farm/v1`; a v1 master still accepts `farm/v0` workers). Commands and flags follow `poly farm worker|submit --help` and `crates/poly-cli/src/farm.rs`.
> Chinese: [FARM-DEPLOY.md](FARM-DEPLOY.md)
> Companion reading: [FARM.en.md](FARM.en.md) (frames / credential model / signature verification), [ROADMAP.en.md](ROADMAP.en.md) §7 (security and secrets), §8 (tests and CI), §9 (M2 risk R3: Apple targets require a macOS host).

## 1. Topology and boundaries

```text
Linux / Windows origin                       macOS build site
poly farm submit                            poly farm worker (mac-01, mac-02, ... processes)
        │                                          │
        └──────────► /srv/polypack-farm ◄──────────┘   v1/<worker_id>/ mailbox pair + JSON Lines frames
                     /srv/polypack-cache                shared-directory CAS (--cache; optional but recommended)
                     /srv/proj                          project: v0 requires the same path on both sides
```

**The origin decides what to build**: it reads `polypack.toml`, splits the request per target, submits `JobSpec`s and collects artifact records. **The build site decides how to build**: it runs the same `Plan` DAG, fetches credentials on demand and owns the platform toolchain. Frames carry only paths, `JobSpec` and artifact records (name / blake3 / bytes); they never carry artifact bytes. In v0 the artifacts stay in the build-site project directory and are retrieved through the shared mount or a sync mechanism.

| Role | Responsibility | Requirements |
|---|---|---|
| Origin | `poly farm submit`: declare targets, wait for the worker pool, merge results | `poly`, project sources, farm directory; no Xcode |
| Build site | `poly farm worker`: register, pull, execute, report | `poly`, platform toolchain (Xcode / SDK), farm directory, credential backend |

## 2. Prerequisites

| Item | Requirement |
|---|---|
| `poly` version | Build both sides from the **same commit** (`poly-cli` has `publish = false` and is not on crates.io yet); `poly --version` must match |
| Shared filesystem | Farm directory and cache directory visible at the **same path** on both sides; the cache directory must provide same-directory POSIX atomic rename (verify NFS export semantics; SMB / sync drives that break rename must not host the shared CAS) |
| Project visibility | v0 requires the project at the path carried by the job frame on the build site (same-path mount, or rsync to the same path); automatic cross-machine sync is out of scope for v0 |
| Platform toolchain | macOS worker: full Xcode (`xcodebuild` / `plutil` / `ditto` / `codesign`) plus three Apple Rust triples; the Linux origin only needs Rust and `poly` |

## 3. Deploy from zero

### 3.1 Shared directories

```bash
sudo mkdir -p /srv/polypack-farm /srv/polypack-cache /srv/proj
sudo chown "$USER": /srv/polypack-farm /srv/polypack-cache
chmod 2770 /srv/polypack-farm /srv/polypack-cache
```

The farm directory hosts the protocol mailboxes (`v1/<worker_id>/to-master`, `to-worker`); the cache directory hosts object shards. Keep them **separate** for quotas and cleanup. The operating users on both sides need read/write access (the example assumes one deployment user; use a shared group or NFS mapping otherwise).

### 3.2 Build site (macOS)

Install a **full Xcode** from the App Store or developer.apple.com (Command Line Tools alone do not include the iOS SDK), then:

```bash
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
sudo xcodebuild -license accept
xcodebuild -version

# poly from the same commit (example repo path /srv/poly-src)
cd /srv/poly-src
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
cargo build --release -p poly-cli --locked
sudo install -m 755 target/release/poly /usr/local/bin/poly
poly --version

# Keychain profile: its name must equal TargetSpec.signing (apple-codesign for Apple targets)
export APPLE_ID='build@example.com'      # non-secret: Apple ID email
export TEAM_ID='TEAMID1234'              # non-secret: team ID
xcrun notarytool store-credentials apple-codesign --apple-id "$APPLE_ID" --team-id "$TEAM_ID"
# Enter the app-specific password interactively: it never enters argv or shell history
export POLYPACK_IOS_IDENTITY='Apple Distribution: Example Ltd (TEAMID1234)'
export POLYPACK_IOS_TEAM_ID="$TEAM_ID"
```

Start the worker pool (one mailbox per process; pooling means multiple processes):

```bash
mkdir -p /var/tmp/polypack-work
poly farm worker --dir /srv/polypack-farm --id mac-01 --target ios --jobs 0 --idle 120 \
  --cache /srv/polypack-cache --work-dir /var/tmp/polypack-work

# Second worker (another terminal / supervisor)
poly farm worker --dir /srv/polypack-farm --id mac-02 --target ios --jobs 0 --idle 120 \
  --cache /srv/polypack-cache --work-dir /var/tmp/polypack-work
```

`--id` accepts only letters, digits, `-` and `_` (it becomes the mailbox directory name). A started worker must receive the master registration frame within `--idle` seconds; for a long-lived pool set `--idle` high (e.g. 120–600) and supervise it with systemd `Restart=always` — the master always sends `goodbye` after a submit batch, so even a `--jobs 0` worker exits normally afterwards.

### 3.3 Origin (Linux)

```bash
cd /srv/poly-src
cargo build --release -p poly-cli --locked
sudo install -m 755 target/release/poly /usr/local/bin/poly
poly --version

poly farm submit --dir /srv/polypack-farm --config /srv/proj/polypack.toml \
  --target ios --wait 1800 --retries 1 --project-dir /srv/proj \
  --cache /srv/polypack-cache
```

An explicit `--target ios` is **not filtered by the origin host**: the job waits for a build site whose host matches the target's declared hosts (macOS). The default `--target all` only takes the origin-reachable subset, so cross-host submission must list target IDs explicitly. `--wait` bounds both "wait for worker registration" and "wait for each job result"; use a value well above a single IPA build. On failure the origin wraps the remote code and hint in `poly::E_FARM_0003` and exits 1.

### 3.4 Artifacts and verification

The `artifacts` list in the result report contains **core binary + `dist/` files** as project-relative paths (name / blake3 / bytes, for reconciliation). The packaged IPA stays in the **build site** project directory and is never carried by frames:

```bash
cd /srv/proj
unzip -o ".polypack/packages/ios/Pax Notes.ipa" -d /tmp/ipa
codesign --verify --deep --strict --verbose=2 "/tmp/ipa/Payload/Pax Notes.app"
spctl --assess --type execute --verbose=4 "/tmp/ipa/Payload/Pax Notes.app"
```

Paths and product names vary per project; the example uses the repository sample product `Pax Notes`. Without a same-path mount, retrieve `.polypack/packages/ios/` from the build site with rsync / scp.

## 4. Worker pool

### 4.1 Worker flags

| Flag | Meaning | Default |
|---|---|---|
| `--dir <DIR>` | Farm directory (mailbox root; master and workers point at the same one) | `.polypack-farm` |
| `--id <ID>` | Worker identity; unique per instance (letters / digits / `-` / `_`) | `<os>-<pid>` |
| `--target <ID>` | Declares buildable targets, repeatable; default = every declared target reachable by this host | host-reachable set |
| `--jobs <N>` | Max jobs in this session; `0` = unlimited until `goodbye` or idle timeout | `0` |
| `--idle <SECS>` | Registration / idle wait limit; idle exit is normal (no work is not a failure, but no registration frame is `E_FARM_0002`) | `60` |
| `--cache <PATH>` | `local` (project-local CAS), a shared directory (cross-worker / cross-machine reuse), or `s3://<bucket>[/<prefix>]` (S3 / OSS-compatible; environment variables in §6.2) | `local` |
| `--work-dir <PATH>` | Parent for one-shot task work directories; each task gets a 0700 private child removed at task end | system temp |
| `--credential <NAME=env:VAR>` | Credential profile name → environment variable; repeatable; only `env:` sources | none |

### 4.2 Submit flags

| Flag | Meaning | Default |
|---|---|---|
| `--config <PATH>` | **Build-site** `polypack.toml` | `polypack.toml` |
| `--dir <DIR>` | Farm directory (same as workers) | `.polypack-farm` |
| `--target <ID>` | Target ID, repeatable; explicit targets are not filtered by the origin host | all origin-reachable targets |
| `--profile <local|ci>` | Build profile | `local` |
| `--wait <SECS>` | Limit for worker registration / each job result | `120` |
| `--retries <N>` | Retries after a job timeout / lost worker (each retry switches worker; 0 = no retry) | `1` |

Also `--project-dir <PATH>` (build-site project path; give it explicitly when layouts differ — relative paths resolve against each worker's own cwd) and `--cache <PATH>` (echoed in the result report for reconciliation only; **the cache backend is selected by the worker's `--cache`**).

Scheduling: the master splits jobs per target; each idle worker takes one job it can build and runs concurrently. Admission requires the worker host to be declared by the target and the worker to declare at least one requested target; dispatch re-checks per job. Timeouts (`E_FARM_0004`) and lost workers (`E_FARM_0005`) are retried per `--retries` on another worker; deterministic build failures reported by a worker are not retried, and their code and hint are returned intact.

## 5. Isolation

### 5.1 Process boundary and one-shot work directory

Two layers, both implemented by the worker itself:

- **Process boundary**: every job runs inside a worker process; the master can only deliver frames and cannot touch the build-site filesystem. Concurrency comes from multiple worker processes; one process runs one job at a time.
- **One-shot work directory**: on startup the worker creates a 0700 private work root under `--work-dir` (default `std::env::temp_dir()`); each job gets a 0700 child whose name contains only pid + an in-process sequence, **never job identifiers or network paths**, created with `create_dir` (a pre-planted symlink cannot redirect it). At task end the whole directory is removed with `remove_dir_all` (which does not follow symlinks inside it); the work root is removed at worker exit; failures surface as `poly::E_FARM_0006` with cleanup hints.

`--work-dir` should be a local, worker-private writable directory (never the shared farm or cache directory). Signing temporary material is redirected into the task directory through `CredentialScope`, so it shares the task's lifetime.

### 5.2 Container boundary (optional hardening layer)

The repository **does not start or manage containers**: the container boundary is an operator-provided hardening layer, not built-in `poly` isolation. If you need a stronger boundary, put the worker process inside a self-managed container and own these constraints and their verification:

- Mount only the required farm directory, cache directory and project; keep the toolchain and base image read-only; point `--work-dir` at a container-local tmpfs / scratch volume.
- Inject credentials only through the process environment (images, `docker run -e` command lines, compose files and logs must never contain plaintext); if the Apple keychain is unavailable inside the container, use the env injection of §7.3 or a host-side keychain.
- One container process per worker with a unique `--id`; resource limits, networking and restart policy belong to the deployment layer.
- This layer is **not covered by this repository's tests**; do not claim "container isolation verified" before validating it in your environment.

## 6. Cache backend

### 6.1 Shared CAS semantics

`--cache local` (default) uses the project-local CAS at `<project>/.polypack/cache`. `--cache <shared-dir>` switches to the shared-directory object store; point every worker at the same directory to reuse across machines — the cache-key formula excludes absolute paths, so the same input produces the same key on different build sites.

Layout and format (`crates/poly-cache/src/store.rs`, `DirSharedStore`):

```text
<root>/<first 2 hex chars>/<hex>      # object file
POLYPACK-FARM-OBJECT v0               # header line (format version)
<blake3(payload) 64 hex chars>        # second line: payload digest
<payload bytes>                       # remainder
```

Writes first land in a **same-directory** dot-prefixed temp file (`.<hex>.tmp-<pid>-<seq>`, never treated as an object), then `rename(2)` atomically. Concurrent writers of the same key cannot truncate each other. Reads verify the header and digest; a mismatch is a miss and the corrupt entry is removed. The shared filesystem must therefore provide **same-directory atomic rename**; verify NFS export semantics, and do not use SMB / sync drives that break rename as the shared CAS.

### 6.2 S3 / OSS-compatible object storage

`--cache s3://<bucket>[/<prefix>]` talks directly to S3-compatible object storage (AWS SigV4, pure Rust, no AWS SDK). OSS uses S3-compatible mode (`x-oss-s3-compat`); native OSS4 signatures are out of scope. The object layout is identical to the shared directory (`<prefix>/<first 2 hex chars>/<hex>` with the `POLYPACK-FARM-OBJECT v0` envelope), so `aws s3 sync` can migrate objects between an old shared directory and a bucket without recomputing the cache.

Environment variables (`POLYPACK_*` wins, `AWS_*` is the fallback):

| Variable | Meaning | Default |
|---|---|---|
| `POLYPACK_S3_ENDPOINT` | `http(s)://host[:port]` (MinIO / OSS / other compatible endpoints) | AWS endpoints |
| `POLYPACK_S3_REGION` | Signing region; falls back to `AWS_REGION` / `AWS_DEFAULT_REGION` | `us-east-1` |
| `POLYPACK_S3_ACCESS_KEY_ID` | Access Key ID; falls back to `AWS_ACCESS_KEY_ID` | required |
| `POLYPACK_S3_SECRET_ACCESS_KEY` | Secret Access Key; falls back to `AWS_SECRET_ACCESS_KEY` | required |
| `POLYPACK_S3_SESSION_TOKEN` | Session token for temporary credentials; falls back to `AWS_SESSION_TOKEN` | none |
| `POLYPACK_S3_PATH_STYLE` | `1`/`true` = path-style; `0`/`false` = virtual-host | `1` when an endpoint is set, else `0` |
| `POLYPACK_S3_OSS_COMPAT` | `1` = send `x-oss-s3-compat: true` (OSS S3-compatible mode) | `0` |

Profiles / instance metadata (IMDS) / STS are not supported: credentials come only from environment variables and never enter argv, logs or diagnostics. Remote errors keep only the endpoint, HTTP status and S3 `<Code>`; Authorization, keys and session tokens are never logged. Configuration errors (bad URI / missing credentials) fail hard with `poly::E_CACHE_0001` before the worker starts; reachability / runtime errors degrade per §6.3.

Minimum IAM permissions (`<prefix>` is the configured prefix; bucket-level list plus object read/write/delete under the prefix):

```json
[
  {
    "Effect": "Allow",
    "Action": ["s3:ListBucket"],
    "Resource": "arn:aws:s3:::<bucket>",
    "Condition": {"StringLike": {"s3:prefix": ["<prefix>/*"]}}
  },
  {
    "Effect": "Allow",
    "Action": ["s3:GetObject", "s3:PutObject", "s3:DeleteObject"],
    "Resource": "arn:aws:s3:::<bucket>/<prefix>/*"
  }
]
```

For OSS S3-compatible mode the RAM actions are `oss:ListObjects` / `oss:GetObject` / `oss:PutObject` / `oss:DeleteObject` on `acs:oss:*:*:<bucket>/<prefix>/*`.

MinIO example:

```bash
export POLYPACK_S3_ENDPOINT=http://127.0.0.1:9000 POLYPACK_S3_REGION=us-east-1
export POLYPACK_S3_ACCESS_KEY_ID=... POLYPACK_S3_SECRET_ACCESS_KEY=...
poly farm worker --cache s3://build-cache/polypack --work-dir /var/tmp/polypack-work
```

On AWS use `--cache s3://<bucket>/<prefix>` with the usual `AWS_*` variables (or explicit `POLYPACK_S3_*`).

### 6.3 Degradation

The cache is an optimization, not a build prerequisite. When the shared directory or S3-compatible backend is absent / unwritable / goes offline, the worker warns once and falls back to the project-local CAS while the build continues:

- Unavailable at construction (shared directory not mounted / path occupied by a file / permissions; or a valid S3 config whose `ListObjectsV2` probe fails): `共享缓存目录不可用（<path>），已降级为工程本地缓存（构建继续）: <reason>` or `S3 兼容对象存储不可用（s3://<bucket>/<prefix> ...），已降级为工程本地缓存（构建继续）: <reason>` (stage `cache`).
- Runtime errors (IO / permissions / 5xx / rejected signature / connection failure): `对象存储后端不可用，已降级到后备缓存: <reason>`; writes continue into the local store so this machine still hits next time.

Hit statistics remain honest (`缓存命中 N/M` in the task-completion event).

## 7. Credential backends

### 7.1 CredentialRef boundary

`JobSpec.secrets` carries only `CredentialRef { name, source }`: `name` is the `TargetSpec.signing` profile (`apple-codesign` / `android-keystore` / `harmonyos-hap-sign` / `tizen-author`, ...), and `source` is `keyring` / `env { variable }` / `vault`. Plaintext never enters farm frames, command lines, plans or logs. The current configuration maps target signing profiles to `keyring` references; the `poly-sign` resolution layer selects a backend per source, and worker-side in-memory injection takes precedence over the backend chain.

### 7.2 macOS keychain

For Apple targets (`apple-codesign`), keep the material in the login keychain as Apple recommends:

```bash
xcrun notarytool store-credentials apple-codesign --apple-id "$APPLE_ID" --team-id "$TEAM_ID"
# Enter the app-specific password interactively; profile name = apple-codesign = TargetSpec.signing
export POLYPACK_IOS_IDENTITY='Apple Distribution: Example Ltd (TEAMID1234)'  # non-secret
export POLYPACK_IOS_TEAM_ID='TEAMID1234'                                      # non-secret
```

These two environment variables must be visible in the **worker process** environment. The notarization chain (notarytool / stapler) is currently outside the DAG (M2 boundary); the in-DAG `sign` stage signs `export/Payload/<product>.app` with `codesign`, taking identity / team from the non-secret variables above.

### 7.3 Worker-side env memory injection

For CI / Vault scenarios, `--credential` binds a profile name to an environment variable; the worker reads it into memory at **task start** and injects it via `CredentialScope`, clearing it when the scope drops at task end:

```bash
# Value = the poly-sign backend payload, base64(JSON) per secret.rs; the variable exists only in the worker env
export POLYPACK_ANDROID_KEYSTORE_B64='<base64(JSON) payload>'   # placeholder; never commit real values

poly farm worker --dir /srv/polypack-farm --id build-01 --target android \
  --credential android-keystore=env:POLYPACK_ANDROID_KEYSTORE_B64 \
  --cache /srv/polypack-cache --work-dir /var/tmp/polypack-work
```

Constraints and behavior (`crates/poly-cli/src/farm.rs`, `crates/poly-sign/src/inject.rs`):

- Only `<profile>=env:<variable>` is accepted; malformed arguments raise `poly::E_FARM_0002` and the **argument text is never echoed** (so a plaintext mistake cannot reach logs).
- Values travel only through the environment: never in argv, never written to disk, never in events / logs (injection warnings list profile names and counts only); they are cleared at task end with zero cross-task residue.
- A missing variable is not a failure: the worker warns "凭据档案 X 的注入变量 VAR 不可用" and the sign stage applies its skip-if-missing credential semantics.
- The payload contract lives in `crates/poly-sign/src/secret.rs` (Android: `keystore` / `store_password` / `key_alias` / `key_password`; HarmonyOS adds `app_cert` / `profile`). The Vault source needs a caller-injected Vault client; the farm CLI does not wire one today — bridge through env injection or inject the material into the keychain first.

## 8. Linux two-worker drill

This machine has no macOS and no cross-machine farm, so the macOS / IPA leg is recorded as a SKIP-with-print environment slot (§9); the stand-in evidence is a scriptable Linux two-worker local drill: two real worker processes, a multi-target `poly farm submit`, a second-run shared-cache hit, credential memory injection and cache degradation.

### 8.1 Run

```bash
cd /srv/poly-src
cargo build -p poly-cli
POLY=target/debug/poly DRILL_OUT=target/farm-drill tools/farm-drill.sh
```

The script creates its own static-UI fixtures (three independent project copies, no node / network) and archives output under `$DRILL_OUT` (`transcript.log` + `logs/`). Every executed command is printed with its actual paths and can be copied to rerun.

The script creates `farm/`, `cache/`, `work/`, `logs/` and the three project fixtures under `$DRILL_OUT`. Rerunning with the same `DRILL_OUT` clears those self-created paths and rewrites `transcript.log`; a non-empty directory without this script's archive marker is refused. The archive is retained until the operator deletes it. On the success path the worker removes its work root; if the drill is interrupted by a signal, `polypack-farm-work-*` may remain — clean up per §10. The §11.1 preflight runs before the rounds.

### 8.2 Pass criteria

The drill log must end with these lines (all really executed):

```text
PASS preflight: dir permissions, path isolation and poly scan
PASS round 1: two targets completed on two workers
PASS round 2: shared-cache hit 1/1 on a fresh project copy
PASS cache format: POLYPACK-FARM-OBJECT v0
PASS round 3: credential injected from env; zero plaintext on disk/logs
PASS round 4: unavailable shared cache degraded to local cache with a warning
PASS isolation: no task work directories left behind
PASS SKIP printed (environment slot); no macOS claims made
```

- Preflight: before the rounds, `tools/farm-preflight.sh` runs static checks on the temporary farm / cache / work directories and `site-a` (permissions, path distinctness, `poly scan`); any FAIL aborts the drill.
- Round 1: the `web` and `vidaa-tv` jobs land on two different workers and `dist/index.html` is merged into the result.
- Round 2: a **fresh** project copy (empty local cache) reports `缓存命中 1/1` on another worker; the hit can only come from the shared CAS.
- Round 3: the sentinel travels only through an `env:` variable; scanning the farm, work, project directories and both logs finds zero hits.
- Round 4: an unavailable shared-cache path triggers a warning and falls back to the local cache while the job still succeeds.
- Isolation: no work root / task directory remains after the workers exit.
- CI wiring suggestion (no workflow change in this task): after `cargo build -p poly-cli` in a Linux build job, run this script and upload `$DRILL_OUT` as an artifact to put the drill log into CI.

## 9. macOS / IPA environment slot

### 9.1 Slot status

**SKIP**: the current execution environment is Linux with no macOS host and no Xcode / `codesign` / `notarytool`, so "deploy a macOS worker pool from zero and build one IPA from Linux (R3)" was not executed here. The drill script **prints the commands and expected output into the log** (SKIP-with-print), leaving the slot open; replace it with real logs after running on a macOS host.

### 9.2 Run on a macOS host

Build site (macOS, after preparing Xcode / triples / poly / keychain per §3.2):

```bash
poly farm worker --dir /srv/polypack-farm --id mac-01 --target ios --jobs 0 --idle 120 \
  --cache /srv/polypack-cache --work-dir /var/tmp/polypack-work
```

Origin (Linux, with the project and farm directory visible at the same path):

```bash
poly farm submit --dir /srv/polypack-farm --config /srv/proj/polypack.toml \
  --target ios --wait 1800 --retries 1 --project-dir /srv/proj \
  --cache /srv/polypack-cache
```

Pass criteria: the origin prints `worker 已注册：mac-01（aarch64-apple-darwin，宿主 Macos）` and `农场任务完成：1 个任务 / 1 个 worker`; the build site produces an IPA under `/srv/proj/.polypack/packages/ios/`; run `codesign --verify` / `spctl --assess` on macOS per §3.4. The slot closes only after both logs and the artifact digest are archived.

## 10. Troubleshooting

| Code | Meaning | Action |
|---|---|---|
| `poly::E_FARM_0001` | Frame parse failure / unsupported version / unexpected frame | Upgrade both sides to the same `poly`; check the frame order (`hello` → `hello_ack` → `job` → `result` → `goodbye`) |
| `poly::E_FARM_0002` | Registration timeout / rejected / invalid worker ID / invalid `--credential` syntax | Start workers before submitting; check target-declared hosts vs worker `--target`; `--id` uses letters, digits, `-`, `_`; credentials are `NAME=env:VAR` |
| `poly::E_FARM_0003` | Worker execution failure (remote code embedded in the message) | Reproduce the same code on the build site; fix per the remote hint and resubmit |
| `poly::E_FARM_0004` | Job result timeout | Confirm the worker is alive; raise `--wait`; add workers or `--retries` |
| `poly::E_FARM_0005` | Lost worker / no available worker | Check farm-directory permissions and worker processes; start a worker declaring the target |
| `poly::E_FARM_0006` | Work root / task work directory creation or removal failed | Check `--work-dir` permissions; use a worker-private writable directory; clean up `polypack-farm-work-*` residue manually |

Farm-directory IO failures report `poly::E_IO_0001` (check existence and permissions on both sides). Shared-cache / S3 warnings (§6.3) are not build failures; if they persist, fix the mount / permissions / endpoint and credentials before looking at hit rates.

## 11. Production hardening checklist (self-hosted)

ADR 0005 makes GA self-hosted only: the project ships software and deployment guidance; machines, network, credentials and data belong to the operator. This table consolidates the controls the operator must own; the "Implementation / verification" column points at a runnable command or an existing section, and items marked **manual** have no built-in control and must be part of the deployment process. The §11.1 preflight covers permissions, paths and credential variables; real builds, networking and production hosts still need operator validation (this repository's drill only reaches the Linux two-worker case, see §8–§9).

| # | Hardening item | Requirement | Implementation / verification | Automation |
|---|---|---|---|---|
| 1 | TLS / reverse-proxy auth | `poly farm` has no network listener: transport is a same-directory mailbox pair (§1) and the protocol itself has no TLS or authentication. Across untrusted networks, share the directory only over a VPN / SSH tunnel or an authenticated NFSv4 / SMB mount; never expose the farm directory through an unauthenticated web server or public SMB / NFS. HTTP / SSH transport and mTLS are M3+ and do not exist today. | §1, §2; confirm no `poly` listening port | Manual; preflight warns on network filesystems (§11.1) |
| 2 | Directory permissions | Set the farm / cache directories to `2770` with a shared group per §3.1; keep them separate; `--work-dir` is a local, worker-private directory where the worker creates a 0700 child per job and removes it at job end. | §3.1, §5.1; `stat -c '%a %U:%G'`; `tools/farm-preflight.sh` checks writability, non-world-writable mode and distinct paths | Preflight |
| 3 | Cache namespace isolation | Cache keys are content-addressed and carry no tenant / origin identity: projects in different trust boundaries must use a separate cache root or S3 `<prefix>`; never point mutually untrusted teams at one cache. Farm, cache and work directories must not overlap. | §6.1, §6.2; per-team `--cache` path / prefix; S3 IAM scoped to that prefix (§6.2 least privilege) | Preflight checks distinct paths; S3 prefix and multi-tenant isolation are **manual** |
| 4 | Credential backend (no plaintext on disk) | Use only the macOS keychain (`apple-codesign`, §7.2) or `--credential NAME=env:VAR` memory injection (§7.3); plaintext must not enter the repository, argv, `polypack.toml`, plans, logs or backups. The Vault source needs caller wiring; the farm CLI does not wire one today. | §7.2, §7.3; preflight verifies the variables exist and are non-empty without printing values | Preflight + `poly scan` |
| 5 | `poly scan` sentinel | Scan for plaintext credentials before commit and in CI: `poly scan --dir <project>` (optionally `--history`); `poly scan --install-hook` installs a pre-commit hook; CI also has `secret-scan.yml`. `poly farm worker` does **not** run the scan automatically. | §7.1; `poly scan --dir /srv/proj --history`; preflight runs `poly scan` when given `--project` | Command + preflight |
| 6 | Worker isolation / one-shot work directories | One 0700 private directory per job, whose name carries no job identifier, removed at job end; the work root is removed at worker exit; the container boundary is optional and the operator verifies it. | §5.1, §5.2; `--work-dir`; the isolation check in `tools/farm-drill.sh` | Drill |
| 7 | Upgrade / rollback | Build both sides from the same commit; `poly --version` must match. Upgrade the origin first (a v1 master accepts `farm/v0` workers), then replace workers one at a time; install to a temporary name and `mv` it into place, keeping the previous binary. Roll back in reverse: workers first, then the origin. | §2; `poly --version`; `poly farm worker --help` / `poly farm submit --help` to confirm flags | Manual |
| 8 | Backup | Back up project sources, `polypack.toml` and the authoritative copy of signing material (back up the keychain the Apple way or keep it reproducible); farm mailboxes and the CAS are disposable intermediates and need no backup. Never write plaintext keys into a backup. | §6.1 (cache is rebuildable), §7.2 (keychain); `rsync` / snapshots are the operator's choice | Manual |
| 9 | Logging / monitoring | Collect worker and submit stdout (`--json` emits JSON Lines); monitor registration failures, job error codes (§10), cache-degradation warnings (§6.3), disk usage of farm / cache / work, and leftover `polypack-farm-work-*`. There is no built-in metrics / alerting endpoint. | §8.1 drill archive; §10 error codes; `--json` | Manual |
| 10 | Rate / concurrency limits | Concurrency = the number of worker processes running per target; each process runs one job at a time. `--jobs N` caps jobs per session (N=1 suits one-shot / untrusted batches), `--jobs 0` is unlimited; `--idle` bounds waiting, `--wait` / `--retries` bound origin waits and retries. There is no built-in queue quota or token bucket; use systemd / cgroup / `ulimit` for resource limits. | §4.1, §4.2, §5.1 | Manual; the drill verifies multi-worker concurrency |

### 11.1 Preflight

`tools/farm-preflight.sh` is the runnable static preflight for the table above: read-only checks for directory existence / writability / non-world-writable mode, distinct farm / cache / work paths, a local work directory, the environment variables referenced by `--credential`, the environment variables required by an S3 cache, and one `poly scan` over `--project`. It does not access the network, print credential values or leave long-lived files.

```bash
# Run on the origin and the build site separately; --project is that machine's project copy
POLY=/usr/local/bin/poly tools/farm-preflight.sh \
  --project /srv/proj \
  --credential android-keystore=env:POLYPACK_ANDROID_KEYSTORE_B64 \
  /srv/polypack-farm /srv/polypack-cache /var/tmp/polypack-work
```

- Positional arguments: farm directory, cache (shared directory / `local` / `s3://<bucket>[/<prefix>]`), and the worker's `--work-dir`; create the directories per §3.1 / §3.2 first.
- `--credential` is repeatable and only checks that the corresponding environment variable is set and non-empty; deployments not using that backend may omit it.
- Exit code 0 = no FAIL; 1 = at least one FAIL; 2 = usage error. WARN does not change the exit code but should be addressed.
- A passing preflight only means the static configuration is sound, not that a build is verified: real builds, cache hits, credential injection and degradation are covered by the §8 drill; macOS / IPA and production-host drills remain external in §9.
- `tools/farm-drill.sh` runs the same preflight on its temporary directories before the rounds and writes the result into `transcript.log` (§8.2).

## 12. References

- [FARM.en.md](FARM.en.md): frames, origin / build-site model, signature verification guidance.
- `crates/poly-cli/src/farm.rs`: `worker` / `submit` flags and scheduling entry points.
- `crates/poly-core/src/farm/`: protocol, pool scheduling, one-shot work directories.
- `crates/poly-cache/src/store.rs`: shared-directory CAS, shared object envelope and fallback backend.
- `crates/poly-cache/src/s3.rs`, `crates/poly-cache/src/s3/sigv4.rs`: S3 / OSS-compatible backend and SigV4 signing.
- `crates/poly-cache/tests/s3_contract.rs`: mock-S3 wire contract (independent SigV4 recomputation); `tests/s3_real.rs` is the real-cloud environment slot (`POLYPACK_S3_REAL=1`).
- `crates/poly-sign/src/secret.rs`, `crates/poly-sign/src/inject.rs`: credential backend contract and memory injection.
- `tools/farm-drill.sh`: the executable drill for §8.
- `tools/farm-preflight.sh`: the production preflight for §11.1.
