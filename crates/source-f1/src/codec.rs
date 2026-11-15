//! Packed little-endian layouts from the yearly Codemasters UDP specifications.
//! https://f1-2019-telemetry.readthedocs.io/en/latest/telemetry-specification.html
//! https://f1-2020-telemetry.readthedocs.io/en/latest/telemetry-specification.html
//! https://github.com/hotlaps/f1-game-udp-specs
//! https://forums.ea.com/blog/f1-games-game-info-hub-en/ea-sports%E2%84%A2-f1%C2%AE25-2026-season-pack-udp-specification/12187347

use cluster_core::{SignalId as S, Snapshot};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

const LAP: u8 = 2;
const TELEMETRY: u8 = 6;
const STATUS: u8 = 7;
const MAX_SAMPLE_AGE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
struct PacketLayout {
    stride: usize,
    tail: usize,
}

const fn layout(stride: usize, tail: usize) -> PacketLayout {
    PacketLayout { stride, tail }
}

struct Format {
    header: usize,
    cars: usize,
    telemetry: PacketLayout,
    status: PacketLayout,
    lap: PacketLayout,
    lap_distance_offset: usize,
}

const FORMATS: [Format; 7] = [
    Format {
        header: 23,
        cars: 20,
        telemetry: layout(66, 4),
        status: layout(56, 0),
        lap: layout(41, 0),
        lap_distance_offset: 20,
    }, // 2019
    Format {
        header: 24,
        cars: 22,
        telemetry: layout(58, 7),
        status: layout(60, 0),
        lap: layout(53, 0),
        lap_distance_offset: 32,
    }, // 2020
    Format {
        header: 24,
        cars: 22,
        telemetry: layout(60, 3),
        status: layout(47, 0),
        lap: layout(43, 0),
        lap_distance_offset: 12,
    }, // 2021
    Format {
        header: 24,
        cars: 22,
        telemetry: layout(60, 3),
        status: layout(47, 0),
        lap: layout(43, 2),
        lap_distance_offset: 12,
    }, // 2022
    Format {
        header: 29,
        cars: 22,
        telemetry: layout(60, 3),
        status: layout(55, 0),
        lap: layout(50, 2),
        lap_distance_offset: 18,
    }, // 2023
    Format {
        header: 29,
        cars: 22,
        telemetry: layout(60, 3),
        status: layout(55, 0),
        lap: layout(57, 2),
        lap_distance_offset: 20,
    }, // 2024
    Format {
        header: 29,
        cars: 22,
        telemetry: layout(60, 3),
        status: layout(55, 0),
        lap: layout(57, 2),
        lap_distance_offset: 20,
    }, // 2025
];

#[derive(Default)]
pub(crate) struct Decoder {
    session: Option<(SocketAddr, u16, u64, u8)>,
    lap: Option<(Instant, Snapshot)>,
    status: Option<(Instant, Snapshot)>,
}

impl Decoder {
    pub(crate) fn decode(&mut self, packet: &[u8], sender: SocketAddr) -> Result<Option<Snapshot>, ()> {
        let year = u16::from_le_bytes(packet.get(..2).ok_or(())?.try_into().unwrap());
        let format = FORMATS.get(usize::from(year.checked_sub(2019).ok_or(())?)).ok_or(())?;
        let header = packet.get(..format.header).ok_or(())?;
        let id_offset = if year < 2023 { 5 } else { 6 };
        let kind = header[id_offset];
        let layout = match kind {
            LAP => format.lap,
            TELEMETRY => format.telemetry,
            STATUS => format.status,
            _ => return Ok(None),
        };
        let player = header[if year < 2023 { 22 } else { 27 }];
        // The game sends 255 when the player has no assigned car.
        if player == 255 {
            return Ok(None);
        }
        if header[id_offset - 1] != 1
            || usize::from(player) >= format.cars
            || packet.len() != format.header + format.cars * layout.stride + layout.tail
        {
            return Err(());
        }
        let uid = u64::from_le_bytes(header[id_offset + 1..id_offset + 9].try_into().unwrap());
        let session = (sender, year, uid, player);
        if self.session != Some(session) {
            *self = Self { session: Some(session), ..Self::default() };
        }
        let offset = format.header + usize::from(player) * layout.stride;
        let car = &packet[offset..offset + layout.stride];
        let now = Instant::now();
        match kind {
            LAP => self.lap = Some((now, read_lap(car, year, format.lap_distance_offset))),
            STATUS => self.status = Some((now, read_status(car))),
            TELEMETRY => {
                let mut live = read_telemetry(car, year);
                live.set(S::RaceTime, float_at(header, id_offset + 9));
                for (at, data) in [self.status.as_ref(), self.lap.as_ref()].into_iter().flatten() {
                    if now.duration_since(*at) < MAX_SAMPLE_AGE {
                        for (id, value) in data.iter() {
                            live.set(id, value);
                        }
                    }
                }
                return Ok(Some(live));
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}

fn read_telemetry(car: &[u8], year: u16) -> Snapshot {
    let mut snapshot = Snapshot::new();
    snapshot.set(S::Speed, f32::from(short_at(car, 0)) / 3.6);
    for (id, offset, min) in [(S::Throttle, 2, 0.0), (S::Steer, 6, -1.0), (S::Brake, 10, 0.0)] {
        let value = float_at(car, offset);
        snapshot.set_opt(id, (min..=1.0).contains(&value).then_some(value));
    }
    let clutch = car[14];
    snapshot.set_opt(S::Clutch, (clutch <= 100).then_some(f32::from(clutch) / 100.0));
    let gear = car[15] as i8;
    snapshot.set_opt(S::Gear, (-1..=8).contains(&gear).then_some(f32::from(gear)));
    snapshot.set(S::Rpm, f32::from(short_at(car, 16)));
    // The four-wheel order throughout the F1 wire format is RL, RR, FL, FR.
    for (wheel, id) in [S::TyreTempRl, S::TyreTempRr, S::TyreTempFl, S::TyreTempFr].into_iter().enumerate() {
        let temp = match year {
            2019 => f32::from(short_at(car, 28 + 2 * wheel)),
            2020 => f32::from(car[28 + wheel]),
            _ => f32::from(car[30 + wheel]),
        };
        snapshot.set_opt(id, positive(temp));
    }
    snapshot
}

fn read_status(car: &[u8]) -> Snapshot {
    let mut snapshot = Snapshot::new();
    snapshot.set_flag(S::PitLimiter, car[4] != 0);
    let fuel = float_at(car, 5) / float_at(car, 9);
    snapshot.set_opt(S::Fuel, (float_at(car, 9) > 0.0 && (0.0..=1.0).contains(&fuel)).then_some(fuel));
    snapshot.set_opt(S::RpmMax, positive(f32::from(short_at(car, 17))));
    snapshot.set_opt(S::RpmIdle, positive(f32::from(short_at(car, 19))));
    snapshot
}

fn read_lap(car: &[u8], year: u16, distance: usize) -> Snapshot {
    let mut snapshot = Snapshot::new();
    let seconds = |offset| {
        if year <= 2020 {
            float_at(car, offset)
        } else {
            u32::from_le_bytes(car[offset..offset + 4].try_into().unwrap()) as f32 / 1000.0
        }
    };
    snapshot.set_opt(S::LastLap, positive(seconds(0)));
    snapshot.set(S::LapTime, seconds(4));
    if year <= 2020 {
        snapshot.set_opt(S::BestLap, positive(seconds(if year == 2019 { 8 } else { 12 })));
    }
    snapshot.set(S::Distance, float_at(car, distance));
    snapshot.set_opt(S::RacePosition, positive(f32::from(car[distance + 12])));
    snapshot.set_opt(S::LapNumber, positive(f32::from(car[distance + 13])));
    snapshot
}

fn positive(value: f32) -> Option<f32> {
    (value > 0.0).then_some(value)
}

fn short_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn float_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(year: u16, id: u8, total: usize) -> Vec<u8> {
        let mut packet = vec![0; total];
        packet[..2].copy_from_slice(&year.to_le_bytes());
        let offset = if year < 2023 { 5 } else { 6 };
        packet[offset - 1] = 1;
        packet[offset] = id;
        packet[if year < 2023 { 22 } else { 27 }] = 1;
        packet
    }

    #[test]
    fn decodes_player_telemetry_in_each_supported_format() {
        let sender = "127.0.0.1:1234".parse().unwrap();
        for (year, len, car) in [
            (2019, 1347, 89),
            (2020, 1307, 82),
            (2021, 1347, 84),
            (2022, 1347, 84),
            (2023, 1352, 89),
            (2024, 1352, 89),
            (2025, 1352, 89),
        ] {
            let mut data = packet(year, TELEMETRY, len);
            data[car..car + 2].copy_from_slice(&180_u16.to_le_bytes());
            data[car + 15] = 4;
            data[car + 16..car + 18].copy_from_slice(&9500_u16.to_le_bytes());
            let mut decoder = Decoder::default();
            let s = decoder.decode(&data, sender).unwrap().unwrap();
            assert_eq!(s.get(S::Speed), Some(50.0), "F1 {year}");
            assert_eq!(s.get(S::Gear), Some(4.0), "F1 {year}");
            assert_eq!(s.get(S::Rpm), Some(9500.0), "F1 {year}");
            assert!(decoder.decode(&data[..len - 1], sender).is_err());
        }
    }

    #[test]
    fn combines_lap_and_status_only_for_the_current_player_session() {
        let mut decoder = Decoder::default();
        let sender = "127.0.0.1:1234".parse().unwrap();
        let mut status = packet(2025, STATUS, 1239);
        let car = 29 + 55;
        status[car + 17..car + 19].copy_from_slice(&12000_u16.to_le_bytes());
        assert!(decoder.decode(&status, sender).unwrap().is_none());
        let mut lap = packet(2025, LAP, 1285);
        let car = 29 + 57;
        lap[car + 4..car + 8].copy_from_slice(&1500_u32.to_le_bytes());
        lap[car + 32] = 3;
        assert!(decoder.decode(&lap, sender).unwrap().is_none());
        let mut data = packet(2025, TELEMETRY, 1352);
        let s = decoder.decode(&data, sender).unwrap().unwrap();
        assert_eq!(s.get(S::LapTime), Some(1.5));
        assert_eq!(s.get(S::RpmMax), Some(12000.0));
        assert_eq!(s.get(S::RacePosition), Some(3.0));
        data[7] = 1;
        let s = decoder.decode(&data, sender).unwrap().unwrap();
        assert_eq!(s.get(S::LapTime), None);
        assert_eq!(s.get(S::RpmMax), None);
        data[27] = 255;
        assert!(decoder.decode(&data, sender).unwrap().is_none());
    }
}
