//! Conversion from base units to the user's preferred display units.

use crate::signal::Quantity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeedUnit {
    #[default]
    Kmh,
    Mph,
    Ms,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TempUnit {
    #[default]
    Celsius,
    Fahrenheit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PressureUnit {
    #[default]
    Bar,
    Psi,
    Kpa,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerUnit {
    #[default]
    Kw,
    Hp,
    Ps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TorqueUnit {
    #[default]
    Nm,
    LbFt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccelUnit {
    #[default]
    G,
    Ms2,
}

/// The user's display-unit choices. Distance follows the speed unit (mph → miles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UnitPrefs {
    pub speed: SpeedUnit,
    pub temperature: TempUnit,
    pub pressure: PressureUnit,
    pub power: PowerUnit,
    pub torque: TorqueUnit,
    pub acceleration: AccelUnit,
}

pub const STANDARD_GRAVITY: f32 = 9.806_65;

impl UnitPrefs {
    pub fn imperial() -> Self {
        UnitPrefs {
            speed: SpeedUnit::Mph,
            temperature: TempUnit::Fahrenheit,
            pressure: PressureUnit::Psi,
            power: PowerUnit::Hp,
            torque: TorqueUnit::LbFt,
            acceleration: AccelUnit::G,
        }
    }

    /// Converts a base-unit value to the display unit for `q`.
    pub fn convert(&self, q: Quantity, v: f32) -> f32 {
        match q {
            Quantity::Speed => match self.speed {
                SpeedUnit::Kmh => v * 3.6,
                SpeedUnit::Mph => v * 2.236_936,
                SpeedUnit::Ms => v,
            },
            Quantity::Temperature => match self.temperature {
                TempUnit::Celsius => v,
                TempUnit::Fahrenheit => v * 9.0 / 5.0 + 32.0,
            },
            Quantity::Pressure => match self.pressure {
                PressureUnit::Bar => v / 100.0,
                PressureUnit::Psi => v * 0.145_037_74,
                PressureUnit::Kpa => v,
            },
            Quantity::Power => match self.power {
                PowerUnit::Kw => v / 1000.0,
                PowerUnit::Hp => v / 745.699_9,
                PowerUnit::Ps => v / 735.498_8,
            },
            Quantity::Torque => match self.torque {
                TorqueUnit::Nm => v,
                TorqueUnit::LbFt => v * 0.737_562_1,
            },
            Quantity::Acceleration => match self.acceleration {
                AccelUnit::G => v / STANDARD_GRAVITY,
                AccelUnit::Ms2 => v,
            },
            Quantity::Distance => match self.speed {
                SpeedUnit::Mph => v / 1609.344,
                _ => v / 1000.0,
            },
            Quantity::Ratio | Quantity::SignedRatio => v * 100.0,
            Quantity::EngineSpeed
            | Quantity::Duration
            | Quantity::Gear
            | Quantity::Flag
            | Quantity::Count => v,
        }
    }

    /// Unit label shown next to a converted value ("" for unitless quantities).
    pub fn unit_label(&self, q: Quantity) -> &'static str {
        match q {
            Quantity::Speed => match self.speed {
                SpeedUnit::Kmh => "km/h",
                SpeedUnit::Mph => "mph",
                SpeedUnit::Ms => "m/s",
            },
            Quantity::Temperature => match self.temperature {
                TempUnit::Celsius => "°C",
                TempUnit::Fahrenheit => "°F",
            },
            Quantity::Pressure => match self.pressure {
                PressureUnit::Bar => "bar",
                PressureUnit::Psi => "psi",
                PressureUnit::Kpa => "kPa",
            },
            Quantity::Power => match self.power {
                PowerUnit::Kw => "kW",
                PowerUnit::Hp => "hp",
                PowerUnit::Ps => "PS",
            },
            Quantity::Torque => match self.torque {
                TorqueUnit::Nm => "N·m",
                TorqueUnit::LbFt => "lb·ft",
            },
            Quantity::Acceleration => match self.acceleration {
                AccelUnit::G => "g",
                AccelUnit::Ms2 => "m/s²",
            },
            Quantity::Distance => match self.speed {
                SpeedUnit::Mph => "mi",
                _ => "km",
            },
            Quantity::Ratio | Quantity::SignedRatio => "%",
            Quantity::EngineSpeed => "rpm",
            Quantity::Duration | Quantity::Gear | Quantity::Flag | Quantity::Count => "",
        }
    }
}

/// Formats seconds as `m:ss.mmm` (or `h:mm:ss.mmm` past an hour).
pub fn format_lap_time(seconds: f32) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "-:--.---".to_owned();
    }
    let total_ms = (f64::from(seconds) * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let s = (total_ms / 1000) % 60;
    let m = (total_ms / 60_000) % 60;
    let h = total_ms / 3_600_000;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{ms:03}")
    } else {
        format!("{m}:{s:02}.{ms:03}")
    }
}

/// Formats a gear value: -1 → "R", 0 → "N", n → "n".
pub fn format_gear(gear: f32) -> String {
    match gear.round() as i32 {
        g if g < 0 => "R".to_owned(),
        0 => "N".to_owned(),
        g => g.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3 * b.abs().max(1.0)
    }

    #[test]
    fn conversions() {
        let m = UnitPrefs::default();
        let i = UnitPrefs::imperial();
        assert!(close(m.convert(Quantity::Speed, 27.777_779), 100.0));
        assert!(close(i.convert(Quantity::Speed, 26.8224), 60.0));
        assert!(close(i.convert(Quantity::Temperature, 100.0), 212.0));
        assert!(close(m.convert(Quantity::Pressure, 150.0), 1.5));
        assert!(close(i.convert(Quantity::Pressure, 100.0), 14.5038));
        assert!(close(i.convert(Quantity::Power, 745.6999), 1.0));
        assert!(close(
            m.convert(Quantity::Acceleration, STANDARD_GRAVITY),
            1.0
        ));
        assert!(close(m.convert(Quantity::Ratio, 0.42), 42.0));
        assert!(close(i.convert(Quantity::Distance, 1609.344), 1.0));
    }

    #[test]
    fn lap_time_and_gear_format() {
        assert_eq!(format_lap_time(83.4567), "1:23.457");
        assert_eq!(format_lap_time(3725.5), "1:02:05.500");
        assert_eq!(format_lap_time(-1.0), "-:--.---");
        assert_eq!(format_gear(-1.0), "R");
        assert_eq!(format_gear(0.0), "N");
        assert_eq!(format_gear(6.0), "6");
    }
}
