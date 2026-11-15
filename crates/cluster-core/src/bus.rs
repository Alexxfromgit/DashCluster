//! The signal bus: a lock-free "latest value" slot written by one source and read by
//! any number of consumers (the UI each frame, the network hub, the recorder).

use crate::snapshot::Snapshot;
use arc_swap::ArcSwap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use web_time::Instant;

/// A snapshot plus the local time it was published.
#[derive(Debug, Clone)]
pub struct Stamped {
    pub snapshot: Snapshot,
    pub at: Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SourceState {
    /// No source running.
    Idle,
    /// Running but no data has arrived yet (e.g. waiting for the game to send).
    Waiting,
    /// Receiving data.
    Live,
    /// Stopped because of an error.
    Error(String),
}

#[derive(Debug, Clone)]
pub struct SourceStatus {
    pub source: String,
    pub state: SourceState,
    /// Human-readable detail, e.g. "Listening on 0.0.0.0:5300" or "FH5 (324 B)".
    pub detail: String,
}

impl Default for SourceStatus {
    fn default() -> Self {
        SourceStatus { source: String::new(), state: SourceState::Idle, detail: String::new() }
    }
}

pub struct SignalBus {
    latest: ArcSwap<Stamped>,
    status: ArcSwap<SourceStatus>,
    publishes: AtomicU64,
}

impl Default for SignalBus {
    fn default() -> Self {
        SignalBus {
            latest: ArcSwap::from_pointee(Stamped { snapshot: Snapshot::new(), at: Instant::now() }),
            status: ArcSwap::from_pointee(SourceStatus::default()),
            publishes: AtomicU64::new(0),
        }
    }
}

impl SignalBus {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn publish(&self, snapshot: Snapshot) {
        self.latest.store(Arc::new(Stamped { snapshot, at: Instant::now() }));
        self.publishes.fetch_add(1, Ordering::Relaxed);
    }

    pub fn latest(&self) -> Arc<Stamped> {
        self.latest.load_full()
    }

    /// Total frames published so far; consumers diff this to get a packet rate.
    pub fn publish_count(&self) -> u64 {
        self.publishes.load(Ordering::Relaxed)
    }

    /// Age of the latest telemetry frame. An empty frame clears the current telemetry.
    pub fn age_secs(&self) -> Option<f32> {
        let latest = self.latest.load();
        latest.snapshot.iter().next().map(|_| latest.at.elapsed().as_secs_f32())
    }

    pub fn status(&self) -> Arc<SourceStatus> {
        self.status.load_full()
    }

    pub fn set_status(&self, status: SourceStatus) {
        self.status.store(Arc::new(status));
    }

    pub fn update_status(&self, f: impl Fn(&mut SourceStatus)) {
        self.status.rcu(|cur| {
            let mut s = SourceStatus::clone(cur);
            f(&mut s);
            s
        });
    }

    /// Clears the latest frame, e.g. when switching sources so stale values vanish.
    pub fn reset(&self) {
        self.latest.store(Arc::new(Stamped { snapshot: Snapshot::new(), at: Instant::now() }));
        self.publishes.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SignalId;

    #[test]
    fn clearing_telemetry_preserves_frame_delivery_without_looking_fresh() {
        let bus = SignalBus::new();
        let mut frame = Snapshot::new();
        frame.set(SignalId::Rpm, 3000.0);
        bus.publish(frame);
        assert!(bus.age_secs().is_some());
        bus.publish(Snapshot::new());
        assert_eq!(bus.publish_count(), 2);
        assert!(bus.age_secs().is_none());
    }
}
