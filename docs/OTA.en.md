# OTA / Production Update Channels (M9-E4)

> Status: the T-M9-10 research matrix and the T-M9-11 signed manifest are implemented; the T-M9-12 Tauri on-device install / rollback drill is environment-gated and is not faked in this repository.
> Retrieval date: 2026-10-20. Official pages that could not be reached are marked "not fetched"; their details are not treated as verified facts.

## 1. Update Mechanism Matrix

| Mechanism | Update model | Signing / verification | Rollout / rollback | Manual steps | Official source (retrieved 2026-10-20) |
| --- | --- | --- | --- | --- | --- |
| Tauri v2 updater (self-hosted static manifest) | `latest.json` on static hosting or a dynamic endpoint; the client compares SemVer, `platforms.<os-arch>` provides `url` and `signature` (contents of the `.sig` file); bundles are AppImage / `.app.tar.gz` / MSI / NSIS | Verification is mandatory; `tauri signer generate` creates the key pair, the public key goes into `plugins.updater.pubkey`, and the artifact `.sig` is checked before install | A static manifest has no percentage rollout (channels use separate URLs); a dynamic server can override version comparison to install a chosen version, which the docs note is useful for rollback; a static JSON respects the built-in version check and does not downgrade by default | Generate and protect the private key once; enable `createUpdaterArtifacts`; build and upload the manifest and artifacts over HTTPS; configure `endpoints` | https://v2.tauri.app/plugin/updater/ (fetched) |
| Google Play in-app updates | Play Core In-App Updates: `flexible` (background download) or `immediate` (full-screen forced restart); Android 5.0 / API 21+; Google Play performs install and restart | Google Play delivers and installs the update; the in-app updates API does not manage signing itself (Play App Signing model, that page was not fetched) | Staged rollout via Play Console / Developer API (`userFraction`, gradual increase, halt); versionCode downgrade is not supported, so rollback means publishing a higher versionCode (staged-rollout details come from a search summary, the official page was not fetched) | Upload AAB/APK → review → staged rollout → watch Android vitals → raise to 100%; on failure halt and publish a higher versionCode | https://developer.android.com/guide/playcore/in-app-updates (fetched, page marked 2026-01-30 UTC); https://developers.google.com/android-publisher/tracks (not fetched) |
| Apple App Store phased release | App Store system updates; optional phased release reaches users with automatic updates over 7 days at 1% / 2% / 5% / 10% / 20% / 50% / 100% | Apple code signing and App Review, then distribution by the App Store; developers do not manage update signing | Pause for up to 30 days with no limit on the number of pauses; removing the app from sale stops that version's phased release; downgrade is not supported, so make the old version unavailable and submit a new version to resume a controlled rollout; manual downloads are not limited by the phase | Submit the version → choose the 7-day phased release → monitor → pause if needed → recover with a new version | https://developer.apple.com/help/app-store-connect/update-your-app/release-a-version-update-in-phases/ (fetched) |
| WeChat Mini Program version management | Development version → version under audit → online version; after release the client checks on launch, periodically, and after long inactivity; asynchronous downloads apply on the next cold start, `wx.getUpdateManager` can prompt a restart; full release reaches 99%+ in about 24 hours | Reviewed and distributed by WeChat; developers do not control binary signing or the update channel; publishing is blocked until review passes | The admin console can roll back a version (development management permission includes release and rollback); "prefer local version" and "minimum usable version" settings exist; the fetched pages did not show a percentage rollout entry | Upload code → submit for audit → publish after approval → roll back or raise the minimum usable version on failure | https://developers.weixin.qq.com/miniprogram/en/dev/quickstart/basic/role.html, .../release.html, https://developers.weixin.qq.com/miniprogram/dev/framework/runtime/update-mechanism.html (all fetched) |
| Self-hosted signed channel (PolyPack) | A static `latest.json` per channel (`/<channel>/latest.json`) with the same structure as Tauri's static manifest; a `/<channel>/<version>.json` version copy is retained | `poly update manifest` signs the manifest with a detached GPG signature (`latest.json.sig`), and `poly update check --trust-root` verifies it; the artifact `signature` field is still the Tauri `.sig`; credentials appear only as `CredentialRef` references | v1 is single-channel with no percentage rollout; rollback copies the version copy back to `latest.json` and re-uploads it (the client must still allow downgrade or switch channels); TUF is used only as a threat-model reference, root / timestamp roles are not implemented | Generate and sign a manifest per platform → upload latest, version copies, and artifacts → keep old version copies → restore an old manifest to roll back | https://theupdateframework.io/docs/overview/, https://theupdateframework.io/docs/security/ (both fetched); this repository's `poly update` |

## 2. Classification

- **Self-updatable**: the Tauri v2 self-hosted channel and the PolyPack self-hosted channel that reuses its manifest format. The actual binary replacement and rollback depend on the Tauri updater embedded in the app; this repository only produces and signs manifests.
- **Store-only updates**: Google Play and the Apple App Store. In-app update APIs / phased release only affect prompting and rollout pace; installation, signing, and review remain with the store.
- **Platform-controlled**: WeChat Mini Programs. Versions must pass platform review, the client decides when to update, and developers can only roll back or set a minimum usable version.

## 3. Manifest Schema and Signing

The required keys in `latest.json` are `version`, `platforms.<target>.url`, and `platforms.<target>.signature`; `notes` and `pub_date` are optional. Platform keys use Tauri's `OS-ARCH` form (for example `linux-x86_64`).

```json
{
  "notes": "fix crash on launch",
  "platforms": {
    "linux-x86_64": {
      "signature": "<contents of the artifact .sig file>",
      "url": "https://updates.example.com/stable/Pax%20Notes_1.2.3_x64.AppImage"
    }
  },
  "pub_date": "2026-10-20T00:00:00Z",
  "version": "1.2.3"
}
```

### 3.1 Canonical Form

- Top-level and platform keys are sorted lexicographically; two-space indentation; exactly one trailing newline.
- `pub_date` must be RFC 3339; `--pub-date` wins, then `SOURCE_DATE_EPOCH`, and only then the current time. Identical input produces byte-identical output across runs.
- The artifact URL joins `--base-url` with the percent-encoded file name; production must use HTTPS.

### 3.2 Signing and Verification

- Manifest signing reuses `poly-sign`'s `GpgSigner` (`RepoSigner::detach_sign`, SHA-256, ASCII armor) and writes `<manifest>.sig`.
- Credentials accept only `env:VAR` / `keyring[:entry]` / `vault[:entry]` references; plaintext passwords and private keys never enter the command line, logs, or event streams.
- Verification uses `ArtifactVerifier` with a trust-root payload of base64(JSON) `{"public_key":"<base64>"}`.
- Tauri natively verifies each platform artifact's `.sig` (Tauri key pair); the GPG manifest signature is a separate layer and neither replaces the other.

### 3.3 Commands

```bash
# Generate and sign (defaults to <package-dir>/<channel>/latest.json and <version>.json)
poly update manifest --channel stable --version 1.2.3 --notes "fix crash" \
  --base-url https://updates.example.com/stable --platform linux --arch x86_64 \
  --package-dir .polypack/packages/linux-reference \
  --credential env:POLYPACK_UPDATE_KEY --pub-date 2026-10-20T00:00:00Z

# Structure check first, then verify with the trust root
poly update check --manifest latest.json \
  --signature latest.json.sig --trust-root env:POLYPACK_UPDATE_TRUST
```

## 4. Rollback Design

1. Every generation keeps `<channel>/<version>.json` and its `.sig`; upload them together with `latest.json` and never overwrite old version copies.
2. To roll back, copy the target old `<version>.json` over `latest.json` and upload it with its `.sig`. The signature covers the same bytes, so no re-signing is needed.
3. The client must allow downgrade: Tauri's static manifest applies its built-in version check, so an older version will not install automatically; switch to a dedicated rollback channel or use a dynamic update server that overrides version comparison.
4. After a rollback, bump the patch version in the next release so that a version-number regression does not confuse later update decisions.
5. This repository only exercises "restore manifest + signature verification"; real installation and downgrade are environment-gated under T-M9-12.

## 5. Environment Gates and Unverified Items

- **Tauri binary update / rollback**: requires a real Tauri app, a Tauri key pair, and an installable artifact; this repository neither performs nor fakes it. `poly update check` verifies only manifest structure and the GPG signature.
- **Google Play staged rollout / Play App Signing**: the official pages were unreachable in this environment, so those details come from a search summary and are marked not fetched; only the In-App Updates flexible / immediate flows were fetched.
- **Apple downgrade**: the official documentation offers no version downgrade; only pausing the phased release or recovering with a new version.
- **WeChat percentage rollout**: the fetched version-management and update-mechanism pages did not list a percentage rollout entry, so the matrix does not promise one.

## 6. Sources

- Tauri v2 Updater: https://v2.tauri.app/plugin/updater/ (fetched 2026-10-20)
- Google Play In-App Updates: https://developer.android.com/guide/playcore/in-app-updates (fetched 2026-10-20; page updated 2026-01-30 UTC)
- Google Play Developer API Tracks (staged rollout): https://developers.google.com/android-publisher/tracks (not fetched 2026-10-20)
- Apple App Store Connect phased release: https://developer.apple.com/help/app-store-connect/update-your-app/release-a-version-update-in-phases/ (fetched 2026-10-20)
- WeChat Mini Program versions and rollback permission: https://developers.weixin.qq.com/miniprogram/en/dev/quickstart/basic/role.html (fetched 2026-10-20)
- WeChat Mini Program release flow: https://developers.weixin.qq.com/miniprogram/en/dev/quickstart/basic/release.html (fetched 2026-10-20)
- WeChat Mini Program update mechanism: https://developers.weixin.qq.com/miniprogram/dev/framework/runtime/update-mechanism.html (fetched 2026-10-20)
- TUF overview and security model: https://theupdateframework.io/docs/overview/, https://theupdateframework.io/docs/security/ (fetched 2026-10-20)
