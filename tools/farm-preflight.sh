#!/usr/bin/env bash
# T-M9-44 农场生产预检：在真实部署目录上检查权限 / 路径隔离 / 凭据变量 / poly scan。
# 只读：临时文件写入目标目录后立即删除；不访问网络；不打印任何凭据值。
#
# 用法（先构建同一提交的 CLI）：
#   cargo build -p poly-cli
#   POLY=target/debug/poly tools/farm-preflight.sh [--project DIR] \
#     [--credential NAME=env:VAR]... <farm-dir> <cache|local|s3://bucket/prefix> <work-dir>
#
# 退出码：0 = 无 FAIL；1 = 有 FAIL；2 = 用法错误。WARN 不影响退出码。
set -uo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
POLY=${POLY:-poly}
if ! command -v "$POLY" >/dev/null 2>&1; then
  if [ -x "$ROOT/target/debug/poly" ]; then
    POLY="$ROOT/target/debug/poly"
  elif [ -x "$ROOT/target/release/poly" ]; then
    POLY="$ROOT/target/release/poly"
  else
    echo "找不到 poly：先 cargo build -p poly-cli，或设 POLY=<二进制路径>" >&2
    exit 2
  fi
fi
POLY=$(command -v "$POLY")
case "$POLY" in /*) ;; *) POLY="$(cd "$(dirname "$POLY")" && pwd)/$(basename "$POLY")" ;; esac

usage() {
  cat >&2 <<'EOF'
用法: tools/farm-preflight.sh [--project DIR] [--credential NAME=env:VAR]... \
        <farm-dir> <cache-path|local|s3://bucket[/prefix]> <work-dir>
EOF
}

PROJECT=
CREDS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --project)
      [ $# -ge 2 ] || { usage; exit 2; }
      PROJECT=$2
      shift 2
      ;;
    --credential)
      [ $# -ge 2 ] || { usage; exit 2; }
      CREDS+=("$2")
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    -*)
      echo "未知参数：$1" >&2
      usage
      exit 2
      ;;
    *)
      break
      ;;
  esac
done
[ $# -eq 3 ] || { usage; exit 2; }
FARM=$1
CACHE=$2
WORK=$3

PASSED=0
FAILED=0
WARNED=0
pass() { printf 'PASS %s\n' "$*"; PASSED=$((PASSED + 1)); }
warn() { printf 'WARN %s\n' "$*"; WARNED=$((WARNED + 1)); }
fail() { printf 'FAIL %s\n' "$*" >&2; FAILED=$((FAILED + 1)); }

mode_of() {
  if stat -c %a "$1" >/dev/null 2>&1; then
    stat -c %a "$1"
  elif stat -f %Lp "$1" >/dev/null 2>&1; then
    stat -f %Lp "$1"
  else
    printf ''
  fi
}
canon() { (cd "$1" 2>/dev/null && pwd -P) || printf '%s\n' "$1"; }
fs_type() {
  if command -v findmnt >/dev/null 2>&1; then
    findmnt -no FSTYPE --target "$1" 2>/dev/null && return 0
  fi
  if stat -f -c %T "$1" >/dev/null 2>&1; then
    stat -f -c %T "$1"
    return 0
  fi
  printf ''
}
world_writable() {
  case "$1" in ''|*[!0-7]*) return 1 ;; esac
  (( (8#$1 & 8#002) != 0 ))
}
has_other_bits() {
  case "$1" in ''|*[!0-7]*) return 1 ;; esac
  (( (8#$1 & 8#007) != 0 ))
}

# ---------------------------------------------------------------- poly
if "$POLY" --version >/dev/null 2>&1; then
  pass "poly：$("$POLY" --version 2>/dev/null | head -n 1)"
else
  fail "poly 不可执行：$POLY"
fi

# ---------------------------------------------------------------- 目录
check_dir() { # label path
  if [ ! -e "$2" ]; then
    fail "$1 不存在：$2（先按 FARM-DEPLOY §3.1 / §3.2 创建）"
    return 1
  fi
  if [ ! -d "$2" ]; then
    fail "$1 不是目录：$2"
    return 1
  fi
  local mode probe
  mode=$(mode_of "$2")
  pass "$1 存在：$2（mode ${mode:-未知}）"
  if probe=$(mktemp "$2/.farm-preflight-XXXXXX" 2>/dev/null); then
    rm -f "$probe"
    pass "$1 可写"
  else
    fail "$1 不可写：$2"
  fi
  if world_writable "$mode"; then
    fail "$1 是 world-writable（mode $mode）：收紧到 2770 / 0700"
  else
    pass "$1 非 world-writable"
  fi
  if has_other_bits "$mode"; then
    warn "$1 对其他用户可见（mode $mode）：建议 2770（农场 / 缓存）或 0700（工作目录）"
  fi
}

check_dir "农场目录" "$FARM"
check_dir "工作目录" "$WORK"

CACHE_IS_PATH=0
case "$CACHE" in
  local)
    pass "缓存：local（工程本地 CAS；无共享命名空间可查）"
    ;;
  s3://*)
    if [ -n "${POLYPACK_S3_ACCESS_KEY_ID:-${AWS_ACCESS_KEY_ID:-}}" ] &&
      [ -n "${POLYPACK_S3_SECRET_ACCESS_KEY:-${AWS_SECRET_ACCESS_KEY:-}}" ]; then
      pass "S3 凭据环境变量已设置（值不打印）"
    else
      fail "S3 缓存需要 POLYPACK_S3_ACCESS_KEY_ID / POLYPACK_S3_SECRET_ACCESS_KEY（或 AWS_*）"
    fi
    case "$CACHE" in
      s3://?*) pass "S3 URI：$CACHE（前缀隔离须人工确认）" ;;
      *) fail "S3 URI 缺少 bucket：$CACHE" ;;
    esac
    warn "S3 连通性未探测：worker 启动探针失败会降级到本地缓存（FARM-DEPLOY §6.3）"
    ;;
  *)
    CACHE_IS_PATH=1
    check_dir "缓存目录" "$CACHE"
    ;;
esac

# ---------------------------------------------------------------- 路径隔离
FARM_C=$(canon "$FARM")
WORK_C=$(canon "$WORK")
if [ "$FARM_C" = "$WORK_C" ]; then
  fail "农场目录与工作目录必须是不同路径：$FARM_C"
else
  pass "路径隔离：农场目录 ≠ 工作目录"
fi
if [ "$CACHE_IS_PATH" -eq 1 ]; then
  CACHE_C=$(canon "$CACHE")
  if [ "$FARM_C" = "$CACHE_C" ]; then
    fail "农场目录与缓存目录必须是不同路径：$FARM_C"
  elif [ "$CACHE_C" = "$WORK_C" ]; then
    fail "缓存目录与工作目录必须是不同路径：$CACHE_C"
  else
    pass "路径隔离：缓存目录 ≠ 农场目录 / 工作目录"
  fi
fi

# ------------------------------------------- 文件系统：协议无 TLS；工作目录必须本机
FT=$(fs_type "$FARM")
case "$FT" in
  '') warn "无法识别农场目录文件系统：人工确认跨机挂载已受 VPN / SSH / 认证保护" ;;
  nfs*|cifs|smb*|fuse*) warn "农场目录位于网络文件系统（$FT）：协议无 TLS / 认证，请用 VPN / SSH 隧道或受认证挂载（§11 第 1 项）" ;;
  *) pass "农场目录为本地文件系统（$FT）：无网络监听面" ;;
esac
FT=$(fs_type "$WORK")
case "$FT" in
  '') warn "无法识别工作目录文件系统：人工确认 --work-dir 在本机磁盘而非共享挂载" ;;
  nfs*|cifs|smb*|fuse*) fail "工作目录必须是 worker 本机磁盘（当前 $FT）：共享挂载会破坏一次性隔离" ;;
  *) pass "工作目录为本地文件系统（$FT）" ;;
esac

# ------------------------------------------- 凭据注入变量（只报变量名，不打印值）
for spec in ${CREDS[@]+"${CREDS[@]}"}; do
  case "$spec" in
    *=env:?*)
      name=${spec%%=*}
      variable=${spec#*=env:}
      if [ -z "$name" ] || [[ ! $variable =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]]; then
        fail "--credential 参数非法：档案名或变量名不合法（不回显参数）"
      elif [ -n "${!variable:-}" ]; then
        pass "凭据档案 $name：注入变量 $variable 已设置（值不打印）"
      else
        fail "凭据档案 $name：注入变量 $variable 未设置或为空"
      fi
      ;;
    *) fail "--credential 参数非法：只接受 NAME=env:VAR（不回显参数）" ;;
  esac
done

# ---------------------------------------------------------------- poly scan 哨兵
if [ -n "$PROJECT" ]; then
  if [ ! -d "$PROJECT" ]; then
    fail "工程目录不存在：$PROJECT"
  else
    SCAN_LOG=$(mktemp "${TMPDIR:-/tmp}/farm-preflight-scan-XXXXXX")
    if "$POLY" scan --dir "$PROJECT" >"$SCAN_LOG" 2>&1; then
      pass "poly scan：未发现明文凭据（$PROJECT）"
    else
      fail "poly scan 命中或失败（$PROJECT）；复跑：$POLY scan --dir $PROJECT"
      tail -n 20 "$SCAN_LOG" >&2
    fi
    rm -f "$SCAN_LOG"
  fi
else
  warn "未提供 --project：跳过 poly scan 哨兵（建议对构建地工程运行）"
fi

printf '\npreflight: %d passed, %d failed, %d warnings\n' "$PASSED" "$FAILED" "$WARNED"
[ "$FAILED" -eq 0 ]
