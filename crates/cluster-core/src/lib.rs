//! Core of DashCluster: the normalized signal catalog every telemetry source maps into,
//! unit conversion for display, the lock-free [`SignalBus`] the UI reads from, and the
//! [`TelemetrySource`] trait sources implement.

pub mod bus;
#[cfg(not(target_arch = "wasm32"))]
pub mod net;
pub mod signal;
pub mod snapshot;
pub mod source;
pub mod units;
pub mod wire;

pub use bus::{SignalBus, SourceState, SourceStatus, Stamped};
pub use signal::{Quantity, SignalId, SignalSet};
pub use snapshot::Snapshot;
#[cfg(not(target_arch = "wasm32"))]
pub use source::SourceRunner;
pub use source::{SourceContext, TelemetrySource};
pub use units::{UnitPrefs, format_gear, format_lap_time};
