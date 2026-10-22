#!/usr/bin/env bash
# T-M8-09 农场部署演练：真实 `polypack farm submit` 多目标双 worker + 共享缓存二跑命中 +
# 凭据内存注入（哨兵扫描）+ 共享缓存降级。macOS / IPA 腿（R3）按环境槽 SKIP-with-print。
#
# 用法（先构建同一提交的 CLI）：
#   cargo build -p polypack-cli
#   POLYPACK=target/debug/polypack tools/farm-drill.sh
#   DRILL_OUT=/tmp/farm-drill tools/farm-drill.sh
#
# 产物归档在 $DRILL_OUT（缺省 target/farm-drill/<UTC 时间戳>）：transcript.log + logs/。
# 本脚本不访问网络、不需要 node / Xcode；夹具只有静态 UI 链（web / vidaa-tv / tizen-tv）。
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
POLYPACK=${POLYPACK:-polypack}
if ! command -v "$POLYPACK" >/dev/null 2>&1; then
  if [ -x "$ROOT/target/debug/polypack" ]; then
    POLYPACK="$ROOT/target/debug/polypack"
  elif [ -x "$ROOT/target/release/polypack" ]; then
    POLYPACK="$ROOT/target/release/polypack"
  else
    echo "找不到 polypack：先 cargo build -p polypack-cli，或设 POLYPACK=<二进制路径>" >&2
    exit 1
  fi
fi
POLYPACK=$(command -v "$POLYPACK")
case "$POLYPACK" in /*) ;; *) POLYPACK="$(cd "$(dirname "$POLYPACK")" && pwd)/$(basename "$POLYPACK")" ;; esac

OUT=${DRILL_OUT:-"$ROOT/target/farm-drill/$(date -u +%Y%m%dT%H%M%SZ)"}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd) # worker 在各自 cwd 下执行，OUT/POLYPACK 必须是绝对路径
SHARED="$OUT/cache"
FARM="$OUT/farm"
WORK="$OUT/work"
LOG="$OUT/transcript.log"
# 重跑同一 DRILL_OUT：只清理本脚本自建的固定路径，并重写 transcript，避免旧日志让本轮
# “假通过”。非空且没有归档标记的目录可能是操作者数据，拒绝清理。
if [ -e "$OUT" ] && [ ! -e "$OUT/.farm-drill" ] && [ -n "$(ls -A "$OUT" 2>/dev/null)" ]; then
  echo "DRILL_OUT 非空且不是 farm-drill 归档，拒绝清理：$OUT" >&2
  exit 1
fi
rm -rf "$OUT/logs" "$FARM" "$SHARED" "$WORK" "$OUT/site-a" "$OUT/site-b" "$OUT/site-c" \
  "$OUT/blocked-cache" "$LOG"
: >"$OUT/.farm-drill"
mkdir -p "$OUT/logs" "$FARM" "$SHARED" "$WORK"
chmod 2770 "$FARM" "$SHARED" # 与 FARM-DEPLOY §3.1 同口径
chmod 700 "$WORK"            # worker 私有父目录（§5.1）
# 整份输出同时落到 transcript（tee 在 EXIT 时自然收尾）。
exec > >(tee "$LOG") 2>&1

PIDS=()
LAST_PID=
stop_all() {
  for pid in "${PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
  done
  for pid in "${PIDS[@]}"; do
    wait "$pid" 2>/dev/null || true
  done
}
trap stop_all EXIT

say() { printf '\n=== %s ===\n' "$*"; }
pass() { printf 'PASS %s\n' "$*"; }
fail() {
  printf 'FAIL %s\n' "$*" >&2
  printf 'archive: %s\n' "$OUT" >&2
  exit 1
}
show() { printf '$'; printf ' %q' "$@"; printf '\n'; }

expect() { # 描述 文件 字面量
  grep -q -- "$3" "$2" || fail "$1：日志缺少「$3」（$2）"
}
expect_count() { # 描述 文件 字面量 次数
  local n
  n=$(grep -o -- "$3" "$2" | wc -l)
  [ "$n" -eq "$4" ] || fail "$1：「$3」出现 $n 次，预期 $4 次（$2）"
}

write_site() {
  local dir=$1
  mkdir -p "$dir/ui/public"
  cat >"$dir/polypack.toml" <<'EOF'
schema = 1

[package]
name = "farm-drill-fixture"
version = "0.1.0"

[ui]
framework = "react"
entry = "ui/index.html"

[core]
crate = "core"
EOF
  cat >"$dir/ui/index.html" <<'EOF'
<!doctype html>
<html>
  <head><title>farm drill</title></head>
  <body></body>
</html>
EOF
  printf '<svg xmlns="http://www.w3.org/2000/svg"/>\n' >"$dir/ui/public/icon.svg"
}

start_worker() { # id 日志 cwd 缓存 [额外参数...]
  local id=$1 log=$2 cwd=$3 cache=$4
  shift 4
  printf '\n$ (cd %s && polypack farm worker --dir %s --id %s --cache %s --work-dir %s' \
    "$cwd" "$FARM" "$id" "$cache" "$WORK"
  printf ' %q' "$@"
  printf ')\n'
  ( cd "$cwd" && exec "$POLYPACK" farm worker --dir "$FARM" --id "$id" \
      --cache "$cache" --work-dir "$WORK" "$@" ) >"$log" 2>&1 &
  LAST_PID=$!
  PIDS+=("$LAST_PID")
}

wait_hello() { # worker id
  local id=$1 n=0
  until find "$FARM/v1/$id/to-master" -maxdepth 1 -type f ! -name '.*' 2>/dev/null | grep -q .; do
    n=$((n + 1))
    [ "$n" -lt 300 ] || fail "worker $id 未在 30s 内注册"
    sleep 0.1
  done
}

wait_worker() { # pid
  wait "$1" || fail "worker pid $1 退出码非 0（见 $OUT/logs）"
}

run_submit() { # 名称 cwd [参数...]
  local name=$1 cwd=$2
  shift 2
  printf '\n$ (cd %s && polypack farm submit' "$cwd"
  printf ' %q' "$@"
  printf ')\n'
  ( cd "$cwd" && "$POLYPACK" farm submit "$@" ) >"$OUT/logs/$name.log" 2>&1 || {
    tail -n 30 "$OUT/logs/$name.log" >&2
    fail "submit $name 退出码非 0"
  }
}

say "farm drill"
printf 'polypack:   %s\n' "$POLYPACK"
"$POLYPACK" --version
printf 'host:   %s\n' "$(uname -s)"
printf 'shared: %s\n' "$SHARED"
printf 'work:   %s\n' "$WORK"

# ---------------------------------------------------------------- 夹具
say "fixture: three independent project copies (absolute paths differ)"
write_site "$OUT/site-a"
write_site "$OUT/site-b"
write_site "$OUT/site-c" # round 2 专用：本地缓存为空，命中只能来自共享 CAS

# ---------------------------------------------------------------- 预检
say "preflight: directory permissions, path isolation, polypack scan"
POLYPACK="$POLYPACK" "$ROOT/tools/farm-preflight.sh" --project "$OUT/site-a" "$FARM" "$SHARED" "$WORK" ||
  fail "preflight 未通过"
pass "preflight: dir permissions, path isolation and polypack scan"

# ---------------------------------------------------------------- Round 1
say "ROUND 1: two workers, two targets, one submit"
start_worker worker-a "$OUT/logs/worker-a.log" "$OUT/site-a" "$SHARED" \
  --target web --target vidaa-tv --jobs 1 --idle 30 --json
PID_A=$LAST_PID
start_worker worker-b "$OUT/logs/worker-b.log" "$OUT/site-b" "$SHARED" \
  --target web --target vidaa-tv --jobs 1 --idle 30 --json
PID_B=$LAST_PID
wait_hello worker-a
wait_hello worker-b
run_submit round-1 "$OUT/site-a" --config polypack.toml --dir "$FARM" \
  --target web --target vidaa-tv --wait 60 --retries 1 --project-dir . \
  --cache "$SHARED" --json
wait_worker "$PID_A"
wait_worker "$PID_B"
expect_count "round 1" "$OUT/logs/round-1.log" '"ok":true' 2
[ "$(grep -o '"worker_id":"[^"]*"' "$OUT/logs/round-1.log" | sort -u | wc -l)" -eq 2 ] ||
  fail "round 1：两个任务未落到两个 worker"
expect "round 1" "$OUT/logs/round-1.log" '"name":"dist/index.html"'
expect "round 1 worker-a" "$OUT/logs/worker-a.log" '完成：'
expect "round 1 worker-b" "$OUT/logs/worker-b.log" '完成：'
pass "round 1: two targets completed on two workers"

# ---------------------------------------------------------------- Round 2
say "ROUND 2: fresh project copy, shared cache must hit"
[ ! -e "$OUT/site-c/.polypack/cache" ] || fail "round 2 前置：site-c 本地缓存应为空"
start_worker worker-c "$OUT/logs/worker-c.log" "$OUT/site-c" "$SHARED" \
  --target web --jobs 1 --idle 30 --json
PID_C=$LAST_PID
wait_hello worker-c
run_submit round-2 "$OUT/site-c" --config polypack.toml --dir "$FARM" \
  --target web --wait 60 --retries 0 --project-dir . --cache "$SHARED" --json
wait_worker "$PID_C"
expect "round 2" "$OUT/logs/worker-c.log" '"hit":true'
expect "round 2" "$OUT/logs/worker-c.log" '缓存命中 1/1'
expect "round 2" "$OUT/logs/round-2.log" '"ok":true'
pass "round 2: shared-cache hit 1/1 on a fresh project copy"

say "shared cache object format"
OBJ=$(find "$SHARED" -mindepth 2 -maxdepth 2 -type f | head -n 1)
[ -n "$OBJ" ] || fail "共享缓存没有对象文件"
[ "$(head -n 1 "$OBJ")" = "POLYPACK-FARM-OBJECT v0" ] ||
  fail "共享对象头行不是 POLYPACK-FARM-OBJECT v0（$OBJ）"
pass "cache format: POLYPACK-FARM-OBJECT v0"

# ---------------------------------------------------------------- Round 3
say "ROUND 3: credential memory injection (env:VAR only; sentinel scan)"
CRED_VAR=POLYPACK_DRILL_CREDENTIAL
SENTINEL="POLYPACK-CRED-SENTINEL-DRILL-$$-$(date +%s)"
export "$CRED_VAR=$SENTINEL"
start_worker worker-cred "$OUT/logs/worker-cred.log" "$OUT/site-a" "$SHARED" \
  --target tizen-tv --jobs 1 --idle 30 --credential "tizen-author=env:$CRED_VAR" --json
PID_CRED=$LAST_PID
wait_hello worker-cred
run_submit round-3 "$OUT/site-a" --config polypack.toml --dir "$FARM" \
  --target tizen-tv --wait 60 --retries 0 --project-dir . --cache "$SHARED" --json
wait_worker "$PID_CRED"
unset "$CRED_VAR"
expect "round 3" "$OUT/logs/worker-cred.log" '内存注入 1 条凭据'
expect "round 3" "$OUT/logs/worker-cred.log" '一次性工作目录'
expect "round 3" "$OUT/logs/round-3.log" '"ok":true'
if grep -R -a -l -e "$SENTINEL" "$OUT" >/dev/null 2>&1; then
  fail "凭据哨兵出现在磁盘 / 日志（明文泄漏）"
fi
pass "round 3: credential injected from env; zero plaintext on disk/logs"

# ---------------------------------------------------------------- Round 4
say "ROUND 4: unavailable shared cache degrades to project-local with a warning"
printf 'not a directory\n' >"$OUT/blocked-cache"
start_worker worker-fallback "$OUT/logs/worker-fallback.log" "$OUT/site-c" \
  "$OUT/blocked-cache/cas" --target web --jobs 1 --idle 30 --json
PID_FB=$LAST_PID
wait_hello worker-fallback
run_submit round-4 "$OUT/site-c" --config polypack.toml --dir "$FARM" \
  --target web --wait 60 --retries 0 --project-dir . --cache "$OUT/blocked-cache/cas" --json
wait_worker "$PID_FB"
expect "round 4" "$OUT/logs/worker-fallback.log" '共享缓存目录不可用'
expect "round 4" "$OUT/logs/worker-fallback.log" '已降级为工程本地缓存'
expect "round 4" "$OUT/logs/round-4.log" '"ok":true'
pass "round 4: unavailable shared cache degraded to local cache with a warning"

# ---------------------------------------------------------------- 隔离残留
say "isolation residue"
if find "$WORK" -mindepth 1 -print -quit | grep -q .; then
  fail "worker 工作根 / 任务目录有残留（$WORK）"
fi
pass "isolation: no task work directories left behind"

# ---------------------------------------------------------------- 环境槽
say "SKIP macOS/IPA (R3): no macOS host on this machine"
printf 'host: %s — Xcode / codesign / notarytool not present; slot stays open.\n' "$(uname -s)"
cat <<'EOF'

Run on the macOS build site (copy-paste):
  xcode-select --install
  rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
  cd /srv/polypack-src && cargo build --release -p polypack-cli --locked
  sudo install -m 755 target/release/polypack /usr/local/bin/polypack
  xcrun notarytool store-credentials apple-codesign --apple-id "$APPLE_ID" --team-id "$TEAM_ID"
  export POLYPACK_IOS_IDENTITY='Apple Distribution: Example Ltd (TEAMID1234)'
  export POLYPACK_IOS_TEAM_ID='TEAMID1234'
  polypack farm worker --dir /srv/polypack-farm --id mac-01 --target ios --jobs 0 --idle 120 \
    --cache /srv/polypack-cache --work-dir /var/tmp/polypack-work

Run on the Linux origin (copy-paste):
  polypack farm submit --dir /srv/polypack-farm --config /srv/proj/polypack.toml \
    --target ios --wait 1800 --retries 1 --project-dir /srv/proj \
    --cache /srv/polypack-cache

Expected: worker 已注册：mac-01（aarch64-apple-darwin，宿主 Macos）
         农场任务完成：1 个任务 / 1 个 worker
         IPA at /srv/proj/.polypack/packages/ios/ (build site, same-path mount)
Then verify on macOS: codesign --verify --deep --strict "Payload/Pax Notes.app"
                     spctl --assess --type execute --verbose=4 "Payload/Pax Notes.app"
EOF
pass "SKIP printed (environment slot); no macOS claims made"

say "SUMMARY"
printf 'archive: %s\n' "$OUT"
