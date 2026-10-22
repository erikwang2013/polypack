#!/usr/bin/env bash
# Build an AppImage for the poly CLI. Usage: build-appimage.sh <binary> <version> <appimagetool> <output.AppImage>
set -euo pipefail

binary=${1:?usage: build-appimage.sh <binary> <version> <appimagetool> <output.AppImage>}
version=${2:?version required}
tool=${3:?appimagetool required}
output=${4:?output required}
appdir=$(mktemp -d)
trap 'rm -rf "$appdir"' EXIT
mkdir -p "$appdir/usr/bin"
install -m 0755 "$binary" "$appdir/usr/bin/poly"
install -m 0755 "$(dirname "$0")/AppRun" "$appdir/AppRun"
install -m 0644 "$(dirname "$0")/poly.desktop" "$appdir/poly.desktop"
install -m 0644 "$(dirname "$0")/../../site/assets/img/pax-mark.svg" "$appdir/poly.svg"
ARCH=x86_64 "$tool" --appimage-extract-and-run "$appdir" "$output"
