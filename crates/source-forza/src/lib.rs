//! Forza Motorsport / Forza Horizon telemetry ("Data Out" UDP).
//!
//! In the game: Settings → HUD and Gameplay → Data Out = On, Data Out IP Address = the
//! machine running DashCluster, Data Out IP Port = 5300 (or whatever you pass here).

pub mod codec;
pub mod ctlog;

use cluster_core::{SignalId, SignalSet, SourceContext, SourceState, TelemetrySource};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 5300;

/// Best guess at this machine's LAN address, for the "point Forza here" hint. Uses a
/// connected UDP socket, which picks the outgoing interface without sending anything.
pub fn local_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?; // TEST-NET-1, never routed
    s.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_unspecified())
}

/// Every signal Forza's dash formats can fill.
pub fn capabilities() -> SignalSet {
    use SignalId as S;
    SignalSet::of(&[
        S::Speed,
        S::Rpm,
        S::RpmMax,
        S::RpmIdle,
        S::Gear,
        S::Power,
        S::Torque,
        S::Boost,
        S::Throttle,
        S::Brake,
        S::Clutch,
        S::Handbrake,
        S::Steer,
        S::Fuel,
        S::TyreTempFl,
        S::TyreTempFr,
        S::TyreTempRl,
        S::TyreTempRr,
        S::SuspFl,
        S::SuspFr,
        S::SuspRl,
        S::SuspRr,
        S::SlipFl,
        S::SlipFr,
        S::SlipRl,
        S::SlipRr,
        S::AccelLat,
        S::AccelLong,
        S::AccelVert,
        S::Distance,
        S::LapTime,
        S::LastLap,
        S::BestLap,
        S::LapNumber,
        S::RacePosition,
        S::RaceTime,
        S::RaceOn,
        S::CarId,
        S::CarClass,
        S::CarPi,
        S::Cylinders,
        S::PosX,
        S::PosY,
        S::PosZ,
    ])
}

/// Listens for Forza packets.
pub struct ForzaSource {
    pub port: u16,
    /// Also write raw packets to this `.ctlog` file.
    pub record: Option<PathBuf>,
    /// Re-send every raw packet to these addresses (to chain other telemetry tools).
    pub forward: Vec<SocketAddr>,
}

impl ForzaSource {
    pub fn new(port: u16) -> Self {
        ForzaSource { port, record: None, forward: vec![] }
    }
}

fn waiting_hint(port: u16) -> String {
    match local_ip() {
        Some(ip) => format!("Listening on UDP {port} — in Forza set Data Out IP to {ip}, port {port}"),
        None => format!("Listening on UDP {port}"),
    }
}

impl TelemetrySource for ForzaSource {
    fn name(&self) -> String {
        format!("Forza (UDP {})", self.port)
    }

    fn capabilities(&self) -> SignalSet {
        capabilities()
    }

    fn run(&mut self, ctx: &SourceContext) -> anyhow::Result<()> {
        let sock = UdpSocket::bind(("0.0.0.0", self.port))
            .map_err(|e| anyhow::anyhow!("can't listen on UDP port {}: {e}", self.port))?;
        sock.set_read_timeout(Some(Duration::from_millis(100)))?;
        let fwd = if self.forward.is_empty() { None } else { Some(UdpSocket::bind("0.0.0.0:0")?) };
        let mut rec = match &self.record {
            Some(p) => Some(ctlog::Writer::create(p, "forza")?),
            None => None,
        };
        ctx.set_state(SourceState::Waiting, waiting_hint(self.port));

        let mut buf = [0u8; 2048];
        let mut seq = 0u64;
        let mut last_packet: Option<Instant> = None;
        let mut last_detail = String::new();
        let mut unknown = 0u64;
        while !ctx.should_stop() {
            let (n, from) = match sock.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    if last_packet.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
                        last_packet = None;
                        last_detail.clear();
                        ctx.set_state(SourceState::Waiting, format!("No packets — {}", waiting_hint(self.port)));
                    }
                    continue;
                }
                Err(e) => return Err(e.into()),
            };
            let pkt = &buf[..n];
            if let Some(w) = &mut rec
                && let Err(e) = w.write(pkt)
            {
                log::warn!("recording stopped: {e}");
                rec = None;
            }
            if let Some(s) = &fwd {
                for to in &self.forward {
                    let _ = s.send_to(pkt, to);
                }
            }
            let Some((format, packet)) = codec::parse(pkt) else {
                unknown += 1;
                if last_packet.is_none() && (unknown == 1 || unknown.is_multiple_of(300)) {
                    ctx.set_state(
                        SourceState::Waiting,
                        format!("Ignoring {n}-byte packets from {} (not a Forza format)", from.ip()),
                    );
                }
                continue;
            };
            seq += 1;
            last_packet = Some(Instant::now());
            ctx.publish(codec::to_snapshot(&packet, seq));
            let detail = format!(
                "{}{} from {}",
                format.describe(),
                if packet.is_race_on { "" } else { " · paused/menu" },
                from.ip()
            );
            if detail != last_detail {
                ctx.set_state(SourceState::Live, detail.clone());
                last_detail = detail;
            }
        }
        if let Some(mut w) = rec {
            w.flush()?;
        }
        Ok(())
    }
}

/// Plays a `.ctlog` recording back with its original timing.
pub struct ReplaySource {
    pub path: PathBuf,
    pub speed: f32,
    pub looping: bool,
}

impl TelemetrySource for ReplaySource {
    fn name(&self) -> String {
        format!("Replay {}", self.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()))
    }

    fn capabilities(&self) -> SignalSet {
        capabilities()
    }

    fn run(&mut self, ctx: &SourceContext) -> anyhow::Result<()> {
        let (tag, records) = ctlog::read(&self.path)?;
        anyhow::ensure!(tag == "forza", "unsupported recording type {tag:?}");
        anyhow::ensure!(!records.is_empty(), "{} has no packets", self.path.display());
        let total = records.last().map(|r| r.t).unwrap_or_default();
        let speed = self.speed.clamp(0.05, 50.0);
        let mut seq = 0u64;
        loop {
            ctx.set_state(
                SourceState::Live,
                format!("{} packets, {:.0} s at {speed}×", records.len(), total.as_secs_f32()),
            );
            let start = Instant::now();
            for r in &records {
                let due = r.t.div_f32(speed);
                while start.elapsed() < due {
                    if ctx.should_stop() {
                        return Ok(());
                    }
                    std::thread::sleep((due - start.elapsed()).min(Duration::from_millis(50)));
                }
                if let Some((_, p)) = codec::parse(&r.payload) {
                    seq += 1;
                    ctx.publish(codec::to_snapshot(&p, seq));
                }
            }
            if !self.looping || ctx.should_stop() {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cluster_core::{SignalBus, SourceRunner};

    #[test]
    fn receives_udp_packets_and_records_them() {
        // Pick a free port.
        let port = UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(format!("forza-src-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("rec.ctlog");
        let bus = SignalBus::new();
        let mut src = ForzaSource::new(port);
        src.record = Some(log.clone());
        let mut runner = SourceRunner::start(bus.clone(), Box::new(src));
        std::thread::sleep(Duration::from_millis(150));

        let tx = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut car = source_demo::DemoCar::new();
        for i in 0..20 {
            car.step(0.05);
            let pkt = codec::encode(codec::Format::Horizon, &codec::from_snapshot(&car.snapshot(), i * 50));
            tx.send_to(&pkt, ("127.0.0.1", port)).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        tx.send_to(&[0u8; 17], ("127.0.0.1", port)).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        assert!(bus.publish_count() >= 20, "got {}", bus.publish_count());
        assert_eq!(bus.status().state, SourceState::Live);
        assert!(bus.latest().snapshot.get(SignalId::Rpm).is_some());
        runner.stop();

        let (tag, recs) = ctlog::read(&log).unwrap();
        assert_eq!(tag, "forza");
        assert_eq!(recs.len(), 21, "records all packets, including unknown ones");

        // And the recording replays.
        let bus2 = SignalBus::new();
        let mut r =
            SourceRunner::start(bus2.clone(), Box::new(ReplaySource { path: log, speed: 10.0, looping: false }));
        std::thread::sleep(Duration::from_millis(300));
        r.stop();
        assert_eq!(bus2.publish_count(), 20);
        std::fs::remove_dir_all(dir).ok();
    }
}
