//! The trait telemetry sources implement, and a runner that drives one on its own thread.

#[cfg(not(target_arch = "wasm32"))]
use crate::bus::SourceStatus;
use crate::bus::{SignalBus, SourceState};
use crate::signal::SignalSet;
use crate::snapshot::Snapshot;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// What a source gets while running.
#[derive(Clone)]
pub struct SourceContext {
    bus: Arc<SignalBus>,
    stop: Arc<AtomicBool>,
}

impl SourceContext {
    pub fn new(bus: Arc<SignalBus>) -> Self {
        SourceContext { bus, stop: Arc::new(AtomicBool::new(false)) }
    }

    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn publish(&self, snapshot: Snapshot) {
        self.bus.publish(snapshot);
    }

    pub fn set_state(&self, state: SourceState, detail: impl Into<String>) {
        let detail = detail.into();
        self.bus.update_status(|s| {
            s.state = state.clone();
            s.detail.clone_from(&detail);
        });
    }

    pub fn bus(&self) -> &Arc<SignalBus> {
        &self.bus
    }
}

/// A producer of telemetry. `run` blocks until `ctx.should_stop()` becomes true or the
/// source fails; it should check `should_stop` at least every ~100 ms.
pub trait TelemetrySource: Send + 'static {
    /// Short display name, e.g. "Demo car" or "Forza (UDP 5300)".
    fn name(&self) -> String;
    /// Signals this source can provide (it may still omit some in a given frame).
    fn capabilities(&self) -> SignalSet;
    fn run(&mut self, ctx: &SourceContext) -> anyhow::Result<()>;
}

/// Runs one source on a dedicated thread and stops it on drop.
#[cfg(not(target_arch = "wasm32"))]
pub struct SourceRunner {
    ctx: SourceContext,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl SourceRunner {
    pub fn start(bus: Arc<SignalBus>, mut source: Box<dyn TelemetrySource>) -> Self {
        bus.reset();
        bus.set_status(SourceStatus {
            source: source.name(),
            state: SourceState::Waiting,
            detail: String::new(),
            capabilities: source.capabilities(),
        });
        let ctx = SourceContext::new(bus);
        let thread_ctx = ctx.clone();
        let handle = std::thread::Builder::new()
            .name("telemetry-source".into())
            .spawn(move || {
                let result = source.run(&thread_ctx);
                match result {
                    Ok(()) => thread_ctx.set_state(SourceState::Idle, "Stopped"),
                    Err(e) => {
                        log::error!("source {} failed: {e:#}", source.name());
                        thread_ctx.set_state(SourceState::Error(format!("{e:#}")), "");
                    }
                }
            })
            .expect("spawn source thread");
        SourceRunner { ctx, handle: Some(handle) }
    }

    pub fn stop(&mut self) {
        self.ctx.request_stop();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for SourceRunner {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::SignalId;

    struct Counter;
    impl TelemetrySource for Counter {
        fn name(&self) -> String {
            "counter".into()
        }
        fn capabilities(&self) -> SignalSet {
            SignalSet::of(&[SignalId::Rpm])
        }
        fn run(&mut self, ctx: &SourceContext) -> anyhow::Result<()> {
            let mut n = 0u64;
            ctx.set_state(SourceState::Live, "");
            while !ctx.should_stop() {
                let mut s = Snapshot::new();
                s.seq = n;
                s.set(SignalId::Rpm, n as f32);
                ctx.publish(s);
                n += 1;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Ok(())
        }
    }

    #[test]
    fn runner_publishes_and_stops() {
        let bus = SignalBus::new();
        let mut r = SourceRunner::start(bus.clone(), Box::new(Counter));
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(bus.publish_count() > 0);
        assert!(bus.latest().snapshot.get(SignalId::Rpm).is_some());
        r.stop();
        assert_eq!(bus.status().state, SourceState::Idle);
        assert_eq!(bus.status().source, "counter");
    }
}
