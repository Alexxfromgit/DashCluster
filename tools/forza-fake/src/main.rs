//! Pretends to be Forza: drives the demo car and sends its telemetry as Data Out
//! packets, or re-sends a `.ctlog` recording with its original timing.
//!
//!     cargo run -p forza-fake -- --to 127.0.0.1:5300
//!     cargo run -p forza-fake -- --to 192.168.1.20:5300 --replay session.ctlog

use clap::{Parser, ValueEnum};
use source_forza::codec::{self, Format};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Copy, Clone, ValueEnum)]
enum Fmt {
    /// Forza Horizon 4/5 (324 B)
    Horizon,
    /// Forza Motorsport 7 dash (311 B)
    Dash,
    /// Forza Motorsport 2023 (331 B)
    Fm2023,
    /// Sled only (232 B)
    Sled,
}

#[derive(Parser)]
#[command(about = "Send fake Forza Data Out packets")]
struct Args {
    /// Where to send (the machine running DashCluster).
    #[arg(long, default_value = "127.0.0.1:5300")]
    to: String,
    #[arg(long, value_enum, default_value_t = Fmt::Horizon)]
    format: Fmt,
    /// Packets per second.
    #[arg(long, default_value_t = 60.0)]
    rate: f32,
    /// Stop after this many seconds (default: run until Ctrl-C).
    #[arg(long)]
    seconds: Option<f32>,
    /// Every N seconds, pretend to pause the game for 3 s (tests the paused state).
    #[arg(long)]
    pause_every: Option<f32>,
    /// Re-send a recording instead of simulating.
    #[arg(long)]
    replay: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.connect(&args.to)?;
    let start = Instant::now();
    let done = |t: Instant| args.seconds.is_some_and(|s| t.duration_since(start).as_secs_f32() >= s);

    if let Some(path) = &args.replay {
        let (_, records) = source_forza::ctlog::read(path)?;
        println!("replaying {} packets to {}", records.len(), args.to);
        for r in records {
            while start.elapsed() < r.t {
                std::thread::sleep((r.t - start.elapsed()).min(Duration::from_millis(20)));
            }
            match sock.send(&r.payload) {
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {}
                other => {
                    other?;
                }
            }
            if done(Instant::now()) {
                break;
            }
        }
        return Ok(());
    }

    let format = match args.format {
        Fmt::Horizon => Format::Horizon,
        Fmt::Dash => Format::Dash,
        Fmt::Fm2023 => Format::Motorsport2023,
        Fmt::Sled => Format::Sled,
    };
    println!("sending {} at {} Hz to {} (Ctrl-C to stop)", format.describe(), args.rate, args.to);
    let period = Duration::from_secs_f32(1.0 / args.rate.clamp(1.0, 1000.0));
    let mut car = source_demo::DemoCar::new();
    let mut last = Instant::now();
    let mut sent = 0u64;
    loop {
        let now = Instant::now();
        if done(now) {
            break;
        }
        car.step((now - last).as_secs_f32());
        last = now;
        let t = start.elapsed().as_secs_f32();
        let mut packet = codec::from_snapshot(&car.snapshot(), start.elapsed().as_millis() as u32);
        if let Some(every) = args.pause_every
            && t % every > every - 3.0
        {
            packet = codec::Packet { is_race_on: false, timestamp_ms: packet.timestamp_ms, ..Default::default() };
            packet.dash = Some(Default::default());
        }
        // Like the game, keep sending even if nobody is listening yet.
        match sock.send(&codec::encode(format, &packet)) {
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {}
            other => {
                other?;
            }
        }
        sent += 1;
        if sent.is_multiple_of(args.rate.max(1.0) as u64 * 5) {
            println!("{sent} packets sent");
        }
        std::thread::sleep(period.saturating_sub(now.elapsed()));
    }
    println!("{sent} packets sent");
    Ok(())
}
