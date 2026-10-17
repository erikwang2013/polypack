#!/bin/sh
# Fetch the pinned FreeRTOS kernel and verify its sha256 (T-M6-07).
#
# Exit codes are consumed by the `freertos-qemu-smoke` verify leg:
#   0 = kernel ready in $KERNEL_DIR
#   1 = download/tooling unavailable (environment slot -> SKIP-with-print)
#   2 = checksum mismatch (hard failure; never fall back to a different kernel)
set -eu

KERNEL_DIR=${KERNEL_DIR:-$(pwd)/.freertos-kernel}
KERNEL_TAG=${KERNEL_TAG:-V11.1.0}
KERNEL_SHA256=${KERNEL_SHA256:-0e21928b3bcc4f9bcaf7333fb1c8c0299d97e2ec9e13e3faa2c5a7ac8a3bc573}
KERNEL_URL=${KERNEL_URL:-https://codeload.github.com/FreeRTOS/FreeRTOS-Kernel/tar.gz/refs/tags/$KERNEL_TAG}

ready=1
for file in tasks.c queue.c list.c include/FreeRTOS.h \
    portable/GCC/ARM_CM3/port.c portable/MemMang/heap_4.c; do
    if [ ! -f "$KERNEL_DIR/$file" ]; then
        ready=0
        break
    fi
done
if [ "$ready" = "1" ]; then
    echo "FreeRTOS kernel $KERNEL_TAG already present: $KERNEL_DIR"
    exit 0
fi

tmp="$KERNEL_DIR.tar.gz"
mkdir -p "$(dirname "$KERNEL_DIR")"
if command -v curl >/dev/null 2>&1; then
    curl -fsSL --max-time 120 -o "$tmp" "$KERNEL_URL" || {
        echo "kernel download failed: $KERNEL_URL (network unavailable?)" >&2
        rm -f "$tmp"
        exit 1
    }
elif command -v wget >/dev/null 2>&1; then
    wget -q -T 120 -O "$tmp" "$KERNEL_URL" || {
        echo "kernel download failed: $KERNEL_URL (network unavailable?)" >&2
        rm -f "$tmp"
        exit 1
    }
else
    echo "kernel fetch needs curl or wget (install one, then re-run make fetch)" >&2
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmp" | cut -d' ' -f1)
elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$tmp" | cut -d' ' -f1)
elif command -v openssl >/dev/null 2>&1; then
    actual=$(openssl dgst -sha256 "$tmp" | awk '{print $NF}')
else
    echo "kernel checksum needs sha256sum, shasum or openssl (install one)" >&2
    rm -f "$tmp"
    exit 1
fi

if [ "$actual" != "$KERNEL_SHA256" ]; then
    echo "sha256 mismatch for $KERNEL_TAG: got $actual, expected $KERNEL_SHA256" >&2
    rm -f "$tmp"
    exit 2
fi

rm -rf "$KERNEL_DIR"
mkdir -p "$KERNEL_DIR"
tar xzf "$tmp" -C "$KERNEL_DIR" --strip-components=1
rm -f "$tmp"
echo "FreeRTOS kernel $KERNEL_TAG ready: $KERNEL_DIR"
