# crates.io Publication Audit (T-M9-45)

> Status: the audit is complete and publication remains disabled. This document authorizes and performs no upload.
> Audit date: 2026-10-20; name availability comes from the crates.io API on that date.
> Chinese: [CRATES-IO.md](CRATES-IO.md)

## 1. Conclusion

- The workspace version is raised from `0.0.0` to `1.13.1` with `Cargo.lock` synced; `poly --version` now matches the latest tag.
- All 9 crates have `description`, `license`, `repository`, `readme`, `keywords`, and `categories`. `readme` inherits the root `README.md`, while `keywords` / `categories` are declared once in the root manifest.
- Internal workspace path dependencies now carry `version = "1.13.1"`; without a version, `cargo package` refuses to package them (path dependency without version).
- Every crate still sets `publish = false`. Nothing was published or uploaded, and no crates.io token was used.
- Two names are already owned by others on crates.io: `poly-cli` and `poly-core`; the other 7 are available. Publication requires a rename or ownership transfer, which is a maintainer decision.
- `cargo package --list` passes 9/9 (content audit); `cargo publish --dry-run` is refused because of `publish = false`. That is the intended release gate, not a metadata error.

## 2. Publish Order

| Order | Crate | Internal dependencies | Name status (2026-10-20) |
| --- | --- | --- | --- |
| 1 | `poly-targets` | — | available |
| 2 | `poly-cache` | — | available |
| 3 | `poly-toolchain` | — | available |
| 4 | `poly-core` | `poly-cache`, `poly-targets` (optional `poly-toolchain`) | taken (0.0.1) |
| 5 | `poly-bundle` | `poly-targets` | available |
| 6 | `poly-sign` | `poly-core` | available |
| 7 | `poly-publish` | `poly-core` | available |
| 8 | `poly-plugin` | `poly-core`, `poly-sign`, `poly-targets` | available |
| 9 | `poly-cli` | all internal crates | taken (0.5.0) |

## 3. Required Metadata

| Field | Source | Current value / status |
| --- | --- | --- |
| `version` | `[workspace.package]` | `1.13.1` |
| `description` | each crate `Cargo.toml` | filled for 9/9 |
| `license` | `[workspace.package]` | `Apache-2.0` (required by crates.io) |
| `repository` | `[workspace.package]` | `https://github.com/erikwang2013/polypack` |
| `readme` | `[workspace.package]` | root `README.md`, archived with every package |
| `keywords` | `[workspace.package]` | `polypack`, `packaging`, `build-tool`, `cross-platform`, `mobile` |
| `categories` | `[workspace.package]` | `development-tools::build-utils`, `command-line-utilities` |
| `publish` | each crate `Cargo.toml` | `false` (9/9, unchanged) |
| `documentation` / `homepage` | unset | not required by crates.io |

## 4. Current Blockers

| Blocker | Evidence | Status / exit condition |
| --- | --- | --- |
| Version was `0.0.0` | root manifest value; `poly --version` printed `0.0.0` | resolved: raised to `1.13.1` |
| Path dependencies had no `version` | `cargo package` reports "all dependencies must have a version specified when packaging" | resolved: workspace dependencies now carry `1.13.1` |
| `publish = false` | set on 9/9 crates | open: remove in a separate commit after the maintainer approves the plan |
| `poly-cli` name taken | crates.io has 0.5.0 owned outside this repository | open: rename to `polypack-cli` (available) or obtain ownership |
| `poly-core` name taken | crates.io has 0.0.1 owned outside this repository | open: rename to `polypack-engine` (available) or obtain ownership |
| No crates.io token | no `CARGO_REGISTRY_TOKEN` in the repository or CI secrets | open: maintainer configures a secret; never commit it |

## 5. Commands

Audit commands; no token and no upload:

```bash
# Content listing (one crate at a time; CI runs only this step, on Linux)
cargo package --list -p poly-targets --locked
cargo package --list -p poly-cache --locked
cargo package --list -p poly-toolchain --locked
cargo package --list -p poly-core --locked
cargo package --list -p poly-bundle --locked
cargo package --list -p poly-sign --locked
cargo package --list -p poly-publish --locked
cargo package --list -p poly-plugin --locked
cargo package --list -p poly-cli --locked

# Inspect the current gate (expected failure: publish = false)
cargo publish --dry-run -p poly-targets
```

Actual publication (only after the maintainer approves and `publish = false` is removed; follow the §2 order and wait for each crate to appear in the index):

```bash
cargo publish -p poly-targets
cargo publish -p poly-cache
cargo publish -p poly-toolchain
cargo publish -p poly-core
cargo publish -p poly-bundle
cargo publish -p poly-sign
cargo publish -p poly-publish
cargo publish -p poly-plugin
cargo publish -p poly-cli
```

## 6. Release Checklist

| # | Check | Command | Pass criterion |
| --- | --- | --- | --- |
| 1 | Contents and metadata | `cargo package --list -p <crate> --locked` | 9/9 succeed; listings contain only sources, tests, README, and Cargo files |
| 2 | Name decision | maintainer reviews §4 | a written conclusion for the `poly-cli` / `poly-core` rename or ownership |
| 3 | Remove the release gate | separate commit deleting `publish = false` | 9/9 crates publishable and CI still green |
| 4 | Tests | `cargo test --workspace --no-fail-fast` | all green |
| 5 | Lint | `cargo clippy --workspace --all-targets -- -D warnings` | all green |
| 6 | Version consistency | `cargo run -p poly-cli -- --version` | prints `1.13.1`, matching `[workspace.package].version` |
| 7 | Topological dry-run | `cargo publish --dry-run -p <crate>` | all pass in §2 order |
| 8 | Actual publication | `cargo publish -p <crate>` | succeeds in §2 order; continue only after the index shows the crate |
| 9 | Install verification | `cargo install poly-cli --version 1.13.1 --locked` | installs bin `poly`; `poly --version` prints `1.13.1` |

## 7. Version and Tag Policy

- `[workspace.package].version` is the single version source; the 9 crates and the workspace dependency `version` values must move together in the same commit.
- Tags use `vX.Y.Z` and match the `[X.Y.Z]` section in `CHANGELOG.md`; the tag points at the same commit used for the release build.
- crates.io versions are immutable: only yanking is allowed, never re-publishing the same version; missing content requires a patch bump.
- Publication follows the §2 dependency graph; prerelease versions (`-alpha.N`, etc.) need an explicit maintainer decision and are not used automatically for regular releases.
