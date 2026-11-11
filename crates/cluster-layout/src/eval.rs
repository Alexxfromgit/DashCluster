//! Per-frame evaluation: turns a [`Snapshot`] into what each widget should show.
//!
//! Two outputs, so the UI can update cheaply:
//! - [`WidgetStatic`]: geometry, ranges, colors, labels. Changes rarely (layout edits,
//!   unit changes, or a dynamic range like `rpm_max` changing when you switch cars).
//! - [`WidgetFrame`]: up to four value/text/color slots plus an on/off flag. Every frame.

use crate::compile::{CBinding, CNum, CWidget, Compiled, Rgba, WidgetColors};
use crate::model::{Align, Format, FormatStyle, ImageFit, WidgetKind};
use cluster_core::units::{STANDARD_GRAVITY, format_gear, format_lap_time};
use cluster_core::{Quantity, SignalId, Snapshot, UnitPrefs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetType {
    AnalogGauge,
    BarGauge,
    Readout,
    Gear,
    ShiftLights,
    Lamp,
    TyreQuad,
    GMeter,
    Pedals,
    LapTimer,
    Label,
    Image,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WidgetStatic {
    pub id: String,
    pub kind: WidgetType,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub z: i32,
    pub colors: WidgetColors,
    pub font_scale: f32,
    pub min: f32,
    pub max: f32,
    pub major_ticks: u32,
    pub minor_ticks: u32,
    pub start_angle: f32,
    pub sweep: f32,
    /// (from, to, color) in shown units.
    pub zones: Vec<(f32, f32, Rgba)>,
    pub caption: String,
    pub label: String,
    pub unit: String,
    pub align: Align,
    pub vertical: bool,
    pub show_value: bool,
    /// Shift-light segment colors, or pedal bar colors.
    pub segment_colors: Vec<Rgba>,
    /// Shift-light flash color, lamp color, or label color.
    pub color: Rgba,
    /// Lamp icon text.
    pub icon: String,
    /// Pedal labels.
    pub names: Vec<String>,
    pub image: String,
    pub image_fit: ImageFit,
    pub max_g: f32,
    pub show_last: bool,
    pub show_best: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slot {
    pub value: f32,
    pub text: String,
    pub color: Rgba,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WidgetFrame {
    /// False when the source doesn't provide the widget's main signal.
    pub valid: bool,
    pub on: bool,
    pub slots: [Slot; 4],
}

// ---------------------------------------------------------------- templates

#[derive(Debug, Clone, PartialEq)]
pub enum TemplatePart {
    Text(String),
    Signal {
        id: SignalId,
        style: FormatStyle,
        decimals: u32,
    },
}

/// Parses `"LAP {lap_number} · {speed:1} km/h · {best_lap:time}"`. `{{`/`}}` escape braces.
pub fn parse_template(s: &str) -> Result<Vec<TemplatePart>, String> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                text.push('}');
            }
            '{' => {
                let mut inner = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => inner.push(ch),
                        None => return Err(format!("unclosed '{{' in {s:?}")),
                    }
                }
                let (name, fmt) = inner.split_once(':').unwrap_or((inner.as_str(), ""));
                let id = SignalId::from_name(name.trim()).ok_or_else(|| {
                    let hint = crate::compile::suggest(name.trim())
                        .map(|s| format!(" — did you mean {s:?}?"))
                        .unwrap_or_default();
                    format!("unknown signal {:?} in text{hint}", name.trim())
                })?;
                let (style, decimals) = match fmt.trim() {
                    "" => (default_style(id), 0),
                    "time" => (FormatStyle::Time, 0),
                    "gear" => (FormatStyle::Gear, 0),
                    "onoff" => (FormatStyle::OnOff, 0),
                    d => (
                        FormatStyle::Number,
                        d.parse().map_err(|_| {
                            format!("bad format {d:?} (use a digit, time, gear or onoff)")
                        })?,
                    ),
                };
                if !text.is_empty() {
                    parts.push(TemplatePart::Text(std::mem::take(&mut text)));
                }
                parts.push(TemplatePart::Signal {
                    id,
                    style,
                    decimals,
                });
            }
            '}' => return Err(format!("unmatched '}}' in {s:?}")),
            c => text.push(c),
        }
    }
    if !text.is_empty() {
        parts.push(TemplatePart::Text(text));
    }
    Ok(parts)
}

fn default_style(id: SignalId) -> FormatStyle {
    match id.quantity() {
        Quantity::Duration => FormatStyle::Time,
        Quantity::Gear => FormatStyle::Gear,
        _ => FormatStyle::Number,
    }
}

// ---------------------------------------------------------------- evaluator

const METRIC: UnitPrefs = UnitPrefs {
    speed: cluster_core::units::SpeedUnit::Kmh,
    temperature: cluster_core::units::TempUnit::Celsius,
    pressure: cluster_core::units::PressureUnit::Bar,
    power: cluster_core::units::PowerUnit::Kw,
    torque: cluster_core::units::TorqueUnit::Nm,
    acceleration: cluster_core::units::AccelUnit::G,
};

pub struct Evaluator {
    compiled: Compiled,
    units: UnitPrefs,
    smooth: Vec<[Option<f32>; 4]>,
    time: f32,
}

impl Evaluator {
    pub fn new(compiled: Compiled, units: UnitPrefs) -> Self {
        let n = compiled.widgets.len();
        Evaluator {
            compiled,
            units,
            smooth: vec![[None; 4]; n],
            time: 0.0,
        }
    }

    pub fn compiled(&self) -> &Compiled {
        &self.compiled
    }

    pub fn units(&self) -> UnitPrefs {
        self.units
    }

    pub fn set_units(&mut self, units: UnitPrefs) {
        self.units = units;
        self.smooth.iter_mut().for_each(|s| *s = [None; 4]);
    }

    fn unit_differs(&self, q: Quantity) -> bool {
        METRIC.convert(q, 1.0) != self.units.convert(q, 1.0)
            || METRIC.convert(q, 0.0) != self.units.convert(q, 0.0)
    }

    /// A layout number (metric, after scale/offset) in the user's units.
    fn remap(&self, x: f32, b: &CBinding) -> f32 {
        let q = b.id.quantity();
        if !b.convert || !self.unit_differs(q) || x == f32::MAX || x == f32::MIN {
            return x;
        }
        let pre = (x - b.offset) / b.scale;
        self.units.convert(q, METRIC.to_base(q, pre)) * b.scale + b.offset
    }

    /// The bound signal in shown units, or the fallback.
    fn shown(&self, b: &CBinding, snap: &Snapshot) -> Option<f32> {
        match snap.get(b.id) {
            Some(raw) => {
                let conv = if b.convert {
                    self.units.convert(b.id.quantity(), raw)
                } else {
                    raw
                };
                let v = conv * b.scale + b.offset;
                Some(b.ceil_to.map_or(v, |c| (v / c).ceil() * c))
            }
            None => b.fallback.map(|f| self.remap(f, b)),
        }
    }

    fn num(&self, n: &CNum, rel: Option<&CBinding>, snap: &Snapshot) -> Option<f32> {
        match n {
            CNum::Fixed(v) => Some(rel.map_or(*v, |b| self.remap(*v, b))),
            CNum::Bound(b) => self.shown(b, snap),
        }
    }

    fn zones(&self, w: &CWidget, snap: &Snapshot) -> Vec<(f32, f32, Rgba)> {
        let rel = w.bindings.first();
        w.zones
            .iter()
            .filter_map(|z| {
                Some((
                    self.num(&z.from, rel, snap)?,
                    self.num(&z.to, rel, snap)?,
                    z.color,
                ))
            })
            .collect()
    }

    fn unit_label(&self, b: &CBinding, f: &Format) -> String {
        if let Some(u) = &f.unit {
            return u.clone();
        }
        let q = b.id.quantity();
        if b.scale == 1.0
            && (b.convert || q == Quantity::EngineSpeed)
            && f.style == FormatStyle::Number
        {
            self.units.unit_label(q).to_owned()
        } else {
            String::new()
        }
    }

    pub fn statics(&self, snap: &Snapshot) -> Vec<WidgetStatic> {
        self.compiled
            .widgets
            .iter()
            .zip(&self.compiled.layout.widgets)
            .map(|(cw, w)| {
                let rel = cw.bindings.first();
                let mut s = WidgetStatic {
                    id: cw.id.clone(),
                    kind: WidgetType::Label,
                    x: w.x,
                    y: w.y,
                    w: w.w,
                    h: w.h,
                    z: w.z,
                    colors: cw.colors,
                    font_scale: cw.font_scale,
                    min: 0.0,
                    max: 1.0,
                    major_ticks: 0,
                    minor_ticks: 0,
                    start_angle: 0.0,
                    sweep: 0.0,
                    zones: self.zones(cw, snap),
                    caption: String::new(),
                    label: String::new(),
                    unit: String::new(),
                    align: Align::Center,
                    vertical: false,
                    show_value: true,
                    segment_colors: vec![],
                    color: cw.colors.ink,
                    icon: String::new(),
                    names: vec![],
                    image: String::new(),
                    image_fit: ImageFit::Contain,
                    max_g: 2.0,
                    show_last: true,
                    show_best: true,
                };
                let range = |s: &mut WidgetStatic| {
                    let min = cw
                        .nums
                        .first()
                        .and_then(|n| self.num(n, rel, snap))
                        .unwrap_or(0.0);
                    let max = cw
                        .nums
                        .get(1)
                        .and_then(|n| self.num(n, rel, snap))
                        .unwrap_or(min + 1.0);
                    s.min = min;
                    s.max = if max > min { max } else { min + 1.0 };
                };
                match &w.kind {
                    WidgetKind::AnalogGauge(g) => {
                        s.kind = WidgetType::AnalogGauge;
                        range(&mut s);
                        s.major_ticks = g.major_ticks.clamp(1, 40);
                        s.minor_ticks = g.minor_ticks.clamp(1, 20);
                        s.start_angle = g.start_angle;
                        s.sweep = g.sweep.clamp(1.0, 360.0);
                        s.caption = g.caption.clone();
                        // Re-round a fixed range that was converted to other units.
                        if let (Some(b), [CNum::Fixed(_), CNum::Fixed(_)]) =
                            (rel, cw.nums.as_slice())
                            && b.convert
                            && self.unit_differs(b.id.quantity())
                        {
                            let (max, ticks) = nice_range(s.min, s.max, s.major_ticks);
                            s.max = max;
                            s.major_ticks = ticks;
                        }
                        if cw.nums.iter().any(|n| matches!(n, CNum::Bound(_))) {
                            s.major_ticks = auto_ticks(s.min, s.max, s.major_ticks);
                        }
                        if s.caption.is_empty()
                            && let Some(b) = rel
                        {
                            s.caption = self.unit_label(b, &Format::default());
                        }
                    }
                    WidgetKind::BarGauge(g) => {
                        s.kind = WidgetType::BarGauge;
                        range(&mut s);
                        s.vertical = g.vertical;
                        s.label = g.label.clone();
                        s.show_value = g.show_value;
                        s.unit = rel
                            .map(|b| self.unit_label(b, &g.format))
                            .unwrap_or_default();
                    }
                    WidgetKind::Readout(r) => {
                        s.kind = WidgetType::Readout;
                        s.label = r.label.clone();
                        s.align = r.align;
                        s.unit = rel
                            .map(|b| self.unit_label(b, &r.format))
                            .unwrap_or_default();
                    }
                    WidgetKind::Gear(_) => s.kind = WidgetType::Gear,
                    WidgetKind::ShiftLights(_) => {
                        s.kind = WidgetType::ShiftLights;
                        let n = cw.extra_colors.len() - 1;
                        s.segment_colors = cw.extra_colors[..n].to_vec();
                        s.color = cw.extra_colors[n];
                    }
                    WidgetKind::Lamp(l) => {
                        s.kind = WidgetType::Lamp;
                        s.icon = l.icon.clone();
                        s.color = cw.extra_colors[0];
                    }
                    WidgetKind::TyreQuad(t) => {
                        s.kind = WidgetType::TyreQuad;
                        s.label = t.label.clone();
                        s.unit = rel
                            .map(|b| self.unit_label(b, &t.format))
                            .unwrap_or_default();
                    }
                    WidgetKind::GMeter(g) => {
                        s.kind = WidgetType::GMeter;
                        s.max_g = g.max_g.max(0.1);
                    }
                    WidgetKind::Pedals(p) => {
                        s.kind = WidgetType::Pedals;
                        s.segment_colors = cw.extra_colors.clone();
                        if p.show_labels {
                            s.names = cw.bindings.iter().map(|b| short_label(b.id)).collect();
                        }
                    }
                    WidgetKind::LapTimer(t) => {
                        s.kind = WidgetType::LapTimer;
                        s.show_last = t.show_last;
                        s.show_best = t.show_best;
                    }
                    WidgetKind::Label(l) => {
                        s.kind = WidgetType::Label;
                        s.align = l.align;
                        s.color = cw.extra_colors[0];
                    }
                    WidgetKind::Image(i) => {
                        s.kind = WidgetType::Image;
                        s.image = i.src.clone();
                        s.image_fit = i.fit;
                    }
                }
                s
            })
            .collect()
    }

    fn smoothed(&mut self, w: usize, slot: usize, target: f32, tau: f32, dt: f32) -> f32 {
        let cell = &mut self.smooth[w][slot];
        let v = match *cell {
            Some(prev) if tau > 0.0 && dt > 0.0 => {
                prev + (target - prev) * (1.0 - (-dt / tau).exp())
            }
            _ => target,
        };
        *cell = Some(v);
        v
    }

    fn format(&self, b: &CBinding, f: &Format, snap: &Snapshot) -> String {
        let raw = snap.get(b.id);
        match f.style {
            FormatStyle::Time => raw.map_or_else(|| format_lap_time(f32::NAN), format_lap_time),
            FormatStyle::Gear => raw.map_or_else(|| "-".into(), format_gear),
            FormatStyle::OnOff => raw.map_or_else(
                || "--".into(),
                |v| if v > 0.5 { "ON" } else { "OFF" }.into(),
            ),
            FormatStyle::Number => self
                .shown(b, snap)
                .map_or_else(|| "--".into(), |v| format!("{:.*}", f.decimals as usize, v)),
        }
    }

    /// Advances smoothing and blink timers by `dt` seconds and evaluates every widget.
    pub fn frame(&mut self, snap: &Snapshot, dt: f32) -> Vec<WidgetFrame> {
        self.time += dt.max(0.0);
        let blink_fast = (self.time * 8.0).fract() < 0.5;
        let blink_slow = (self.time * 2.0).fract() < 0.6;
        let n = self.compiled.widgets.len();
        let mut out = Vec::with_capacity(n);
        for wi in 0..n {
            let cw = self.compiled.widgets[wi].clone();
            let kind = self.compiled.layout.widgets[wi].kind.clone();
            let mut f = WidgetFrame::default();
            let b0 = cw.bindings.first().copied();
            let main = b0.and_then(|b| self.shown(&b, snap));
            f.valid = main.is_some();
            let zone_color = |zones: &[(f32, f32, Rgba)], v: f32, d: Rgba| {
                zones
                    .iter()
                    .find(|(a, b, _)| v >= *a && v < *b)
                    .map_or(d, |z| z.2)
            };
            match &kind {
                WidgetKind::AnalogGauge(g) => {
                    let b = b0.expect("gauge binding");
                    let v = main.unwrap_or(0.0);
                    f.slots[0].value = self.smoothed(wi, 0, v, b.smoothing, dt);
                    f.slots[0].text = match (&g.center, cw.bindings.get(1)) {
                        (Some(c), Some(cb)) => self.format(cb, &c.format, snap),
                        _ => self.format(&b, &Format::default(), snap),
                    };
                    let zones = self.zones(&cw, snap);
                    f.slots[0].color = zone_color(&zones, v, cw.colors.accent);
                    f.slots[1].color = zone_color(&zones, v, cw.colors.ink);
                }
                WidgetKind::BarGauge(g) => {
                    let b = b0.expect("bar binding");
                    let v = main.unwrap_or(0.0);
                    let min = cw
                        .nums
                        .first()
                        .and_then(|n| self.num(n, Some(&b), snap))
                        .unwrap_or(0.0);
                    let max = cw
                        .nums
                        .get(1)
                        .and_then(|n| self.num(n, Some(&b), snap))
                        .unwrap_or(min + 1.0);
                    let frac = ((v - min) / (max - min).max(f32::EPSILON)).clamp(0.0, 1.0);
                    f.slots[0].value = self.smoothed(wi, 0, frac, b.smoothing, dt);
                    f.slots[0].text = self.format(&b, &g.format, snap);
                    f.slots[0].color = zone_color(&self.zones(&cw, snap), v, cw.colors.accent);
                }
                WidgetKind::Readout(r) => {
                    let b = b0.expect("readout binding");
                    f.slots[0].value = main.unwrap_or(0.0);
                    f.slots[0].text = self.format(&b, &r.format, snap);
                    f.slots[0].color = main.map_or(cw.colors.muted, |v| {
                        zone_color(&self.zones(&cw, snap), v, cw.colors.ink)
                    });
                }
                WidgetKind::Gear(_) => {
                    let b = b0.expect("gear binding");
                    f.slots[0].text = snap.get(b.id).map_or_else(|| "-".into(), format_gear);
                    if let (Some(rpm_b), Some(n)) = (cw.bindings.get(1), cw.nums.first()) {
                        let rpm = self.shown(rpm_b, snap);
                        let at = self.num(n, None, snap);
                        f.on = matches!((rpm, at), (Some(r), Some(a)) if r >= a);
                    }
                }
                WidgetKind::ShiftLights(_) => {
                    let b = b0.expect("shift binding");
                    let v = main.unwrap_or(0.0);
                    let start = cw.nums.first().and_then(|n| self.num(n, Some(&b), snap));
                    let limit = cw.nums.get(1).and_then(|n| self.num(n, Some(&b), snap));
                    if let (Some(s), Some(l)) = (start, limit) {
                        if v >= l {
                            f.on = true;
                            f.slots[0].value = if blink_fast { 1.0 } else { 0.0 };
                        } else {
                            f.slots[0].value =
                                ((v - s) / (l - s).max(f32::EPSILON)).clamp(0.0, 1.0);
                        }
                    } else {
                        f.valid = false;
                    }
                }
                WidgetKind::Lamp(l) => {
                    let cond = match (main, l.below) {
                        (Some(v), Some(below)) => v < below,
                        (Some(v), None) => v > l.above.unwrap_or(0.5),
                        _ => false,
                    };
                    f.on = cond && (!l.blink || blink_slow);
                }
                WidgetKind::TyreQuad(t) => {
                    let zones = self.zones(&cw, snap);
                    for (i, b) in cw.bindings.iter().enumerate().take(4) {
                        let v = self.shown(b, snap);
                        f.slots[i].value = v.unwrap_or(0.0);
                        f.slots[i].text = self.format(b, &t.format, snap);
                        f.slots[i].color =
                            v.map_or(cw.colors.muted, |v| zone_color(&zones, v, cw.colors.ink));
                    }
                }
                WidgetKind::GMeter(_) => {
                    let lat = snap.get(SignalId::AccelLat).map(|v| v / STANDARD_GRAVITY);
                    let lon = snap.get(SignalId::AccelLong).map(|v| v / STANDARD_GRAVITY);
                    f.valid = lat.is_some() || lon.is_some();
                    let (lat, lon) = (lat.unwrap_or(0.0), lon.unwrap_or(0.0));
                    f.slots[0].value = self.smoothed(wi, 0, lat, 0.08, dt);
                    f.slots[1].value = self.smoothed(wi, 1, lon, 0.08, dt);
                    f.slots[0].text = format!("{:.2} g", lat.hypot(lon));
                }
                WidgetKind::Pedals(_) => {
                    f.valid = false;
                    for (i, b) in cw.bindings.iter().enumerate().take(4) {
                        let v = snap.get(b.id);
                        f.valid |= v.is_some();
                        f.slots[i].value = v.unwrap_or(0.0).clamp(0.0, 1.0);
                        f.slots[i].color = cw.extra_colors[i];
                    }
                }
                WidgetKind::LapTimer(_) => {
                    let t = |id| {
                        snap.get(id)
                            .map_or_else(|| format_lap_time(f32::NAN), format_lap_time)
                    };
                    f.valid = snap.get(SignalId::LapTime).is_some();
                    f.slots[0].text = t(SignalId::LapTime);
                    f.slots[1].text = t(SignalId::LastLap);
                    f.slots[2].text = t(SignalId::BestLap);
                    f.slots[3].text = snap
                        .get(SignalId::LapNumber)
                        .map_or_else(|| "LAP -".into(), |n| format!("LAP {}", n as i64));
                }
                WidgetKind::Label(_) => {
                    f.valid = true;
                    f.slots[0].text = self.render_template(&cw.template, snap);
                    f.slots[0].color = cw.extra_colors[0];
                }
                WidgetKind::Image(_) => f.valid = true,
            }
            out.push(f);
        }
        out
    }

    pub fn render_template(&self, parts: &[TemplatePart], snap: &Snapshot) -> String {
        let mut s = String::new();
        for p in parts {
            match p {
                TemplatePart::Text(t) => s.push_str(t),
                TemplatePart::Signal {
                    id,
                    style,
                    decimals,
                } => {
                    let b = CBinding {
                        id: *id,
                        scale: 1.0,
                        offset: 0.0,
                        smoothing: 0.0,
                        ceil_to: None,
                        convert: true,
                        fallback: None,
                    };
                    s.push_str(&self.format(
                        &b,
                        &Format {
                            style: *style,
                            decimals: *decimals,
                            unit: None,
                        },
                        snap,
                    ));
                }
            }
        }
        s
    }
}

fn short_label(id: SignalId) -> String {
    match id {
        SignalId::Throttle => "THR".into(),
        SignalId::Brake => "BRK".into(),
        SignalId::Clutch => "CLU".into(),
        SignalId::Handbrake => "HB".into(),
        other => other
            .label()
            .chars()
            .take(3)
            .collect::<String>()
            .to_uppercase(),
    }
}

/// For a dynamic range (e.g. 0..9 from `rpm_max`), picks a major-tick count that puts
/// labels on round numbers, preferring one close to `preferred`.
pub fn auto_ticks(min: f32, max: f32, preferred: u32) -> u32 {
    let span = max - min;
    let mut best: Option<(u32, u32)> = None; // (distance, count)
    for exp in -3..=6 {
        for m in [1.0f32, 2.0, 2.5, 5.0] {
            let step = m * 10f32.powi(exp);
            let n = span / step;
            if (n - n.round()).abs() < 1e-3 && (3.0..=12.0).contains(&n.round()) {
                let n = n.round() as u32;
                let d = n.abs_diff(preferred);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, n));
                }
            }
        }
    }
    best.map_or(preferred, |(_, n)| n)
}

/// Rounds `max` up so ticks land on 1/2/2.5/5×10ⁿ steps, keeping roughly `ticks` majors.
pub fn nice_range(min: f32, max: f32, ticks: u32) -> (f32, u32) {
    let span = (max - min).max(f32::EPSILON);
    let rough = span / ticks.max(1) as f32;
    let mag = 10f32.powf(rough.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|s| *s >= rough)
        .unwrap_or(10.0 * mag);
    let n = (span / step).ceil().max(1.0);
    (min + n * step, n as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::Compiled;

    const SAMPLE: &str = r##"{
        name: "test",
        canvas: { width: 1000, height: 400 },
        widgets: [
            { id: "tach", type: "analog_gauge", x: 0, y: 0, w: 400, h: 400,
              bind: { signal: "rpm", scale: 0.001 },
              max: { signal: "rpm_max", scale: 0.001, ceil_to: 1, fallback: 8 },
              zones: [ { from: { signal: "rpm_max", scale: 0.00095 }, color: "danger" } ],
              center: { bind: "gear", format: { style: "gear" } } },
            { id: "speedo", type: "analog_gauge", x: 400, y: 0, w: 400, h: 400,
              bind: "speed", max: 280, major_ticks: 7 },
            { id: "water", type: "bar_gauge", x: 800, y: 0, w: 200, h: 40,
              bind: "coolant_temp", min: 50, max: 130,
              zones: [ { from: 110, color: "danger" } ] },
            { id: "lap", type: "label", x: 800, y: 50, w: 200, h: 40, text: "LAP {lap_number} {best_lap}" },
            { id: "abs", type: "lamp", x: 800, y: 100, w: 40, h: 40, bind: "abs_active", icon: "ABS" },
            { id: "shift", type: "shift_lights", x: 0, y: 0, w: 400, h: 20,
              start: { signal: "rpm_max", scale: 0.7 }, limit: { signal: "rpm_max", scale: 0.95 } },
        ],
    }"##;

    fn snap() -> Snapshot {
        let mut s = Snapshot::new();
        s.set(SignalId::Rpm, 7400.0);
        s.set(SignalId::RpmMax, 7600.0);
        s.set(SignalId::Gear, 3.0);
        s.set(SignalId::Speed, 25.0);
        s.set(SignalId::CoolantTemp, 115.0);
        s.set(SignalId::LapNumber, 2.0);
        s.set(SignalId::BestLap, 83.25);
        s.set_flag(SignalId::AbsActive, true);
        s
    }

    #[test]
    fn evaluates_metric() {
        let c = Compiled::from_json5(SAMPLE).unwrap();
        let mut e = Evaluator::new(c, UnitPrefs::default());
        let st = e.statics(&snap());
        assert_eq!(
            st[0].max, 8.0,
            "tach max rounds rpm_max up to whole thousands"
        );
        assert!((st[0].zones[0].0 - 7.22).abs() < 0.01);
        assert_eq!(st[1].max, 280.0);
        assert_eq!(st[1].caption, "km/h");
        let fr = e.frame(&snap(), 1.0 / 60.0);
        assert_eq!(fr[0].slots[0].text, "3");
        assert!((fr[0].slots[0].value - 7.4).abs() < 1e-3);
        assert_eq!(
            fr[0].slots[0].color,
            e.compiled().palette.danger,
            "needle in red zone"
        );
        assert!((fr[1].slots[0].value - 90.0).abs() < 1e-3);
        assert!((fr[2].slots[0].value - 0.8125).abs() < 1e-3);
        assert_eq!(fr[2].slots[0].color, e.compiled().palette.danger);
        assert_eq!(fr[3].slots[0].text, "LAP 2 1:23.250");
        assert!(fr[4].on);
        assert!(fr[5].on, "past the limit the shift lights flash");
    }

    #[test]
    fn imperial_rerounds_ranges() {
        let c = Compiled::from_json5(SAMPLE).unwrap();
        let e = Evaluator::new(c, UnitPrefs::imperial());
        let st = e.statics(&snap());
        assert_eq!(st[1].caption, "mph");
        // 280 km/h ≈ 174 mph → re-rounded to 175 with 25 mph majors
        assert!(st[1].max >= 174.0 && st[1].max <= 200.0, "{}", st[1].max);
        assert_eq!((st[1].max, st[1].major_ticks), (175.0, 7));
        // 110 °C → 230 °F
        assert!((st[2].zones[0].0 - 230.0).abs() < 0.1);
        assert_eq!(st[0].max, 8.0, "rpm doesn't change with units");
    }

    #[test]
    fn missing_signals_are_invalid_not_zero() {
        let c = Compiled::from_json5(SAMPLE).unwrap();
        let mut e = Evaluator::new(c, UnitPrefs::default());
        let fr = e.frame(&Snapshot::new(), 0.016);
        assert!(!fr[1].valid);
        assert_eq!(fr[1].slots[0].text, "--");
        assert_eq!(fr[3].slots[0].text, "LAP -- -:--.---");
        let st = e.statics(&Snapshot::new());
        assert_eq!(st[0].max, 8.0, "falls back when rpm_max is missing");
    }

    #[test]
    fn smoothing_converges() {
        let src = r#"{ name: "s", canvas: {width: 10, height: 10}, widgets: [
            { type: "analog_gauge", x: 0, y: 0, w: 10, h: 10, bind: { signal: "rpm", smoothing: 0.1 }, max: 8000 } ] }"#;
        let mut e = Evaluator::new(Compiled::from_json5(src).unwrap(), UnitPrefs::default());
        let mut s = Snapshot::new();
        s.set(SignalId::Rpm, 0.0);
        e.frame(&s, 0.016);
        s.set(SignalId::Rpm, 6000.0);
        let first = e.frame(&s, 0.016)[0].slots[0].value;
        assert!(first > 0.0 && first < 2000.0, "{first}");
        let mut last = first;
        for _ in 0..120 {
            last = e.frame(&s, 0.016)[0].slots[0].value;
        }
        assert!((last - 6000.0).abs() < 1.0);
    }

    #[test]
    fn templates() {
        assert!(
            parse_template("{{literal}}").unwrap() == vec![TemplatePart::Text("{literal}".into())]
        );
        assert!(
            parse_template("{rmp}")
                .unwrap_err()
                .contains("did you mean \"rpm\"")
        );
        assert!(parse_template("{rpm").is_err());
        assert!(parse_template("{speed:x}").is_err());
    }

    #[test]
    fn nice_ranges() {
        assert_eq!(nice_range(0.0, 174.0, 7), (175.0, 7));
        assert_eq!(nice_range(0.0, 8.0, 8), (8.0, 8));
        assert_eq!(nice_range(0.0, 97.0, 5), (100.0, 5));
        assert_eq!(auto_ticks(0.0, 9.0, 8), 9);
        assert_eq!(auto_ticks(0.0, 8.0, 8), 8);
        assert_eq!(auto_ticks(0.0, 20.0, 8), 8); // 2.5 steps
    }
}
