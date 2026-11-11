//! Validation: turns a parsed [`Layout`] into a [`Compiled`] one with resolved signal IDs
//! and colors, or a list of human-readable problems.

use crate::model::*;
use cluster_core::SignalId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba { r, g, b, a: 255 }
    }

    /// Parses `#rgb`, `#rrggbb` or `#rrggbbaa`.
    pub fn parse_hex(s: &str) -> Option<Rgba> {
        let h = s.strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        let nib = |i: usize| u8::from_str_radix(h.get(i..i + 1)?, 16).ok().map(|v| v * 17);
        match h.len() {
            3 => Some(Rgba::rgb(nib(0)?, nib(1)?, nib(2)?)),
            6 => Some(Rgba::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: byte(6)? }),
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }
}

/// Theme colors resolved to RGBA.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub ink: Rgba,
    pub muted: Rgba,
    pub accent: Rgba,
    pub danger: Rgba,
    pub warn: Rgba,
    pub ok: Rgba,
    pub info: Rgba,
    pub face: Rgba,
    pub edge: Rgba,
}

impl Palette {
    fn named(&self, name: &str) -> Option<Rgba> {
        Some(match name {
            "ink" => self.ink,
            "muted" => self.muted,
            "accent" => self.accent,
            "danger" => self.danger,
            "warn" => self.warn,
            "ok" => self.ok,
            "info" => self.info,
            "face" => self.face,
            "edge" => self.edge,
            "transparent" => Rgba::default(),
            _ => return None,
        })
    }

    /// Resolves a theme name or hex color.
    pub fn resolve(&self, spec: &str) -> Option<Rgba> {
        self.named(spec.trim()).or_else(|| Rgba::parse_hex(spec.trim()))
    }
}

pub const THEME_COLOR_NAMES: &[&str] =
    &["ink", "muted", "accent", "danger", "warn", "ok", "info", "face", "edge", "transparent"];

/// A problem found while compiling a layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Where, e.g. `widgets[2] "tach" › max`.
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.path.is_empty() { f.write_str(&self.message) } else { write!(f, "{}: {}", self.path, self.message) }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("could not parse dashboard: {0}")]
    Parse(String),
    #[error("dashboard has {} problem(s):\n{}", .0.len(), .0.iter().map(|p| format!("  • {p}")).collect::<Vec<_>>().join("\n"))]
    Invalid(Vec<Problem>),
}

/// A resolved binding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CBinding {
    pub id: SignalId,
    pub scale: f32,
    pub offset: f32,
    pub smoothing: f32,
    pub ceil_to: Option<f32>,
    pub convert: bool,
    pub fallback: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CNum {
    Fixed(f32),
    Bound(CBinding),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CZone {
    pub from: CNum,
    pub to: CNum,
    pub color: Rgba,
}

/// Widget colors after applying style overrides to the palette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidgetColors {
    pub ink: Rgba,
    pub muted: Rgba,
    pub accent: Rgba,
    pub face: Rgba,
    pub danger: Rgba,
    pub edge: Rgba,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CWidget {
    pub index: usize,
    pub id: String,
    pub colors: WidgetColors,
    pub font_scale: f32,
    /// Bindings referenced by this widget, in slot order (see `eval`).
    pub bindings: Vec<CBinding>,
    pub nums: Vec<CNum>,
    pub zones: Vec<CZone>,
    /// Extra colors (shift-light segments, pedal bars, lamp color, label color).
    pub extra_colors: Vec<Rgba>,
    /// Parsed label template (for `label` widgets).
    pub template: Vec<crate::eval::TemplatePart>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Compiled {
    pub layout: Layout,
    pub palette: Palette,
    pub background: (Rgba, Option<Rgba>),
    pub widgets: Vec<CWidget>,
}

impl Compiled {
    pub fn from_json5(text: &str) -> Result<Compiled, LayoutError> {
        let layout: Layout = json5::from_str(text).map_err(|e| LayoutError::Parse(e.to_string()))?;
        compile(layout)
    }
}

struct Ctx {
    problems: Vec<Problem>,
    path: String,
}

impl Ctx {
    fn err(&mut self, field: &str, msg: impl Into<String>) {
        let path = if field.is_empty() {
            self.path.clone()
        } else if self.path.is_empty() {
            field.to_owned()
        } else {
            format!("{} › {field}", self.path)
        };
        self.problems.push(Problem { path, message: msg.into() });
    }

    fn color(&mut self, pal: &Palette, field: &str, spec: &str) -> Rgba {
        pal.resolve(spec).unwrap_or_else(|| {
            self.err(
                field,
                format!("unknown color {spec:?} (use #rrggbb or one of: {})", THEME_COLOR_NAMES.join(", ")),
            );
            Rgba::rgb(255, 0, 255)
        })
    }

    fn binding(&mut self, field: &str, b: &Binding) -> CBinding {
        let id = match SignalId::from_name(&b.signal) {
            Some(id) => id,
            None => {
                let hint = suggest(&b.signal).map(|s| format!(" — did you mean {s:?}?")).unwrap_or_default();
                self.err(field, format!("unknown signal {:?}{hint}", b.signal));
                SignalId::Speed
            }
        };
        if !b.scale.is_finite() || b.scale == 0.0 {
            self.err(field, "scale must be a non-zero number");
        }
        if b.smoothing < 0.0 {
            self.err(field, "smoothing must be ≥ 0");
        }
        CBinding {
            id,
            scale: if b.scale == 0.0 { 1.0 } else { b.scale },
            offset: b.offset,
            smoothing: b.smoothing.max(0.0),
            ceil_to: b.ceil_to.filter(|c| *c > 0.0),
            convert: b.convert,
            fallback: b.fallback,
        }
    }

    fn num(&mut self, field: &str, n: &Num) -> CNum {
        match n {
            Num::Fixed(v) => CNum::Fixed(*v),
            Num::Bound(b) => CNum::Bound(self.binding(field, b)),
        }
    }

    fn zones(&mut self, pal: &Palette, zones: &[Zone]) -> Vec<CZone> {
        zones
            .iter()
            .enumerate()
            .map(|(i, z)| CZone {
                from: self.num(&format!("zones[{i}].from"), &z.from),
                to: self.num(&format!("zones[{i}].to"), &z.to),
                color: self.color(pal, &format!("zones[{i}].color"), &z.color),
            })
            .collect()
    }
}

/// Closest signal name by edit distance, if reasonably close.
pub fn suggest(name: &str) -> Option<&'static str> {
    fn dist(a: &str, b: &str) -> usize {
        let b: Vec<char> = b.chars().collect();
        let mut prev: Vec<usize> = (0..=b.len()).collect();
        for (i, ca) in a.chars().enumerate() {
            let mut cur = vec![i + 1];
            for (j, cb) in b.iter().enumerate() {
                cur.push((prev[j] + usize::from(ca != *cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
            }
            prev = cur;
        }
        prev[b.len()]
    }
    SignalId::ALL
        .iter()
        .map(|id| (dist(name, id.name()), id.name()))
        .min()
        .filter(|(d, n)| *d <= 2.max(n.len() / 3))
        .map(|(_, n)| n)
}

pub fn compile(layout: Layout) -> Result<Compiled, LayoutError> {
    let mut cx = Ctx { problems: Vec::new(), path: String::new() };
    if !(layout.canvas.width > 0.0 && layout.canvas.height > 0.0) {
        cx.err("canvas", "width and height must be positive");
    }
    let t = &layout.theme;
    let base = Palette {
        ink: Rgba::rgb(230, 237, 243),
        muted: Rgba::rgb(125, 133, 144),
        accent: Rgba::rgb(255, 122, 26),
        danger: Rgba::rgb(255, 59, 48),
        warn: Rgba::rgb(255, 176, 32),
        ok: Rgba::rgb(47, 209, 107),
        info: Rgba::rgb(47, 129, 247),
        face: Rgba::rgb(13, 17, 23),
        edge: Rgba::rgb(48, 54, 61),
    };
    let mut hex = |field: &str, spec: &str, dflt: Rgba| {
        Rgba::parse_hex(spec).unwrap_or_else(|| {
            cx.err(&format!("theme.{field}"), format!("theme colors must be hex (#rrggbb), got {spec:?}"));
            dflt
        })
    };
    let palette = Palette {
        ink: hex("ink", &t.ink, base.ink),
        muted: hex("muted", &t.muted, base.muted),
        accent: hex("accent", &t.accent, base.accent),
        danger: hex("danger", &t.danger, base.danger),
        warn: hex("warn", &t.warn, base.warn),
        ok: hex("ok", &t.ok, base.ok),
        info: hex("info", &t.info, base.info),
        face: hex("face", &t.face, base.face),
        edge: hex("edge", &t.edge, base.edge),
    };
    let bg = layout.background.clone();
    let background = (
        cx.color(&palette, "background.color", &bg.color),
        bg.gradient_to.as_deref().map(|g| cx.color(&palette, "background.gradient_to", g)),
    );

    let mut ids = std::collections::HashSet::new();
    let mut widgets = Vec::with_capacity(layout.widgets.len());
    for (index, w) in layout.widgets.iter().enumerate() {
        let id = if w.id.is_empty() { format!("{}_{index}", w.kind.type_name()) } else { w.id.clone() };
        cx.path = format!("widgets[{index}] {id:?}");
        if !ids.insert(id.clone()) {
            cx.err("id", "duplicate widget id");
        }
        if !(w.w > 0.0 && w.h > 0.0) {
            cx.err("w/h", "width and height must be positive");
        }
        let s = &w.style;
        let mut pick = |field: &str, v: &Option<String>, d: Rgba| {
            v.as_deref().map_or(d, |c| cx.color(&palette, &format!("style.{field}"), c))
        };
        let colors = WidgetColors {
            ink: pick("ink", &s.ink, palette.ink),
            muted: pick("muted", &s.muted, palette.muted),
            accent: pick("accent", &s.accent, palette.accent),
            face: pick("face", &s.face, palette.face),
            danger: pick("danger", &s.danger, palette.danger),
            edge: palette.edge,
        };
        let mut cw = CWidget {
            index,
            id,
            colors,
            font_scale: s.font_scale.unwrap_or(1.0).clamp(0.2, 5.0),
            bindings: vec![],
            nums: vec![],
            zones: vec![],
            extra_colors: vec![],
            template: vec![],
        };
        match &w.kind {
            WidgetKind::AnalogGauge(g) => {
                cw.bindings.push(cx.binding("bind", &g.bind));
                if let Some(c) = &g.center {
                    cw.bindings.push(cx.binding("center.bind", &c.bind));
                }
                cw.nums = vec![cx.num("min", &g.min), cx.num("max", &g.max)];
                cw.zones = cx.zones(&palette, &g.zones);
                if g.major_ticks == 0 || g.major_ticks > 40 {
                    cx.err("major_ticks", "must be between 1 and 40");
                }
                if g.minor_ticks == 0 || g.minor_ticks > 20 {
                    cx.err("minor_ticks", "must be between 1 and 20");
                }
                if !(1.0..=360.0).contains(&g.sweep) {
                    cx.err("sweep", "must be between 1 and 360 degrees");
                }
            }
            WidgetKind::BarGauge(g) => {
                cw.bindings.push(cx.binding("bind", &g.bind));
                cw.nums = vec![cx.num("min", &g.min), cx.num("max", &g.max)];
                cw.zones = cx.zones(&palette, &g.zones);
            }
            WidgetKind::Readout(r) => {
                cw.bindings.push(cx.binding("bind", &r.bind));
                cw.zones = cx.zones(&palette, &r.zones);
            }
            WidgetKind::Gear(g) => {
                cw.bindings.push(cx.binding("bind", &g.bind));
                if let Some(n) = &g.shift_at {
                    cw.bindings.push(CBinding {
                        id: SignalId::Rpm,
                        scale: 1.0,
                        offset: 0.0,
                        smoothing: 0.0,
                        ceil_to: None,
                        convert: false,
                        fallback: None,
                    });
                    cw.nums.push(cx.num("shift_at", n));
                }
            }
            WidgetKind::ShiftLights(l) => {
                cw.bindings.push(cx.binding("bind", &l.bind));
                cw.nums = vec![cx.num("start", &l.start), cx.num("limit", &l.limit)];
                if l.segments == 0 || l.segments > 40 {
                    cx.err("segments", "must be between 1 and 40");
                }
                let n = l.segments.clamp(1, 40) as usize;
                cw.extra_colors = if l.colors.is_empty() {
                    (0..n)
                        .map(|i| {
                            let f = i as f32 / n as f32;
                            if f < 0.4 {
                                palette.ok
                            } else if f < 0.7 {
                                palette.warn
                            } else {
                                palette.danger
                            }
                        })
                        .collect()
                } else {
                    let given: Vec<Rgba> = l
                        .colors
                        .iter()
                        .enumerate()
                        .map(|(i, c)| cx.color(&palette, &format!("colors[{i}]"), c))
                        .collect();
                    (0..n).map(|i| given[i.min(given.len() - 1)]).collect()
                };
                cw.extra_colors.push(cx.color(&palette, "flash_color", &l.flash_color));
            }
            WidgetKind::Lamp(l) => {
                cw.bindings.push(cx.binding("bind", &l.bind));
                cw.extra_colors.push(cx.color(&palette, "color", &l.color));
            }
            WidgetKind::TyreQuad(t) => {
                for (i, b) in t.signals.iter().enumerate() {
                    cw.bindings.push(cx.binding(&format!("signals[{i}]"), b));
                }
                cw.zones = cx.zones(&palette, &t.zones);
            }
            WidgetKind::GMeter(g) => {
                cw.bindings.push(cx.binding("", &Binding::new("accel_lat")));
                cw.bindings.push(cx.binding("", &Binding::new("accel_long")));
                if g.max_g <= 0.0 {
                    cx.err("max_g", "must be positive");
                }
            }
            WidgetKind::Pedals(p) => {
                if p.signals.is_empty() || p.signals.len() > 4 {
                    cx.err("signals", "needs 1 to 4 signals");
                }
                for (i, b) in p.signals.iter().take(4).enumerate() {
                    cw.bindings.push(cx.binding(&format!("signals[{i}]"), b));
                }
                cw.extra_colors = (0..cw.bindings.len())
                    .map(|i| p.colors.get(i).map_or(palette.ink, |c| cx.color(&palette, &format!("colors[{i}]"), c)))
                    .collect();
            }
            WidgetKind::LapTimer(_) => {
                for s in ["lap_time", "last_lap", "best_lap", "lap_number"] {
                    cw.bindings.push(cx.binding("", &Binding::new(s)));
                }
            }
            WidgetKind::Label(l) => {
                match crate::eval::parse_template(&l.text) {
                    Ok(t) => cw.template = t,
                    Err(e) => cx.err("text", e),
                }
                cw.extra_colors.push(l.color.as_deref().map_or(colors.ink, |c| cx.color(&palette, "color", c)));
            }
            WidgetKind::Image(i) => {
                if i.src.trim().is_empty() {
                    cx.err("src", "image path is empty");
                }
            }
        }
        widgets.push(cw);
    }

    if cx.problems.is_empty() {
        Ok(Compiled { layout, palette, background, widgets })
    } else {
        Err(LayoutError::Invalid(cx.problems))
    }
}
