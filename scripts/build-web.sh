#!/usr/bin/env bash
# Builds the web display (WASM) into apps/cluster-web/dist. The desktop app embeds
# that folder at compile time, so rebuild it afterwards:  cargo build -p cluster-desktop
#
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <same as wasm-bindgen in Cargo.lock>
set -euo pipefail
cd "$(dirname "$0")/.."
profile="${1:-wasm-release}"
flag=$([ "$profile" = dev ] && echo "" || echo "--profile $profile")
cargo build -p cluster-web --target wasm32-unknown-unknown $flag
out=apps/cluster-web/dist
rm -rf "$out" && mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" --out-name cluster_web \
  "target/wasm32-unknown-unknown/$([ "$profile" = dev ] && echo debug || echo "$profile")/cluster_web.wasm"
if command -v wasm-opt >/dev/null; then
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int -o "$out/cluster_web_bg.wasm" "$out/cluster_web_bg.wasm"
fi
cp apps/cluster-web/static/* "$out/"
ls -lh "$out"
