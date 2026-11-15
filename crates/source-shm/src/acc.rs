//! Kunos ACC shared-memory reference (V1.8). The fields used here are in the
//! Windows C layout with four-byte alignment and two-byte wide characters.
//! https://www.assettocorsamods.net/threads/doc-shared-memory-reference-acc.3061/

use super::windows::{Mapping, Sample};
use cluster_core::{SignalId as S, Snapshot};
use std::io;

pub(crate) struct Acc {
    physics: Mapping,
    graphics: Mapping,
    car: Option<Mapping>,
}

const PHYSICS_PREFIX: usize = 252;
const GRAPHICS_PREFIX: usize = 152;
const STATIC_PREFIX: usize = 420;

impl Acc {
    pub(crate) fn open() -> io::Result<Option<Self>> {
        let Some(physics) = Mapping::read_only("Local\\acpmf_physics", PHYSICS_PREFIX)? else { return Ok(None) };
        let Some(graphics) = Mapping::read_only("Local\\acpmf_graphics", GRAPHICS_PREFIX)? else { return Ok(None) };
        let car = Mapping::read_only("Local\\acpmf_static", STATIC_PREFIX)?;
        Ok(Some(Self { physics, graphics, car }))
    }

    pub(crate) fn sample(&self) -> io::Result<Option<Sample>> {
        let mut graphics = [0; GRAPHICS_PREFIX];
        if !stable_read(&self.graphics, &mut graphics)? || int_at(&graphics, 4) != 2 {
            return Ok(None);
        }
        let mut physics = [0; PHYSICS_PREFIX];
        if !stable_read(&self.physics, &mut physics)? {
            return Ok(None);
        }
        let mut car = [0; STATIC_PREFIX];
        if let Some(map) = &self.car {
            map.read(0, &mut car)?;
        }
        Ok(Some(((u64::from(int_at(&physics, 0) as u32), 0), decode(&physics, &graphics, &car))))
    }
}

fn stable_read(mapping: &Mapping, bytes: &mut [u8]) -> io::Result<bool> {
    mapping.read(0, bytes)?;
    let mut id = [0; 4];
    mapping.read(0, &mut id)?;
    Ok(id == bytes[..4])
}

fn decode(physics: &[u8], graphics: &[u8], car: &[u8]) -> Snapshot {
    let mut s = Snapshot::new();
    let floats = |offset| f32::from_le_bytes(physics[offset..offset + 4].try_into().unwrap());
    s.set_flag(S::RaceOn, true);
    s.set(S::Speed, floats(28) / 3.6);
    s.set(S::Gear, int_at(physics, 16) as f32 - 1.0);
    s.set(S::Rpm, int_at(physics, 20) as f32);
    s.set(S::Throttle, floats(4));
    s.set(S::Brake, floats(8));
    s.set(S::Steer, floats(24));
    for (offset, id) in [(152, S::TyreTempFl), (156, S::TyreTempFr), (160, S::TyreTempRl), (164, S::TyreTempRr)] {
        let temp = floats(offset);
        s.set_opt(id, (temp > 0.0).then_some(temp));
    }
    s.set_flag(S::PitLimiter, int_at(physics, 248) != 0);
    let completed_laps = int_at(graphics, 132);
    s.set_opt(S::LapNumber, (completed_laps >= 0).then_some(completed_laps as f32 + 1.0));
    let position = int_at(graphics, 136);
    s.set_opt(S::RacePosition, (position > 0).then_some(position as f32));
    for (offset, id) in [(140, S::LapTime), (144, S::LastLap), (148, S::BestLap)] {
        let ms = int_at(graphics, offset);
        s.set_opt(id, (ms > 0).then_some(ms as f32 / 1000.0));
    }
    let max_rpm = int_at(car, 412);
    s.set_opt(S::RpmMax, (max_rpm > 0).then_some(max_rpm as f32));
    let max_fuel = f32::from_le_bytes(car[416..420].try_into().unwrap());
    let fuel = floats(12) / max_fuel;
    s.set_opt(S::Fuel, (max_fuel > 0.0 && (0.0..=1.0).contains(&fuel)).then_some(fuel));
    s
}

fn int_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_acc_gear_speed_lap_and_fuel() {
        let mut physics = [0; PHYSICS_PREFIX];
        let mut graphics = [0; GRAPHICS_PREFIX];
        let mut car = [0; STATIC_PREFIX];
        physics[12..16].copy_from_slice(&20.0_f32.to_le_bytes());
        physics[28..32].copy_from_slice(&108.0_f32.to_le_bytes());
        graphics[140..144].copy_from_slice(&6250_i32.to_le_bytes());
        car[412..416].copy_from_slice(&8000_i32.to_le_bytes());
        car[416..420].copy_from_slice(&50.0_f32.to_le_bytes());
        let s = decode(&physics, &graphics, &car);
        assert_eq!(s.get(S::Gear), Some(-1.0));
        assert_eq!(s.get(S::Speed), Some(30.0));
        assert_eq!(s.get(S::LapTime), Some(6.25));
        assert_eq!(s.get(S::RpmMax), Some(8000.0));
        assert_eq!(s.get(S::Fuel), Some(0.4));
        assert_eq!(s.get(S::LastLap), None);
    }
}
