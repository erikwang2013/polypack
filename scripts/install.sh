#!/usr/bin/env bash
# PolyPack CLI installer (M9-G1 / T-M9-47).
#
# Detects Linux x86_64, macOS arm64, or Windows x86_64 (Git Bash), downloads the
# matching asset from a GitHub Release (latest, or POLYPACK_VERSION), verifies it
# against SHA256SUMS.txt, installs `polypack` into POLYPACK_INSTALL_DIR (default
# ~/.local/polypack/bin), and prints the installed version.
#
# Fail closed: a missing SHA256SUMS.txt entry, a digest mismatch, or a failed
# signature check (when POLYPACK_REQUIRE_SIGNATURE=1) aborts before anything is
# installed. There is no unverified or unsigned fallback.
#
# Environment:
#   POLYPACK_VERSION             release tag (v2.0.0 or 2.0.0); default: latest
#   POLYPACK_INSTALL_DIR         install directory; default: ~/.local/polypack/bin
#   POLYPACK_REPO                GitHub repo; default: erikwang2013/polypack
#   POLYPACK_RELEASE_BASE_URL    override release download base (mirrors / tests)
#   POLYPACK_REQUIRE_SIGNATURE   set to 1 to require a valid minisign signature
#   POLYPACK_MINISIGN_PUBKEY     path to the minisign public key (strict mode)
set -euo pipefail

readonly DEFAULT_REPO="erikwang2013/polypack"
readonly DEFAULT_INSTALL_DIR="${HOME}/.local/polypack/bin"

log()  { printf 'install: %s\n' "$*"; }
warn() { printf 'install: warning: %s\n' "$*" >&2; }
die()  { printf 'install: error: %s\n' "$*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os" in
    Linux)
      [ "$arch" = "x86_64" ] || die "unsupported Linux architecture: $arch (only x86_64 prebuilt assets exist; build from source)"
      TARGET="x86_64-unknown-linux-gnu"
      ASSET_EXT="tar.gz"
      BIN_NAME="polypack"
      ;;
    Darwin)
      [ "$arch" = "arm64" ] || die "unsupported macOS architecture: $arch (only arm64 prebuilt assets exist; build from source)"
      TARGET="aarch64-apple-darwin"
      ASSET_EXT="tar.gz"
      BIN_NAME="polypack"
      ;;
    MINGW* | MSYS* | CYGWIN*)
      [ "$arch" = "x86_64" ] || die "unsupported Windows architecture: $arch (only x86_64 prebuilt assets exist)"
      TARGET="x86_64-pc-windows-msvc"
      ASSET_EXT="zip"
      BIN_NAME="polypack.exe"
      ;;
    *)
      die "unsupported operating system: $os (supported: Linux x86_64, macOS arm64, Windows x86_64 Git Bash)"
      ;;
  esac
  ASSET_PREFIX="polypack"
  ASSET="${ASSET_PREFIX}-${TARGET}.${ASSET_EXT}"
}

resolve_tag() {
  if [ -n "${POLYPACK_VERSION:-}" ]; then
    case "$POLYPACK_VERSION" in
      v*) TAG="$POLYPACK_VERSION" ;;
      *) TAG="v$POLYPACK_VERSION" ;;
    esac
    return 0
  fi
  [ -z "${POLYPACK_RELEASE_BASE_URL:-}" ] || die "POLYPACK_VERSION is required when POLYPACK_RELEASE_BASE_URL is set"
  local effective
  effective="$(curl -fsSIL --proto '=https' --tlsv1.2 -o /dev/null -w '%{url_effective}' \
    "https://github.com/${REPO}/releases/latest")" \
    || die "failed to resolve the latest release for ${REPO}"
  TAG="${effective##*/}"
  case "$TAG" in
    v*) ;;
    *) die "could not resolve the latest release tag (got '${TAG}')" ;;
  esac
}

base_url() {
  if [ -n "${POLYPACK_RELEASE_BASE_URL:-}" ]; then
    printf '%s' "${POLYPACK_RELEASE_BASE_URL%/}"
  else
    printf '%s' "https://github.com/${REPO}/releases/download/${TAG}"
  fi
}

fetch() {
  local url="$1" dest="$2"
  local -a args=(-fsSL --retry 3 --retry-delay 2)
  case "$url" in
    https://*) args+=(--proto '=https' --tlsv1.2) ;;
  esac
  curl "${args[@]}" -o "$dest" "$url" || die "download failed: $url"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$1" | awk '{print $NF}'
  else
    die "no SHA-256 tool found (need sha256sum, shasum, or openssl)"
  fi
}

verify_signature() {
  if [ "${POLYPACK_REQUIRE_SIGNATURE:-}" != "1" ]; then
    warn "minisign check not requested (POLYPACK_REQUIRE_SIGNATURE=1 to enforce); SHA-256 is always verified"
    return 0
  fi
  [ -n "${POLYPACK_MINISIGN_PUBKEY:-}" ] || die "POLYPACK_REQUIRE_SIGNATURE=1 requires POLYPACK_MINISIGN_PUBKEY"
  [ -r "$POLYPACK_MINISIGN_PUBKEY" ] || die "minisign public key not readable: $POLYPACK_MINISIGN_PUBKEY"
  need minisign
  fetch "${BASE}/SHA256SUMS.txt.minisig" "$SIG_FILE"
  minisign -Vm "$SUMS_FILE" -p "$POLYPACK_MINISIGN_PUBKEY" -x "$SIG_FILE" >/dev/null \
    || die "minisign signature verification failed (unsigned fallback is disabled)"
  log "signature verified: SHA256SUMS.txt.minisig"
}

verify_checksum() {
  local expected actual
  expected="$(awk -v asset="$ASSET" '$2 == asset { print $1; exit }' "$SUMS_FILE")"
  [ -n "$expected" ] || die "SHA256SUMS.txt has no entry for ${ASSET}"
  actual="$(sha256_of "$ARCHIVE")"
  expected="$(printf '%s' "$expected" | tr 'A-F' 'a-f')"
  actual="$(printf '%s' "$actual" | tr 'A-F' 'a-f')"
  [ "$expected" = "$actual" ] \
    || die "SHA-256 mismatch for ${ASSET} (expected ${expected}, got ${actual})"
  log "checksum verified: ${ASSET}"
}

extract_archive() {
  case "$ASSET_EXT" in
    tar.gz)
      need tar
      tar -xzf "$ARCHIVE" -C "$WORKDIR"
      BIN_SRC="$WORKDIR/${ASSET_PREFIX}-${TARGET}"
      ;;
    zip)
      if command -v unzip >/dev/null 2>&1; then
        unzip -q "$ARCHIVE" -d "$WORKDIR"
      elif command -v powershell.exe >/dev/null 2>&1 && command -v cygpath >/dev/null 2>&1; then
        powershell.exe -NoProfile -NonInteractive -Command \
          "Expand-Archive -LiteralPath '$(cygpath -w "$ARCHIVE")' -DestinationPath '$(cygpath -w "$WORKDIR")' -Force" \
          >/dev/null
      else
        die "no zip extractor found (need unzip or powershell.exe + cygpath)"
      fi
      BIN_SRC="$WORKDIR/${ASSET_PREFIX}-${TARGET}.exe"
      ;;
    *)
      die "unsupported archive type: ${ASSET_EXT}"
      ;;
  esac
  [ -f "$BIN_SRC" ] || die "archive did not contain the expected binary: $(basename "$BIN_SRC")"
}

install_binary() {
  local dest="${INSTALL_DIR}/${BIN_NAME}" tmp
  mkdir -p "$INSTALL_DIR" || die "cannot create install directory: $INSTALL_DIR"
  tmp="${dest}.tmp.$$"
  cp "$BIN_SRC" "$tmp" || die "copy failed: $tmp"
  chmod 0755 "$tmp"
  mv -f "$tmp" "$dest" || { rm -f "$tmp"; die "cannot replace ${dest}"; }
  INSTALLED_BIN="$dest"
}

main() {
  REPO="${POLYPACK_REPO:-$DEFAULT_REPO}"
  INSTALL_DIR="${POLYPACK_INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
  need curl
  detect_target
  resolve_tag
  BASE="$(base_url)"
  WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/polypack-install.XXXXXX")"
  trap 'rm -rf "$WORKDIR"' EXIT
  ARCHIVE="$WORKDIR/$ASSET"
  SUMS_FILE="$WORKDIR/SHA256SUMS.txt"
  SIG_FILE="$WORKDIR/SHA256SUMS.txt.minisig"

  log "installing polypack ${TAG} for ${TARGET}"
  fetch "${BASE}/${ASSET}" "$ARCHIVE"
  fetch "${BASE}/SHA256SUMS.txt" "$SUMS_FILE"
  verify_signature
  verify_checksum
  extract_archive
  install_binary

  [ -x "$INSTALLED_BIN" ] || die "installed binary is not executable: $INSTALLED_BIN"
  local version
  version="$("$INSTALLED_BIN" --version 2>&1)" || die "installed binary failed to run: $INSTALLED_BIN"
  log "installed ${version} -> ${INSTALLED_BIN}"
  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *) warn "${INSTALL_DIR} is not on PATH; add it (e.g. export PATH=\"${INSTALL_DIR}:\$PATH\")" ;;
  esac
}

main "$@"
