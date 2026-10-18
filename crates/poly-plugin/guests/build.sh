#!/usr/bin/env bash
# 重新生成 crates/poly-plugin/tests/fixtures/*.wasm。
#
# 前置：`rustup target add wasm32-unknown-unknown`（无需全局 wasm-tools）。
# 制品提交入库，日常测试不执行本脚本。
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
target=wasm32-unknown-unknown

# §7：制品不得含宿主绝对路径。panic location 会带源文件路径（strip 只去调试符号），
# 把 cargo registry / 本仓库前缀映射为空 → 变成相对路径（dlmalloc 是已知携带者）。
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}= --remap-path-prefix=$repo="

build() {
  local crate=$1 stem=$2 fixture=$3
  cargo build --release --target "$target" -j 1 \
    --manifest-path "$here/$crate/Cargo.toml"
  cargo run --release -j 1 --manifest-path "$here/componentize/Cargo.toml" -- \
    "$here/$crate/target/$target/release/$stem.wasm" \
    "$repo/crates/poly-plugin/tests/fixtures/$fixture"
}

build reference polypack_reference_plugin reference-plugin.wasm
build capability-probe polypack_capability_probe capability-probe.wasm
build outdated-version polypack_outdated_version outdated-version.wasm
