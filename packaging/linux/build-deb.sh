#!/usr/bin/env bash
# Build a .deb for the polypack CLI. Usage: build-deb.sh <binary> <version> <output.deb> [arch]
set -euo pipefail

binary=${1:?usage: build-deb.sh <binary> <version> <output.deb> [arch]}
version=${2:?version required}
output=${3:?output required}
arch=${4:-amd64}
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/DEBIAN" "$stage/opt/polypack/bin" "$stage/usr/bin" \
  "$stage/usr/share/applications" "$stage/usr/share/icons/hicolor/scalable/apps"
install -m 0755 "$binary" "$stage/opt/polypack/bin/polypack"
ln -s /opt/polypack/bin/polypack "$stage/usr/bin/polypack"
install -m 0644 "$(dirname "$0")/polypack.desktop" "$stage/usr/share/applications/polypack.desktop"
install -m 0644 "$(dirname "$0")/../../site/assets/img/pax-mark.svg" \
  "$stage/usr/share/icons/hicolor/scalable/apps/polypack.svg"
cat > "$stage/DEBIAN/control" <<EOF
Package: polypack
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
