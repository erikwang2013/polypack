#!/usr/bin/env bash
# T-M8-02：比对 `world polypack-target` 与 ROADMAP §4.5 代码块，逐行一致。
#
# 归一化仅吸收 WIT 语法强制项：
#   - 行尾分号（§4.5 片段省略；WIT 必需）
#   - `%package` 关键字转义（§4.5 直接写 package；WIT 中 package 是关键字）
#   - world 末尾的 `use types.{...}`（WIT 不允许 world 直接声明类型，必须 use）
set -euo pipefail

root=$(cd "$(dirname "$0")/../../.." && pwd)
wit="$root/crates/polypack-plugin/wit/polypack-target.wit"
roadmap="$root/docs/ROADMAP.md"

normalize() {
  awk '{
    sub(/[[:space:]]+$/, "")
    sub(/;$/, "")
    sub(/export %package:/, "export package:")
    if ($0 !~ /^[[:space:]]*use types\./) print
  }'
}

extract_roadmap() {
  awk '
    /^### 4\.5 / { section = 1; next }
    section && /^```wit$/ { code = 1; next }
    code && /^```$/ { exit }
    code { print }
  ' "$roadmap"
}

extract_wit() {
  awk '
    /^world polypack-target \{/ { world = 1 }
    world { print }
    world && /^\}$/ { exit }
  ' "$wit"
}

# PATH 可能被交叉编译 SDK 的工具链覆盖（其伪 diff 忽略参数且恒返回 0），优先系统 diff。
diff_cmd=/usr/bin/diff
[[ -x $diff_cmd ]] || diff_cmd=$(command -v diff)

"$diff_cmd" -u \
  <(extract_roadmap | normalize) \
  <(extract_wit | normalize)

echo "OK: world polypack-target 与 ROADMAP §4.5 逐行一致（已归一化 ; / %package / use）"
