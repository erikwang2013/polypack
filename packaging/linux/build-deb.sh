#!/usr/bin/env bash
# Build a .deb for the poly CLI. Usage: build-deb.sh <binary> <version> <output.deb> [arch]
set -euo pipefail

binary=${1:?usage: build-deb.sh <binary> <version> <output.deb> [arch]}
version=${2:?version required}
output=${3:?output required}
arch=${4:-amd64}
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/DEBIAN" "$stage/usr/bin"
install -m 0755 "$binary" "$stage/usr/bin/poly"
cat > "$stage/DEBIAN/control" <<EOF
Package: polypack-cli
Version: $version
Section: devel
Priority: optional
Architecture: $arch
Maintainer: Erik <erik@erik.xyz>
Homepage: https://package.erik.xyz/polypack/
Description: PolyPack cross-platform packaging CLI
 Rust core, React UI adapters and a verifiable build/release pipeline
 for desktop, mobile, mini-program, TV, wearable and embedded targets.
EOF
dpkg-deb --build --root-owner-group "$stage" "$output"
