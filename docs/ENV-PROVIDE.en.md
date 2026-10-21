# How to provide missing environment-slot inputs (ENV-PROVIDE)

> Status: user entry-point guide (2026-10-21, baseline `main` v1.14.0). This page explains how to hand over inputs **only you can supply**: credentials, authorizations, hardware / hosts, and human trials.
> Real slot status and evidence stay authoritative in [ENV-SLOTS](ENV-SLOTS.en.md); **providing an input does not close a slot** — closure still needs the real-run evidence and review in §4.2.
> Chinese: [ENV-PROVIDE.md](ENV-PROVIDE.md)

## 1. Pick an entry point first

| What you provide | Entry point | Red line |
| --- | --- | --- |
| Password / token / login state / private-key file | [GitHub Actions Secrets](https://github.com/erikwang2013/polypack/settings/secrets/actions) ([new](https://github.com/erikwang2013/polypack/settings/secrets/actions/new)) or `node tools/env-provide.mjs --set-secret NAME` | Never paste into an issue / chat / log / commit |
| Non-secret configuration (Team ID, CLI path, device IP) | [GitHub Actions Variables](https://github.com/erikwang2013/polypack/settings/variables/actions) ([new](https://github.com/erikwang2013/polypack/settings/variables/actions/new)) or `--set-var NAME=VALUE` | Do not disguise a secret as a variable |
| Vendor authorization / license / NDA | `node tools/env-provide.mjs --open ENV-XX` opens the official portal; record the approval reference with the [authorization form](https://github.com/erikwang2013/polypack/issues/new?template=env-authorization.yml) | Never paste license files, keys, or NDA text into an issue |
| Device / host / KVM / LAN | [Hardware / host request](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml) | Never post addresses, access codes, or phone numbers in a public issue |
| Human external trial | [Trial sign-up](https://github.com/erikwang2013/polypack/issues/new?template=human-trial-signup.yml); protocol in [HUMAN-TRIAL](HUMAN-TRIAL.en.md) | Never post names, email addresses, employers, or other sensitive identifiers |

The Secrets page needs repository admin permission; if it will not open, give the **name and source** to a maintainer to upload in the settings UI — do not fall back to pasting the value in an issue.

## 2. Credentials: GitHub Secrets / Variables

1. Open [Secrets](https://github.com/erikwang2013/polypack/settings/secrets/actions) or [Variables](https://github.com/erikwang2013/polypack/settings/variables/actions) and click **New repository secret / variable**.
2. The name must match the §4 table exactly (case-sensitive); a wrong name leaves the workflow reading an empty string.
3. Locally use `node tools/env-provide.mjs` (it reads `tools/env-providers.json` and prints names and links only, never values):

```sh
node tools/env-provide.mjs --slot ENV-02        # missing names, settings links, gh commands, authorization portals
node tools/env-provide.mjs --set-secret POLYPACK_APPLE_APP_PASSWORD   # hidden TTY prompt, uploaded via gh secret set
node tools/env-provide.mjs --set-secret POLYPACK_GARMIN_DEVKEY_B64 --file developer_key.der  # file -> base64
node tools/env-provide.mjs --set-var POLYPACK_IOS_TEAM_ID=TEAMID1234  # non-secret variable
node tools/env-provide.mjs --check --slot ENV-02 --strict             # name-only check; non-zero exit when missing
```

4. `--file` is only for manifest entries that require a file-backed secret (for example the ENV-14 `.der` devkey; an Apple `.p12` follows the same pattern once the manifest names it); raw certificates / private keys never enter the repo or an issue. Apple certificates are imported into the macos-14 keychain through Apple's official flow or supplied as a file-backed secret.
5. Values appear only in the hidden prompt or on `gh` stdin; `--dry-run` prints the names and links it would use. If the script is not in your checkout yet, use the direct links in step 1.
6. Never screenshot a value; confirm with the name and upload time only.

## 3. Authorization: official portal + recorded reference

Authorization is an organizational action; an agent cannot approve it. Run `node tools/env-provide.mjs --open ENV-XX` to open the slot's official portals registered in `tools/env-providers.json` (the table lists the main entry points; `--dry-run` prints URLs only; console NDAs have no slot yet and are table links only). Then record the **non-secret** reference with the [authorization form](https://github.com/erikwang2013/polypack/issues/new?template=env-authorization.yml); license files / keys still go through §2 as secrets.

| Provider | Official entry point (https) | Linked slot | What to record (non-secret) |
| --- | --- | --- | --- |
| Apple Developer | [Programs](https://developer.apple.com/programs/) · [Account](https://developer.apple.com/account) | ENV-02 / ENV-03 | Team ID, certificate Common Name, program approval reference |
| BlackBerry QNX | [developers.blackberry.com](https://developers.blackberry.com/) | ENV-08 | License / entitlement ID |
| Wind River | [VxWorks](https://www.windriver.com/products/vxworks) | ENV-09 | Workbench license / support account ID |
| Garmin Connect IQ | [developer.garmin.com/connect-iq](https://developer.garmin.com/connect-iq/) | ENV-14 | Developer account ID / SDK agreement reference |
| Roku | [developer.roku.com](https://developer.roku.com/) | ENV-05 | Developer account ID / device developer-mode confirmation |
| WeChat | [Official platform](https://mp.weixin.qq.com/) · [DevTools](https://developers.weixin.qq.com/miniprogram/dev/devtools/download.html) | ENV-04 | appid / admin approval reference |
| Alipay | [open.alipay.com](https://open.alipay.com/) | ENV-04 | appid / account ID |
| Douyin | [developer.open-douyin.com](https://developer.open-douyin.com/) | ENV-04 | appid / qualification approval reference |
| Firebase Test Lab | [firebase.google.com/docs/test-lab](https://firebase.google.com/docs/test-lab) | ENV-20 (optional provider) | GCP project ID |
| BrowserStack | [App Automate](https://www.browserstack.com/app-automate) | ENV-20 (optional provider) | Account / plan ID |
| AWS Device Farm | [aws.amazon.com/device-farm](https://aws.amazon.com/device-farm/) | ENV-20 (optional provider) | AWS account ID / non-secret part of the project ARN |
| PlayStation Partners | [partners.playstation.net](https://partners.playstation.net/) | Console NDA (no ENV slot yet) | Partner ID / NDA reference |
| Xbox / ID@Xbox | [xbox.com/developers/id](https://www.xbox.com/en-US/developers/id) | Console NDA (no ENV slot yet) | Publisher / ID@Xbox approval reference |
| Nintendo Developer | [developer.nintendo.com](https://developer.nintendo.com/) | Console NDA (no ENV slot yet) | Developer account / NDA reference |
| Zepp OS | [developer.zepp.com](https://developer.zepp.com/) | ENV-13 | Developer account ID |

If the reference itself is confidential, type `confidential — sent via private channel` in the form and choose the private contact preference; **never** paste a confidential reference into a public issue.

## 4. Slot by slot: what is missing and where it goes

The table lists **non-CLOSED** slots only. Names match the [ENV-SLOTS](ENV-SLOTS.en.md) run cards; do not invent names that are not listed.

| Slot | Required secrets | Required variables | Authorization / official entry | Hardware / human entry | Docs anchor |
| --- | --- | --- | --- | --- | --- |
| <a id="env-02"></a>ENV-02 | `POLYPACK_APPLE_APP_PASSWORD` | `POLYPACK_IOS_IDENTITY`, `POLYPACK_IOS_TEAM_ID`, `POLYPACK_APPLE_ID` | [Apple](https://developer.apple.com/programs/) (Team ID / certificate CN; certificate imported into the keychain) | None (hosted macos-14 runner) | [§ENV-02](ENV-SLOTS.en.md#slot-env-02) |
| <a id="env-03"></a>ENV-03 | — (Apple credentials live in the worker keychain) | `POLYPACK_IOS_IDENTITY`, `POLYPACK_IOS_TEAM_ID` | [Apple](https://developer.apple.com/programs/) | [Hardware request](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml) (macOS host + shared farm directory) | [§ENV-03](ENV-SLOTS.en.md#slot-env-03) |
| <a id="env-04"></a>ENV-04 | `POLYPACK_WECHAT_DEVTOOLS_LOGIN`, `POLYPACK_ALIPAY_DEVTOOLS_LOGIN`, `POLYPACK_DOUYIN_DEVTOOLS_LOGIN` | `POLYPACK_WECHAT_DEVTOOLS_CLI`, `POLYPACK_ALIPAY_DEVTOOLS_CLI`, `POLYPACK_DOUYIN_DEVTOOLS_CLI`, `POLYPACK_DEVTOOLS_LOGIN_FILE` | [WeChat](https://mp.weixin.qq.com/) · [Alipay](https://open.alipay.com/) · [Douyin](https://developer.open-douyin.com/) | [Hardware request](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml) (macOS GUI host for QR / 2FA) | [§ENV-04](ENV-SLOTS.en.md#slot-env-04) |
| <a id="env-05"></a>ENV-05 | `ROKU_DEV_PASSWORD` | `ROKU_DEV_TARGET` | [Roku](https://developer.roku.com/) (developer account) | [Hardware request](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml) (Roku + same-LAN host) | [§ENV-05](ENV-SLOTS.en.md#slot-env-05) |
| <a id="env-06"></a>ENV-06 | — | — | None (OpenBSD is open source) | [Hardware request](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml) (KVM self-hosted runner / OpenBSD host) | [§ENV-06](ENV-SLOTS.en.md#slot-env-06) |
| <a id="env-08"></a>ENV-08 | — | `POLYPACK_QNX_SDP`, `POLYPACK_QNX_QEMU`, `POLYPACK_QNX_QEMU_ARGS` | [BlackBerry QNX](https://developers.blackberry.com/) (commercial license) | [Hardware request](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml) (self-hosted runner + license server) | [§ENV-08](ENV-SLOTS.en.md#slot-env-08) |
| <a id="env-09"></a>ENV-09 | — | `WR_CC`, `POLYPACK_VXWORKS_TRIPLE` | [Wind River VxWorks](https://www.windriver.com/products/vxworks) (commercial license) | [Hardware request](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml) (self-hosted runner + Workbench SDK) | [§ENV-09](ENV-SLOTS.en.md#slot-env-09) |
| <a id="env-10"></a>ENV-10 | — | `POLYPACK_WEAR_FORM_APK`, `POLYPACK_WEAR_FORM_PACKAGE` | None (emulator); a physical device needs ADB authorization | [Hardware request](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml) (KVM runner or Wear OS device) | [§ENV-10](ENV-SLOTS.en.md#slot-env-10) |
| <a id="env-13"></a>ENV-13 | — | — | [Zepp OS](https://developer.zepp.com/) (developer account) | [Hardware request](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml) (GUI simulator host) | [§ENV-13](ENV-SLOTS.en.md#slot-env-13) |
| <a id="env-14"></a>ENV-14 | `POLYPACK_GARMIN_DEVKEY_B64` (`.der` via `--file`) | `CIQ_HOME`, `CONNECTIQ_HOME`, `CIQ_MONKEYC`, `CIQ_DEVKEY` (runner-local path) | [Garmin Connect IQ](https://developer.garmin.com/connect-iq/) | None (CI leg; workflow not built yet) | [§ENV-14](ENV-SLOTS.en.md#slot-env-14) |
| <a id="env-19"></a>ENV-19 | — | — | None (human process) | [Trial sign-up](https://github.com/erikwang2013/polypack/issues/new?template=human-trial-signup.yml) · [Protocol](HUMAN-TRIAL.en.md) | [§ENV-19](ENV-SLOTS.en.md#slot-env-19) |
| <a id="env-20"></a>ENV-20 | — (provider adapter is not wired; do not invent names) | — | [Firebase](https://firebase.google.com/docs/test-lab) · [BrowserStack](https://www.browserstack.com/app-automate) · [AWS](https://aws.amazon.com/device-farm/) (optional cloud account) | [Hardware request](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml) (Android + Android TV or a cloud farm) | [§ENV-20](ENV-SLOTS.en.md#slot-env-20) |

## 5. Hardware / hosts

- Entry point: [Hardware / host request](https://github.com/erikwang2013/polypack/issues/new?template=env-hardware-request.yml). The slot dropdown covers ENV-03 / 05 / 06 / 08 / 09 / 10 / 13 / 20.
- The form collects **non-secret** information only: device model, purpose, request type (device loan / shipping, host access, KVM runner, same LAN, cloud farm), coarse time zone, and a GitHub contact.
- **Shipping addresses, phone numbers, access codes, and serial numbers go only through a private channel arranged by a maintainer**; never post them in a public issue.
- A maintainer replies in the issue to arrange the hand-over; **until hardware is actually shipped or access is granted, never claim it was provided** — the slot stays OPEN / PARTIAL.

## 6. Human trial (ENV-19)

- Entry point: [Trial sign-up](https://github.com/erikwang2013/polypack/issues/new?template=human-trial-signup.yml); the protocol and consent terms are in [HUMAN-TRIAL](HUMAN-TRIAL.en.md).
- The form collects: operating system, Rust / mobile / release background, 90-minute availability, screen / voice recording consent, and contact preference.
- It **does not collect** names, email addresses, phone numbers, employers, addresses, or other sensitive identifiers; reports use P1 / P2 IDs and are anonymized before publication.
- Fewer than 3 real reports cannot close ENV-19; internal rehearsals or AI role-play do not count.

## 7. `env-provide` commands (intended behavior)

The script lives at `tools/env-provide.mjs` and reads `tools/env-providers.json`; this page describes behavior only — values never enter argv or logs.

| Command | Intended behavior |
| --- | --- |
| `--list [--json]` | List every slot's secrets / variables and whether they are configured; show unknown when `gh` is unavailable |
| `--check [--slot ENV-XX] [--strict]` | Check names only; `--strict` exits non-zero when anything is missing |
| `--slot ENV-XX` | Print that slot's missing names, Settings links, `gh` commands, and authorization portals |
| `--open ENV-XX [--dry-run]` | Open the slot's official portals in the system browser; `--dry-run` prints URLs only |
| `--set-secret NAME [--file PATH]` | Hidden TTY prompt, or base64-encode `PATH`; after confirmation upload via `gh secret set` from stdin |
| `--set-var NAME=VALUE` | Upload a **non-secret** repository variable; the value is visible in argv, so never use it for secrets |
| `--repo OWNER/REPO` | Override the default target `erikwang2013/polypack` |

## 8. Safety and what "provided" means

- Every secret / token / private key / license file goes only through Secrets or `--set-secret`; issues, chat, logs, screenshots, and commits are forbidden.
- On a leak: rotate or revoke immediately in GitHub / the vendor portal, then re-capture evidence per [ENV-SLOTS](ENV-SLOTS.en.md) §4.2.
- Authorization references, Team IDs, appids, and project IDs are non-secret references; they never substitute for credentials.
- A green `--check` means names exist only; **it does not prove the value is correct, permissions are sufficient, or the real leg ran**. Slot closure follows ENV-SLOTS §4.2 evidence.
- All links on this page use https and official domains; when a deep path is uncertain, the page links the official landing page only.

## 9. References

- [ENV-SLOTS](ENV-SLOTS.en.md) · [HUMAN-TRIAL](HUMAN-TRIAL.en.md) · [DEVICE-PREVIEW](DEVICE-PREVIEW.en.md) · [FARM-DEPLOY](FARM-DEPLOY.en.md) · [GA](GA.en.md)
- Chinese: [ENV-PROVIDE.md](ENV-PROVIDE.md)
