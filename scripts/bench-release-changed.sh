#!/usr/bin/env bash
# T-M9-09：5 目标仓库二次 `poly release --changed` 实测（无网络）。
#
# 首跑发布 5 个目标；只改一个产物后二跑应只发布 1 个、其余 4 个输出
# `release-skipped`，且 skipped 回执零重写。后端调用以 published 事件计数
# （--changed 命中 skipped 时不触仓库后端）。
#
# 依赖：gpg（一次性签名密钥）、dpkg-deb（夹具产物）、POLY_BIN 或 cargo。
# Linux-only（GNU date / base64 -w0）。用法：
#   scripts/bench-release-changed.sh
#   POLY_BIN=target/debug/poly scripts/bench-release-changed.sh
#   POLY_BENCH_DIR=/tmp/bench POLY_BENCH_KEEP=1 scripts/bench-release-changed.sh
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
work=${POLY_BENCH_DIR:-$(mktemp -d)}
keep=${POLY_BENCH_KEEP:-0}
cleanup() { [ "$keep" = 1 ] || rm -rf "$work"; }
trap cleanup EXIT

if [ "$(uname -s)" != "Linux" ]; then
  echo "bench-release-changed: Linux-only（GNU date / base64 -w0）" >&2
  exit 2
fi
for tool in gpg dpkg-deb base64 grep; do
  command -v "$tool" >/dev/null || {
    echo "bench-release-changed: 缺少 $tool" >&2
    exit 2
  }
done

poly() {
  if [ -n "${POLY_BIN:-}" ]; then
    "$POLY_BIN" "$@"
  else
    (cd "$repo_root" && cargo run -q -p poly-cli -- "$@")
  fi
}

now_ms() { echo $(($(date +%s%N) / 1000000)); }
fail() { echo "bench-release-changed: $*" >&2; exit 1; }

# 预热：计时不含 cargo 编译 / 进程首次启动的噪声。
poly --version >/dev/null

targets=(linux web windows macos android)
changed=android
app="$work/app"
mkdir -p "$app"

{
  printf '[package]\nname = "release-bench"\nversion = "0.1.0"\n'
  printf '[ui]\nframework = "react"\nentry = "index.html"\n'
  printf '[core]\ncrate = "core"\n'
  for target in "${targets[@]}"; do
    printf '[targets.%s]\nformat = "deb"\n' "$target"
  done
} >"$app/polypack.toml"

# 一次性 GPG 私钥 → env: 凭据载荷（base64(JSON)），与 CI 注入同契约。
export GNUPGHOME="$work/gnupg"
mkdir -m 700 "$GNUPGHOME"
pass=release-bench-pass
gpg --batch --no-options --pinentry-mode loopback --passphrase "$pass" \
  --quick-generate-key "PolyPack Release Bench <bench@polypack.invalid>" \
  ed25519 sign 0 >/dev/null 2>&1
key=$(gpg --batch --no-options --pinentry-mode loopback --passphrase "$pass" \
  --export-secret-keys | base64 -w0)
[ -n "$key" ] || fail "gpg 私钥导出为空"
payload_json=$(printf '{"keystore":"%s","store_password":"%s"}' "$key" "$pass")
POLY_RELEASE_KEY=$(printf '%s' "$payload_json" | base64 -w0)
export POLY_RELEASE_KEY
gpgconf --homedir "$GNUPGHOME" --kill gpg-agent >/dev/null 2>&1 || true

make_deb() { # target marker
  local target=$1 marker=$2 stage="$work/stage/$1" out="$app/.polypack/packages/$1"
  rm -rf "$stage"
  mkdir -p "$stage/DEBIAN" "$stage/usr/share/fixture" "$out"
  printf 'Package: fixture-app\nVersion: 0.1.0\nArchitecture: amd64\n' >"$stage/DEBIAN/control"
  printf 'Maintainer: PolyPack <bench@polypack.invalid>\nDescription: release changed bench\n' \
    >>"$stage/DEBIAN/control"
  printf 'target=%s marker=%s\n' "$target" "$marker" >"$stage/usr/share/fixture/payload.txt"
  # -Zgzip：后端只解析 control.tar.gz（dpkg 缺省 control.tar.xz）。
  dpkg-deb -Zgzip --build --root-owner-group "$stage" \
    "$out/fixture-app_0.1.0_amd64.deb" >/dev/null
}

for target in "${targets[@]}"; do
  make_deb "$target" "$target-v1"
done

target_list=$(IFS=,; echo "${targets[*]}")
common=(release --config "$app/polypack.toml" --repo "$work/repo" \
  --target "$target_list" --changed --credential env:POLY_RELEASE_KEY --json)

first_start=$(now_ms)
poly "${common[@]}" >"$work/first.jsonl" || fail "首跑失败（见 $work/first.jsonl）"
first_ms=$(($(now_ms) - first_start))

first_published=$(grep -c '"event":"published"' "$work/first.jsonl" || true)
first_skipped=$(grep -c '"event":"release-skipped"' "$work/first.jsonl" || true)

# skipped 目标回执基线（排除被改目标）。
receipts_before=$(find "$app/.polypack/release" -type f -name '*.json' |
  grep -v "/$changed/" | sort | xargs sha256sum)

# 只改一个产物（data 里的 marker 变化 → 清单摘要变化）。
make_deb "$changed" "$changed-v2"

second_start=$(now_ms)
poly "${common[@]}" >"$work/second.jsonl" || fail "二跑失败（见 $work/second.jsonl）"
second_ms=$(($(now_ms) - second_start))

second_published=$(grep -c '"event":"published"' "$work/second.jsonl" || true)
second_skipped=$(grep -c '"event":"release-skipped"' "$work/second.jsonl" || true)
changed_published=$(grep '"event":"published"' "$work/second.jsonl" |
  grep -c "\"target\":\"$changed\"" || true)

receipts_after=$(find "$app/.polypack/release" -type f -name '*.json' |
  grep -v "/$changed/" | sort | xargs sha256sum)

[ "$first_published" -eq 5 ] || fail "首跑 published=$first_published（期望 5）"
[ "$first_skipped" -eq 0 ] || fail "首跑 release-skipped=$first_skipped（期望 0）"
[ "$second_published" -eq 1 ] || fail "二跑 published=$second_published（期望 1）"
[ "$second_skipped" -eq 4 ] || fail "二跑 release-skipped=$second_skipped（期望 4）"
[ "$changed_published" -eq 1 ] || fail "二跑的 published 不是变更目标 $changed"
[ "$receipts_before" = "$receipts_after" ] || fail "skipped 目标回执被重写"

call_saving=$(((first_published - second_published) * 100 / first_published))
if [ "$first_ms" -gt 0 ]; then
  wall_saving=$(((first_ms - second_ms) * 100 / first_ms))
else
  wall_saving=0
fi
[ "$call_saving" -ge 50 ] || fail "后端调用节省 $call_saving%（期望 ≥ 50%）"

echo "bench-release-changed: targets=${#targets[@]} changed=$changed"
echo "  first : published=$first_published skipped=$first_skipped backend_calls=$first_published wall_ms=$first_ms"
echo "  second: published=$second_published skipped=$second_skipped backend_calls=$second_published wall_ms=$second_ms"
echo "  saving: backend_calls=$call_saving% wall_clock=$wall_saving%"
echo "  skipped receipts: byte-identical (sha256 baseline unchanged)"
if [ "$keep" = 1 ]; then
  echo "  workdir: $work"
fi
