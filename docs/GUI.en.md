# GUI build-history matrix (M9-E6 · T-M9-18)

> Status: T-M9-18 delivered (Tauri v2 + static frontend); the real Tauri window was not launched in this environment, disclosed as environment-gated.
> Data source: `.polypack/history/events.jsonl` (same `HistoryRecord` schema as `polypack history`).

## 1. What it is / is not

| Is | Is not |
| --- | --- |
| A read-only desktop view: target matrix + recent-build detail per target + empty-state guide | Not a new data source: it never rewrites, deletes, or rotates the history file |
| A Tauri v2 `history_records` command reading the local JSONL and redacting paths again on read | Not a multi-user server, cloud sync, or remote-farm scheduling UI |
| A single-project, single-machine post-build review | Not an installer: `bundle.active=false`, so no deb / MSI / AppImage is produced |

## 2. Data source and privacy

`polypack build` appends `.polypack/history/events.jsonl` under the project root, one `HistoryRecord` per line (`schema_version`, `build_id`, `ts`, `target`, `event`, `status`, `duration_ms`, optional `artifact` / `artifact_digest` / `error_code`). The GUI deserializes through `polypack_core::events::HistoryRecord`, so the field definition is shared with the CLI instead of copied.

Paths are handled twice: before writing, the CLI turns in-project paths into relative paths and writes `<redacted>` for absolute paths outside the project and `..` traversal; on read, the GUI calls `polypack_core::events::redact_path` again for every `artifact`, so hand-edited or legacy records cannot bypass redaction.

A missing history file means empty history and shows the guide; unparseable lines are skipped and the skipped count is displayed.

## 3. Build and run

### 3.1 Prerequisites

Linux needs the GTK3 and WebKitGTK 4.1 development packages (Debian/Ubuntu: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev`); Windows uses WebView2; macOS uses the system WKWebView. `app/src-tauri` is a standalone Cargo workspace and is not part of the root workspace's `crates/*` build.

### 3.2 Run

```bash
# 1) Generate history in any project root containing polypack.toml
polypack build

# 2) Start the GUI from that project root (keeps cwd = project root)
cargo run --manifest-path /path/to/polypack/app/src-tauri/Cargo.toml
```

An empty `Project root` uses the process working directory; an absolute path also works. An empty `Target` shows every target; `Per target` controls how many recent records are returned per target (1–200, default 20). Click a matrix row or target name to open details, and `Back to matrix` to return. CLI equivalent: `polypack history [--target ID] [--limit N] [--json]`.

### 3.3 Without a Tauri build

If the WebKitGTK / GTK development packages are missing, `cargo check` fails. In that case `app/web/index.html` can still be inspected as static structure, but the page will not load data; this delivery does not ship a static `history.json` fallback because the Tauri build is available in this repository's target environment. For a pure static panel, run `polypack history --json > history.json` and host it yourself; the fields match this GUI.

## 4. View snapshot

These are text illustrations, not screenshots; the data has the same shape as the unit-test fixtures. The real window was not launched in this environment (see §5).

### 4.1 Matrix

```text
TARGET   LATEST STATUS   TIME (UTC)             DURATION   ARTIFACT                                   ERROR CODE
linux    success         2026-10-20T10:42:11Z   1.25s      .polypack/packages/linux/app.AppImage       -
web      warning         2026-10-20T10:41:03Z   842ms      .polypack/packages/web/app.zip             -
tizen    failed          2026-10-20T10:39:55Z   2.10s      <redacted>                                 polypack::E_COMPILE_0001
```

### 4.2 Detail

```text
← Back to matrix     linux · 2 most recent record(s) · latest 2026-10-20T10:42:11Z
BUILD                  STATUS    TIME (UTC)             DURATION   ARTIFACT                              DIGEST        ERROR CODE
build-1750416131000    success   2026-10-20T10:42:11Z   1.25s      .polypack/packages/linux/app.AppImage  sha256:abc    -
build-1750416073000    failed    2026-10-20T10:41:13Z   980ms      <redacted>                             -             polypack::E_COMPILE_0001
```

### 4.3 Empty state

```text
No build history yet
Run a build in the project root, then refresh this view.
1. Run polypack build in the project root.
2. History is appended to .polypack/history/events.jsonl.
3. Click Refresh; clear the target filter if one is set.
CLI equivalent: polypack history --json
```

## 5. Tests and verification

- `cargo test --manifest-path app/src-tauri/Cargo.toml`: empty history, multi-target aggregation, latest-N per target, read-time path redaction, malformed-line skipping, and a static structure check for the matrix / detail / empty-state markers; all 6 passed in this environment.
- `cargo clippy --manifest-path app/src-tauri/Cargo.toml --all-targets -- -D warnings`: passed in this environment.
- Launching the real Tauri window is environment-gated: it needs a working graphical session and the WebKitGTK runtime; this delivery did not pop a window on the maintainer's desktop and does not claim an end-to-end GUI click test.

## 6. Known limitations

- Read-only with manual refresh: no file watching, no auto-refresh, no pagination or full-text search.
- One project root at a time; the root is typed rather than chosen with a native directory picker.
- The whole history file is read (the CLI already caps it at 5 MiB / 10,000 records); unparseable lines only produce a count, not per-line details.
- `bundle.active=false`: no installer is produced; real Windows / macOS windows are not verified in this environment.
- History times are shown in UTC; the UI does not convert time zones.
