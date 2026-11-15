//! Windows x64 layout verified against the game's installed
//! `Support/SharedMemoryInterface/SharedMemoryInterface.hpp` and `InternalsPlugin.hpp`.
//! https://guide.lemansultimate.com/hc/en-gb/articles/14556121957775-V1-2-Update-2

use super::windows::{Mapping, Sample, wide_name};
use cluster_core::{SignalId as S, Snapshot};
use std::io;
use std::sync::atomic::{AtomicI32, Ordering};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, HANDLE};
use windows_sys::Win32::System::Memory::FILE_MAP_WRITE;
use windows_sys::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent};

const SCORING: usize = 1632;
const VEHICLE_COUNT: usize = SCORING + 104;
const SCORING_CARS: usize = SCORING + 560;
const SCORING_STRIDE: usize = 584;
const TELEMETRY: usize = 128_464;
const TELEMETRY_CARS: usize = TELEMETRY + 4;
const TELEMETRY_STRIDE: usize = 1888;
const MAX_CARS: usize = 104;
const MAP_LEN: usize = TELEMETRY_CARS + MAX_CARS * TELEMETRY_STRIDE;

pub(crate) struct Lmu {
    data: Mapping,
    lock: Mapping,
    event: UnlockEvent,
}

impl Lmu {
    pub(crate) fn open() -> io::Result<Option<Self>> {
        let Some(data) = Mapping::read_only("LMU_Data", MAP_LEN)? else { return Ok(None) };
        let Some(lock) = Mapping::open("LMU_SharedMemoryLockData", 8, FILE_MAP_WRITE)? else { return Ok(None) };
        let Some(event) = UnlockEvent::open()? else { return Ok(None) };
        Ok(Some(Self { data, lock, event }))
    }

    pub(crate) fn sample(&self) -> io::Result<Option<Sample>> {
        let Some(guard) = self.try_lock() else { return Ok(None) };
        let mut selection = [0; 3];
        self.data.read(TELEMETRY, &mut selection)?;
        let count = usize::from(selection[0]);
        let index = usize::from(selection[1]);
        if selection[2] != 1 || index >= count || count > MAX_CARS {
            return Ok(None);
        }
        let mut car = [0; TELEMETRY_STRIDE];
        self.data.read(TELEMETRY_CARS + index * TELEMETRY_STRIDE, &mut car)?;
        let mut count = [0; 4];
        self.data.read(VEHICLE_COUNT, &mut count)?;
        let count = i32::from_le_bytes(count).clamp(0, MAX_CARS as i32) as usize;
        let mut all_scores = [0; MAX_CARS * SCORING_STRIDE];
        let scores = &mut all_scores[..count * SCORING_STRIDE];
        if count != 0 {
            self.data.read(SCORING_CARS, scores)?;
        }
        drop(guard);
        let id = int_at(&car, 0);
        let elapsed = double_at(&car, 12);
        if !elapsed.is_finite() || id < 0 {
            return Ok(None);
        }
        let (rows, _) = scores.as_chunks::<SCORING_STRIDE>();
        let score = rows.iter().map(|row| row.as_slice()).find(|row| row[196] == 1 && int_at(row, 0) == id);
        Ok(Some(((elapsed.to_bits(), id), decode(&car, score))))
    }

    #[allow(unsafe_code)]
    fn try_lock(&self) -> Option<Guard<'_>> {
        // SAFETY: this writable mapping holds two aligned Win32 LONGs. The game accesses
        // them with interlocked operations. The guard cannot outlive the mapping or event.
        let (waiters, busy) = unsafe {
            let base = self.lock.view.Value.cast::<i32>();
            (AtomicI32::from_ptr(base), AtomicI32::from_ptr(base.add(1)))
        };
        busy.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Relaxed).ok()?;
        Some(Guard { waiters, busy, event: &self.event })
    }
}

struct Guard<'a> {
    waiters: &'a AtomicI32,
    busy: &'a AtomicI32,
    event: &'a UnlockEvent,
}

impl Drop for Guard<'_> {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        self.busy.swap(0, Ordering::AcqRel);
        if self.waiters.load(Ordering::Acquire) > 0 {
            // SAFETY: the named event is live for the lifetime of the guard.
            unsafe { SetEvent(self.event.0) };
        }
    }
}

struct UnlockEvent(HANDLE);

impl UnlockEvent {
    #[allow(unsafe_code)]
    fn open() -> io::Result<Option<Self>> {
        let name = wide_name("LMU_SharedMemoryLockEvent");
        // SAFETY: the name is a NUL-terminated UTF-16 string and we own any handle returned.
        let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if event.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) { Ok(None) } else { Err(error) };
        }
        Ok(Some(Self(event)))
    }
}

impl Drop for UnlockEvent {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: this object owns the handle and drop runs once.
        unsafe { CloseHandle(self.0) };
    }
}

fn decode(car: &[u8], score: Option<&[u8]>) -> Snapshot {
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
        s.set(S::Distance, double_at(row, 104) as f32);
        s.set_opt(S::BestLap, positive(double_at(row, 144) as f32));
        s.set_opt(S::LastLap, positive(double_at(row, 168) as f32));
        s.set_opt(S::RacePosition, positive(f32::from(row[199])));
    }
    s
}

fn positive(value: f32) -> Option<f32> {
    (value > 0.0).then_some(value)
}

fn int_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn double_at(bytes: &[u8], offset: usize) -> f64 {
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
        score[199] = 4;
        let s = decode(&car, Some(&score));
        assert_eq!(s.get(S::LapTime), Some(3.0));
        assert_eq!(s.get(S::Speed), Some(35.0));
        assert_eq!(s.get(S::AccelLong), Some(1.5));
        assert_eq!(s.get(S::TyreTempFl), Some(80.0));
        assert_eq!(s.get(S::RacePosition), Some(4.0));
        assert_eq!(s.get(S::TyreTempFr), None);
        assert_eq!(decode(&car, None).get(S::RacePosition), None);
    }
}
