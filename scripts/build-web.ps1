# Windows version of build-web.sh: builds the web display into apps/cluster-web/dist.
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <same as wasm-bindgen in Cargo.lock>
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
cargo build -p cluster-web --target wasm32-unknown-unknown --profile wasm-release
$out = "apps/cluster-web/dist"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force $out | Out-Null
wasm-bindgen --target web --no-typescript --out-dir $out --out-name cluster_web `
  target/wasm32-unknown-unknown/wasm-release/cluster_web.wasm
Copy-Item apps/cluster-web/static/* $out
Get-ChildItem $out
