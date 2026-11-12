# Third-party notices

DashCluster is licensed under the [MIT License](LICENSE). It uses the following third-party
software.

## Slint

The user interface is built with [Slint](https://slint.dev), © SixtyFPS GmbH, used under the
**Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0**
(<https://slint.dev/terms-and-conditions#royalty-free>). Slint is also available under GPLv3 and a
commercial license. The attribution the royalty-free license asks for is in the app (Settings →
About) and in the README.

## Inter font

Slint embeds the [Inter](https://rsms.me/inter/) typeface, © The Inter Project Authors, under the
**SIL Open Font License 1.1** (<https://openfontlicense.org>).

## Rust crates

The application statically links Rust libraries from [crates.io](https://crates.io). Almost all are
licensed under **MIT** and/or **Apache-2.0**; others are under permissive licenses (Zlib, BSD-2/3-Clause,
ISC, BSL-1.0, Unicode-3.0, Unlicense, CC0-1.0) and one under **MPL-2.0** (`option-ext`, unmodified;
source at <https://crates.io/crates/option-ext>). Where a crate offers a choice, DashCluster uses it
under MIT or Apache-2.0.

Every release package includes `THIRD-PARTY-LICENSES.txt` with the license texts of all
these crates and the crates using each. It is generated from the source by
`cargo run -p license-notices -- --out THIRD-PARTY-LICENSES.txt` (`tools/license-notices`).

Notable direct dependencies: `slint`, `tokio`, `axum` (web server), `tungstenite` (WebSocket client),
`mdns-sd` (network discovery), `serde`, `serde_json`, `json5`, `notify` (file watching), `rfd` (file
dialogs), `zip`, `qrcode`, `png`, `clap`, `directories`, `wasm-bindgen` and `web-sys` (web display).

## Trademarks

Forza, Forza Horizon and Forza Motorsport are trademarks of Microsoft Corporation. DashCluster is an
independent project and is not affiliated with or endorsed by Microsoft or Playground Games. It only
reads the telemetry the games publish through their *Data Out* setting.
