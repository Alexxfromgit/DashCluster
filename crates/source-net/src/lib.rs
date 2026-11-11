//! Mirrors another DashCluster hub: e.g. the Windows laptop receives Forza and this
//! Mac shows the same telemetry.

use cluster_core::wire::{self, ServerMsg};
use cluster_core::{SignalSet, SourceContext, SourceState, TelemetrySource};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub const MDNS_SERVICE: &str = "_dashcluster._tcp.local.";

/// Finds hubs on the LAN via mDNS. Returns (instance name, address).
pub fn discover(timeout: Duration) -> Vec<(String, SocketAddr)> {
    let Ok(daemon) = mdns_sd::ServiceDaemon::new() else { return vec![] };
    let Ok(rx) = daemon.browse(MDNS_SERVICE) else { return vec![] };
    let end = Instant::now() + timeout;
    let mut found = Vec::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) => {
                let name = info.get_fullname().trim_end_matches(MDNS_SERVICE).trim_end_matches('.').to_owned();
                if let Some(ip) = info.get_addresses_v4().into_iter().next() {
                    let addr = SocketAddr::new(std::net::IpAddr::V4(ip), info.get_port());
                    if !found.iter().any(|(_, a)| *a == addr) {
                        found.push((name, addr));
                    }
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = daemon.shutdown();
    found
}

/// Where to connect.
#[derive(Debug, Clone)]
pub enum HubTarget {
    /// `host:port`
    Address(String),
    /// First hub found via mDNS (skipping `exclude_port` on this machine, i.e. ourselves).
    Discover { exclude_port: Option<u16> },
}

pub struct HubSource {
    pub target: HubTarget,
}

impl HubSource {
    fn resolve(&self, ctx: &SourceContext) -> Option<String> {
        match &self.target {
            HubTarget::Address(a) => Some(a.clone()),
            HubTarget::Discover { exclude_port } => {
                ctx.set_state(SourceState::Waiting, "Looking for a DashCluster hub on the network…");
                let own_ip = local_ip();
                discover(Duration::from_secs(3))
                    .into_iter()
                    .find(|(_, a)| !(Some(a.ip()) == own_ip && Some(a.port()) == *exclude_port))
                    .map(|(_, a)| a.to_string())
            }
        }
    }
}

fn local_ip() -> Option<std::net::IpAddr> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    s.local_addr().ok().map(|a| a.ip())
}

impl TelemetrySource for HubSource {
    fn name(&self) -> String {
        match &self.target {
            HubTarget::Address(a) => format!("Hub {a}"),
            HubTarget::Discover { .. } => "Network hub".into(),
        }
    }

    fn capabilities(&self) -> SignalSet {
        SignalSet::all()
    }

    fn run(&mut self, ctx: &SourceContext) -> anyhow::Result<()> {
        let mut backoff = Duration::from_millis(500);
        while !ctx.should_stop() {
            let Some(addr) = self.resolve(ctx) else {
                ctx.set_state(SourceState::Waiting, "No hub found yet — is DashCluster running on the other machine?");
                sleep_unless_stopped(ctx, Duration::from_secs(2));
                continue;
            };
            match self.session(ctx, &addr) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    log::info!("hub {addr}: {e:#}");
                    ctx.set_state(SourceState::Waiting, format!("Hub {addr} unavailable ({e}); retrying…"));
                    sleep_unless_stopped(ctx, backoff);
                    backoff = (backoff * 2).min(Duration::from_secs(5));
                }
            }
        }
        Ok(())
    }
}

fn sleep_unless_stopped(ctx: &SourceContext, d: Duration) {
    let end = Instant::now() + d;
    while Instant::now() < end && !ctx.should_stop() {
        std::thread::sleep(Duration::from_millis(50));
    }
}

impl HubSource {
    /// One connection; returns Ok(()) only when asked to stop.
    fn session(&self, ctx: &SourceContext, addr: &str) -> anyhow::Result<()> {
        let sock_addr = addr.to_socket_addrs()?.next().ok_or_else(|| anyhow::anyhow!("bad address"))?;
        let stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(3))?;
        stream.set_read_timeout(Some(Duration::from_millis(200)))?;
        stream.set_nodelay(true)?;
        let (mut ws, _) =
            tungstenite::client(format!("ws://{addr}/ws"), stream).map_err(|e| anyhow::anyhow!("handshake: {e}"))?;
        ctx.set_state(SourceState::Waiting, format!("Connected to hub {addr}"));
        let mut upstream = String::new();
        loop {
            if ctx.should_stop() {
                let _ = ws.close(None);
                return Ok(());
            }
            match ws.read() {
                Ok(tungstenite::Message::Binary(b)) => {
                    if let Some(s) = wire::decode_frame(&b) {
                        ctx.publish(s);
                    }
                }
                Ok(tungstenite::Message::Text(t)) => {
                    if let Ok(ServerMsg::Status { source, state, detail }) = serde_json::from_str::<ServerMsg>(&t) {
                        let summary = format!("via hub {addr}: {source} ({state})");
                        if summary != upstream {
                            upstream = summary.clone();
                            let st = if state == "live" { SourceState::Live } else { SourceState::Waiting };
                            let detail = if detail.is_empty() { summary } else { format!("{summary} — {detail}") };
                            ctx.set_state(st, detail);
                        }
                    }
                }
                Ok(tungstenite::Message::Close(_)) => anyhow::bail!("hub closed the connection"),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cluster_core::{SignalBus, SignalId, Snapshot, SourceRunner};

    #[test]
    fn mirrors_a_hub() {
        let upstream = SignalBus::new();
        let hub = cluster_net::Hub::start(
            upstream.clone(),
            cluster_net::Catalog::default(),
            cluster_net::HubConfig { port: 0, advertise: false, ..Default::default() },
        )
        .unwrap();
        let local = SignalBus::new();
        let target = HubTarget::Address(format!("127.0.0.1:{}", hub.addr.port()));
        let mut runner = SourceRunner::start(local.clone(), Box::new(HubSource { target }));
        std::thread::sleep(Duration::from_millis(300));
        for i in 0..10 {
            let mut s = Snapshot::new();
            s.seq = i;
            s.set(SignalId::Speed, i as f32);
            upstream.publish(s);
            std::thread::sleep(Duration::from_millis(30));
        }
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(local.latest().snapshot.get(SignalId::Speed), Some(9.0));
        runner.stop();
    }
}
