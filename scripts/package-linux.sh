#!/usr/bin/env bash
# Builds dist/DashCluster-<version>-linux-x64.tar.gz (phone display included).
# Needs: pkg-config, libfontconfig1-dev, libxkbcommon-dev.
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

./scripts/build-web.sh
cargo build --release -p cluster-desktop

stage=dist/DashCluster
rm -rf "$stage" && mkdir -p "$stage"
cp target/release/dashcluster "$stage/dashcluster"
cp README.md LICENSE THIRD-PARTY-NOTICES.md "$stage/"
tar -czf "dist/DashCluster-${version}-linux-x64.tar.gz" -C dist DashCluster
echo "Built dist/DashCluster-${version}-linux-x64.tar.gz"
