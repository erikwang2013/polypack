#!/usr/bin/env bash
# Build an unsigned .pkg for the polypack CLI. Usage: build-pkg.sh <binary> <version> <output.pkg>
set -euo pipefail

binary=${1:?usage: build-pkg.sh <binary> <version> <output.pkg>}
version=${2:?version required}
output=${3:?output required}
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT
mkdir -p "$root/usr/local/polypack/bin" "$root/usr/local/bin"
install -m 0755 "$binary" "$root/usr/local/polypack/bin/polypack"
ln -s ../polypack/bin/polypack "$root/usr/local/bin/polypack"
pkgbuild --root "$root" --identifier dev.polypack.cli --version "$version" \
  --install-location / "$output"
