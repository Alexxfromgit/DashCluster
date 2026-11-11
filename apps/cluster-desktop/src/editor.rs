//! Visual editor state: selection, pointer drags, inspector rows, and file operations.

use cluster_layout::compile::{Palette, THEME_COLOR_NAMES};
use cluster_layout::edit::{self, Doc, PropKind, PropSpec};
use cluster_ui::{PaletteItem, PropRow};
use serde_json::Value;
use slint::{ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const GRID: f32 = 10.0;
const MIN_SIZE: f32 = 10.0;

#[derive(Clone, Copy, PartialEq)]
enum DragMode {
    Move,
    Resize,
}

struct Drag {
    mode: DragMode,
    start: (f32, f32),
    rects: Vec<(usize, [f32; 4])>,
    before: Value,
    moved: bool,
}

pub struct Editor {
    pub doc: Doc,
    /// File to save into; `None` until "Save as" (built-ins and new dashboards).
    pub path: Option<PathBuf>,
    pub selected: Vec<usize>,
    pub snap: bool,
    /// Opened from a built-in dashboard: the first save suggests "<name> copy".
    pub from_builtin: bool,
    pub message: String,
    errors: HashMap<String, String>,
    drag: Option<Drag>,
}

/// What a pointer/drag event changed, so the app updates only what's needed.
#[derive(Debug, PartialEq)]
pub enum Change {
    None,
    Selection,
    /// Live geometry for these widgets (drag in progress).
    Rects(Vec<(usize, [f32; 4])>),
    /// Document changed (needs recompile).
    Doc,
}

fn snap(v: f32, on: bool) -> f32 {
    if on { (v / GRID).round() * GRID } else { (v * 10.0).round() / 10.0 }
}

impl Editor {
    pub fn new(doc: Doc, path: Option<PathBuf>) -> Self {
        Editor {
            doc,
            path,
            selected: vec![],
            snap: true,
            from_builtin: false,
            message: String::new(),
            errors: HashMap::new(),
            drag: None,
        }
    }

    pub fn base_dir(&self) -> Option<PathBuf> {
        self.path.as_deref().and_then(Path::parent).map(Path::to_path_buf)
    }

    pub fn name(&self) -> String {
        self.doc.value().get("name").and_then(Value::as_str).unwrap_or("Untitled").to_owned()
    }

    /// Widget indices, topmost first.
    fn hit_order(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.doc.widgets().len()).collect();
        idx.sort_by_key(|&i| (self.doc.widgets()[i].get("z").and_then(Value::as_i64).unwrap_or(0), i));
        idx.reverse();
        idx
    }

    pub fn pointer_down(&mut self, x: f32, y: f32, additive: bool, handle: f32) -> Change {
        self.message.clear();
        // Resize handle of a selected widget?
        for &i in &self.selected {
            if let Some(r) = self.doc.rect(i)
                && (x - (r[0] + r[2])).abs() <= handle
                && (y - (r[1] + r[3])).abs() <= handle
            {
                self.drag = Some(Drag {
                    mode: DragMode::Resize,
                    start: (x, y),
                    rects: vec![(i, r)],
                    before: self.doc.value().clone(),
                    moved: false,
                });
                return Change::None;
            }
        }
        let hit = self.hit_order().into_iter().find(|&i| {
            self.doc.rect(i).is_some_and(|r| x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3])
        });
        let before = self.selected.clone();
        match hit {
            Some(i) if additive => {
                if let Some(p) = self.selected.iter().position(|&s| s == i) {
                    self.selected.remove(p);
                } else {
                    self.selected.push(i);
                }
            }
            Some(i) if !self.selected.contains(&i) => self.selected = vec![i],
            Some(_) => {}
            None if !additive => self.selected.clear(),
            None => {}
        }
        if hit.is_some_and(|i| self.selected.contains(&i)) {
            self.drag = Some(Drag {
                mode: DragMode::Move,
                start: (x, y),
                rects: self.selected.iter().filter_map(|&i| self.doc.rect(i).map(|r| (i, r))).collect(),
                before: self.doc.value().clone(),
                moved: false,
            });
        }
        if before != self.selected {
            self.errors.clear();
            Change::Selection
        } else {
            Change::None
        }
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) -> Change {
        let snap_on = self.snap;
        let Some(d) = &mut self.drag else { return Change::None };
        let (dx, dy) = (x - d.start.0, y - d.start.1);
        if !d.moved && dx.abs() < 1.0 && dy.abs() < 1.0 {
            return Change::None;
        }
        d.moved = true;
        let out: Vec<(usize, [f32; 4])> = match d.mode {
            DragMode::Move => {
                // Snap the first widget's position and move the rest by the same amount.
                let r0 = d.rects[0].1;
                let sx = snap(r0[0] + dx, snap_on) - r0[0];
                let sy = snap(r0[1] + dy, snap_on) - r0[1];
                d.rects.iter().map(|(i, r)| (*i, [r[0] + sx, r[1] + sy, r[2], r[3]])).collect()
            }
            DragMode::Resize => d
                .rects
                .iter()
                .map(|(i, r)| {
                    let w = (snap(r[0] + r[2] + dx, snap_on) - r[0]).max(MIN_SIZE);
                    let h = (snap(r[1] + r[3] + dy, snap_on) - r[1]).max(MIN_SIZE);
                    (*i, [r[0], r[1], w, h])
                })
                .collect(),
        };
        for (i, r) in &out {
            self.doc.set_rect_live(*i, *r);
        }
        Change::Rects(out)
    }

    pub fn pointer_up(&mut self) -> Change {
        match self.drag.take() {
            Some(d) if d.moved => {
                self.doc.commit_from(d.before);
                Change::Doc
            }
            _ => Change::None,
        }
    }

    fn run(&mut self, r: Result<(), String>) -> Change {
        match r {
            Ok(()) => {
                self.message.clear();
                Change::Doc
            }
            Err(e) => {
                self.message = e;
                Change::None
            }
        }
    }

    pub fn add(&mut self, kind: &str) -> Change {
        let c = &self.doc.value()["canvas"];
        let (cx, cy) = (c["width"].as_f64().unwrap_or(1600.0) / 2.0, c["height"].as_f64().unwrap_or(600.0) / 2.0);
        match self.doc.add_widget(kind, cx as f32, cy as f32) {
            Ok(i) => {
                self.selected = vec![i];
                self.errors.clear();
                Change::Doc
            }
            Err(e) => {
                self.message = e;
                Change::None
            }
        }
    }

    pub fn delete(&mut self) -> Change {
        if self.selected.is_empty() {
            return Change::None;
        }
        let sel = std::mem::take(&mut self.selected);
        let r = self.doc.remove(&sel);
        self.run(r)
    }

    pub fn duplicate(&mut self) -> Change {
        if self.selected.is_empty() {
            return Change::None;
        }
        match self.doc.duplicate(&self.selected.clone()) {
            Ok(new) => {
                self.selected = new;
                Change::Doc
            }
            Err(e) => {
                self.message = e;
                Change::None
            }
        }
    }

    pub fn nudge(&mut self, dx: f32, dy: f32) -> Change {
        if self.selected.is_empty() {
            return Change::None;
        }
        let r = self.doc.nudge(&self.selected.clone(), dx, dy);
        self.run(r)
    }

    pub fn restack(&mut self, delta: i64) -> Change {
        let r = self.doc.restack(&self.selected.clone(), delta);
        self.run(r)
    }

    pub fn align(&mut self, how: &str) -> Change {
        let r = self.doc.align(&self.selected.clone(), how);
        self.run(r)
    }

    pub fn undo(&mut self) -> Change {
        if self.doc.undo() {
            self.clamp_selection();
            Change::Doc
        } else {
            Change::None
        }
    }

    pub fn redo(&mut self) -> Change {
        if self.doc.redo() {
            self.clamp_selection();
            Change::Doc
        } else {
            Change::None
        }
    }

    fn clamp_selection(&mut self) {
        let n = self.doc.widgets().len();
        self.selected.retain(|&i| i < n);
        self.errors.clear();
    }

    fn schema(&self) -> (Option<usize>, Vec<PropSpec>) {
        match self.selected.as_slice() {
            [i] => (Some(*i), edit::widget_schema(self.doc.widget_type(*i).unwrap_or(""))),
            [] => (None, edit::layout_schema()),
            _ => (None, vec![]),
        }
    }

    pub fn set_prop(&mut self, key: &str, value: &str) -> Change {
        let (target, schema) = self.schema();
        let Some(spec) = schema.iter().find(|s| s.key == key) else { return Change::None };
        if self.selected.len() > 1 {
            return Change::None;
        }
        match self.doc.set_prop(target, spec, value) {
            Ok(()) => {
                self.errors.remove(key);
                Change::Doc
            }
            Err(e) => {
                self.errors.insert(key.to_owned(), e);
                Change::Selection // refresh rows to show the error
            }
        }
    }

    pub fn inspector_title(&self) -> String {
        match self.selected.as_slice() {
            [] => "Dashboard".into(),
            [i] => {
                let t = self.doc.widget_type(*i).unwrap_or("widget");
                let label = edit::WIDGET_TYPES.iter().find(|(k, _)| *k == t).map_or(t, |(_, l)| l);
                let id = self.doc.widgets()[*i].get("id").and_then(Value::as_str).unwrap_or("");
                format!("{label} · {id}")
            }
            s => format!("{} widgets selected", s.len()),
        }
    }

    pub fn rows(&self, palette: &Palette) -> Vec<PropRow> {
        let (target, schema) = self.schema();
        let signals: Vec<SharedString> =
            cluster_core::SignalId::ALL.iter().map(|s| SharedString::from(s.name())).collect();
        schema
            .iter()
            .map(|s| {
                let current = self.doc.prop(target, s.key);
                let shown = if s.key.is_empty() {
                    String::new()
                } else {
                    edit::display_value(current.or(if s.default.is_null() { None } else { Some(&s.default) }))
                };
                let (kind, options): (i32, Vec<SharedString>) = match &s.kind {
                    PropKind::Header => (0, vec![]),
                    PropKind::Text => (1, vec![]),
                    PropKind::Number => (2, vec![]),
                    PropKind::Bool => (3, vec![]),
                    PropKind::Color => (4, vec![]),
                    PropKind::Signal => (5, signals.clone()),
                    PropKind::Choice(o) => (6, o.iter().map(|x| SharedString::from(*x)).collect()),
                    PropKind::Json => (7, vec![]),
                    PropKind::Image => (8, vec![]),
                };
                let swatch = if kind == 4 {
                    palette.resolve(&shown).map(cluster_ui::view::color).unwrap_or_default()
                } else {
                    Default::default()
                };
                PropRow {
                    key: s.key.into(),
                    label: s.label.into(),
                    kind,
                    value: shown.into(),
                    options: ModelRc::new(VecModel::from(options)),
                    swatch,
                    error: self.errors.get(s.key).cloned().unwrap_or_default().into(),
                }
            })
            .collect()
    }

    /// Writes the document to `path`.
    pub fn save_to(&mut self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.doc.to_text())?;
        self.path = Some(path.to_path_buf());
        self.doc.mark_saved();
        self.message = format!("Saved {}", path.display());
        Ok(())
    }
}

pub fn palette_items() -> Vec<PaletteItem> {
    edit::WIDGET_TYPES.iter().map(|(k, l)| PaletteItem { kind: (*k).into(), label: (*l).into() }).collect()
}

pub fn theme_swatches(p: &Palette) -> (Vec<slint::Color>, Vec<SharedString>) {
    let names: Vec<&str> = THEME_COLOR_NAMES.iter().copied().filter(|n| *n != "transparent").collect();
    let colors = names.iter().map(|n| cluster_ui::view::color(p.resolve(n).unwrap_or_default())).collect();
    (colors, names.into_iter().map(SharedString::from).collect())
}

/// Image paths a dashboard references (background and image widgets), relative only.
pub fn referenced_assets(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(s) = v.pointer("/background/image").and_then(Value::as_str) {
        out.push(s.to_owned());
    }
    for w in v.get("widgets").and_then(Value::as_array).into_iter().flatten() {
        if w.get("type").and_then(Value::as_str) == Some("image")
            && let Some(s) = w.get("src").and_then(Value::as_str)
        {
            out.push(s.to_owned());
        }
    }
    out.retain(|p| !Path::new(p).is_absolute() && !p.contains(".."));
    out.sort();
    out.dedup();
    out
}

/// Writes a `.cluster` package: `dashboard.json5` plus the images it references.
pub fn export_package(doc: &Doc, base_dir: Option<&Path>, out: &Path) -> anyhow::Result<usize> {
    let file = std::fs::File::create(out)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default();
    zip.start_file("dashboard.json5", opts)?;
    zip.write_all(doc.to_text().as_bytes())?;
    let mut n = 0;
    if let Some(dir) = base_dir {
        for rel in referenced_assets(doc.value()) {
            if let Ok(bytes) = std::fs::read(dir.join(&rel)) {
                zip.start_file(rel.replace('\\', "/"), opts)?;
                zip.write_all(&bytes)?;
                n += 1;
            }
        }
    }
    zip.finish()?;
    Ok(n)
}

/// Unpacks a `.cluster` package into a new folder in `dest_root`; returns the path of
/// its `dashboard.json5`.
pub fn import_package(pkg: &Path, dest_root: &Path) -> anyhow::Result<PathBuf> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(pkg)?)?;
    let mut text = String::new();
    zip.by_name("dashboard.json5")
        .map_err(|_| anyhow::anyhow!("{} is not a DashCluster package (no dashboard.json5)", pkg.display()))?
        .read_to_string(&mut text)?;
    cluster_layout::Compiled::from_json5(&text)?;
    let stem = pkg.file_stem().map_or_else(|| "imported".into(), |s| crate::library::slug(&s.to_string_lossy()));
    let dir = crate::library::unique_path(dest_root, &stem, "");
    std::fs::create_dir_all(&dir)?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let Some(rel) = f.enclosed_name() else { continue };
        if f.is_dir() {
            continue;
        }
        let target = dir.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes)?;
        std::fs::write(target, bytes)?;
    }
    Ok(dir.join("dashboard.json5"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn editor() -> Editor {
        Editor::new(
            Doc::new(json!({ "name": "t", "canvas": { "width": 1000, "height": 500 }, "widgets": [
                { "id": "a", "type": "readout", "x": 100, "y": 100, "w": 100, "h": 50, "bind": "speed" },
                { "id": "b", "type": "readout", "x": 150, "y": 120, "w": 100, "h": 50, "bind": "rpm", "z": 1 },
            ] })),
            None,
        )
    }

    #[test]
    fn click_selects_topmost_and_drag_moves_with_snap() {
        let mut e = editor();
        assert_eq!(e.pointer_down(160.0, 130.0, false, 5.0), Change::Selection);
        assert_eq!(e.selected, vec![1], "b is on top (z = 1)");
        assert_eq!(e.pointer_move(173.0, 136.0), Change::Rects(vec![(1, [160.0, 130.0, 100.0, 50.0])]));
        assert_eq!(e.pointer_up(), Change::Doc);
        assert_eq!(e.doc.rect(1).unwrap(), [160.0, 130.0, 100.0, 50.0]);
        assert!(e.doc.can_undo());
    }

    #[test]
    fn shift_click_multi_select_and_corner_resize() {
        let mut e = editor();
        e.pointer_down(110.0, 110.0, false, 5.0);
        e.pointer_up();
        e.pointer_down(240.0, 160.0, true, 5.0);
        e.pointer_up();
        assert_eq!(e.selected, vec![0, 1]);
        // Grab b's bottom-right corner.
        e.pointer_down(250.0, 170.0, false, 5.0);
        e.pointer_move(287.0, 199.0);
        e.pointer_up();
        assert_eq!(e.doc.rect(1).unwrap(), [150.0, 120.0, 140.0, 80.0]);
        assert_eq!(e.selected, vec![0, 1], "resizing keeps the selection");
        // Click on empty canvas clears.
        assert_eq!(e.pointer_down(900.0, 450.0, false, 5.0), Change::Selection);
        assert!(e.selected.is_empty());
    }

    #[test]
    fn inspector_rows_and_errors() {
        let mut e = editor();
        e.selected = vec![0];
        let pal = e.doc.compile().unwrap().palette;
        let rows = e.rows(&pal);
        let sig = rows.iter().find(|r| r.key == "bind.signal").unwrap();
        assert_eq!(sig.value, "speed");
        assert_eq!(sig.kind, 5);
        assert_eq!(e.set_prop("w", "abc"), Change::Selection);
        let rows = e.rows(&pal);
        assert!(!rows.iter().find(|r| r.key == "w").unwrap().error.is_empty());
        assert_eq!(e.set_prop("w", "120"), Change::Doc);
        assert_eq!(e.doc.rect(0).unwrap()[2], 120.0);
    }

    #[test]
    fn package_round_trip() {
        let dir = std::env::temp_dir().join(format!("dc-pkg-{}", std::process::id()));
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("logo.png"), b"\x89PNGfake").unwrap();
        let doc = Doc::new(json!({ "name": "pkg", "canvas": { "width": 100, "height": 100 }, "widgets": [
            { "id": "i", "type": "image", "x": 0, "y": 0, "w": 10, "h": 10, "src": "logo.png" } ] }));
        let pkg = dir.join("My Dash.cluster");
        assert_eq!(export_package(&doc, Some(&src), &pkg).unwrap(), 1);
        let dest = dir.join("dest");
        let p = import_package(&pkg, &dest).unwrap();
        assert!(p.ends_with("my-dash/dashboard.json5"));
        assert_eq!(std::fs::read(p.parent().unwrap().join("logo.png")).unwrap(), b"\x89PNGfake");
        let again = import_package(&pkg, &dest).unwrap();
        assert!(again.ends_with("my-dash-2/dashboard.json5"));
        std::fs::remove_dir_all(dir).ok();
    }
}
