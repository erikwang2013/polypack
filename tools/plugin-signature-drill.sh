#!/usr/bin/env bash
# GA §6.6 插件签名 / 信任根演练：真跑 poly-plugin 签名链集成测试（8 用例）。
# 覆盖有效签名 / 篡改字节 / 缺签名 / 错公钥 / SRI 先行 / 信任根不可用 / 无泄漏 / 配置引用。
#
# 用法：tools/plugin-signature-drill.sh
# 输出：PASS 8/8；宿主无 gpg 或 Windows MSYS 时输出 SKIP（测试本身也会 SKIP）。
# 日志：$DRILL_OUT/signature.log（缺省 target/signature-drill/）。
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
OUT=${DRILL_OUT:-"$ROOT/target/signature-drill"}
LOG="$OUT/signature.log"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target"}
mkdir -p "$OUT"

pass() { printf 'PASS %s\n' "$*"; }
fail() {
  printf 'FAIL %s\n' "$*" >&2
  printf 'log: %s\n' "$LOG" >&2
  exit 1
}

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) printf 'SKIP：Windows MSYS gpg 路径不兼容（测试自行 SKIP）\n'; exit 0 ;;
esac
command -v gpg >/dev/null 2>&1 ||
  { printf 'SKIP：宿主没有 gpg，签名真跑不可用（测试自行 SKIP）\n'; exit 0; }

cd "$ROOT"
printf '$ CARGO_TARGET_DIR=%s cargo test -p poly-plugin --test signature\n' "$CARGO_TARGET_DIR"
cargo test -p poly-plugin --test signature >"$LOG" 2>&1 || {
  tail -n 40 "$LOG" >&2
  fail "cargo test 退出码非 0"
}

grep -q 'test result: ok. 8 passed; 0 failed' "$LOG" || fail "结果不是 8 passed / 0 failed"
for name in \
  valid_signature_with_matching_sri_loads_and_runs \
  tampered_bytes_with_updated_sri_fail_signature \
  missing_signature_is_rejected \
  wrong_public_key_is_rejected \
  sri_mismatch_is_rejected_even_with_valid_signature \
  unavailable_trust_root_is_reported_without_host_paths \
  errors_and_artifacts_leak_no_host_paths_or_private_material \
  config_references_trust_root_without_inline_secrets; do
  grep -q "^test $name ... ok$" "$LOG" || fail "缺少通过用例：$name"
done
pass "signature/trust-root chain 8/8（有效 / 篡改 / 缺签名 / 错公钥 / SRI 先行 / 信任根不可用 / 无泄漏 / 配置引用）"
