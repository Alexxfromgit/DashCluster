//! Forza Motorsport / Forza Horizon telemetry ("Data Out" UDP).
//!
//! In the game: Settings → HUD and Gameplay → Data Out = On, Data Out IP Address = the
//! machine running DashCluster, Data Out IP Port = 5300 (or whatever you pass here).

pub mod codec;
pub mod ctlog;

use cluster_core::{SourceContext, SourceState, TelemetrySource};
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 5300;

/// Big enough for any UDP datagram, so an oversized packet from some other program is read
/// whole and ignored (on Windows a too-small buffer is an error, not a truncation).
const RECV_BUF: usize = 65_536;

/// Listens for Forza packets.
pub struct ForzaSource {
    pub port: u16,
    /// Also append raw packets to this recording (shared across source restarts).
    pub record: Option<ctlog::SharedWriter>,
    /// Re-send every raw packet to these addresses (to chain other telemetry tools).
    pub forward: Vec<SocketAddr>,
}

impl ForzaSource {
    pub fn new(port: u16) -> Self {
        ForzaSource { port, record: None, forward: vec![] }
    }
}

fn waiting_hint(port: u16) -> String {
    match cluster_core::net::lan_ip() {
        Some(ip) => format!("Listening on UDP {port} — in Forza set Data Out IP to {ip}, port {port}"),
        None => format!("Listening on UDP {port}"),
    }
}

impl TelemetrySource for ForzaSource {
    fn name(&self) -> String {
        format!("Forza (UDP {})", self.port)
    }

    fn run(&mut self, ctx: &SourceContext) -> anyhow::Result<()> {
        let sock = UdpSocket::bind(("0.0.0.0", self.port))
            .map_err(|e| anyhow::anyhow!("can't listen on UDP port {}: {e}", self.port))?;
        sock.set_read_timeout(Some(Duration::from_millis(100)))?;
        let fwd = if self.forward.is_empty() { None } else { Some(UdpSocket::bind("0.0.0.0:0")?) };
        let mut recording = self.record.is_some();
        ctx.set_state(SourceState::Waiting, waiting_hint(self.port));

        let mut buf = vec![0u8; RECV_BUF];
        let mut seq = 0u64;
        let mut last_packet: Option<Instant> = None;
        let mut last_detail = String::new();
        let mut unknown = 0u64;
        let mut bad = 0u64;
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
                // A bad datagram (e.g. Windows' WSAECONNRESET / WSAEMSGSIZE) affects only that
                // packet: skip it and keep listening.
                Err(e) => {
                    bad += 1;
                    if bad == 1 || bad.is_multiple_of(100) {
                        log::warn!("ignoring a UDP receive error ({bad} so far): {e}");
                    }
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
            };
            let pkt = &buf[..n];
            if recording
                && let Some(w) = &self.record
                && let Err(e) =
                    w.lock().map_err(|_| std::io::Error::other("recorder poisoned")).and_then(|mut w| w.write(pkt))
            {
                log::warn!("recording stopped: {e}");
                recording = false;
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
        if let Some(w) = &self.record
            && let Ok(mut w) = w.lock()
        {
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

    fn run(&mut self, ctx: &SourceContext) -> anyhow::Result<()> {
        let speed = self.speed.clamp(0.05, 50.0);
        let mut seq = 0u64;
        loop {
            let mut reader = ctlog::Reader::open(&self.path)?;
            anyhow::ensure!(reader.tag == "forza", "unsupported recording type {:?}", reader.tag);
            ctx.set_state(SourceState::Live, format!("{} at {speed}×", self.path.display()));
            let start = Instant::now();
            let mut count = 0u64;
            let mut last_t = Duration::ZERO;
            while let Some(r) = reader.next_record()? {
                count += 1;
                last_t = r.t;
                let due = r.t.div_f32(speed);
                while start.elapsed() < due {
                    if ctx.should_stop() {
                        return Ok(());
                    }
                    std::thread::sleep((due - start.elapsed()).min(Duration::from_millis(50)));
                }
                if ctx.should_stop() {
                    return Ok(());
                }
                if let Some((_, p)) = codec::parse(&r.payload) {
                    seq += 1;
                    ctx.publish(codec::to_snapshot(&p, seq));
                }
            }
            anyhow::ensure!(count > 0, "{} has no packets", self.path.display());
            if !self.looping || ctx.should_stop() {
                return Ok(());
            }
            // A recording with no duration would otherwise spin a CPU core.
            if last_t < Duration::from_millis(100) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cluster_core::{SignalBus, SignalId, SourceRunner};

    #[test]
    fn receives_udp_packets_and_records_them() {
        // Pick a free port.
        let port = UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(format!("forza-src-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("rec.ctlog");
        let recorder = ctlog::Writer::shared(&log, "forza").unwrap();
        let bus = SignalBus::new();
        let mut src = ForzaSource::new(port);
        src.record = Some(recorder.clone());
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

        // Restarting the source (e.g. demo car and back) keeps appending to the recording.
        let mut src = ForzaSource::new(port);
        src.record = Some(recorder.clone());
        let mut runner = SourceRunner::start(bus.clone(), Box::new(src));
        std::thread::sleep(Duration::from_millis(100));
        for i in 0..3 {
            let pkt = codec::encode(codec::Format::Horizon, &codec::from_snapshot(&car.snapshot(), 2000 + i));
            tx.send_to(&pkt, ("127.0.0.1", port)).unwrap();
        }
        // An oversized datagram is ignored, not fatal.
        tx.send_to(&vec![0u8; 4000], ("127.0.0.1", port)).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(bus.status().state, SourceState::Live);
        runner.stop();
        drop(recorder);

        let (tag, recs) = ctlog::read(&log).unwrap();
        assert_eq!(tag, "forza");
        assert_eq!(recs.len(), 25, "records all packets of both runs, including unknown ones");

        // And the recording replays.
        let bus2 = SignalBus::new();
        let mut r =
            SourceRunner::start(bus2.clone(), Box::new(ReplaySource { path: log, speed: 10.0, looping: false }));
        std::thread::sleep(Duration::from_millis(300));
        r.stop();
        assert_eq!(bus2.publish_count(), 23);
        std::fs::remove_dir_all(dir).ok();
    }
}
