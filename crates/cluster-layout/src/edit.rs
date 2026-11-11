//! Editing operations for the visual editor. Edits work on the layout's JSON value so
//! anything the file format supports can be edited, and every change is validated with
//! the same compiler the renderer uses.

use crate::compile::{Compiled, LayoutError, THEME_COLOR_NAMES};
use crate::model::Layout;
use cluster_core::SignalId;
use serde_json::{Map, Value, json};

/// How a property is edited in the inspector.
#[derive(Debug, Clone, PartialEq)]
pub enum PropKind {
    /// Section header (no value).
    Header,
    Text,
    Number,
    Bool,
    /// Theme color name or hex.
    Color,
    /// A signal name.
    Signal,
    Choice(&'static [&'static str]),
    /// Free-form JSON5 (zones, dynamic ranges, ...).
    Json,
    /// An image path relative to the dashboard (picked with a file dialog).
    Image,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropSpec {
    /// Dotted path inside the widget (or layout), e.g. `bind.signal`.
    pub key: &'static str,
    pub label: &'static str,
    pub kind: PropKind,
    /// Shown when the key is absent. `Value::Null` = no default (optional).
    pub default: Value,
}

const fn p(key: &'static str, label: &'static str, kind: PropKind) -> (&'static str, &'static str, PropKind) {
    (key, label, kind)
}

fn spec(rows: Vec<(&'static str, &'static str, PropKind)>, defaults: Value) -> Vec<PropSpec> {
    rows.into_iter()
        .map(|(key, label, kind)| PropSpec {
            key,
            label,
            default: get_path(&defaults, key).cloned().unwrap_or(Value::Null),
            kind,
        })
        .collect()
}

/// Widget type names in palette order, with display names.
pub const WIDGET_TYPES: &[(&str, &str)] = &[
    ("analog_gauge", "Analog gauge"),
    ("bar_gauge", "Bar gauge"),
    ("readout", "Digital readout"),
    ("gear", "Gear indicator"),
    ("shift_lights", "Shift lights"),
    ("lamp", "Warning lamp"),
    ("tyre_quad", "Tyre temperatures"),
    ("g_meter", "G-force meter"),
    ("pedals", "Pedal inputs"),
    ("lap_timer", "Lap timer"),
    ("label", "Label / text"),
    ("image", "Image"),
];

const ALIGN: &[&str] = &["left", "center", "right"];
const FORMAT_STYLE: &[&str] = &["number", "time", "gear", "on_off"];
const FIT: &[&str] = &["contain", "cover", "fill"];

fn common() -> Vec<(&'static str, &'static str, PropKind)> {
    vec![
        p("", "Widget", PropKind::Header),
        p("id", "ID", PropKind::Text),
        p("x", "X", PropKind::Number),
        p("y", "Y", PropKind::Number),
        p("w", "Width", PropKind::Number),
        p("h", "Height", PropKind::Number),
        p("z", "Layer (z)", PropKind::Number),
    ]
}

fn style() -> Vec<(&'static str, &'static str, PropKind)> {
    vec![
        p("", "Style", PropKind::Header),
        p("style.ink", "Text color", PropKind::Color),
        p("style.accent", "Accent color", PropKind::Color),
        p("style.face", "Face color", PropKind::Color),
        p("style.muted", "Muted color", PropKind::Color),
        p("style.font_scale", "Text size ×", PropKind::Number),
    ]
}

fn binding(prefix: &'static str) -> Vec<(&'static str, &'static str, PropKind)> {
    match prefix {
        "bind" => vec![
            p("", "Data", PropKind::Header),
            p("bind.signal", "Signal", PropKind::Signal),
            p("bind.scale", "Scale ×", PropKind::Number),
            p("bind.offset", "Offset +", PropKind::Number),
            p("bind.smoothing", "Smoothing (s)", PropKind::Number),
        ],
        _ => vec![],
    }
}

/// Inspector rows for a widget type.
pub fn widget_schema(kind: &str) -> Vec<PropSpec> {
    let d = default_widget(kind);
    let mut rows = common();
    match kind {
        "analog_gauge" => {
            rows.extend(binding("bind"));
            rows.extend([
                p("", "Dial", PropKind::Header),
                p("min", "Min", PropKind::Json),
                p("max", "Max", PropKind::Json),
                p("major_ticks", "Major ticks", PropKind::Number),
                p("minor_ticks", "Minor per major", PropKind::Number),
                p("start_angle", "Start angle °", PropKind::Number),
                p("sweep", "Sweep °", PropKind::Number),
                p("caption", "Caption", PropKind::Text),
                p("center", "Center text (JSON)", PropKind::Json),
                p("zones", "Zones (JSON)", PropKind::Json),
            ]);
        }
        "bar_gauge" => {
            rows.extend(binding("bind"));
            rows.extend([
                p("", "Bar", PropKind::Header),
                p("label", "Label", PropKind::Text),
                p("min", "Min", PropKind::Json),
                p("max", "Max", PropKind::Json),
                p("vertical", "Vertical", PropKind::Bool),
                p("show_value", "Show value", PropKind::Bool),
                p("format.decimals", "Decimals", PropKind::Number),
                p("format.unit", "Unit label", PropKind::Text),
                p("zones", "Zones (JSON)", PropKind::Json),
            ]);
        }
        "readout" => {
            rows.extend(binding("bind"));
            rows.extend([
                p("", "Readout", PropKind::Header),
                p("label", "Label", PropKind::Text),
                p("format.style", "Format", PropKind::Choice(FORMAT_STYLE)),
                p("format.decimals", "Decimals", PropKind::Number),
                p("format.unit", "Unit label", PropKind::Text),
                p("align", "Align", PropKind::Choice(ALIGN)),
                p("zones", "Zones (JSON)", PropKind::Json),
            ]);
        }
        "gear" => {
            rows.extend([
                p("", "Gear", PropKind::Header),
                p("bind.signal", "Signal", PropKind::Signal),
                p("shift_at", "Highlight at RPM (JSON)", PropKind::Json),
            ]);
        }
        "shift_lights" => {
            rows.extend([
                p("", "Shift lights", PropKind::Header),
                p("bind.signal", "Signal", PropKind::Signal),
                p("start", "First light at (JSON)", PropKind::Json),
                p("limit", "Flash from (JSON)", PropKind::Json),
                p("segments", "Lights", PropKind::Number),
                p("colors", "Colors (JSON)", PropKind::Json),
                p("flash_color", "Flash color", PropKind::Color),
            ]);
        }
        "lamp" => {
            rows.extend([
                p("", "Lamp", PropKind::Header),
                p("bind.signal", "Signal", PropKind::Signal),
                p("icon", "Icon text", PropKind::Text),
                p("color", "Color", PropKind::Color),
                p("above", "On above", PropKind::Number),
                p("below", "On below", PropKind::Number),
                p("blink", "Blink", PropKind::Bool),
            ]);
        }
        "tyre_quad" => rows.extend([
            p("", "Tyres", PropKind::Header),
            p("label", "Label", PropKind::Text),
            p("format.decimals", "Decimals", PropKind::Number),
            p("signals", "Signals (JSON)", PropKind::Json),
            p("zones", "Zones (JSON)", PropKind::Json),
        ]),
        "g_meter" => rows.extend([p("", "G-meter", PropKind::Header), p("max_g", "Max g", PropKind::Number)]),
        "pedals" => rows.extend([
            p("", "Pedals", PropKind::Header),
            p("signals", "Signals (JSON)", PropKind::Json),
            p("colors", "Colors (JSON)", PropKind::Json),
            p("show_labels", "Labels", PropKind::Bool),
        ]),
        "lap_timer" => rows.extend([
            p("", "Lap timer", PropKind::Header),
            p("show_last", "Show last lap", PropKind::Bool),
            p("show_best", "Show best lap", PropKind::Bool),
        ]),
        "label" => rows.extend([
            p("", "Label", PropKind::Header),
            p("text", "Text ({signal} placeholders)", PropKind::Text),
            p("align", "Align", PropKind::Choice(ALIGN)),
            p("color", "Color", PropKind::Color),
        ]),
        "image" => rows.extend([
            p("", "Image", PropKind::Header),
            p("src", "File", PropKind::Image),
            p("fit", "Fit", PropKind::Choice(FIT)),
        ]),
        _ => {}
    }
    if kind != "image" {
        rows.extend(style());
    }
    let mut out = spec(rows, d);
    // Values that have defaults in the format but not in the template:
    for s in &mut out {
        if s.default.is_null() {
            s.default = match s.key {
                "z" => json!(0),
                "bind.scale" | "style.font_scale" => json!(1),
                "bind.offset" | "bind.smoothing" => json!(0),
                "major_ticks" => json!(8),
                "minor_ticks" => json!(4),
                "start_angle" => json!(-135),
                "sweep" => json!(270),
                "min" => json!(0),
                "show_value" | "show_labels" | "show_last" | "show_best" => json!(true),
                "vertical" | "blink" => json!(false),
                "format.decimals" => json!(0),
                "format.style" => json!("number"),
                "align" => json!("center"),
                "segments" => json!(10),
                "flash_color" => json!("info"),
                "color" if kind == "lamp" => json!("warn"),
                "max_g" => json!(2),
                "fit" => json!("contain"),
                _ => Value::Null,
            };
        }
    }
    out
}

/// Inspector rows for the dashboard itself (when nothing is selected).
pub fn layout_schema() -> Vec<PropSpec> {
    let mut rows = vec![
        p("", "Dashboard", PropKind::Header),
        p("name", "Name", PropKind::Text),
        p("description", "Description", PropKind::Text),
        p("canvas.width", "Canvas width", PropKind::Number),
        p("canvas.height", "Canvas height", PropKind::Number),
        p("", "Background", PropKind::Header),
        p("background.color", "Color", PropKind::Color),
        p("background.gradient_to", "Gradient to", PropKind::Color),
        p("background.gradient_angle", "Gradient angle °", PropKind::Number),
        p("background.image", "Image", PropKind::Image),
        p("", "Theme colors", PropKind::Header),
    ];
    for (key, label) in [
        ("theme.ink", "Ink (text)"),
        ("theme.muted", "Muted"),
        ("theme.accent", "Accent"),
        ("theme.danger", "Danger"),
        ("theme.warn", "Warning"),
        ("theme.ok", "OK"),
        ("theme.info", "Info"),
        ("theme.face", "Gauge face"),
        ("theme.edge", "Edges"),
    ] {
        rows.push(p(key, label, PropKind::Color));
    }
    let theme = serde_json::to_value(crate::model::Theme::default()).unwrap_or_default();
    let defaults = json!({ "background": { "color": "#05070a", "gradient_angle": 180 }, "theme": theme });
    spec(rows, defaults)
}

/// A sensible starting point for a new widget of `kind`.
pub fn default_widget(kind: &str) -> Value {
    match kind {
        "analog_gauge" => json!({ "type": kind, "w": 360, "h": 360,
            "bind": { "signal": "rpm", "scale": 0.001, "smoothing": 0.04 },
            "max": { "signal": "rpm_max", "scale": 0.001, "ceil_to": 1, "fallback": 8 },
            "zones": [ { "from": { "signal": "rpm_max", "scale": 0.00092 }, "color": "danger" } ],
            "caption": "×1000 r/min" }),
        "bar_gauge" => json!({ "type": kind, "w": 340, "h": 36, "bind": "fuel", "max": 100, "label": "FUEL" }),
        "readout" => json!({ "type": kind, "w": 220, "h": 90, "bind": "speed", "label": "SPEED" }),
        "gear" => json!({ "type": kind, "w": 180, "h": 180, "bind": "gear",
            "shift_at": { "signal": "rpm_max", "scale": 0.93 } }),
        "shift_lights" => json!({ "type": kind, "w": 600, "h": 50, "bind": "rpm",
            "start": { "signal": "rpm_max", "scale": 0.72 }, "limit": { "signal": "rpm_max", "scale": 0.95 } }),
        "lamp" => {
            json!({ "type": kind, "w": 90, "h": 50, "bind": "handbrake", "above": 10, "icon": "HB", "color": "danger" })
        }
        "tyre_quad" => json!({ "type": kind, "w": 240, "h": 240, "label": "TYRE °C",
            "zones": [ { "to": 65, "color": "info" }, { "from": 65, "to": 100, "color": "ok" }, { "from": 100, "color": "danger" } ] }),
        "g_meter" => json!({ "type": kind, "w": 240, "h": 240, "max_g": 2 }),
        "pedals" => json!({ "type": kind, "w": 180, "h": 260 }),
        "lap_timer" => json!({ "type": kind, "w": 320, "h": 260 }),
        "label" => json!({ "type": kind, "w": 300, "h": 50, "text": "LAP {lap_number}" }),
        "image" => json!({ "type": kind, "w": 200, "h": 200, "src": "image.png" }),
        _ => json!({ "type": "label", "w": 300, "h": 50, "text": kind }),
    }
}

pub fn blank_layout(name: &str) -> Value {
    json!({ "name": name, "version": 1, "canvas": { "width": 1600, "height": 600 }, "widgets": [] })
}

// ---------------------------------------------------------------- paths

/// Reads a dotted path. A binding's short form (`"rpm"`) reads as `{signal: "rpm"}`.
pub fn get_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    for (i, seg) in path.split('.').enumerate() {
        if seg.is_empty() {
            return Some(cur);
        }
        cur = match cur {
            Value::Object(m) => m.get(seg)?,
            Value::String(_) if seg == "signal" && i > 0 => return Some(cur),
            _ => return None,
        };
    }
    Some(cur)
}

/// Writes a dotted path, creating objects as needed (and expanding a binding's short
/// form). `None` removes the key.
pub fn set_path(v: &mut Value, path: &str, new: Option<Value>) {
    let segs: Vec<&str> = path.split('.').collect();
    let mut cur = &mut *v;
    for seg in &segs[..segs.len() - 1] {
        if let Value::String(s) = cur {
            *cur = json!({ "signal": s.clone() });
        }
        if !cur.is_object() {
            *cur = Value::Object(Map::new());
        }
        let m = cur.as_object_mut().expect("object");
        cur = m.entry(seg.to_string()).or_insert_with(|| Value::Object(Map::new()));
    }
    let last = segs[segs.len() - 1];
    if let Value::String(s) = cur {
        *cur = json!({ "signal": s.clone() });
    }
    if !cur.is_object() {
        *cur = Value::Object(Map::new());
    }
    let m = cur.as_object_mut().expect("object");
    match new {
        Some(x) => {
            m.insert(last.to_owned(), x);
        }
        None => {
            m.remove(last);
        }
    }
    // Collapse `{signal: "x"}` back to the short form, and drop empty objects.
    tidy(v);
}

fn tidy(v: &mut Value) {
    if let Value::Object(m) = v {
        for (_, child) in m.iter_mut() {
            tidy(child);
            if let Value::Object(cm) = child
                && cm.len() == 1
                && let Some(Value::String(sig)) = cm.get("signal")
            {
                *child = Value::String(sig.clone());
            }
        }
        m.retain(|k, c| !(c.as_object().is_some_and(|o| o.is_empty()) && k != "theme" && k != "background"));
    } else if let Value::Array(a) = v {
        a.iter_mut().for_each(tidy);
    }
}

/// Formats a value for a text field.
pub fn display_value(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => {
            let f = n.as_f64().unwrap_or(0.0);
            if f.fract() == 0.0 && f.abs() < 1e15 { format!("{}", f as i64) } else { n.to_string() }
        }
        Some(Value::Bool(b)) => b.to_string(),
        Some(other) => json5_compact(other),
    }
}

/// Compact JSON5-ish rendering: unquoted keys.
pub fn json5_compact(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let inner: Vec<String> = m.iter().map(|(k, v)| format!("{k}: {}", json5_compact(v))).collect();
            format!("{{ {} }}", inner.join(", "))
        }
        Value::Array(a) => format!("[{}]", a.iter().map(json5_compact).collect::<Vec<_>>().join(", ")),
        other => other.to_string(),
    }
}

/// Parses inspector input for a property kind. Empty input = remove (use default).
pub fn parse_input(kind: &PropKind, text: &str) -> Result<Option<Value>, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    Ok(Some(match kind {
        PropKind::Header => return Ok(None),
        PropKind::Number => {
            let f: f64 = t.parse().map_err(|_| format!("{t:?} is not a number"))?;
            if f.fract() == 0.0 && f.abs() < 1e12 { json!(f as i64) } else { json!(f) }
        }
        PropKind::Bool => json!(matches!(t, "true" | "yes" | "on" | "1")),
        PropKind::Signal => {
            if SignalId::from_name(t).is_none() {
                return Err(format!("unknown signal {t:?}"));
            }
            json!(t)
        }
        PropKind::Color => {
            if !THEME_COLOR_NAMES.contains(&t) && crate::compile::Rgba::parse_hex(t).is_none() {
                return Err(format!("use #rrggbb or a theme color ({})", THEME_COLOR_NAMES.join(", ")));
            }
            json!(t)
        }
        PropKind::Choice(opts) => {
            if !opts.contains(&t) {
                return Err(format!("choose one of {}", opts.join(", ")));
            }
            json!(t)
        }
        PropKind::Json => json5::from_str::<Value>(t).map_err(|e| format!("invalid JSON: {e}"))?,
        PropKind::Text | PropKind::Image => json!(t),
    }))
}

// ---------------------------------------------------------------- document

/// The layout being edited, with undo/redo.
#[derive(Debug, Clone)]
pub struct Doc {
    value: Value,
    undo: Vec<Value>,
    redo: Vec<Value>,
    /// Value at the last save, for the dirty flag.
    saved: Value,
}

pub const MAX_UNDO: usize = 200;

/// A coordinate as JSON: whole numbers stay integers (so `700` isn't rewritten as
/// `700.0`, which would also count as a change), others keep one decimal.
fn coord(v: f64) -> Value {
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 { json!(r as i64) } else { json!(r) }
}

impl Doc {
    pub fn new(value: Value) -> Doc {
        Doc { saved: value.clone(), value, undo: vec![], redo: vec![] }
    }

    pub fn from_layout(layout: &Layout) -> Doc {
        Doc::new(serde_json::to_value(layout).expect("layout serializes"))
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn is_dirty(&self) -> bool {
        self.value != self.saved
    }

    pub fn mark_saved(&mut self) {
        self.saved = self.value.clone();
    }

    pub fn compile(&self) -> Result<Compiled, LayoutError> {
        let layout: Layout =
            serde_json::from_value(self.value.clone()).map_err(|e| LayoutError::Parse(e.to_string()))?;
        crate::compile(layout)
    }

    /// Pretty JSON for saving (valid JSON5).
    pub fn to_text(&self) -> String {
        serde_json::to_string_pretty(&self.value).expect("serializable") + "\n"
    }

    pub fn widgets(&self) -> &[Value] {
        self.value.get("widgets").and_then(Value::as_array).map_or(&[], Vec::as_slice)
    }

    fn widgets_mut(&mut self) -> &mut Vec<Value> {
        if !self.value.get("widgets").is_some_and(Value::is_array) {
            self.value["widgets"] = json!([]);
        }
        self.value["widgets"].as_array_mut().expect("array")
    }

    pub fn widget_type(&self, i: usize) -> Option<&str> {
        self.widgets().get(i)?.get("type")?.as_str()
    }

    pub fn rect(&self, i: usize) -> Option<[f32; 4]> {
        let w = self.widgets().get(i)?;
        let n = |k: &str| w.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        Some([n("x"), n("y"), n("w"), n("h")])
    }

    /// Runs `f` as one undoable step. If the result doesn't compile, it's rolled back and
    /// the error returned.
    pub fn apply(&mut self, f: impl FnOnce(&mut Value)) -> Result<(), String> {
        let before = self.value.clone();
        f(&mut self.value);
        if self.value == before {
            return Ok(());
        }
        if let Err(e) = self.compile() {
            self.value = before;
            return Err(match e {
                LayoutError::Invalid(p) => p.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"),
                other => other.to_string(),
            });
        }
        self.undo.push(before);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(())
    }

    /// Changes without recording undo or validating (for live drags); finish with
    /// [`Doc::commit_from`].
    pub fn set_rect_live(&mut self, i: usize, r: [f32; 4]) {
        if let Some(w) = self.widgets_mut().get_mut(i) {
            for (k, v) in ["x", "y", "w", "h"].iter().zip(r) {
                w[*k] = coord(f64::from(v));
            }
        }
    }

    /// Records an undo step from `before` to the current value (after live edits).
    pub fn commit_from(&mut self, before: Value) {
        if before != self.value {
            self.undo.push(before);
            self.redo.clear();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(v) => {
                self.redo.push(std::mem::replace(&mut self.value, v));
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(v) => {
                self.undo.push(std::mem::replace(&mut self.value, v));
                true
            }
            None => false,
        }
    }

    fn unique_id(&self, base: &str) -> String {
        let used: std::collections::HashSet<&str> =
            self.widgets().iter().filter_map(|w| w.get("id").and_then(Value::as_str)).collect();
        (1..).map(|n| format!("{base}_{n}")).find(|id| !used.contains(id.as_str())).expect("infinite")
    }

    /// Adds a widget centered at (cx, cy); returns its index.
    pub fn add_widget(&mut self, kind: &str, cx: f32, cy: f32) -> Result<usize, String> {
        let template = default_widget(kind);
        let (ww, hh) = (template["w"].as_f64().unwrap_or(100.0), template["h"].as_f64().unwrap_or(100.0));
        let (mut x, mut y) =
            (((f64::from(cx) - ww / 2.0) / 10.0).round() * 10.0, ((f64::from(cy) - hh / 2.0) / 10.0).round() * 10.0);
        // Cascade instead of stacking new widgets exactly on top of each other.
        while self.widgets().iter().any(|o| o["x"].as_f64() == Some(x) && o["y"].as_f64() == Some(y)) {
            x += 20.0;
            y += 20.0;
        }
        // Same key order as hand-written files: id, type, position, size, then the rest.
        let mut w = Map::new();
        w.insert("id".into(), json!(self.unique_id(kind)));
        w.insert("type".into(), template["type"].clone());
        w.insert("x".into(), coord(x));
        w.insert("y".into(), coord(y));
        if let Value::Object(rest) = template {
            for (k, v) in rest {
                if k != "type" {
                    w.insert(k, v);
                }
            }
        }
        let w = Value::Object(w);
        self.apply(|v| {
            if !v.get("widgets").is_some_and(Value::is_array) {
                v["widgets"] = json!([]);
            }
            v["widgets"].as_array_mut().expect("array").push(w);
        })?;
        Ok(self.widgets().len() - 1)
    }

    /// Removes widgets (indices in any order).
    pub fn remove(&mut self, indices: &[usize]) -> Result<(), String> {
        let mut idx = indices.to_vec();
        idx.sort_unstable();
        idx.dedup();
        self.apply(|v| {
            if let Some(a) = v.get_mut("widgets").and_then(Value::as_array_mut) {
                for i in idx.iter().rev() {
                    if *i < a.len() {
                        a.remove(*i);
                    }
                }
            }
        })
    }

    /// Duplicates widgets, offset by 20 units; returns the new indices.
    pub fn duplicate(&mut self, indices: &[usize]) -> Result<Vec<usize>, String> {
        let copies: Vec<Value> = indices.iter().filter_map(|&i| self.widgets().get(i).cloned()).collect();
        let start = self.widgets().len();
        let mut ids = Vec::new();
        let mut taken: Vec<String> = Vec::new();
        for c in &copies {
            let base = c.get("type").and_then(Value::as_str).unwrap_or("widget");
            let mut id = self.unique_id(base);
            while taken.contains(&id) {
                let n: usize = id.rsplit('_').next().and_then(|n| n.parse().ok()).unwrap_or(1);
                id = format!("{base}_{}", n + 1);
            }
            taken.push(id.clone());
            ids.push(id);
        }
        self.apply(|v| {
            let a = v["widgets"].as_array_mut().expect("array");
            for (mut c, id) in copies.into_iter().zip(ids) {
                c["id"] = json!(id);
                for k in ["x", "y"] {
                    c[k] = coord(c[k].as_f64().unwrap_or(0.0) + 20.0);
                }
                a.push(c);
            }
        })?;
        Ok((start..self.widgets().len()).collect())
    }

    /// Moves widgets by (dx, dy) in one undo step.
    pub fn nudge(&mut self, indices: &[usize], dx: f32, dy: f32) -> Result<(), String> {
        self.apply(|v| {
            if let Some(a) = v.get_mut("widgets").and_then(Value::as_array_mut) {
                for &i in indices {
                    if let Some(w) = a.get_mut(i) {
                        w["x"] = coord(w["x"].as_f64().unwrap_or(0.0) + f64::from(dx));
                        w["y"] = coord(w["y"].as_f64().unwrap_or(0.0) + f64::from(dy));
                    }
                }
            }
        })
    }

    /// Raises (`delta > 0`) or lowers a widget's layer.
    pub fn restack(&mut self, indices: &[usize], delta: i64) -> Result<(), String> {
        self.apply(|v| {
            if let Some(a) = v.get_mut("widgets").and_then(Value::as_array_mut) {
                for &i in indices {
                    if let Some(w) = a.get_mut(i) {
                        let z = w.get("z").and_then(Value::as_i64).unwrap_or(0) + delta;
                        if z == 0 {
                            w.as_object_mut().map(|o| o.remove("z"));
                        } else {
                            w["z"] = json!(z);
                        }
                    }
                }
            }
        })
    }

    /// Aligns widgets: `how` is left/hcenter/right/top/vcenter/bottom. With one widget,
    /// aligns to the canvas; with several, to their combined bounds.
    pub fn align(&mut self, indices: &[usize], how: &str) -> Result<(), String> {
        let rects: Vec<(usize, [f32; 4])> = indices.iter().filter_map(|&i| self.rect(i).map(|r| (i, r))).collect();
        if rects.is_empty() {
            return Ok(());
        }
        let cw = self.value["canvas"]["width"].as_f64().unwrap_or(0.0) as f32;
        let ch = self.value["canvas"]["height"].as_f64().unwrap_or(0.0) as f32;
        let (x0, y0, x1, y1) = if rects.len() == 1 {
            (0.0, 0.0, cw, ch)
        } else {
            rects.iter().fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(a, b, c, d), (_, r)| {
                (a.min(r[0]), b.min(r[1]), c.max(r[0] + r[2]), d.max(r[1] + r[3]))
            })
        };
        self.apply(|v| {
            let a = v["widgets"].as_array_mut().expect("array");
            for (i, r) in &rects {
                let (nx, ny) = match how {
                    "left" => (x0, r[1]),
                    "right" => (x1 - r[2], r[1]),
                    "hcenter" => ((x0 + x1 - r[2]) / 2.0, r[1]),
                    "top" => (r[0], y0),
                    "bottom" => (r[0], y1 - r[3]),
                    "vcenter" => (r[0], (y0 + y1 - r[3]) / 2.0),
                    _ => (r[0], r[1]),
                };
                a[*i]["x"] = coord(f64::from(nx));
                a[*i]["y"] = coord(f64::from(ny));
            }
        })
    }

    /// Reads a property for the inspector (`widget = None` → the layout itself).
    pub fn prop(&self, widget: Option<usize>, key: &str) -> Option<&Value> {
        match widget {
            Some(i) => get_path(self.widgets().get(i)?, key),
            None => get_path(&self.value, key),
        }
    }

    /// Sets a property from inspector text. Setting the default value removes the key.
    pub fn set_prop(&mut self, widget: Option<usize>, spec: &PropSpec, text: &str) -> Result<(), String> {
        let mut new = parse_input(&spec.kind, text)?;
        if new.as_ref() == Some(&spec.default) && !spec.default.is_null() {
            new = None;
        }
        let key = spec.key;
        self.apply(|v| {
            let target = match widget {
                Some(i) => match v.get_mut("widgets").and_then(Value::as_array_mut).and_then(|a| a.get_mut(i)) {
                    Some(w) => w,
                    None => return,
                },
                None => v,
            };
            set_path(target, key, new);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Doc {
        Doc::new(json!({ "name": "t", "canvas": { "width": 1000, "height": 500 }, "widgets": [
            { "id": "a", "type": "readout", "x": 10, "y": 10, "w": 100, "h": 50, "bind": "speed" },
            { "id": "b", "type": "lamp", "x": 200, "y": 10, "w": 60, "h": 40, "bind": "abs_active", "icon": "ABS" },
        ] }))
    }

    #[test]
    fn every_widget_type_has_a_valid_default_and_schema() {
        let mut d = Doc::new(blank_layout("x"));
        for (kind, _) in WIDGET_TYPES {
            if *kind == "image" {
                continue; // needs a real file to render, but still validates
            }
            d.add_widget(kind, 500.0, 300.0).unwrap_or_else(|e| panic!("{kind}: {e}"));
            let schema = widget_schema(kind);
            assert!(schema.iter().any(|s| s.key == "x"), "{kind}");
        }
        assert!(d.add_widget("image", 0.0, 0.0).is_ok());
        assert_eq!(d.widgets().len(), WIDGET_TYPES.len());
        assert!(d.compile().is_ok());
    }

    #[test]
    fn binding_short_form_expands_and_collapses() {
        let mut d = doc();
        let spec = widget_schema("readout").into_iter().find(|s| s.key == "bind.scale").unwrap();
        assert_eq!(display_value(d.prop(Some(0), "bind.signal")), "speed");
        d.set_prop(Some(0), &spec, "2").unwrap();
        assert_eq!(d.widgets()[0]["bind"], json!({ "signal": "speed", "scale": 2 }));
        d.set_prop(Some(0), &spec, "1").unwrap(); // default → removed → short form again
        assert_eq!(d.widgets()[0]["bind"], json!("speed"));
    }

    #[test]
    fn invalid_edits_are_rejected_and_rolled_back() {
        let mut d = doc();
        let sig = widget_schema("readout").into_iter().find(|s| s.key == "bind.signal").unwrap();
        assert!(d.set_prop(Some(0), &sig, "nope").unwrap_err().contains("unknown signal"));
        let id = widget_schema("readout").into_iter().find(|s| s.key == "id").unwrap();
        assert!(d.set_prop(Some(0), &id, "b").unwrap_err().contains("duplicate"));
        assert_eq!(d.widgets()[0]["id"], "a");
        assert!(!d.can_undo());
    }

    #[test]
    fn undo_redo_and_dirty() {
        let mut d = doc();
        assert!(!d.is_dirty());
        d.nudge(&[0, 1], 5.0, -5.0).unwrap();
        assert_eq!(d.rect(1).unwrap()[..2], [205.0, 5.0]);
        assert!(d.is_dirty());
        assert!(d.undo());
        assert_eq!(d.rect(1).unwrap()[..2], [200.0, 10.0]);
        assert!(!d.is_dirty());
        assert!(d.redo());
        assert_eq!(d.rect(0).unwrap()[..2], [15.0, 5.0]);
        d.mark_saved();
        assert!(!d.is_dirty());
    }

    #[test]
    fn new_widgets_cascade_and_keep_tidy_json() {
        let mut d = Doc::new(blank_layout("x"));
        d.add_widget("gear", 800.0, 300.0).unwrap();
        d.add_widget("gear", 800.0, 300.0).unwrap();
        assert_eq!(d.rect(0).unwrap()[..2], [710.0, 210.0]);
        assert_eq!(d.rect(1).unwrap()[..2], [730.0, 230.0]);
        let keys: Vec<&String> = d.widgets()[0].as_object().unwrap().keys().take(6).collect();
        assert_eq!(keys, ["id", "type", "x", "y", "w", "h"]);
        assert!(d.to_text().contains("\"x\": 710,"), "{}", d.to_text());
    }

    #[test]
    fn duplicate_remove_restack_align() {
        let mut d = doc();
        let new = d.duplicate(&[0, 1]).unwrap();
        assert_eq!(new, vec![2, 3]);
        assert_eq!(d.widgets()[2]["id"], "readout_1");
        assert_eq!(d.widgets()[3]["id"], "lamp_1");
        assert_eq!(d.rect(2).unwrap()[..2], [30.0, 30.0]);
        d.remove(&[2, 0]).unwrap();
        assert_eq!(d.widgets().len(), 2);
        assert_eq!(d.widgets()[0]["id"], "b");
        d.restack(&[0], 1).unwrap();
        assert_eq!(d.widgets()[0]["z"], 1);
        d.restack(&[0], -1).unwrap();
        assert!(d.widgets()[0].get("z").is_none());
        d.align(&[0], "hcenter").unwrap();
        assert_eq!(d.rect(0).unwrap()[0], 470.0);
        d.align(&[0, 1], "top").unwrap();
        assert_eq!(d.rect(0).unwrap()[1], d.rect(1).unwrap()[1]);
    }

    #[test]
    fn layout_props_and_live_drags() {
        let mut d = doc();
        let name = layout_schema().into_iter().find(|s| s.key == "name").unwrap();
        d.set_prop(None, &name, "Renamed").unwrap();
        assert_eq!(d.value()["name"], "Renamed");
        let before = d.value().clone();
        d.set_rect_live(0, [33.33, 44.0, 120.0, 60.0]);
        d.commit_from(before);
        assert_eq!(d.rect(0).unwrap(), [33.3, 44.0, 120.0, 60.0]);
        assert!(d.undo());
        assert_eq!(d.rect(0).unwrap()[0], 10.0);

        // A drag that ends where it started is not a change.
        let before = d.value().clone();
        d.set_rect_live(0, [10.0, 10.0, 100.0, 50.0]);
        assert_eq!(d.value(), &before);
        assert_eq!(d.widgets()[0]["x"], json!(10));
    }

    #[test]
    fn parses_inspector_input() {
        assert_eq!(parse_input(&PropKind::Number, "2.5").unwrap(), Some(json!(2.5)));
        assert_eq!(parse_input(&PropKind::Number, "").unwrap(), None);
        assert!(parse_input(&PropKind::Number, "abc").is_err());
        assert!(parse_input(&PropKind::Color, "pink").is_err());
        assert_eq!(parse_input(&PropKind::Color, "#ff0000").unwrap(), Some(json!("#ff0000")));
        assert_eq!(
            parse_input(&PropKind::Json, "{ signal: 'rpm_max', scale: 0.001 }").unwrap(),
            Some(json!({ "signal": "rpm_max", "scale": 0.001 }))
        );
        assert_eq!(
            display_value(Some(&json!({ "signal": "rpm_max", "scale": 0.001 }))),
            "{ signal: \"rpm_max\", scale: 0.001 }"
        );
    }
}
