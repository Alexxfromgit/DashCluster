//! Regression checks using actual game datagrams in capture order.

use crate::codec;
use cluster_core::{SignalId as S, Snapshot};
use source_forza::ctlog;
use std::path::Path;

fn decode_capture(game: &str, size: usize) -> Vec<Snapshot> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(format!("{game}.ctlog"));
    let (tag, records) = ctlog::read(&path).unwrap_or_else(|e| panic!("can't read {}: {e:#}", path.display()));
    assert_eq!(tag, game);
    assert_eq!(records.len(), 3, "{game}: missing capture packets");
    assert!(records[0].t.is_zero(), "{game}: recording starts at its first packet");
    assert!(records.windows(2).all(|p| p[0].t <= p[1].t), "{game}: capture timestamps went backwards");

    records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            assert_eq!(record.payload.len(), size, "{game}: packet {index}");
            let snapshot = codec::parse(&record.payload).unwrap_or_else(|| panic!("{game}: rejected packet {index}"));
            for signal in [S::Speed, S::Rpm, S::Gear, S::Throttle, S::Brake, S::Clutch, S::Distance, S::LapTime] {
                assert!(snapshot.get(signal).is_some(), "{game}: packet {index} lost {}", signal.name());
            }
            snapshot
        })
        .collect()
}

fn check_frame(game: &str, frames: &[Snapshot], index: usize, signals: &[(S, f32)]) {
    for &(signal, expected) in signals {
        let context = format!("{game}: packet {index}, {}", signal.name());
        let value = frames[index].get(signal).unwrap_or_else(|| panic!("{context}: missing signal"));
        let tolerance = 0.001 + expected.abs() * 1e-6;
        assert!((value - expected).abs() <= tolerance, "{context}: expected {expected}, got {value}");
    }
}

#[test]
fn captured_dirt_rally_packets() {
    let game = "dirt-rally";
    let frames = decode_capture(game, 264);
    check_frame(game, &frames, 0, &[(S::Speed, 0.0), (S::Rpm, 1600.0), (S::Distance, 0.0)]);
    check_frame(
        game,
        &frames,
        1,
        &[
            (S::Speed, 33.4953),
            (S::Rpm, 3178.241),
            (S::RpmMax, 7500.0),
            (S::RpmIdle, 1600.0),
            (S::Gear, 4.0),
            (S::Distance, 513.125),
            (S::Brake, 1.0),
            (S::Clutch, 0.388889),
            (S::AccelLat, -1.013922),
            (S::AccelLong, 1.807924),
            (S::LapTime, 26.682266),
        ],
    );
    check_frame(game, &frames, 2, &[(S::Distance, 1159.538), (S::Steer, -0.882086), (S::Throttle, 1.0)]);
}

#[test]
fn captured_grid_2_packets() {
    let game = "grid-2";
    let frames = decode_capture(game, 152);
    check_frame(game, &frames, 0, &[(S::Rpm, 1100.0), (S::Distance, 0.0)]);
    check_frame(
        game,
        &frames,
        1,
        &[
            (S::Speed, 57.163143),
            (S::Rpm, 5726.724),
            (S::Gear, 5.0),
            (S::Distance, 904.5654),
            (S::Throttle, 1.0),
            (S::Brake, 0.184314),
            (S::Steer, 0.041199),
            (S::AccelLat, -1.759913),
            (S::AccelLong, 0.356666),
            (S::LapTime, 28.554_03),
        ],
    );
    assert_eq!(frames[1].get(S::RpmMax), None);
    assert_eq!(frames[1].get(S::RpmIdle), None);
    check_frame(game, &frames, 2, &[(S::Distance, 2663.8975), (S::Gear, 3.0)]);
}
