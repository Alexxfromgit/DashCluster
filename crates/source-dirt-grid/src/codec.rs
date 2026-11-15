//! Legacy Codemasters UDP packets are sequences of little-endian floats.
//!
//! See Codemasters' *Setting up UDP output for DiRT 4*, appendices 2 and 4:
//! https://pdfcoffee.com/udp-output-setup-pdf-free.html
//! The 256-byte extended layout ends at the rev limit. The 264-byte variant
//! adds idle RPM and the forward gear count.

use cluster_core::{SignalId as S, Snapshot};

const STANDARD_GRAVITY: f32 = 9.806_65;
/// The legacy engine speed fields are angular rates in radians per second.
const RPM_SCALE: f32 = 30.0 / std::f32::consts::PI;

pub(crate) fn parse(packet: &[u8]) -> Option<Snapshot> {
    if !matches!(packet.len(), 152 | 256 | 264) {
        return None;
    }
    let field = |index: usize| {
        let offset = index * size_of::<f32>();
        f32::from_le_bytes(packet[offset..offset + size_of::<f32>()].try_into().unwrap())
    };

    let speed = field(7);
    let rpm = field(37) * RPM_SCALE;
    let gear = match field(33) {
        -1.0 | 10.0 => -1.0,
        gear if (0.0..=9.0).contains(&gear) && gear.fract() == 0.0 => gear,
        _ => return None,
    };
    // Check the common fields so a malformed packet cannot bring the source online.
    if !(0.0..=200.0).contains(&speed) || !(0.0..=30_000.0).contains(&rpm) {
        return None;
    }

    let mut snapshot = Snapshot::new();
    snapshot.set(S::Speed, speed);
    snapshot.set(S::Gear, gear);
    snapshot.set(S::Rpm, rpm);
    for (signal, index) in [(S::RaceTime, 0), (S::LapTime, 1)] {
        let value = field(index);
        snapshot.set_opt(signal, (value >= 0.0).then_some(value));
    }
    // The packet counts completed laps; the dashboard shows the lap being driven.
    let completed_laps = field(36);
    snapshot.set_opt(
        S::LapNumber,
        (completed_laps >= 0.0 && completed_laps.fract() == 0.0).then_some(completed_laps + 1.0),
    );
    for (signal, index) in [(S::PosX, 4), (S::PosY, 5), (S::PosZ, 6)] {
        snapshot.set(signal, field(index));
    }
    for (signal, index) in [(S::Throttle, 29), (S::Brake, 31), (S::Clutch, 32)] {
        let value = field(index);
        snapshot.set_opt(signal, (0.0..=1.0).contains(&value).then_some(value));
    }
    let steer = field(30);
    snapshot.set_opt(S::Steer, (-1.0..=1.0).contains(&steer).then_some(steer));
    snapshot.set(S::AccelLat, field(34) * STANDARD_GRAVITY);
    snapshot.set(S::AccelLong, field(35) * STANDARD_GRAVITY);

    // The fourth float is fractional race progress. The preceding float is
    // distance in metres on the current lap, with lap zero being the first lap.
    let lap_distance = field(2);
    let completed_distance = match field(36) {
        0.0 => Some(0.0),
        laps if packet.len() >= 256 && laps > 0.0 && laps.fract() == 0.0 => {
            let track_length = field(61);
            (track_length.is_finite() && track_length > 0.0).then_some(laps * track_length)
        }
        _ => None,
    };
    snapshot.set_opt(
        S::Distance,
        completed_distance.filter(|_| lap_distance.is_finite()).map(|completed| (completed + lap_distance).max(0.0)),
    );

    if packet.len() >= 256 {
        // Many rally cars leave the circuit-only fields at zero.
        for (signal, value) in
            [(S::RacePosition, field(39)), (S::LastLap, field(59)), (S::RpmMax, field(63) * RPM_SCALE)]
        {
            snapshot.set_opt(signal, (value > 0.0).then_some(value));
        }
        let fuel = field(45) / field(46);
        snapshot.set_opt(S::Fuel, (field(46) > 0.0 && (0.0..=1.0).contains(&fuel)).then_some(fuel));
    }
    if packet.len() == 264 {
        let idle = field(64) * RPM_SCALE;
        snapshot.set_opt(S::RpmIdle, (idle > 0.0).then_some(idle));
    }
    Some(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_standard_and_extended_layouts() {
        // Synthetic datagram with explicit wire offsets for each value under test.
        let mut packet = [0; 264];
        for (offset, value) in [
            (4, 12.25_f32),
            (8, 150.0),
            (12, 0.06),
            (28, 30.0),
            (116, 0.75),
            (120, -0.25),
            (124, 0.5),
            (128, 0.125),
            (132, 10.0),
            (140, 1.0),
            (148, 680.6784),
            (156, 2.0),
            (180, 20.0),
            (184, 50.0),
            (236, 60.0),
            (252, 785.3982),
            (256, 167.5516),
        ] {
            packet[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        for size in [152, 256, 264] {
            let s = parse(&packet[..size]).unwrap();
            assert_eq!(s.get(S::Speed), Some(30.0));
            assert!((s.get(S::Rpm).unwrap() - 6500.0).abs() < 0.01);
            assert_eq!(s.get(S::Gear), Some(-1.0));
            assert_eq!(s.get(S::Throttle), Some(0.75));
            assert_eq!(s.get(S::Steer), Some(-0.25));
            assert_eq!(s.get(S::Brake), Some(0.5));
            assert_eq!(s.get(S::Clutch), Some(0.125));
            assert_eq!(s.get(S::AccelLong), Some(STANDARD_GRAVITY));
            assert_eq!(s.get(S::LapTime), Some(12.25));
            assert_eq!(s.get(S::LapNumber), Some(1.0), "the opening lap is lap 1");
            assert_eq!(s.get(S::Distance), Some(150.0));
            assert_eq!(s.get(S::RpmMax).map(f32::round), (size >= 256).then_some(7500.0));
            assert_eq!(s.get(S::RpmIdle).map(f32::round), (size == 264).then_some(1600.0));
            assert_eq!(s.get(S::RacePosition), (size >= 256).then_some(2.0));
            assert_eq!(s.get(S::Fuel), (size >= 256).then_some(0.4));
            assert_eq!(s.get(S::LastLap), (size >= 256).then_some(60.0));
            assert_eq!(s.get(S::Handbrake), None);
        }
    }

    #[test]
    fn ignores_malformed_packets_and_unavailable_extended_fields() {
        let mut packet = [0; 264];
        for size in [0, 68, 151, 153, 255, 263] {
            assert!(parse(&packet[..size]).is_none());
        }
        let s = parse(&packet).unwrap();
        assert_eq!(s.get(S::Gear), Some(0.0));
        assert_eq!(s.get(S::Fuel), None);
        assert_eq!(s.get(S::RpmMax), None);
        assert_eq!(s.get(S::LastLap), None);
        packet[132..136].copy_from_slice(&3.5_f32.to_le_bytes());
        assert!(parse(&packet).is_none());
        packet[132..136].copy_from_slice(&2.0_f32.to_le_bytes());
        packet[148..152].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(parse(&packet).is_none());
    }

    #[test]
    fn uses_metres_on_first_lap_and_requires_track_length_afterward() {
        let mut packet = [0; 264];
        for (offset, value) in [(8, 625.0_f32), (12, 0.125), (244, 5000.0)] {
            packet[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(parse(&packet).unwrap().get(S::Distance), Some(625.0));
        assert_eq!(parse(&packet[..152]).unwrap().get(S::Distance), Some(625.0));
        packet[144..148].copy_from_slice(&1.0_f32.to_le_bytes());
        assert_eq!(parse(&packet).unwrap().get(S::Distance), Some(5625.0));
        assert_eq!(parse(&packet).unwrap().get(S::LapNumber), Some(2.0), "one lap done: on lap 2");
        assert_eq!(parse(&packet[..152]).unwrap().get(S::Distance), None);
    }
}
