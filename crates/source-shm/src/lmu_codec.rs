//! Decode the native LMU row layout as bytes on any host. The Windows reader
//! supplies these rows from the shared-memory mapping.

use cluster_core::{SignalId as S, Snapshot};

pub(super) const TELEMETRY_STRIDE: usize = 1888;
pub(super) const SCORING_STRIDE: usize = 584;

pub(super) fn decode(car: &[u8], score: Option<&[u8]>) -> Snapshot {
    let mut s = Snapshot::new();
    let value = |offset| double_at(car, offset) as f32;
    s.set(S::Speed, value(184).hypot(value(192)).hypot(value(200)));
    s.set(S::Gear, int_at(car, 352) as f32);
    s.set(S::Rpm, value(356));
    s.set_opt(S::RpmMax, positive(value(532)));
    s.set(S::CoolantTemp, value(364));
    s.set(S::OilTemp, value(372));
    s.set(S::Throttle, value(388));
    s.set(S::Brake, value(396));
    s.set(S::Steer, value(404));
    s.set(S::Clutch, value(412));
    s.set(S::AccelLat, -value(208)); // game axes: left, up, rear
    s.set(S::AccelVert, value(216));
    s.set(S::AccelLong, -value(224));
    s.set_flag(S::PitLimiter, car[604] != 0);
    let fuel = value(524) / value(608);
    s.set_opt(S::Fuel, (value(608) > 0.0 && (0.0..=1.0).contains(&fuel)).then_some(fuel));
    let elapsed = double_at(car, 12);
    s.set(S::RaceTime, elapsed as f32);
    let lap = (elapsed - double_at(car, 24)) as f32;
    s.set_opt(S::LapTime, (lap >= 0.0).then_some(lap));
    s.set_opt(S::LapNumber, positive(int_at(car, 20) as f32));
    for (offset, id) in [(160, S::PosX), (168, S::PosY), (176, S::PosZ)] {
        s.set(id, value(offset));
    }
    for (wheel, id) in [S::TyreTempFl, S::TyreTempFr, S::TyreTempRl, S::TyreTempRr].into_iter().enumerate() {
        let center_k = value(848 + wheel * 260 + 136);
        s.set_opt(id, (center_k > 0.0).then_some(center_k - 273.15));
    }
    if let Some(row) = score {
        s.set_opt(S::BestLap, positive(double_at(row, 144) as f32));
        s.set_opt(S::LastLap, positive(double_at(row, 168) as f32));
        s.set_opt(S::RacePosition, positive(f32::from(row[199])));
    }
    s
}

fn positive(value: f32) -> Option<f32> {
    (value > 0.0).then_some(value)
}

pub(super) fn int_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

pub(super) fn double_at(bytes: &[u8], offset: usize) -> f64 {
    f64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_lmu_velocity_axes_temperature_and_scoring() {
        let mut car = [0; TELEMETRY_STRIDE];
        let mut score = [0; SCORING_STRIDE];
        car[12..20].copy_from_slice(&43.0_f64.to_le_bytes());
        car[24..32].copy_from_slice(&40.0_f64.to_le_bytes());
        car[200..208].copy_from_slice(&(-35.0_f64).to_le_bytes());
        car[224..232].copy_from_slice(&(-1.5_f64).to_le_bytes());
        car[984..992].copy_from_slice(&353.15_f64.to_le_bytes());
        score[104..112].copy_from_slice(&150.0_f64.to_le_bytes()); // distance into the current lap
        score[144..152].copy_from_slice(&126.25_f64.to_le_bytes()); // best lap
        score[168..176].copy_from_slice(&131.5_f64.to_le_bytes()); // last lap
        score[199] = 4;
        let s = decode(&car, Some(&score));
        assert_eq!(s.get(S::LapTime), Some(3.0));
        assert_eq!(s.get(S::Speed), Some(35.0));
        assert_eq!(s.get(S::AccelLong), Some(1.5));
        assert_eq!(s.get(S::TyreTempFl), Some(80.0));
        assert_eq!(s.get(S::RacePosition), Some(4.0));
        assert_eq!(s.get(S::BestLap), Some(126.25));
        assert_eq!(s.get(S::LastLap), Some(131.5));
        assert_eq!(s.get(S::Distance), None);
        assert_eq!(s.get(S::TyreTempFr), None);
        assert_eq!(decode(&car, None).get(S::RacePosition), None);
    }

    #[test]
    fn captured_lmu_race_preserves_shifts_and_completed_laps() {
        const FRAME_HEADER: usize = 9;
        const FRAME_SIZE: usize = FRAME_HEADER + TELEMETRY_STRIDE + SCORING_STRIDE;
        let capture = include_bytes!("../fixtures/lmu-race.lmucap");
        assert_eq!(&capture[..8], b"LMURAW01");
        assert_eq!(u32::from_le_bytes(capture[8..12].try_into().unwrap()) as usize, TELEMETRY_STRIDE);
        assert_eq!(u32::from_le_bytes(capture[12..16].try_into().unwrap()) as usize, SCORING_STRIDE);
        let (frames, remainder) = capture[16..].as_chunks::<FRAME_SIZE>();
        assert!(remainder.is_empty(), "partial LMU frame");
        assert_eq!(frames.len(), 7);

        let mut previous = None;
        let mut gears = [false; 6];
        for (index, frame) in frames.iter().enumerate() {
            let tick = u64::from_le_bytes(frame[..8].try_into().unwrap());
            assert!(previous.is_none_or(|earlier| tick > earlier), "frame {index}: timestamps went backwards");
            previous = Some(tick);
            assert_eq!(frame[8], 1, "frame {index}: missing player scoring row");
            let (car, score) = frame[FRAME_HEADER..].split_at(TELEMETRY_STRIDE);
            assert_eq!(int_at(car, 0), int_at(score, 0), "frame {index}: player and scoring slot differ");
            let s = decode(car, Some(score));
            let gear = s.get(S::Gear).expect("captured gear");
            assert!((0.0..=5.0).contains(&gear) && gear.fract() == 0.0, "frame {index}: gear {gear}");
            gears[gear as usize] = true;
            for signal in [S::Speed, S::Rpm, S::LapTime, S::LastLap, S::BestLap, S::Fuel, S::TyreTempFl] {
                assert!(s.get(signal).is_some(), "frame {index}: no {}", signal.name());
            }
            let expected: &[(S, f32)] = match index {
                0 => &[(S::Gear, 5.0), (S::LapNumber, 1.0), (S::LastLap, 158.294), (S::BestLap, 158.294)],
                1 => &[(S::Speed, 70.7471), (S::Rpm, 6964.43), (S::TyreTempFl, 54.5397)],
                5 => &[
                    (S::Gear, 4.0),
                    (S::LapNumber, 2.0),
                    (S::LapTime, 0.2388),
                    (S::LastLap, 149.837),
                    (S::BestLap, 149.837),
                    (S::RacePosition, 33.0),
                ],
                6 => &[(S::Gear, 1.0), (S::Speed, 19.0783), (S::Brake, 0.4043), (S::LapTime, 5.2988)],
                _ => &[],
            };
            for &(signal, reference) in expected {
                let value = s.get(signal).unwrap_or_else(|| panic!("frame {index}: no {}", signal.name()));
                assert!(
                    (value - reference).abs() < 0.002 + reference.abs() * 1e-6,
                    "frame {index}: {} expected {reference}, got {value}",
                    signal.name()
                );
            }
        }
        assert_eq!(gears, [true; 6]);
    }
}
