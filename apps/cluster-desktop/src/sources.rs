//! Which telemetry source is running, and switching between them.

use cluster_core::{SignalBus, SourceRunner, TelemetrySource};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum SourceKind {
    Demo,
    Forza,
    Replay(PathBuf),
    /// Mirror another DashCluster hub (`None` = discover via mDNS).
    Hub(Option<String>),
}

#[derive(Debug, Clone)]
pub struct SourceConfig {
    pub forza_port: u16,
    pub record: Option<PathBuf>,
    pub forward: Vec<SocketAddr>,
    pub replay_speed: f32,
    /// Our own hub's port, so discovery doesn't connect to ourselves.
    pub own_hub_port: Option<u16>,
}

pub struct Sources {
    bus: Arc<SignalBus>,
    pub config: SourceConfig,
    pub kind: SourceKind,
    /// The last non-demo source, so "toggle demo" can go back to it.
    pub primary: SourceKind,
    runner: Option<SourceRunner>,
}

impl Sources {
    pub fn new(bus: Arc<SignalBus>, config: SourceConfig, kind: SourceKind) -> Self {
        let mut s = Sources {
            bus,
            config,
            primary: if kind == SourceKind::Demo { SourceKind::Forza } else { kind.clone() },
            kind: kind.clone(),
            runner: None,
        };
        s.start(kind);
        s
    }

    pub fn start(&mut self, kind: SourceKind) {
        if let Some(mut r) = self.runner.take() {
            r.stop();
        }
        let source: Box<dyn TelemetrySource> = match &kind {
            SourceKind::Demo => Box::new(source_demo::DemoSource::default()),
            SourceKind::Forza => {
                let mut f = source_forza::ForzaSource::new(self.config.forza_port);
                f.record = self.config.record.clone();
                f.forward = self.config.forward.clone();
                Box::new(f)
            }
            SourceKind::Replay(path) => Box::new(source_forza::ReplaySource {
                path: path.clone(),
                speed: self.config.replay_speed,
                looping: true,
            }),
            SourceKind::Hub(addr) => Box::new(source_net::HubSource {
                target: match addr {
                    Some(a) => source_net::HubTarget::Address(a.clone()),
                    None => source_net::HubTarget::Discover { exclude_port: self.config.own_hub_port },
                },
            }),
        };
        if kind != SourceKind::Demo {
            self.primary = kind.clone();
        }
        self.kind = kind;
        self.runner = Some(SourceRunner::start(self.bus.clone(), source));
    }

    pub fn toggle_demo(&mut self) {
        let next = if self.kind == SourceKind::Demo { self.primary.clone() } else { SourceKind::Demo };
        self.start(next);
    }
}
