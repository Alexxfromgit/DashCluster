//! The on-disk dashboard format (JSON5). This is what users and the visual editor write.
//!
//! Units: numbers in a layout (gauge ranges, zones) are in **metric display units**
//! (km/h, °C, bar, kW, N·m, g) after the binding's `scale`/`offset`. When the user picks
//! other units, ranges are converted and re-rounded automatically.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub name: String,
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Design-space size. Widgets are positioned in these units and the whole canvas is
    /// scaled to fit the screen, keeping its aspect ratio.
    pub canvas: Canvas,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub background: Background,
    pub widgets: Vec<Widget>,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    pub width: f32,
    pub height: f32,
}

/// Named colors widgets can refer to (e.g. `color: "accent"`), so a theme can be
/// re-colored in one place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub ink: String,
    pub muted: String,
    pub accent: String,
    pub danger: String,
    pub warn: String,
    pub ok: String,
    pub info: String,
    pub face: String,
    pub edge: String,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            ink: "#e6edf3".into(),
            muted: "#7d8590".into(),
            accent: "#ff7a1a".into(),
            danger: "#ff3b30".into(),
            warn: "#ffb020".into(),
            ok: "#2fd16b".into(),
            info: "#2f81f7".into(),
            face: "#0d1117".into(),
            edge: "#30363d".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Background {
    pub color: String,
    /// Optional second color for a linear gradient.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gradient_to: Option<String>,
    /// Gradient angle in degrees (0 = top→bottom... as in CSS: 180 = top to bottom).
    pub gradient_angle: f32,
    /// Image path relative to the dashboard file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

impl Default for Background {
    fn default() -> Self {
        Background { color: "#05070a".into(), gradient_to: None, gradient_angle: 180.0, image: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Widget {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub z: i32,
    #[serde(default, skip_serializing_if = "Style::is_default")]
    pub style: Style,
    #[serde(flatten)]
    pub kind: WidgetKind,
}

fn is_zero(v: &i32) -> bool {
    *v == 0
}

/// Per-widget color overrides (theme names or `#rrggbb[aa]`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ink: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub danger: Option<String>,
    /// Multiplies the widget's default text size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_scale: Option<f32>,
}

impl Style {
    pub fn is_default(&self) -> bool {
        *self == Style::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WidgetKind {
    AnalogGauge(AnalogGauge),
    BarGauge(BarGauge),
    Readout(Readout),
    Gear(Gear),
    ShiftLights(ShiftLights),
    Lamp(Lamp),
    TyreQuad(TyreQuad),
    GMeter(GMeter),
    Pedals(Pedals),
    LapTimer(LapTimer),
    Label(Label),
    Image(Image),
}

impl WidgetKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            WidgetKind::AnalogGauge(_) => "analog_gauge",
            WidgetKind::BarGauge(_) => "bar_gauge",
            WidgetKind::Readout(_) => "readout",
            WidgetKind::Gear(_) => "gear",
            WidgetKind::ShiftLights(_) => "shift_lights",
            WidgetKind::Lamp(_) => "lamp",
            WidgetKind::TyreQuad(_) => "tyre_quad",
            WidgetKind::GMeter(_) => "g_meter",
            WidgetKind::Pedals(_) => "pedals",
            WidgetKind::LapTimer(_) => "lap_timer",
            WidgetKind::Label(_) => "label",
            WidgetKind::Image(_) => "image",
        }
    }
}

/// A signal binding. Accepts the short form `"rpm"` or the full object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "BindingRepr")]
pub struct Binding {
    pub signal: String,
    /// Applied after unit conversion: `shown = converted * scale + offset`.
    #[serde(default = "unit_scale", skip_serializing_if = "is_one")]
    pub scale: f32,
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub offset: f32,
    /// Needle smoothing time constant in seconds (0 = none).
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub smoothing: f32,
    /// Round the shown value up to a multiple of this (e.g. tach max to whole 1000s).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ceil_to: Option<f32>,
    /// Convert to the user's display units (default true).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub convert: bool,
    /// Used when the source doesn't provide the signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<f32>,
}

impl Binding {
    pub fn new(signal: &str) -> Self {
        Binding {
            signal: signal.into(),
            scale: 1.0,
            offset: 0.0,
            smoothing: 0.0,
            ceil_to: None,
            convert: true,
            fallback: None,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BindingRepr {
    Name(String),
    Full {
        signal: String,
        #[serde(default = "unit_scale")]
        scale: f32,
        #[serde(default)]
        offset: f32,
        #[serde(default)]
        smoothing: f32,
        #[serde(default)]
        ceil_to: Option<f32>,
        #[serde(default = "yes")]
        convert: bool,
        #[serde(default)]
        fallback: Option<f32>,
    },
}

impl From<BindingRepr> for Binding {
    fn from(r: BindingRepr) -> Self {
        match r {
            BindingRepr::Name(signal) => Binding::new(&signal),
            BindingRepr::Full { signal, scale, offset, smoothing, ceil_to, convert, fallback } => {
                Binding { signal, scale, offset, smoothing, ceil_to, convert, fallback }
            }
        }
    }
}

fn unit_scale() -> f32 {
    1.0
}
fn yes() -> bool {
    true
}
fn is_one(v: &f32) -> bool {
    *v == 1.0
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_zero_f(v: &f32) -> bool {
    *v == 0.0
}

/// A number that is either fixed or read from a signal (e.g. tach max from `rpm_max`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Num {
    Fixed(f32),
    Bound(Binding),
}

impl Default for Num {
    fn default() -> Self {
        Num::Fixed(0.0)
    }
}

impl From<f32> for Num {
    fn from(v: f32) -> Self {
        Num::Fixed(v)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    #[serde(default = "neg_inf_num")]
    pub from: Num,
    #[serde(default = "inf_num")]
    pub to: Num,
    pub color: String,
}

fn neg_inf_num() -> Num {
    Num::Fixed(f32::MIN)
}
fn inf_num() -> Num {
    Num::Fixed(f32::MAX)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatStyle {
    #[default]
    Number,
    /// Seconds as m:ss.mmm
    Time,
    /// R / N / 1..
    Gear,
    /// ON / OFF
    OnOff,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Format {
    pub style: FormatStyle,
    pub decimals: u32,
    /// Unit label; omitted = automatic from the user's units, "" = no unit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalogGauge {
    pub bind: Binding,
    #[serde(default)]
    pub min: Num,
    pub max: Num,
    #[serde(default)]
    pub zones: Vec<Zone>,
    #[serde(default = "d8")]
    pub major_ticks: u32,
    #[serde(default = "d4")]
    pub minor_ticks: u32,
    #[serde(default = "d_start")]
    pub start_angle: f32,
    #[serde(default = "d_sweep")]
    pub sweep: f32,
    #[serde(default)]
    pub caption: String,
    /// Text in the gauge center; defaults to the gauge's own value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<ValueText>,
}

fn d8() -> u32 {
    8
}
fn d4() -> u32 {
    4
}
fn d10() -> u32 {
    10
}
fn d_start() -> f32 {
    -135.0
}
fn d_sweep() -> f32 {
    270.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueText {
    pub bind: Binding,
    #[serde(default)]
    pub format: Format,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BarGauge {
    pub bind: Binding,
    #[serde(default)]
    pub min: Num,
    pub max: Num,
    #[serde(default)]
    pub zones: Vec<Zone>,
    #[serde(default)]
    pub vertical: bool,
    #[serde(default)]
    pub label: String,
    #[serde(default = "yes")]
    pub show_value: bool,
    #[serde(default)]
    pub format: Format,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Readout {
    pub bind: Binding,
    #[serde(default)]
    pub format: Format,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub zones: Vec<Zone>,
    #[serde(default)]
    pub align: Align,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gear {
    #[serde(default = "gear_binding")]
    pub bind: Binding,
    /// Highlight the gear when RPM reaches this (same units as `rpm`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shift_at: Option<Num>,
}

fn gear_binding() -> Binding {
    Binding::new("gear")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShiftLights {
    #[serde(default = "rpm_binding")]
    pub bind: Binding,
    /// First light comes on here.
    pub start: Num,
    /// All lights on and flashing from here.
    pub limit: Num,
    #[serde(default = "d10")]
    pub segments: u32,
    /// One color per segment, or fewer (the last repeats). Default green→yellow→red.
    #[serde(default)]
    pub colors: Vec<String>,
    #[serde(default = "info_name")]
    pub flash_color: String,
}

fn rpm_binding() -> Binding {
    Binding::new("rpm")
}
fn info_name() -> String {
    "info".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lamp {
    pub bind: Binding,
    /// On when value > above (default 0.5, i.e. a flag).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub above: Option<f32>,
    /// On when value < below (overrides `above`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub below: Option<f32>,
    /// Text or symbol shown on the lamp: "ABS", "TC", "◀", "⚠", ...
    pub icon: String,
    #[serde(default = "warn_name")]
    pub color: String,
    #[serde(default)]
    pub blink: bool,
}

fn warn_name() -> String {
    "warn".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TyreQuad {
    /// FL, FR, RL, RR.
    #[serde(default = "tyre_signals")]
    pub signals: [Binding; 4],
    #[serde(default)]
    pub zones: Vec<Zone>,
    #[serde(default)]
    pub format: Format,
    #[serde(default)]
    pub label: String,
}

fn tyre_signals() -> [Binding; 4] {
    ["tyre_temp_fl", "tyre_temp_fr", "tyre_temp_rl", "tyre_temp_rr"].map(Binding::new)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GMeter {
    #[serde(default = "two")]
    pub max_g: f32,
}

fn two() -> f32 {
    2.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pedals {
    /// Up to 4 ratio signals, drawn as vertical bars.
    #[serde(default = "pedal_signals")]
    pub signals: Vec<Binding>,
    /// Bar colors (theme names or hex), matched by position.
    #[serde(default = "pedal_colors")]
    pub colors: Vec<String>,
    #[serde(default = "yes")]
    pub show_labels: bool,
}

fn pedal_signals() -> Vec<Binding> {
    ["throttle", "brake", "clutch"].map(Binding::new).to_vec()
}
fn pedal_colors() -> Vec<String> {
    ["ok", "danger", "info", "warn"].map(String::from).to_vec()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LapTimer {
    #[serde(default = "yes")]
    pub show_last: bool,
    #[serde(default = "yes")]
    pub show_best: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
    /// Text with optional `{signal}` / `{signal:1}` / `{signal:time}` / `{signal:gear}`
    /// placeholders.
    pub text: String,
    #[serde(default)]
    pub align: Align,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFit {
    #[default]
    Contain,
    Cover,
    Fill,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Image {
    /// Path relative to the dashboard file.
    pub src: String,
    #[serde(default)]
    pub fit: ImageFit,
}
