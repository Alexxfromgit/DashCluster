#!/usr/bin/env bash
# Builds the phone/browser display (WASM) into apps/cluster-web/dist. The desktop app embeds that
# folder when it is compiled, so build this first, then the desktop app.
# Installs what it needs on first use: the wasm32 target and the wasm-bindgen CLI matching
# Cargo.lock.
#
#   ./scripts/build-web.sh            # optimized (profile wasm-release)
#   ./scripts/build-web.sh dev        # faster, unoptimized
set -euo pipefail
cd "$(dirname "$0")/.."

profile="${1:-wasm-release}"
want=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"/\1/p')
if command -v rustup >/dev/null; then
  rustup target add wasm32-unknown-unknown >/dev/null
fi
have=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [ "$have" != "$want" ]; then
  echo "Installing wasm-bindgen-cli $want (one time, takes a few minutes)…"
  cargo install wasm-bindgen-cli --version "$want" --locked
fi

if [ "$profile" = dev ]; then
  cargo build -p cluster-web --target wasm32-unknown-unknown
  wasm=target/wasm32-unknown-unknown/debug/cluster_web.wasm
else
  cargo build -p cluster-web --target wasm32-unknown-unknown --profile "$profile"
  wasm="target/wasm32-unknown-unknown/$profile/cluster_web.wasm"
fi
out=apps/cluster-web/dist
rm -rf "$out" && mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" --out-name cluster_web "$wasm"
if command -v wasm-opt >/dev/null; then
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int -o "$out/cluster_web_bg.wasm" "$out/cluster_web_bg.wasm"
fi
cp apps/cluster-web/static/* "$out/"
ls -lh "$out"
