//! Dashboard layouts: the JSON5 file format ([`model`]), validation ([`compile`]) and
//! per-frame evaluation ([`eval`]). No UI dependencies, so it's shared by the desktop
//! app, the web display and the hub.

pub mod compile;
pub mod eval;
pub mod model;

pub use compile::{Compiled, LayoutError, Problem, Rgba, compile};
pub use eval::{Evaluator, Slot, WidgetFrame, WidgetStatic, WidgetType};
pub use model::Layout;

/// Built-in sample dashboards: (file name, JSON5 source).
pub const BUILTIN: &[(&str, &str)] = &[
    (
        "grand-tourer.json5",
        include_str!("../../../dashboards/grand-tourer.json5"),
    ),
    ("race.json5", include_str!("../../../dashboards/race.json5")),
    (
        "rally.json5",
        include_str!("../../../dashboards/rally.json5"),
    ),
    (
        "phone-portrait.json5",
        include_str!("../../../dashboards/phone-portrait.json5"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_dashboards_compile() {
        for (name, src) in BUILTIN {
            if let Err(e) = Compiled::from_json5(src) {
                panic!("{name}: {e}");
            }
        }
    }

    #[test]
    fn reports_helpful_errors() {
        let src = r##"{ name: "bad", canvas: { width: 100, height: 100 }, widgets: [
            { id: "a", type: "readout", x: 0, y: 0, w: 10, h: 10, bind: "spede" },
            { id: "a", type: "lamp", x: 0, y: 0, w: 0, h: 10, bind: "abs_active", icon: "ABS", color: "pink" },
        ] }"##;
        let err = Compiled::from_json5(src).unwrap_err().to_string();
        assert!(
            err.contains(
                "widgets[0] \"a\" › bind: unknown signal \"spede\" — did you mean \"speed\"?"
            ),
            "{err}"
        );
        assert!(err.contains("duplicate widget id"), "{err}");
        assert!(err.contains("width and height must be positive"), "{err}");
        assert!(err.contains("unknown color \"pink\""), "{err}");
    }

    #[test]
    fn parse_errors_are_reported() {
        let err = Compiled::from_json5("{ name: ").unwrap_err();
        assert!(matches!(err, LayoutError::Parse(_)));
        let err = Compiled::from_json5(r#"{ name: "x", canvas: {width: 1, height: 1}, widgets: [ { type: "warp_drive", x: 0, y: 0, w: 1, h: 1 } ] }"#).unwrap_err();
        assert!(err.to_string().contains("warp_drive"), "{err}");
    }

    #[test]
    fn layouts_round_trip_through_json() {
        for (name, src) in BUILTIN {
            let a: Layout = json5::from_str(src).unwrap();
            let json = serde_json::to_string_pretty(&a).unwrap();
            let b: Layout = json5::from_str(&json).unwrap();
            assert_eq!(a, b, "{name}");
        }
    }
}
