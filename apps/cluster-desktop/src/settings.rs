//! Persistent user settings (`settings.json` next to the dashboards folder).

use cluster_core::UnitPrefs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "forza", "dirt-grid", "f1", "acc", "lmu", "demo" or "hub".
    pub source: String,
    pub forza_port: u16,
    pub dirt_grid_port: u16,
    pub f1_port: u16,
    /// Hub to mirror when `source == "hub"`; empty = find one automatically.
    pub hub_address: String,
    /// Run this app's own hub (phones and other displays).
    pub hub_enabled: bool,
    pub hub_port: u16,
    pub units: UnitPrefs,
    /// Last dashboard shown.
    pub dashboard: String,
    pub start_fullscreen: bool,
    pub always_on_top: bool,
    pub show_status: bool,
    /// The setup guide was shown once.
    pub setup_seen: bool,
    /// Dashboards saved outside the dashboards folder ("Save as" elsewhere).
    pub extra_dashboards: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            source: "forza".into(),
            forza_port: source_forza::DEFAULT_PORT,
            dirt_grid_port: source_dirt_grid::DEFAULT_PORT,
            f1_port: source_f1::DEFAULT_PORT,
            hub_address: String::new(),
            hub_enabled: true,
            hub_port: cluster_net::DEFAULT_PORT,
            units: UnitPrefs::default(),
            dashboard: String::new(),
            start_fullscreen: false,
            always_on_top: false,
            show_status: true,
            setup_seen: false,
            extra_dashboards: Vec::new(),
        }
    }
}

pub fn path() -> PathBuf {
    crate::library::user_dir().parent().map_or_else(|| PathBuf::from("settings.json"), |d| d.join("settings.json"))
}

impl Settings {
    pub const SOURCES: [&'static str; 7] = ["forza", "demo", "hub", "dirt-grid", "f1", "acc", "lmu"];

    pub fn source_index(&self) -> i32 {
        Self::SOURCES.iter().position(|id| *id == self.source).unwrap_or(0) as i32
    }

    pub fn load() -> Settings {
        match std::fs::read_to_string(path()) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("ignoring unreadable settings ({e})");
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self) {
        let p = path();
        let result = p
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&p, serde_json::to_string_pretty(self).expect("serializable")));
        if let Err(e) = result {
            log::warn!("couldn't save settings to {}: {e}", p.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_use_defaults() {
        let s: Settings = serde_json::from_str(r#"{ "forza_port": 6000, "units": { "speed": "mph" } }"#).unwrap();
        assert_eq!(s.forza_port, 6000);
        assert_eq!(s.dirt_grid_port, 20777);
        assert_eq!(s.units.speed, cluster_core::units::SpeedUnit::Mph);
        assert_eq!(s.source, "forza");
        assert!(s.hub_enabled);
    }
}
