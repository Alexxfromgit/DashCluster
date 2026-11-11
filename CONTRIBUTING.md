# Contributing to DashCluster

Thanks for helping! Bug reports, dashboard designs, game integrations and pull requests are all
welcome.

## Getting started

You need [Rust](https://rustup.rs) (stable; `rust-toolchain.toml` selects it). See the
[README](README.md#build-and-run) for the few system packages per OS. Then:

```bash
cargo test --workspace
cargo run -p cluster-desktop -- --source demo
```

See [docs/architecture.md](docs/architecture.md) for how the crates fit together.

### Without the game

| Command | What it does |
|---|---|
| `cargo run -p cluster-desktop -- --source demo` | Simulated car (also press **D** in the app) |
| `cargo run -p forza-fake -- --to 127.0.0.1:5300` | Sends Forza Horizon packets from the demo car, as the game would; `--format dash\|fm2023\|sled`, `--pause-every 20` |
| `cargo run -p cluster-desktop -- --record s.ctlog` | Records the raw packets of a real session |
| `cargo run -p cluster-desktop -- --source replay --replay s.ctlog` | Plays a recording back (`forza-fake --replay s.ctlog` re-sends it over UDP) |
| `cargo run -p cluster-desktop -- snapshot -d race --out race.png` | Renders a dashboard headlessly; `--live` runs the real source, `--edit <id>` / `--overlay settings\|guide\|qr` render the editor and panels |
| `cargo run -p cluster-desktop -- --dashboards my-dash.json5` | Loads extra dashboard files (hot-reloaded) |

Run `dashcluster --help` for every option.

### Phone display

`./scripts/build-web.sh` (Windows: `.\scripts\build-web.ps1`) builds `apps/cluster-web` to
WebAssembly into `apps/cluster-web/dist`; the desktop app embeds that folder when it compiles. While
working on the web display, `--web-dir apps/cluster-web/dist` serves it from disk instead, so only
the web build needs repeating.

## Pull requests

- Keep each PR to one change and describe what it does and how you tested it.
- Title the PR as a [Conventional Commit](https://www.conventionalcommits.org): PRs are
  squash-merged, and the title becomes the commit that drives versioning and release notes.
  - `feat: add a boost gauge widget` → new feature, minor release
  - `fix: keep shift lights off in neutral`, `perf: …`, `deps: …` → patch release
  - `feat!: …`, or a `BREAKING CHANGE:` line in the description → breaking change
  - `docs:`, `refactor:`, `test:`, `build:`, `ci:`, `chore:` → no release on their own, not in the notes

  Write the subject for users: it shows up as-is in the changelog.
- CI runs `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test` on Windows, macOS and
  Linux, builds the web display, and renders every sample dashboard plus the editor and settings
  panels (the PNGs are attached to the run). Builds must stay warning-free.
- Add tests next to the code (`#[cfg(test)]`). Parsers and evaluation are pure functions on
  purpose, so most behaviour can be tested without a window.

## Ground rules

- **Missing is not zero.** Sources leave signals they don't have empty; widgets show "no data".
  Never fill in a fake 0.
- **Base units inside, display units outside.** Signals are stored in SI-ish base units
  (`cluster_core::Quantity`); conversion happens only in `UnitPrefs`. Numbers in dashboard files
  are metric display units (see [docs/dashboard-format.md](docs/dashboard-format.md)).
- **One signal catalog.** Signals are defined only in `crates/cluster-core/src/signal.rs`. Adding
  one: add it there, map it in the sources that have it, and list it in `docs/dashboard-format.md`.
- **Original artwork.** Sample dashboards must be original designs: no manufacturer logos or
  copies of real instrument clusters.
- **Game data stays local.** Nothing leaves the user's network; the hub only listens on the LAN.

## Adding a game

Implement `cluster_core::TelemetrySource` in a new `crates/source-<game>` crate: map the game's data
into a `Snapshot`, set the source state (`Waiting` with setup hints, `Live`), and check
`ctx.should_stop()` at least every 100 ms. Add a codec test with real packet captures, then a
`SourceKind` in `apps/cluster-desktop/src/sources.rs` and an entry in the settings panel.
`crates/source-forza` is the reference.

## Packaging and releases

| Script | Output |
|---|---|
| `scripts/package-windows.ps1` | `dist/DashCluster-<version>-windows.zip` |
| `scripts/package-macos.sh` | `dist/DashCluster.app` (ad-hoc signed) and `dist/DashCluster-<version>-macos.dmg` |
| `scripts/package-linux.sh` | `dist/DashCluster-<version>-linux-x64.tar.gz` |

Each builds the web display first, so the packaged app serves phones out of the box.

**Releases:** [Release Please](https://github.com/googleapis/release-please) keeps a release PR open
on `main` that bumps the version in `Cargo.toml` and updates `CHANGELOG.md` from the PR titles.
Merging it tags `vX.Y.Z`; the *Release* workflow then tests and packages Windows, macOS and Linux,
starts each packaged app once, and publishes the release with `SHA256SUMS.txt` and build
provenance. To rebuild the files of an existing release, run the *Release* workflow by hand with its
tag.

## Reporting bugs

Use the issue templates. A short recording (`--record session.ctlog`) makes telemetry bugs easy to
reproduce. For security problems, follow [SECURITY.md](SECURITY.md) instead of opening a public
issue.
