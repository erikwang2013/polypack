#!/usr/bin/env bash
# 构建 PolyPack 插件：core wasm（wasm32-unknown-unknown）→ 组件模型（target-spec/v1）。
#
# 前置：rustup target add wasm32-unknown-unknown
# 产物：dist/plugin.wasm；SRI 摘要用 `poly plugin digest dist/plugin.wasm`。
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
target=wasm32-unknown-unknown

# 制品不得含宿主绝对路径：panic location 会带源文件路径（strip 只去调试符号），
# 把 cargo registry / 本仓库前缀映射为空 → 变成相对路径。
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}= --remap-path-prefix=$here="

cargo build --release --target "$target" --manifest-path "$here/plugin/Cargo.toml"
mkdir -p "$here/dist"
cargo run --release --manifest-path "$here/componentize/Cargo.toml" -- \
  "$here/plugin/target/$target/release/plugin.wasm" \
  "$here/dist/plugin.wasm"
echo "plugin: $here/dist/plugin.wasm"
