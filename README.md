# DashCluster

A customizable digital instrument cluster for racing games, written in Rust with
[Slint](https://slint.dev). Plays nicely with **Forza Horizon 5** today; built so other
sims and real cars (OBD-II) can plug in later.

- Live dashboards on your computer, your **phone's browser**, and other computers, all at once
- 4 original sample dashboards, 12 widget types, a **visual editor**, and hot-reloaded files
- Metric or imperial (per unit), record & replay sessions, demo car for testing without a game

## Quick start: Forza Horizon 5 on a Windows laptop

1. Run DashCluster on the computer you want the dashboard on (the laptop itself, or a Mac
   on the same Wi-Fi). The first launch shows a setup guide (**H** shows it again).
2. In FH5: **Settings → HUD and Gameplay → Data Out: On**, IP address = the address the
   guide shows, port **5300**.
   - Same PC as the game, Microsoft Store/Xbox version: allow loopback once (admin PowerShell):
     `Get-AppxPackage *Forza* | % { CheckNetIsolation LoopbackExempt -a -n="$($_.PackageFamilyName)" }`
     and use `127.0.0.1`.
3. Phone: press **Q**, scan the QR code (same Wi-Fi). Tap the left/right edge to switch
   dashboards, the middle for fullscreen. iPhone: Share → Add to Home Screen.
4. Second computer: run DashCluster there, **P** → Source → *Another DashCluster*.

Allow the app on private networks when Windows Firewall / macOS asks.

## Keys

| Key | |
|---|---|
| ← → | Previous / next dashboard |
| E | Edit the current dashboard (visual editor) |
| P | Settings · H Setup guide · Q Phone QR code |
| D | Toggle the demo car · U metric/imperial · F fullscreen · S status bar |

Editor: click/shift-click to select, drag to move, drag the corner to resize, arrows to
nudge, Delete, Ctrl/⌘+D duplicate, Ctrl/⌘+Z undo, Ctrl/⌘+S save, G snap.

## Dashboards

Your dashboards live in `~/Library/Application Support/DashCluster/dashboards` (macOS) or
`%APPDATA%\DashCluster\dashboards` (Windows). Edit them visually (E) or in a text editor; the
app reloads on save. Share them as `.cluster` packages (Export/Import in the editor).
The file format is described in [docs/dashboard-format.md](docs/dashboard-format.md).

## Develop

```sh
cargo run -p cluster-desktop -- --source demo        # live window with the demo car
cargo run -p forza-fake -- --to 127.0.0.1:5300       # pretend to be Forza (any machine)
cargo run -p cluster-desktop -- --record s.ctlog     # record real Forza packets
cargo run -p cluster-desktop -- --source replay --replay s.ctlog
cargo run -p cluster-desktop -- snapshot -d race --out race.png   # headless render
cargo test --workspace
./scripts/build-web.sh        # phone display (WASM); rebuild the desktop app afterwards
./scripts/package-macos.sh    # dist/DashCluster.app + .dmg   (Windows: scripts/package-windows.ps1)
```

The web build needs `rustup target add wasm32-unknown-unknown` and
`cargo install wasm-bindgen-cli --version <wasm-bindgen version in Cargo.lock>`.

| Path | What |
|---|---|
| `crates/cluster-core` | Signal catalog, units, signal bus, source trait, wire protocol |
| `crates/cluster-layout` | Dashboard format, validation, evaluation, editing operations |
| `crates/cluster-ui` | Slint widgets, dashboard view, editor/settings UI, offscreen renderer |
| `crates/cluster-net` | Hub: web display + WebSocket stream + mDNS |
| `crates/source-*` | Telemetry sources: demo car, Forza (UDP, record/replay), another hub |
| `apps/cluster-desktop` | The desktop app (`dashcluster`) |
| `apps/cluster-web` | The phone/browser display (WASM) |
| `tools/forza-fake` | Fake Forza packet sender |

Made with Slint, used under the Slint Royalty-free License (attribution is in Settings →
About). Dashboard artwork is original; no manufacturer designs or logos.
