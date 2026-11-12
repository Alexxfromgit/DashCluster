#!/usr/bin/env bash
# Builds dist/DashCluster.app and dist/DashCluster-<version>-macos.dmg.
# The app is ad-hoc signed (fine on your own Macs; other people's Macs will need
# right-click → Open the first time, or a Developer ID signature).
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

./scripts/build-web.sh
cargo build --release -p cluster-desktop

app=dist/DashCluster.app
rm -rf "$app" && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/dashcluster "$app/Contents/MacOS/dashcluster"
cp LICENSE THIRD-PARTY-NOTICES.md "$app/Contents/Resources/"
cargo run -q --release -p license-notices -- --out "$app/Contents/Resources/THIRD-PARTY-LICENSES.txt"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>DashCluster</string>
  <key>CFBundleDisplayName</key><string>DashCluster</string>
  <key>CFBundleIdentifier</key><string>dev.dashcluster.app</string>
  <key>CFBundleExecutable</key><string>dashcluster</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version}</string>
  <key>CFBundleVersion</key><string>${version}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <!-- macOS asks before an app talks to the local network (Forza packets, phones). -->
  <key>NSLocalNetworkUsageDescription</key>
  <string>DashCluster receives telemetry from your game and shares it with your phone on this network.</string>
  <key>NSBonjourServices</key><array><string>_dashcluster._tcp</string></array>
</dict></plist>
PLIST
codesign --force --deep --sign - "$app"

dmg="dist/DashCluster-${version}-macos.dmg"
rm -f "$dmg"
hdiutil create -volname DashCluster -srcfolder "$app" -ov -format UDZO "$dmg" >/dev/null
echo "Built $app and $dmg"
