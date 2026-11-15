# DashCluster

[![CI](https://github.com/Alexxfromgit/DashCluster/actions/workflows/ci.yml/badge.svg)](https://github.com/Alexxfromgit/DashCluster/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Alexxfromgit/DashCluster?sort=semver)](https://github.com/Alexxfromgit/DashCluster/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Made with Slint](https://img.shields.io/badge/Made%20with-Slint-2379F4)](https://slint.dev)

A customizable digital instrument cluster for racing games. Drive in **Forza Horizon 5** and see
your speed, revs, gear, shift lights, tyres and lap times on a second screen, on your phone, and on
other computers on your network, all at once. Design your own dashboards in the built-in editor.

Written in Rust with [Slint](https://slint.dev). The same dashboards render natively on Windows,
macOS and Linux and, compiled to WebAssembly, in any phone or tablet browser.

## Built-in dashboard gallery

Screenshots use the demo car. Landscape dashboards are designed for 16:9 screens and Phone Portrait
for tall phones. At other aspect ratios, the instruments scale together and the theme fills the space around them.

### Grand Tourer

![Grand Tourer with blue needle gauges and a full-width telemetry strip](docs/images/grand-tourer.png)

| Race | Rally |
|---|---|
| ![Race with live tyre temperatures and pedal inputs](docs/images/race.png) | ![Rally with speed inside a cyan tachometer](docs/images/rally.png) |
| **Endurance** | **Club Sport** |
| ![Endurance with lap timing and a central gear indicator](docs/images/endurance.png) | ![Club Sport with a yellow tachometer on graphite](docs/images/club-sport.png) |
| **Sprint** | **Apex** |
| ![Sprint with a white rising RPM strip](docs/images/sprint.png) | ![Apex with bright yellow dual arcs on black](docs/images/apex.png) |

### Heritage

![Heritage with white dials and red needles](docs/images/heritage.png)

### Phone Portrait

<img src="docs/images/phone.png" width="300" alt="Phone Portrait with a large gear and speed display above lap times and pedal inputs">

### Visual editor

![Race dashboard open in the visual editor](docs/images/editor.png)

## What is in it

| Area | Details |
|---|---|
| Games | Forza Horizon 4/5 and Forza Motorsport (7 and 2023). Preview sources for DiRT 4, DiRT Rally, DiRT Rally 2.0, GRID 2, GRID Autosport, GRID (2019), GRID Legends, F1 2019–25, Assetto Corsa Competizione and Le Mans Ultimate |
| Dashboards | Nine built-in layouts and 13 widget types, including analog dials, segmented RPM bars, gear indicators, lap timers and layered panels |
| Editor | Visual editor (move, resize, snap, align, undo/redo, property inspector) or plain JSON5 files that reload when saved; `.cluster` packages to share dashboards with their images |
| Screens | Desktop window (fullscreen, always on top), phone/tablet browser via QR code (no app to install), other computers mirror the first one automatically |
| Units | Metric or imperial per quantity (speed, temperature, pressure, power, torque), also on phones; gauge scales re-round themselves |
| Tools | Demo car, session recording and replay (`.ctlog`), a fake Forza sender for testing without the game, headless PNG rendering |

## Download

Get the latest version from [Releases](https://github.com/Alexxfromgit/DashCluster/releases/latest):

| Platform | File |
|---|---|
| Windows 10/11 (x64) | `DashCluster-<version>-windows.zip` → unzip, run `DashCluster.exe` |
| macOS 11+ (Apple silicon) | `DashCluster-<version>-macos.dmg` |
| Linux (x64, glibc 2.35+: Ubuntu 22.04, Debian 12 or newer) | `DashCluster-<version>-linux-x64.tar.gz` |

`SHA256SUMS.txt` on each release lists the checksums, and every file's build provenance can be
checked with `gh attestation verify <file> -R Alexxfromgit/DashCluster`. The builds are not
code-signed yet: Windows SmartScreen asks you to confirm ("More info" → "Run anyway"), and macOS
needs right-click → Open the first time.

## Connect Forza Horizon 5

The first launch shows these steps with your computer's address filled in (**H** shows them again).

1. **In the game:** Settings → HUD and Gameplay → **Data Out: On**.
2. **Data Out IP Address:** the computer running DashCluster. **Port:** `5300`.
3. **Game and DashCluster on the same Windows PC** (Microsoft Store / Xbox app version of FH5):
   Windows blocks apps from sending to their own PC. Allow it once in an administrator PowerShell,
   then use `127.0.0.1` as the address:
   ```powershell
   Get-AppxPackage *Forza* | % { CheckNetIsolation LoopbackExempt -a -n="$($_.PackageFamilyName)" }
   ```
   Not needed when DashCluster runs on another computer (e.g. a laptop next to your PC or console).
4. Allow DashCluster on **private networks** when Windows Firewall or macOS asks.
5. **Phone:** press **Q** and scan the QR code (same Wi-Fi). Tap the left/right edge to change
   dashboards, the middle for fullscreen. On iPhone, Share → *Add to Home Screen* gives a
   full-screen app. Phones only let secure (HTTPS) pages keep the screen awake, so turn off
   auto-lock while you drive (iPhone: Display & Brightness → Auto-Lock; Android: Display →
   Screen timeout). Image widgets and background images show on the computer only, not yet on
   phones.
6. **Another computer:** run DashCluster there and choose Settings (**P**) → Source →
   *Another DashCluster*; it finds the first one by itself.

Forza does not send coolant/oil temperature, ABS/TC or indicator data, so widgets bound to those
stay dimmed with Forza. The sample dashboards use what Forza provides.

## Connect DiRT or GRID on PC

1. Select **DiRT / GRID (UDP)** in DashCluster Settings (**P**) and apply. The default port is `20777`.
2. Close the game and edit its `hardware_settings_config.xml` in `Documents/My Games/<game>/hardwaresettings/`.
   The game creates the file on its first launch. Edit the existing telemetry entry for your title.

   | Game | XML entry | `extradata` |
   |---|---|---|
   | DiRT 4, DiRT Rally, DiRT Rally 2.0, GRID (2019), GRID Legends | `<udp ... />` under `<motion_platform>` | `3` |
   | GRID Autosport | `<motion ... />` | `3` |
   | GRID 2 | `<motion ... />` | `1` |

   Set `enabled="true"`, `port="20777"`, `delay="1"`, and `ip="127.0.0.1"` for the same PC.
   For a separate DashCluster computer, use its LAN address. If you chose a different port, set it in both places.
3. Restart the game and enter a race or stage. Speed, RPM, gear and pedals use the common packet layout.
   Lap, fuel and RPM limit availability depends on the game and packet size.

From a terminal, use `cargo run -p cluster-desktop -- --source dirt-grid`. The guide uses the
[DiRT](https://horizonlabshud.com/games/dirt/) and [GRID](https://horizonlabshud.com/games/grid/)
telemetry settings documented by Horizon Labs.

## Connect F1 2019–25

1. Select **F1 2019–25 (UDP)** in DashCluster Settings (**P**) and apply. The default port is `20777`.
2. In F1's telemetry settings, enable **UDP Telemetry**. Set the port to match DashCluster and set
   the IP address to `127.0.0.1` for the same PC or the DashCluster computer's LAN address.
3. Choose a UDP format from **2019** through **2025**. If F1 25 defaults to the 2026 Season Pack format,
   change it to **2025**. Start driving to populate the gauges.

The terminal equivalent is `cargo run -p cluster-desktop -- --source f1`. See
[EA's F1 25 UDP specifications](https://forums.ea.com/blog/f1-games-game-info-hub-en/ea-sports%E2%84%A2-f1%C2%AE25-2026-season-pack-udp-specification/12187347).

## Connect Assetto Corsa Competizione or Le Mans Ultimate

Run DashCluster on the Windows PC running the game. In Settings (**P**), choose **Assetto Corsa
Competizione (Windows)** or **Le Mans Ultimate (Windows)**, apply, and start a driving session.
LMU uses the native `LMU_Data` interface shipped with current builds. A phone can connect to
DashCluster's hub while the desktop reads the game locally.

The terminal sources are `--source acc` and `--source lmu`. See the
[ACC shared-memory reference](https://www.assettocorsamods.net/threads/doc-shared-memory-reference-acc.3061/)
and [LMU shared-memory release notes](https://guide.lemansultimate.com/hc/en-gb/articles/14556121957775-V1-2-Update-2).

## Keys

| Key | |
|---|---|
| ← → | Previous / next dashboard |
| E | Edit the current dashboard |
| P · H · Q | Settings · setup guide · phone QR code |
| D | Switch to the demo car and back |
| U · F · S | Metric/imperial · fullscreen · status bar |

In the editor: click / Shift-click to select, drag to move, drag the corner square to resize,
arrow keys to nudge (Shift: 10), Delete, Ctrl/⌘+D duplicate, Ctrl/⌘+Z undo (+Shift redo),
Ctrl/⌘+S save, G snap, Esc deselect.

## Your dashboards

Use **← / →** to cycle through the built-in dashboards, or select one at startup with `--dashboard sprint`.
Press **D** to see it driven by the demo car and **E** to customize it in the visual editor.

Your layouts are saved in `%APPDATA%\DashCluster\dashboards` (Windows), `~/Library/Application Support/DashCluster/dashboards`
(macOS) or `~/.local/share/DashCluster/dashboards` (Linux), next to `settings.json`. Edit them in
the app (**E**) or in any text editor; the app reloads a dashboard when its file is saved. The format
is documented in [docs/dashboard-format.md](docs/dashboard-format.md).

## Build and run

You need [Rust](https://rustup.rs) 1.92 or newer (stable). Everything else is fetched by Cargo.

- **Windows:** install Rust with `rustup-init.exe`; when it asks, let it install the Visual Studio
  C++ Build Tools (the "Desktop development with C++" workload).
- **macOS:** `xcode-select --install`, then rustup.
- **Linux:** `sudo apt-get install -y pkg-config libfontconfig1-dev libxkbcommon-dev` (or your
  distribution's equivalents), then rustup.

```bash
git clone https://github.com/Alexxfromgit/DashCluster.git
cd DashCluster
cargo run --release -p cluster-desktop                    # listens for Forza on UDP 5300
cargo run --release -p cluster-desktop -- --source demo   # or drive the demo car
```

The phone display is a separate WebAssembly build that the desktop app embeds when it is compiled.
Build it once before the desktop app (it installs the `wasm32` target and the matching
`wasm-bindgen` CLI on first use); without it, phones see a short "not built" page:

```bash
./scripts/build-web.sh            # Windows: .\scripts\build-web.ps1
cargo run --release -p cluster-desktop
```

More: [CONTRIBUTING.md](CONTRIBUTING.md) (tests, tools, packaging, releases),
[docs/architecture.md](docs/architecture.md) (how it fits together),
[docs/testing-with-forza.md](docs/testing-with-forza.md) (a checklist for trying every workflow
with the real game).

## License

[MIT](LICENSE). DashCluster uses [Slint](https://slint.dev) under the Slint Royalty-free License
and other open-source libraries; see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md). Every
download includes the full license texts (`THIRD-PARTY-LICENSES.txt`). The dashboard
designs are original. Forza, Forza Horizon and Forza Motorsport are trademarks of Microsoft; this
project is not affiliated with or endorsed by Microsoft or Playground Games.
