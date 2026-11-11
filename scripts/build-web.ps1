# Windows version of build-web.sh: builds the phone/browser display (WASM) into
# apps/cluster-web/dist. Build this first, then the desktop app (which embeds it).
# Installs what it needs on first use: the wasm32 target and the wasm-bindgen CLI matching
# Cargo.lock.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

$lock = Get-Content Cargo.lock -Raw
$want = [regex]::Match($lock, 'name = "wasm-bindgen"\r?\nversion = "([^"]+)"').Groups[1].Value
rustup target add wasm32-unknown-unknown | Out-Null
$have = ""
if (Get-Command wasm-bindgen -ErrorAction SilentlyContinue) { $have = ((wasm-bindgen --version) -split ' ')[1] }
if ($have -ne $want) {
  Write-Host "Installing wasm-bindgen-cli $want (one time, takes a few minutes)..."
  cargo install wasm-bindgen-cli --version $want --locked
  if ($LASTEXITCODE) { throw "cargo install wasm-bindgen-cli failed" }
}

cargo build -p cluster-web --target wasm32-unknown-unknown --profile wasm-release
if ($LASTEXITCODE) { throw "web build failed" }
$out = "apps/cluster-web/dist"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force $out | Out-Null
wasm-bindgen --target web --no-typescript --out-dir $out --out-name cluster_web `
  target/wasm32-unknown-unknown/wasm-release/cluster_web.wasm
if ($LASTEXITCODE) { throw "wasm-bindgen failed" }
Copy-Item apps/cluster-web/static/* $out
Get-ChildItem $out
