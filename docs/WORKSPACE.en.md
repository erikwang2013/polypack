# Multi-app workspace (M9-E2)

> Status: code and tests delivered (T-M9-04/05/06); single-app behavior and cache keys are unchanged. `poly build`, `poly verify`, and `poly retry` all accept `--workspace` / `--app`.

## 1. Layout

```text
my-workspace/
  polypack.toml              # root config: schema + [workspace].members
  apps/
    notes/
      polypack.toml          # the member's own single-app config
      core/
      ui/
    timer/
      polypack.toml
      core/
      ui/
```

The member id is the final directory segment (`notes`, `timer`) and must be unique; member paths must be relative to the workspace root. `..`, absolute paths, and symlink escapes are rejected.

## 2. Root config

```toml
schema = 1

[workspace]
members = ["apps/notes", "apps/timer"]
```

The root config only declares members; `[package]` / `[ui]` / `[core]` / `[targets.*]` remain in each member's own `polypack.toml`. Without `[workspace]`, the old single-app path is used unchanged.

## 3. Member config

A member config is the existing single-app config, for example:

```toml
schema = 1

[package]
name = "Pax Notes"
version = "1.2.3"

[ui]
framework = "react"
entry = "src/main.tsx"

[core]
crate = "core"

[targets.linux]
format = "deb"
```

## 4. Commands

```bash
# Build every member in declaration order
poly build --workspace

# Build one member
poly build --app notes

# Combine with a target
poly build --workspace --target linux

# Verify / retry also accept the workspace selectors
poly verify --workspace
poly retry --from verify --app notes
```

`--workspace` and `--app` are mutually exclusive; neither flag keeps the single-app behavior. Outputs and snapshots are isolated per app:

```text
<workspace>/.polypack/packages/<app>/<target>/
<workspace>/.polypack/artifacts/<app>/<target>/
<workspace>/.polypack/cache/            # shared by the workspace
```

The shared core compile cache key excludes the app id (reused across apps); the UI bundle key includes it (no cross-app collisions).

> Run `scripts/demo-workspace.sh` to generate two apps with `poly init` and demonstrate a second-run cache hit; use `POLY_BIN` to point at the `poly` binary and `POLY_DEMO_TARGET` to override the target.

## 5. Diagnostics

| Code | Meaning |
| --- | --- |
| `E_WS_0001` | invalid `[workspace]` declaration (type / empty members / reserved path, etc.) |
| `E_WS_0002` | member directory or member `polypack.toml` missing / unloadable |
| `E_WS_0003` | member path escapes the workspace root (including after symlink canonicalization) |
| `E_WS_0004` | `--app <id>` names an undeclared member |

## 6. Known limitations

- No cross-app task graph, target inheritance, or concurrent scheduling; members build sequentially.
- Member ids are directory names; renaming a directory changes the app id and cache keys.
- Build history currently lands under each member root, not aggregated at the workspace root.
