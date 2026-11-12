# Architecture

```
 Sources (own thread each)          Core                                Outputs
 ┌───────────────────┐
 │ source-demo       │──┐
 │ source-forza (UDP)│──┼─► Snapshot ─► SignalBus (latest value) ─┬─► desktop UI (Slint, pulls each frame)
 │   + record/replay │  │   normalized    lock-free, arc-swap       ├─► hub (cluster-net) ─► phones (WASM)
 │ source-net (hub)  │──┘   base units                             │                    └─► other computers
 └───────────────────┘                                              └─► (recorder: raw packets → .ctlog)
```

## Crates

| Crate | Role |
|---|---|
| `cluster-core` | The signal catalog (`SignalId`, one place), `Snapshot` (one frame; missing ≠ 0), unit conversion (`UnitPrefs`), `SignalBus`, the `TelemetrySource` trait and runner, the hub wire protocol, and network helpers (LAN address, mDNS service name, this app's instance ID) |
| `cluster-layout` | Dashboard files: the JSON5 model, validation with readable errors (`compile`), per-frame evaluation (`eval`: units, zones, smoothing, templates, dynamic ranges) and editing operations (`edit`: property schemas, undo/redo). No UI dependency, so it is shared by desktop and web and fully unit-tested |
| `cluster-ui` | Slint: the widget library (`widgets.slint`), the scaled `Dashboard`, the editor and settings panels, `DashboardView` (keeps the Slint models in sync with the evaluator) and an offscreen renderer for headless PNGs |
| `cluster-net` | The hub: axum serves the embedded web display and `/ws`, which streams binary frames and JSON control messages; mDNS advertisement |
| `source-demo` | A deterministic simulated car (gearbox, track, temperatures); also a test fixture |
| `source-forza` | Forza *Data Out* codec (format by packet length), UDP listener, `.ctlog` recorder and replay |
| `source-net` | Mirrors another hub (address or mDNS discovery) |
| `apps/cluster-desktop` | The `dashcluster` binary: library of dashboards, hot reload, editor controller, settings, sources, hub, CLI |
| `apps/cluster-web` | The browser display: the same `cluster-ui` compiled to WebAssembly, fed by the hub's WebSocket |
| `tools/forza-fake` | Sends Forza packets from the demo car or a recording |
| `tools/license-notices` | Writes the license texts of every linked crate for the release packages |

## Data flow per frame

1. A source thread parses input into a `Snapshot` and publishes it to the `SignalBus`.
2. A 60 Hz Slint timer reads the latest snapshot (`bus.latest()`), so a burst of packets never
   floods the UI.
3. `Evaluator::frame` turns it into up to four value/text/color slots per widget (`WidgetFrame`);
   `DashboardView` pushes only changed rows into the Slint model.
4. Slower-changing widget config (`WidgetStatic`: geometry, ranges from `rpm_max`, colors) is
   re-evaluated four times a second and pushed only when it changes.

The hub task reads the same bus and sends each new snapshot to every connected display
(`cluster_core::wire`: ~5 bytes per signal), plus the dashboard JSON5 each display asked for.

## Dashboards and the editor

A dashboard file is the single source of truth: the renderer compiles it, and the editor edits its
JSON value (`cluster_layout::edit::Doc`), validating every change with the same compiler and
rolling back what doesn't compile. Drags update geometry live without recompiling and are committed
as one undo step on release. While editing, phones that follow the desktop see the edits live.

## Units

Signals are stored in base units (m/s, rpm, °C, kPa, W, N·m, m/s², 0–1 ratios). Numbers in dashboard
files are metric display units after the binding's `scale`; when the user picks other units, fixed
ranges are converted and re-rounded to tidy tick steps (`eval::nice_range`).
