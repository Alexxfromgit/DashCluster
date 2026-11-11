//! The signal catalog. Every source maps its data into these IDs, and every dashboard
//! widget binds to one of them by its stable string name (e.g. `"rpm"`).

use serde::{Deserialize, Serialize};

/// Physical quantity of a signal. Values are stored in the base unit listed here and
/// converted to the user's preferred unit only for display (see [`crate::units`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Quantity {
    /// m/s
    Speed,
    /// revolutions per minute
    EngineSpeed,
    /// °C
    Temperature,
    /// kPa (gauge pressure for boost)
    Pressure,
    /// 0..1, shown as percent
    Ratio,
    /// -1..1 (e.g. steering)
    SignedRatio,
    /// W
    Power,
    /// N·m
    Torque,
    /// m/s², shown as g by default
    Acceleration,
    /// m
    Distance,
    /// seconds, shown as m:ss.mmm
    Duration,
    /// -1 = reverse, 0 = neutral, 1.. = forward gears
    Gear,
    /// 0 or 1
    Flag,
    /// a plain number with no unit (lap number, position, IDs)
    Count,
}

macro_rules! signals {
    ($( $(#[$doc:meta])* $variant:ident => $name:literal, $label:literal, $q:ident; )*) => {
        /// Identifier of a normalized vehicle signal.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u8)]
        pub enum SignalId { $( $(#[$doc])* $variant, )* }

        impl SignalId {
            /// Every signal, in ID order.
            pub const ALL: &'static [SignalId] = &[ $( SignalId::$variant, )* ];
            /// Number of signals in the catalog.
            pub const COUNT: usize = Self::ALL.len();

            /// Stable name used in dashboard files and the network protocol.
            pub const fn name(self) -> &'static str {
                match self { $( SignalId::$variant => $name, )* }
            }

            /// Human-readable label for editors.
            pub const fn label(self) -> &'static str {
                match self { $( SignalId::$variant => $label, )* }
            }

            pub const fn quantity(self) -> Quantity {
                match self { $( SignalId::$variant => Quantity::$q, )* }
            }

            pub fn from_name(name: &str) -> Option<SignalId> {
                match name { $( $name => Some(SignalId::$variant), )* _ => None }
            }

            pub const fn index(self) -> usize { self as usize }

            pub fn from_index(i: usize) -> Option<SignalId> { Self::ALL.get(i).copied() }
        }
    };
}

signals! {
    // Motion and engine
    Speed => "speed", "Speed", Speed;
    Rpm => "rpm", "Engine RPM", EngineSpeed;
    RpmMax => "rpm_max", "Max RPM", EngineSpeed;
    RpmIdle => "rpm_idle", "Idle RPM", EngineSpeed;
    Gear => "gear", "Gear", Gear;
    Power => "power", "Power", Power;
    Torque => "torque", "Torque", Torque;
    Boost => "boost", "Boost", Pressure;
    // Driver inputs
    Throttle => "throttle", "Throttle", Ratio;
    Brake => "brake", "Brake", Ratio;
    Clutch => "clutch", "Clutch", Ratio;
    Handbrake => "handbrake", "Handbrake", Ratio;
    Steer => "steer", "Steering", SignedRatio;
    // Fluids and temperatures
    Fuel => "fuel", "Fuel level", Ratio;
    CoolantTemp => "coolant_temp", "Coolant temp", Temperature;
    OilTemp => "oil_temp", "Oil temp", Temperature;
    TyreTempFl => "tyre_temp_fl", "Tyre temp FL", Temperature;
    TyreTempFr => "tyre_temp_fr", "Tyre temp FR", Temperature;
    TyreTempRl => "tyre_temp_rl", "Tyre temp RL", Temperature;
    TyreTempRr => "tyre_temp_rr", "Tyre temp RR", Temperature;
    // Chassis
    SuspFl => "susp_fl", "Suspension FL", Ratio;
    SuspFr => "susp_fr", "Suspension FR", Ratio;
    SuspRl => "susp_rl", "Suspension RL", Ratio;
    SuspRr => "susp_rr", "Suspension RR", Ratio;
    SlipFl => "slip_fl", "Tyre slip FL", Ratio;
    SlipFr => "slip_fr", "Tyre slip FR", Ratio;
    SlipRl => "slip_rl", "Tyre slip RL", Ratio;
    SlipRr => "slip_rr", "Tyre slip RR", Ratio;
    /// Lateral acceleration, positive to the right.
    AccelLat => "accel_lat", "Lateral accel", Acceleration;
    /// Longitudinal acceleration, positive forward.
    AccelLong => "accel_long", "Longitudinal accel", Acceleration;
    AccelVert => "accel_vert", "Vertical accel", Acceleration;
    // Session
    Distance => "distance", "Distance", Distance;
    LapTime => "lap_time", "Current lap", Duration;
    LastLap => "last_lap", "Last lap", Duration;
    BestLap => "best_lap", "Best lap", Duration;
    LapNumber => "lap_number", "Lap", Count;
    RacePosition => "race_position", "Position", Count;
    RaceTime => "race_time", "Race time", Duration;
    RaceOn => "race_on", "Driving", Flag;
    // Warning lamps (sources that can't provide these simply leave them empty)
    EngineWarning => "engine_warning", "Engine warning", Flag;
    AbsActive => "abs_active", "ABS active", Flag;
    TcActive => "tc_active", "Traction control", Flag;
    Lights => "lights", "Headlights", Flag;
    BlinkerLeft => "blinker_left", "Left indicator", Flag;
    BlinkerRight => "blinker_right", "Right indicator", Flag;
    PitLimiter => "pit_limiter", "Pit limiter", Flag;
    // Car identity (for per-car layouts later)
    CarId => "car_id", "Car ID", Count;
    CarClass => "car_class", "Car class", Count;
    CarPi => "car_pi", "Performance index", Count;
    Cylinders => "cylinders", "Cylinders", Count;
    // World position (for maps later)
    PosX => "pos_x", "Position X", Distance;
    PosY => "pos_y", "Position Y", Distance;
    PosZ => "pos_z", "Position Z", Distance;
}

/// A set of signals, used for source capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SignalSet(u128);

const _: () = assert!(SignalId::COUNT <= 128, "SignalSet holds at most 128 signals");

impl SignalSet {
    pub const EMPTY: SignalSet = SignalSet(0);

    pub fn all() -> SignalSet {
        SignalSet(if SignalId::COUNT == 128 { u128::MAX } else { (1u128 << SignalId::COUNT) - 1 })
    }

    pub fn of(ids: &[SignalId]) -> SignalSet {
        let mut s = SignalSet::EMPTY;
        for &id in ids {
            s.insert(id);
        }
        s
    }

    pub fn insert(&mut self, id: SignalId) {
        self.0 |= 1u128 << id.index();
    }

    pub fn contains(self, id: SignalId) -> bool {
        self.0 & (1u128 << id.index()) != 0
    }

    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = SignalId> {
        SignalId::ALL.iter().copied().filter(move |&id| self.contains(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for &id in SignalId::ALL {
            assert_eq!(SignalId::from_name(id.name()), Some(id));
            assert_eq!(SignalId::from_index(id.index()), Some(id));
            assert!(seen.insert(id.name()), "duplicate name {}", id.name());
        }
        assert_eq!(SignalId::from_name("nope"), None);
    }

    #[test]
    fn signal_set_ops() {
        let s = SignalSet::of(&[SignalId::Rpm, SignalId::Speed]);
        assert!(s.contains(SignalId::Rpm));
        assert!(!s.contains(SignalId::Gear));
        assert_eq!(s.len(), 2);
        assert_eq!(SignalSet::all().len(), SignalId::COUNT);
    }
}
