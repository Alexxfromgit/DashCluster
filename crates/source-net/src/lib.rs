//! Mirrors another DashCluster hub: e.g. the Windows laptop receives Forza and this
//! Mac shows the same telemetry.

use cluster_core::net::{MDNS_ID_KEY, MDNS_SERVICE, instance_id, lan_ip};
use cluster_core::wire::{self, ServerMsg};
use cluster_core::{SourceContext, SourceState, TelemetrySource};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// A hub found on the network.
#[derive(Debug, Clone)]
pub struct FoundHub {
    pub name: String,
    /// Every address it advertised, best first (same subnet as this machine).
    pub addrs: Vec<SocketAddr>,
}

fn same_subnet(a: IpAddr, b: Option<IpAddr>) -> bool {
    match (a, b) {
        (IpAddr::V4(a), Some(IpAddr::V4(b))) => a.octets()[..3] == b.octets()[..3],
        _ => false,
    }
}

/// Finds other hubs on the LAN via mDNS (never this app's own hub). Returns early when
/// `stop()` says so.
pub fn discover(timeout: Duration, stop: &dyn Fn() -> bool) -> Vec<FoundHub> {
    let Ok(daemon) = mdns_sd::ServiceDaemon::new() else { return vec![] };
    let Ok(rx) = daemon.browse(MDNS_SERVICE) else { return vec![] };
    let end = Instant::now() + timeout;
    let own_ip = lan_ip();
    let mut found: Vec<FoundHub> = Vec::new();
    while Instant::now() < end && !stop() {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) => {
                if info.get_property_val_str(MDNS_ID_KEY) == Some(instance_id()) {
                    continue; // ourselves
                }
                let name = info.get_fullname().trim_end_matches(MDNS_SERVICE).trim_end_matches('.').to_owned();
                let mut addrs: Vec<SocketAddr> = info
                    .get_addresses_v4()
                    .into_iter()
                    .map(|ip| SocketAddr::new(IpAddr::V4(ip), info.get_port()))
                    .collect();
                // Same subnet first; loopback last (only useful when the hub is on this machine).
                addrs.sort_by_key(|a| (a.ip().is_loopback(), !same_subnet(a.ip(), own_ip), a.ip()));
                if !addrs.is_empty() && !found.iter().any(|f| f.name == name) {
                    found.push(FoundHub { name, addrs });
                }
            }
            Ok(_) => {}
            Err(_) => {
                if rx.is_disconnected() {
                    break;
                }
            }
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
    /// The first hub found via mDNS.
    Discover,
}

pub struct HubSource {
    pub target: HubTarget,
}

/// Runs a blocking step (DNS lookup, TCP connect) on a helper thread so the source can stop
/// right away instead of waiting for it. Returns `None` if asked to stop first.
fn cancellable<T: Send + 'static>(
    ctx: &SourceContext,
    f: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> Option<std::io::Result<T>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(r) => return Some(r),
            Err(mpsc::RecvTimeoutError::Timeout) if !ctx.should_stop() => {}
            Err(_) => return None,
        }
    }
}

fn sleep_unless_stopped(ctx: &SourceContext, d: Duration) {
    let end = Instant::now() + d;
    while Instant::now() < end && !ctx.should_stop() {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// How a session ended.
enum Ended {
    Stopped,
    /// Lost or refused; `connected` says whether data flowed first (resets the backoff).
    Failed {
        connected: bool,
        error: anyhow::Error,
    },
}

impl HubSource {
    /// Candidate addresses to try, best first.
    fn candidates(&self, ctx: &SourceContext) -> Option<Vec<String>> {
        match &self.target {
            HubTarget::Address(a) => Some(vec![a.clone()]),
            HubTarget::Discover => {
                ctx.set_state(SourceState::Waiting, "Looking for a DashCluster hub on the network…");
                let hubs = discover(Duration::from_secs(3), &|| ctx.should_stop());
                hubs.into_iter().next().map(|h| h.addrs.into_iter().map(|a| a.to_string()).collect())
            }
        }
    }

    fn connect(ctx: &SourceContext, addr: &str) -> Result<TcpStream, Option<anyhow::Error>> {
        let target = addr.to_owned();
        let resolved = match cancellable(ctx, move || {
            target.to_socket_addrs()?.next().ok_or_else(|| std::io::Error::other("address not found"))
        }) {
            None => return Err(None),
            Some(r) => r.map_err(|e| Some(anyhow::anyhow!("{e}")))?,
        };
        match cancellable(ctx, move || TcpStream::connect_timeout(&resolved, Duration::from_secs(3))) {
            None => Err(None),
            Some(r) => r.map_err(|e| Some(anyhow::anyhow!("{e}"))),
        }
    }

    /// One connection to `addr`, until stopped or lost.
    fn session(ctx: &SourceContext, addr: &str) -> Ended {
        let stream = match Self::connect(ctx, addr) {
            Ok(s) => s,
            Err(None) => return Ended::Stopped,
            Err(Some(error)) => return Ended::Failed { connected: false, error },
        };
        let fail = |e: anyhow::Error, connected| Ended::Failed { connected, error: e };
        // Generous timeout for the HTTP upgrade (a phone hotspot or sleepy Wi-Fi can be slow),
        // then a short one so the loop notices `should_stop` quickly.
        if let Err(e) = stream.set_read_timeout(Some(Duration::from_secs(3))).and_then(|()| stream.set_nodelay(true)) {
            return fail(e.into(), false);
        }
        let (mut ws, _) = match tungstenite::client(format!("ws://{addr}/ws"), stream) {
            Ok(v) => v,
            Err(e) => return fail(anyhow::anyhow!("handshake: {e}"), false),
        };
        if let Err(e) = ws.get_ref().set_read_timeout(Some(Duration::from_millis(200))) {
            return fail(e.into(), false);
        }
        ctx.set_state(SourceState::Waiting, format!("Connected to hub {addr}"));
        let mut upstream = String::new();
        let mut connected = false;
        loop {
            if ctx.should_stop() {
                let _ = ws.close(None);
                return Ended::Stopped;
            }
            match ws.read() {
                Ok(tungstenite::Message::Binary(b)) => {
                    if let Some(s) = wire::decode_frame(&b) {
                        connected = true;
                        ctx.publish(s);
                    }
                }
                Ok(tungstenite::Message::Text(t)) => {
                    connected = true;
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
                Ok(tungstenite::Message::Close(_)) => {
                    return fail(anyhow::anyhow!("hub closed the connection"), connected);
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return fail(e.into(), connected),
            }
        }
    }
}

impl TelemetrySource for HubSource {
    fn name(&self) -> String {
        match &self.target {
            HubTarget::Address(a) => format!("Hub {a}"),
            HubTarget::Discover => "Network hub".into(),
        }
    }

    fn run(&mut self, ctx: &SourceContext) -> anyhow::Result<()> {
        let mut backoff = Duration::from_millis(500);
        while !ctx.should_stop() {
            let Some(addrs) = self.candidates(ctx) else {
                if !ctx.should_stop() {
                    ctx.set_state(
                        SourceState::Waiting,
                        "No hub found yet — is DashCluster running on the other machine?",
                    );
                    sleep_unless_stopped(ctx, Duration::from_secs(2));
                }
                continue;
            };
            let mut last_error = None;
            for addr in &addrs {
                match Self::session(ctx, addr) {
                    Ended::Stopped => return Ok(()),
                    Ended::Failed { connected, error } => {
                        log::info!("hub {addr}: {error:#}");
                        if connected {
                            backoff = Duration::from_millis(500);
                        }
                        last_error = Some((addr.clone(), error));
                    }
                }
            }
            if let Some((addr, e)) = last_error {
                ctx.set_state(SourceState::Waiting, format!("Hub {addr} unavailable ({e}); retrying…"));
            }
            sleep_unless_stopped(ctx, backoff);
            backoff = (backoff * 2).min(Duration::from_secs(5));
        }
        Ok(())
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

    #[test]
    fn stopping_is_prompt_even_while_connecting() {
        // 192.0.2.1 is TEST-NET-1: connecting hangs until the connect timeout.
        let bus = SignalBus::new();
        let mut runner =
            SourceRunner::start(bus, Box::new(HubSource { target: HubTarget::Address("192.0.2.1:5380".into()) }));
        std::thread::sleep(Duration::from_millis(200));
        let t = Instant::now();
        runner.stop();
        assert!(t.elapsed() < Duration::from_millis(500), "stop took {:?}", t.elapsed());
    }

    #[test]
    fn prefers_addresses_on_our_subnet() {
        let own = Some(IpAddr::from([192, 168, 1, 20]));
        assert!(same_subnet(IpAddr::from([192, 168, 1, 7]), own));
        assert!(!same_subnet(IpAddr::from([172, 27, 0, 1]), own));
    }
}
