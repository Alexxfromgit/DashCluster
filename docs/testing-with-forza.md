# Testing with Forza Horizon 5

A checklist for trying every workflow with the real game, e.g. on a Windows laptop with FH5 and a
phone on the same Wi-Fi. Tick what works; anything that doesn't is worth an issue (attach a
recording: `--record`).

## 0. Build on Windows

```powershell
git clone https://github.com/Alexxfromgit/DashCluster.git
cd DashCluster
.\scripts\package-windows.ps1          # builds the phone display + DashCluster.exe into dist\
```

The first run installs the `wasm32` target and `wasm-bindgen-cli` (a few minutes). Or download the
zip from Releases. Start `dist\DashCluster\DashCluster.exe` (or `cargo run --release -p cluster-desktop`).

- [ ] Windows Firewall asks about network access → allow on **private** networks
- [ ] The setup guide appears on first launch and shows this PC's IP address

## 1. Game → same PC

DashCluster and FH5 on the laptop (Microsoft Store / Xbox app version needs the loopback exemption).

- [ ] Run the `CheckNetIsolation` command from the guide (administrator PowerShell)
- [ ] FH5: Data Out **On**, IP `127.0.0.1`, port `5300`
- [ ] Drive: the status bar shows `Forza Horizon 4/5 (324 B) from 127.0.0.1` and ~60 Hz
- [ ] Speed, RPM, gear, shift lights, tyre temperatures, pedals, g-meter, boost, power and torque move
- [ ] Lap timer and race position fill in during a race; last/best lap appear after a lap
- [ ] Pause the game / open the map: "Game paused or in a menu", gauges dim; resume: live again
- [ ] Quit to the menu for a few seconds: "Waiting for telemetry…" after ~2 s
- [ ] `--record fh5.ctlog`, drive a lap, then `--source replay --replay fh5.ctlog` plays it back

## 2. Game → another computer

DashCluster on the Mac (or another PC), FH5 on the laptop.

- [ ] FH5 Data Out IP = the other computer's IP (shown in its guide, **H**), port `5300`
- [ ] Live data there; no loopback exemption needed

## 3. Phone

- [ ] **Q** on the computer that receives the game shows a QR code; scanning opens the dashboard
- [ ] Live at full speed; tapping the right/left edge changes the phone's dashboard only
- [ ] Middle tap toggles fullscreen (Android); iPhone: Share → Add to Home Screen, then open it
- [ ] With auto-lock turned off, the screen stays on while driving (phones don't allow plain-HTTP
      pages to keep the screen awake by themselves)
- [ ] Phone shows "Waiting for telemetry" when the game pauses; reconnects if the computer app restarts
- [ ] The status bar on the computer counts connected displays

## 4. Second computer mirroring

- [ ] On the second computer: **P** → Source → *Another DashCluster* → Apply
- [ ] It finds the first one by itself and shows the same live data
- [ ] With the address typed in (e.g. `192.168.1.20:5380`) it connects too

## 5. Dashboards and the editor

- [ ] ←/→ cycle the four sample dashboards; **U** switches metric/imperial (speedo becomes 0–175 mph)
- [ ] **E** opens the editor with live data; select, drag, resize (corner), arrow-key nudge, snap toggle
- [ ] Change a property (e.g. a gauge's Max), a signal from the dropdown, a color swatch
- [ ] Undo/redo (buttons and Ctrl+Z / Ctrl+Shift+Z) restore positions *and* the inspector fields
- [ ] Save a built-in dashboard → it suggests "<name> copy" in your dashboards folder
- [ ] **New** → add widgets → Save → it appears in the ←/→ list
- [ ] Phones following the computer see edits live while editing
- [ ] Export a `.cluster`, Import it again (with an image widget: the image comes along)
- [ ] Edit a saved dashboard in Notepad, save: the app reloads it; a typo shows a clear error

## 6. Settings

- [ ] **P**: change Forza port (and in FH5), units per quantity, always on top, open fullscreen
- [ ] Turn "Share telemetry" off: the phone disconnects and **Q** says sharing is off; on again works
- [ ] Settings survive a restart; the last dashboard is shown again
