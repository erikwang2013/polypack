#!/usr/bin/env bash
# M9-E2 demo: generate two apps with `poly init`, declare a root workspace, and
# build them twice to show per-app outputs and second-run cache hits.
#
# Usage:
#   POLY_BIN=/path/to/poly ./scripts/demo-workspace.sh
#   # or with cargo: POLY_BIN="cargo run -q -p poly-cli --" ./scripts/demo-workspace.sh
set -euo pipefail

POLY_BIN="${POLY_BIN:-poly}"
TARGET="${POLY_DEMO_TARGET:-}"

if [ -z "$TARGET" ]; then
  case "$(uname -s)" in
    Linux) TARGET=linux ;;
    Darwin) TARGET=macos ;;
    MINGW* | MSYS* | CYGWIN*) TARGET=windows ;;
    *) echo "unsupported host: $(uname -s)" >&2; exit 1 ;;
  esac
fi

root="$(mktemp -d "${TMPDIR:-/tmp}/polypack-workspace-demo.XXXXXX")"
trap 'rm -rf "$root"' EXIT

for app in notes timer; do
  mkdir -p "$root/apps/$app"
  (
    cd "$root/apps/$app"
    # shellcheck disable=SC2086
    $POLY_BIN init >/dev/null
  )
done

cat >"$root/polypack.toml" <<'EOF'
schema = 1

[workspace]
members = ["apps/notes", "apps/timer"]
EOF

echo "== first workspace build"
(
  cd "$root"
  # shellcheck disable=SC2086
  $POLY_BIN build --workspace --target "$TARGET"
)

echo "== second workspace build (expect cache hits)"
(
  cd "$root"
  # shellcheck disable=SC2086
  $POLY_BIN build --workspace --target "$TARGET"
)

echo "== per-app packages"
find "$root/.polypack/packages" -maxdepth 3 -type f -printf '%P\n' | sort
