# DashCluster

A customizable digital instrument cluster for racing sims (and, later, real cars over
OBD-II), written in Rust with [Slint](https://slint.dev).

> Status: early development. Part 1 (MVP) targets Forza Horizon 5 telemetry, shown on
> desktop (macOS/Windows) and on phones through a browser.

## Run

```sh
cargo run -p cluster-desktop                 # live window driven by the demo car
cargo run -p cluster-desktop -- snapshot     # headless render to snapshot.png
cargo test --workspace
```

## Layout

| Path | What |
|---|---|
| `crates/cluster-core` | Signal catalog, units, signal bus, source trait |
| `crates/source-demo` | Simulated car (deterministic; also a test fixture) |
| `crates/cluster-ui` | Slint widget library, offscreen renderer |
| `apps/cluster-desktop` | Desktop app (`dashcluster` binary) |

Slint is used under its Royalty-free license for desktop, mobile and web apps, which
requires attribution ("Made with Slint") in the app or its About screen.
