//! Forza "Data Out" packet layouts, all little-endian:
//!
//! | Format | Size | Games |
//! |---|---|---|
//! | Sled | 232 B | FM7 "Sled" setting |
//! | Dash | 311 B | FM7 "Car Dash" |
//! | Horizon | 324 B | Forza Horizon 4 / 5 (Dash with 12 extra bytes after the sled block, 1 trailing) |
//! | Motorsport 2023 | 331 B | Forza Motorsport (2023): Dash + tyre wear + track ID |
//!
//! The format is detected from the packet length.

use cluster_core::{SignalId as S, Snapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Sled,
    Dash,
    Horizon,
    Motorsport2023,
}

impl Format {
    pub fn from_len(len: usize) -> Option<Format> {
        match len {
            232 => Some(Format::Sled),
            311 => Some(Format::Dash),
            324 => Some(Format::Horizon),
            331 => Some(Format::Motorsport2023),
            _ => None,
        }
    }

    pub fn packet_len(self) -> usize {
        match self {
            Format::Sled => 232,
            Format::Dash => 311,
            Format::Horizon => 324,
            Format::Motorsport2023 => 331,
        }
    }

    /// Offset of the dash block (after the sled block), if the format has one.
    fn dash_offset(self) -> Option<usize> {
        match self {
            Format::Sled => None,
            Format::Dash | Format::Motorsport2023 => Some(232),
            Format::Horizon => Some(244),
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Format::Sled => "Forza sled (232 B)",
            Format::Dash => "Forza Motorsport 7 dash (311 B)",
            Format::Horizon => "Forza Horizon 4/5 (324 B)",
            Format::Motorsport2023 => "Forza Motorsport 2023 (331 B)",
        }
    }
}

/// Raw packet fields we use, in the game's own units.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Packet {
    pub is_race_on: bool,
    pub timestamp_ms: u32,
    pub engine_max_rpm: f32,
    pub engine_idle_rpm: f32,
    pub current_engine_rpm: f32,
    /// Car-local acceleration, m/s²: x = right, y = up, z = forward.
    pub accel: [f32; 3],
    pub velocity: [f32; 3],
    /// 0 = fully extended, 1 = fully compressed. FL, FR, RL, RR.
    pub susp_travel: [f32; 4],
    pub tire_slip_ratio: [f32; 4],
    pub tire_combined_slip: [f32; 4],
    pub car_ordinal: i32,
    pub car_class: i32,
    pub car_pi: i32,
    pub drivetrain: i32,
    pub cylinders: i32,
    pub dash: Option<Dash>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dash {
    pub position: [f32; 3],
    /// m/s
    pub speed: f32,
    /// W
    pub power: f32,
    /// N·m
    pub torque: f32,
    /// °F. FL, FR, RL, RR.
    pub tire_temp_f: [f32; 4],
    /// psi
    pub boost_psi: f32,
    /// 0..1
    pub fuel: f32,
    pub distance: f32,
    pub best_lap: f32,
    pub last_lap: f32,
    pub current_lap: f32,
    pub race_time: f32,
    pub lap_number: u16,
    pub race_position: u8,
    pub accel: u8,
    pub brake: u8,
    pub clutch: u8,
    pub handbrake: u8,
    /// 0 = reverse, 1.. = forward, 11 = neutral.
    pub gear: u8,
    pub steer: i8,
}

// Sled-block offsets.
const IS_RACE_ON: usize = 0;
const TIMESTAMP: usize = 4;
const RPM_MAX: usize = 8;
const RPM_IDLE: usize = 12;
const RPM: usize = 16;
const ACCEL: usize = 20;
const VELOCITY: usize = 32;
const SUSP: usize = 68;
const SLIP_RATIO: usize = 84;
const COMBINED_SLIP: usize = 180;
const CAR_ORDINAL: usize = 212;
const CAR_CLASS: usize = 216;
const CAR_PI: usize = 220;
const DRIVETRAIN: usize = 224;
const CYLINDERS: usize = 228;

// Dash-block offsets (relative to the dash block start).
const D_POS: usize = 0;
const D_SPEED: usize = 12;
const D_POWER: usize = 16;
const D_TORQUE: usize = 20;
const D_TIRE_TEMP: usize = 24;
const D_BOOST: usize = 40;
const D_FUEL: usize = 44;
const D_DISTANCE: usize = 48;
const D_BEST: usize = 52;
const D_LAST: usize = 56;
const D_CURRENT: usize = 60;
const D_RACE_TIME: usize = 64;
const D_LAP: usize = 68;
const D_POSITION: usize = 70;
const D_ACCEL: usize = 71;
const D_BRAKE: usize = 72;
const D_CLUTCH: usize = 73;
const D_HANDBRAKE: usize = 74;
const D_GEAR: usize = 75;
const D_STEER: usize = 76;

pub const NEUTRAL_GEAR: u8 = 11;

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn f32x<const N: usize>(b: &[u8], o: usize) -> [f32; N] {
    std::array::from_fn(|i| f32_at(b, o + 4 * i))
}

/// Parses a packet; returns `None` for unknown lengths.
pub fn parse(b: &[u8]) -> Option<(Format, Packet)> {
    let format = Format::from_len(b.len())?;
    let mut p = Packet {
        is_race_on: i32_at(b, IS_RACE_ON) != 0,
        timestamp_ms: u32_at(b, TIMESTAMP),
        engine_max_rpm: f32_at(b, RPM_MAX),
        engine_idle_rpm: f32_at(b, RPM_IDLE),
        current_engine_rpm: f32_at(b, RPM),
        accel: f32x(b, ACCEL),
        velocity: f32x(b, VELOCITY),
        susp_travel: f32x(b, SUSP),
        tire_slip_ratio: f32x(b, SLIP_RATIO),
        tire_combined_slip: f32x(b, COMBINED_SLIP),
        car_ordinal: i32_at(b, CAR_ORDINAL),
        car_class: i32_at(b, CAR_CLASS),
        car_pi: i32_at(b, CAR_PI),
        drivetrain: i32_at(b, DRIVETRAIN),
        cylinders: i32_at(b, CYLINDERS),
        dash: None,
    };
    if let Some(d) = format.dash_offset() {
        p.dash = Some(Dash {
            position: f32x(b, d + D_POS),
            speed: f32_at(b, d + D_SPEED),
            power: f32_at(b, d + D_POWER),
            torque: f32_at(b, d + D_TORQUE),
            tire_temp_f: f32x(b, d + D_TIRE_TEMP),
            boost_psi: f32_at(b, d + D_BOOST),
            fuel: f32_at(b, d + D_FUEL),
            distance: f32_at(b, d + D_DISTANCE),
            best_lap: f32_at(b, d + D_BEST),
            last_lap: f32_at(b, d + D_LAST),
            current_lap: f32_at(b, d + D_CURRENT),
            race_time: f32_at(b, d + D_RACE_TIME),
            lap_number: u16::from_le_bytes([b[d + D_LAP], b[d + D_LAP + 1]]),
            race_position: b[d + D_POSITION],
            accel: b[d + D_ACCEL],
            brake: b[d + D_BRAKE],
            clutch: b[d + D_CLUTCH],
            handbrake: b[d + D_HANDBRAKE],
            gear: b[d + D_GEAR],
            steer: b[d + D_STEER] as i8,
        });
    }
    Some((format, p))
}

/// Encodes a packet (used by tests and the `forza-fake` tool).
pub fn encode(format: Format, p: &Packet) -> Vec<u8> {
    let mut b = vec![0u8; format.packet_len()];
    let mut put = |o: usize, bytes: &[u8]| b[o..o + bytes.len()].copy_from_slice(bytes);
    put(IS_RACE_ON, &i32::from(p.is_race_on).to_le_bytes());
    put(TIMESTAMP, &p.timestamp_ms.to_le_bytes());
    put(RPM_MAX, &p.engine_max_rpm.to_le_bytes());
    put(RPM_IDLE, &p.engine_idle_rpm.to_le_bytes());
    put(RPM, &p.current_engine_rpm.to_le_bytes());
    let mut putx = |o: usize, v: &[f32]| {
        for (i, x) in v.iter().enumerate() {
            put(o + 4 * i, &x.to_le_bytes());
        }
    };
    putx(ACCEL, &p.accel);
    putx(VELOCITY, &p.velocity);
    putx(SUSP, &p.susp_travel);
    putx(SLIP_RATIO, &p.tire_slip_ratio);
    putx(COMBINED_SLIP, &p.tire_combined_slip);
    let mut put = |o: usize, bytes: &[u8]| b[o..o + bytes.len()].copy_from_slice(bytes);
    put(CAR_ORDINAL, &p.car_ordinal.to_le_bytes());
    put(CAR_CLASS, &p.car_class.to_le_bytes());
    put(CAR_PI, &p.car_pi.to_le_bytes());
    put(DRIVETRAIN, &p.drivetrain.to_le_bytes());
    put(CYLINDERS, &p.cylinders.to_le_bytes());
    if let (Some(d), Some(dash)) = (format.dash_offset(), &p.dash) {
        let mut putf = |o: usize, v: f32| put(d + o, &v.to_le_bytes());
        for i in 0..3 {
            putf(D_POS + 4 * i, dash.position[i]);
        }
        putf(D_SPEED, dash.speed);
        putf(D_POWER, dash.power);
        putf(D_TORQUE, dash.torque);
        for i in 0..4 {
            putf(D_TIRE_TEMP + 4 * i, dash.tire_temp_f[i]);
        }
        putf(D_BOOST, dash.boost_psi);
        putf(D_FUEL, dash.fuel);
        putf(D_DISTANCE, dash.distance);
        putf(D_BEST, dash.best_lap);
        putf(D_LAST, dash.last_lap);
        putf(D_CURRENT, dash.current_lap);
        putf(D_RACE_TIME, dash.race_time);
        put(d + D_LAP, &dash.lap_number.to_le_bytes());
        put(d + D_POSITION, &[dash.race_position]);
        put(d + D_ACCEL, &[dash.accel]);
        put(d + D_BRAKE, &[dash.brake]);
        put(d + D_CLUTCH, &[dash.clutch]);
        put(d + D_HANDBRAKE, &[dash.handbrake]);
        put(d + D_GEAR, &[dash.gear]);
        put(d + D_STEER, &[dash.steer as u8]);
    }
    b
}

const PSI_TO_KPA: f32 = 6.894_757;

fn f_to_c(f: f32) -> f32 {
    (f - 32.0) * 5.0 / 9.0
}

/// Maps a packet into normalized signals. While the game is paused or in menus
/// (`is_race_on == false`) all values are zero, so only `race_on = 0` is reported and
/// widgets show "no data" instead of dropping to zero.
pub fn to_snapshot(p: &Packet, seq: u64) -> Snapshot {
    let mut s = Snapshot::new();
    s.seq = seq;
    s.set_flag(S::RaceOn, p.is_race_on);
    if !p.is_race_on {
        return s;
    }
    s.set(S::Rpm, p.current_engine_rpm);
    s.set(S::RpmMax, p.engine_max_rpm);
    s.set(S::RpmIdle, p.engine_idle_rpm);
    s.set(S::AccelLat, p.accel[0]);
    s.set(S::AccelVert, p.accel[1]);
    s.set(S::AccelLong, p.accel[2]);
    for (i, id) in [S::SuspFl, S::SuspFr, S::SuspRl, S::SuspRr].into_iter().enumerate() {
        s.set(id, p.susp_travel[i]);
    }
    for (i, id) in [S::SlipFl, S::SlipFr, S::SlipRl, S::SlipRr].into_iter().enumerate() {
        s.set(id, p.tire_combined_slip[i].abs());
    }
    s.set(S::CarId, p.car_ordinal as f32);
    s.set(S::CarClass, p.car_class as f32);
    s.set(S::CarPi, p.car_pi as f32);
    s.set(S::Cylinders, p.cylinders as f32);
    match &p.dash {
        Some(d) => {
            s.set(S::Speed, d.speed);
            s.set(S::Power, d.power);
            s.set(S::Torque, d.torque);
            for (i, id) in [S::TyreTempFl, S::TyreTempFr, S::TyreTempRl, S::TyreTempRr].into_iter().enumerate() {
                s.set(id, f_to_c(d.tire_temp_f[i]));
            }
            s.set(S::Boost, d.boost_psi * PSI_TO_KPA);
            s.set(S::Fuel, d.fuel);
            s.set(S::Distance, d.distance);
            s.set_opt(S::BestLap, (d.best_lap > 0.0).then_some(d.best_lap));
            s.set_opt(S::LastLap, (d.last_lap > 0.0).then_some(d.last_lap));
            s.set(S::LapTime, d.current_lap);
            s.set(S::RaceTime, d.race_time);
            // Forza counts completed laps from 0; show the lap being driven.
            s.set(S::LapNumber, f32::from(d.lap_number) + 1.0);
            s.set_opt(S::RacePosition, (d.race_position > 0).then_some(f32::from(d.race_position)));
            s.set(S::Throttle, f32::from(d.accel) / 255.0);
            s.set(S::Brake, f32::from(d.brake) / 255.0);
            s.set(S::Clutch, f32::from(d.clutch) / 255.0);
            s.set(S::Handbrake, f32::from(d.handbrake) / 255.0);
            s.set(
                S::Gear,
                match d.gear {
                    0 => -1.0,
                    NEUTRAL_GEAR => 0.0,
                    g => f32::from(g),
                },
            );
            s.set(S::Steer, f32::from(d.steer) / 127.0);
            s.set(S::PosX, d.position[0]);
            s.set(S::PosY, d.position[1]);
            s.set(S::PosZ, d.position[2]);
        }
        None => {
            // Sled format has no speed; derive it from the velocity vector.
            let [x, y, z] = p.velocity;
            s.set(S::Speed, (x * x + y * y + z * z).sqrt());
        }
    }
    s
}

/// Builds a packet from a normalized snapshot (inverse of [`to_snapshot`] for the
/// fields Forza has). Used by `forza-fake` to replay the demo car as a game would.
pub fn from_snapshot(s: &Snapshot, timestamp_ms: u32) -> Packet {
    let g = |id| s.get(id).unwrap_or(0.0);
    let u8r = |id| (g(id).clamp(0.0, 1.0) * 255.0).round() as u8;
    let gear = match s.get(S::Gear).map(|v| v.round() as i32) {
        Some(n) if n < 0 => 0,
        Some(0) | None => NEUTRAL_GEAR,
        Some(n) => n.clamp(1, 10) as u8,
    };
    Packet {
        is_race_on: s.get(S::RaceOn).is_none_or(|v| v > 0.5),
        timestamp_ms,
        engine_max_rpm: g(S::RpmMax),
        engine_idle_rpm: g(S::RpmIdle),
        current_engine_rpm: g(S::Rpm),
        accel: [g(S::AccelLat), g(S::AccelVert), g(S::AccelLong)],
        velocity: [0.0, 0.0, g(S::Speed)],
        susp_travel: [g(S::SuspFl), g(S::SuspFr), g(S::SuspRl), g(S::SuspRr)],
        tire_slip_ratio: [g(S::SlipFl), g(S::SlipFr), g(S::SlipRl), g(S::SlipRr)],
        tire_combined_slip: [g(S::SlipFl), g(S::SlipFr), g(S::SlipRl), g(S::SlipRr)],
        car_ordinal: g(S::CarId) as i32,
        car_class: g(S::CarClass) as i32,
        car_pi: g(S::CarPi) as i32,
        drivetrain: 1,
        cylinders: g(S::Cylinders) as i32,
        dash: Some(Dash {
            position: [g(S::PosX), g(S::PosY), g(S::PosZ)],
            speed: g(S::Speed),
            power: g(S::Power),
            torque: g(S::Torque),
            tire_temp_f: [S::TyreTempFl, S::TyreTempFr, S::TyreTempRl, S::TyreTempRr]
                .map(|id| g(id) * 9.0 / 5.0 + 32.0),
            boost_psi: g(S::Boost) / PSI_TO_KPA,
            fuel: g(S::Fuel),
            distance: g(S::Distance),
            best_lap: g(S::BestLap),
            last_lap: g(S::LastLap),
            current_lap: g(S::LapTime),
            race_time: g(S::RaceTime),
            lap_number: (g(S::LapNumber) - 1.0).max(0.0) as u16,
            race_position: g(S::RacePosition) as u8,
            accel: u8r(S::Throttle),
            brake: u8r(S::Brake),
            clutch: u8r(S::Clutch),
            handbrake: u8r(S::Handbrake),
            gear,
            steer: (g(S::Steer).clamp(-1.0, 1.0) * 127.0).round() as i8,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Packet {
        Packet {
            is_race_on: true,
            timestamp_ms: 123_456,
            engine_max_rpm: 8000.0,
            engine_idle_rpm: 900.0,
            current_engine_rpm: 6543.5,
            accel: [3.0, -0.5, 7.25],
            velocity: [1.0, 0.0, 40.0],
            susp_travel: [0.4, 0.5, 0.6, 0.7],
            tire_slip_ratio: [0.01, 0.02, 0.03, 0.04],
            tire_combined_slip: [0.1, -0.2, 0.3, 0.4],
            car_ordinal: 3277,
            car_class: 5,
            car_pi: 800,
            drivetrain: 2,
            cylinders: 8,
            dash: Some(Dash {
                position: [10.0, 20.0, 30.0],
                speed: 40.0125,
                power: 250_000.0,
                torque: 500.0,
                tire_temp_f: [212.0, 176.0, 140.0, 32.0],
                boost_psi: 14.5,
                fuel: 0.75,
                distance: 1234.5,
                best_lap: 81.5,
                last_lap: 0.0,
                current_lap: 12.25,
                race_time: 95.0,
                lap_number: 1,
                race_position: 3,
                accel: 255,
                brake: 0,
                clutch: 51,
                handbrake: 0,
                gear: 4,
                steer: -64,
            }),
        }
    }

    #[test]
    fn round_trips_every_format() {
        for f in [Format::Dash, Format::Horizon, Format::Motorsport2023] {
            let bytes = encode(f, &sample());
            assert_eq!(bytes.len(), f.packet_len());
            let (got_f, p) = parse(&bytes).unwrap();
            assert_eq!(got_f, f);
            assert_eq!(p, sample(), "{f:?}");
        }
        let (f, p) = parse(&encode(Format::Sled, &sample())).unwrap();
        assert_eq!(f, Format::Sled);
        assert!(p.dash.is_none());
        assert_eq!(p.current_engine_rpm, 6543.5);
    }

    #[test]
    fn horizon_dash_block_is_shifted_by_12_bytes() {
        let h = encode(Format::Horizon, &sample());
        let d = encode(Format::Dash, &sample());
        assert_eq!(&h[..232], &d[..232]);
        assert_eq!(&h[244..244 + 79], &d[232..232 + 79]);
        assert!(h[232..244].iter().all(|b| *b == 0));
    }

    #[test]
    fn rejects_unknown_lengths() {
        for len in [0, 10, 231, 233, 310, 325, 400] {
            assert!(parse(&vec![0u8; len]).is_none(), "{len}");
        }
    }

    #[test]
    fn normalizes_units_and_conventions() {
        let s = to_snapshot(&sample(), 7);
        assert_eq!(s.seq, 7);
        assert_eq!(s.get(S::Gear), Some(4.0));
        assert!((s.get(S::TyreTempFl).unwrap() - 100.0).abs() < 1e-3);
        assert!((s.get(S::TyreTempRr).unwrap() - 0.0).abs() < 1e-3);
        assert!((s.get(S::Boost).unwrap() - 99.97).abs() < 0.01);
        assert_eq!(s.get(S::Throttle), Some(1.0));
        assert!((s.get(S::Clutch).unwrap() - 0.2).abs() < 1e-3);
        assert!((s.get(S::Steer).unwrap() + 0.504).abs() < 1e-3);
        assert_eq!(s.get(S::LastLap), None, "0 means no lap yet");
        assert_eq!(s.get(S::BestLap), Some(81.5));
        assert_eq!(s.get(S::LapNumber), Some(2.0));
        assert_eq!(s.get(S::SlipFr), Some(0.2));

        let mut p = sample();
        p.dash.as_mut().unwrap().gear = 0;
        assert_eq!(to_snapshot(&p, 0).get(S::Gear), Some(-1.0));
        p.dash.as_mut().unwrap().gear = NEUTRAL_GEAR;
        assert_eq!(to_snapshot(&p, 0).get(S::Gear), Some(0.0));
    }

    #[test]
    fn paused_reports_only_race_on() {
        let mut p = sample();
        p.is_race_on = false;
        let s = to_snapshot(&p, 0);
        assert_eq!(s.get(S::RaceOn), Some(0.0));
        assert_eq!(s.present().len(), 1);
    }

    #[test]
    fn demo_car_survives_a_round_trip() {
        let mut car = source_demo::DemoCar::new();
        for _ in 0..600 {
            car.step(1.0 / 60.0);
        }
        let original = car.snapshot();
        let bytes = encode(Format::Horizon, &from_snapshot(&original, 10_000));
        let back = to_snapshot(&parse(&bytes).unwrap().1, original.seq);
        for id in [S::Speed, S::Rpm, S::RpmMax, S::Gear, S::Boost, S::TyreTempFl, S::LapTime] {
            let (a, b) = (original.get(id).unwrap(), back.get(id).unwrap());
            assert!((a - b).abs() < 0.01 * a.abs().max(1.0), "{id:?}: {a} vs {b}");
        }
    }
}
