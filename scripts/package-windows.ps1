# Builds dist/DashCluster-<version>-windows.zip containing DashCluster.exe.
# Run on Windows from a PowerShell prompt:  .\scripts\package-windows.ps1
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches.Groups[1].Value

& .\scripts\build-web.ps1
cargo build --release -p cluster-desktop

$stage = "dist/DashCluster"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target/release/dashcluster.exe "$stage/DashCluster.exe"
@"
DashCluster $version

Start DashCluster.exe. The first launch shows how to point Forza's Data Out at it.
Allow it through Windows Firewall on private networks when asked.
Press H for help, P for settings, Q to connect a phone, E to edit dashboards.
"@ | Set-Content "$stage/README.txt"

$zip = "dist/DashCluster-$version-windows.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path "$stage/*" -DestinationPath $zip
Write-Host "Built $zip"
