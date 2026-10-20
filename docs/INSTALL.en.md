# Installing the PolyPack CLI

> Applies to GitHub Releases prebuilt artifacts (Linux x86_64 / macOS arm64 / Windows x86_64) and source builds.
> This page also records the current crates.io blocker (T-M9-45): every workspace crate is `publish = false`; do not change that before a maintainer decision.
> Chinese: [INSTALL.md](INSTALL.md)

## 1. Supported matrix

| Platform | Architecture | Rust target | Release asset |
| --- | --- | --- | --- |
| Linux | x86_64 | `x86_64-unknown-linux-gnu` | `poly-x86_64-unknown-linux-gnu.tar.gz` |
| macOS | Apple Silicon (arm64) | `aarch64-apple-darwin` | `poly-aarch64-apple-darwin.tar.gz` |
| Windows (Git Bash) | x86_64 | `x86_64-pc-windows-msvc` | `poly-x86_64-pc-windows-msvc.zip` |

The install script covers only the table above; other combinations (Linux aarch64, macOS x86_64, Windows arm64, etc.) have no prebuilt artifact, so the script exits with a clear error instead of falling back to an unverified download.

## 2. Source install

Prerequisites: Rust 1.98+ (see `rust-toolchain.toml`) and the platform C toolchain.

```bash
git clone https://github.com/erikwang2013/polypack.git
cd polypack
cargo install --path crates/poly-cli --locked
poly --version
```

`cargo install` installs only the `poly` bin; the first build is slow, and later builds reuse `target/`.

## 3. Prebuilt binaries

Download the matching asset from the GitHub Release and extract it:

```bash
TAG=v1.11.0
TARGET=x86_64-unknown-linux-gnu
curl -fsSLO "https://github.com/erikwang2013/polypack/releases/download/$TAG/poly-$TARGET.tar.gz"
tar -xzf "poly-$TARGET.tar.gz"
install -m 0755 "poly-$TARGET" "$HOME/.local/bin/poly"
```

The Windows asset is a `.zip`; extract `poly-x86_64-pc-windows-msvc.exe`, place it on PATH, and rename it to `poly.exe`.

## 4. Checksums

Every Release ships `SHA256SUMS.txt`. Download the asset plus the checksum file, then run this in the same directory:

```bash
sha256sum --check --ignore-missing SHA256SUMS.txt
```

On macOS without `sha256sum`, use `shasum -a 256 -c SHA256SUMS.txt`. A checksum only proves that the download matches the published manifest; publisher identity needs a signature (see §5).

## 5. Install script

Prefer downloading and reviewing the script over piping it straight into a shell:

```bash
curl -fsSLO https://raw.githubusercontent.com/erikwang2013/polypack/main/scripts/install.sh
less install.sh
bash install.sh
```

Environment variables:

| Variable | Default | Purpose |
| --- | --- | --- |
| `POLY_VERSION` | latest Release | Pin a tag (`v1.11.0` or `1.11.0`) |
| `POLY_INSTALL_DIR` | `~/.local/bin` | Install directory |
| `POLY_REPO` | `erikwang2013/polypack` | GitHub repository |
| `POLY_RELEASE_BASE_URL` | GitHub Release URL | Mirror / offline-test base URL |
| `POLY_REQUIRE_SIGNATURE` | empty | Set to `1` to enforce minisign verification |
| `POLY_MINISIGN_PUBKEY` | empty | Path to the minisign public key (required in strict mode) |

What it does: detect the platform → resolve the tag → download the asset and `SHA256SUMS.txt` → enforce SHA-256 → install → run `poly --version` and print the version. There is no switch that skips verification; a missing checksum file, a missing asset entry, or a digest mismatch fails before anything is written. Strict signature mode also requires `SHA256SUMS.txt.minisig` and a working `minisign`; if either is missing, nothing is installed.

Signing is an optional release step: the Release workflow signs and uploads `SHA256SUMS.txt.minisig` only when the `MINISIGN_SECRET_KEY` secret (a base64-encoded minisign private-key file) is configured; otherwise it skips with a clear message and the release carries checksums only. The public key is published by maintainers alongside the docs; never generate or invent release keys.

## 6. crates.io blocker (T-M9-45)

| Check | Current state | Consequence |
| --- | --- | --- |
| workspace crate `publish` | `false` for every crate | `cargo install poly-cli` does not work |
| root manifest version | `1.13.1` | aligned with the latest tag |
| release audit | `cargo package --list` 9/9 passed; `cargo publish --dry-run` refused by `publish = false` | audit complete, actual publishing closed |
| name availability | `poly-cli` and `poly-core` are taken by others | rename or obtain ownership before publishing |
| crates.io token | none in the repository or CI | no publishing secret configured |
| install entry points | source path / GitHub Release | the only supported paths today |

Conclusion: the T-M9-45 audit is complete and version/metadata are ready, but every workspace crate still sets `publish = false`, the `poly-cli` / `poly-core` names are taken, and there is no publishing token. Until those maintainer decisions are made, do not change the flag or claim crates.io support. See `docs/CRATES-IO.md` for details.

## 7. Verify and uninstall

```bash
poly --version
poly doctor
```

Uninstall: delete `poly` (`poly.exe` on Windows) from `POLY_INSTALL_DIR`. The install script never writes to system directories and never edits shell profiles.
