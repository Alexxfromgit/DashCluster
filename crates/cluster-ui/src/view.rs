//! Drives the Slint dashboard models from a compiled layout and live snapshots.

use crate::{WFrame, WStatic, WType, WZone};
use cluster_core::{Snapshot, UnitPrefs};
use cluster_layout::Compiled;
use cluster_layout::compile::Rgba;
use cluster_layout::eval::{Evaluator, WidgetFrame, WidgetStatic, WidgetType};
use cluster_layout::model::{Align, ImageFit};
use slint::{Color, Model, ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

/// How often slow-changing widget config (ranges from `rpm_max`, etc.) is re-checked.
const STATICS_INTERVAL: f32 = 0.25;

pub fn color(c: Rgba) -> Color {
    Color::from_argb_u8(c.a, c.r, c.g, c.b)
}

fn kind(k: WidgetType) -> WType {
    match k {
        WidgetType::AnalogGauge => WType::AnalogGauge,
        WidgetType::BarGauge => WType::BarGauge,
        WidgetType::Readout => WType::Readout,
        WidgetType::Gear => WType::Gear,
        WidgetType::ShiftLights => WType::ShiftLights,
        WidgetType::Lamp => WType::Lamp,
        WidgetType::TyreQuad => WType::TyreQuad,
        WidgetType::GMeter => WType::GMeter,
        WidgetType::Pedals => WType::Pedals,
        WidgetType::LapTimer => WType::LapTimer,
        WidgetType::Label => WType::Label,
        WidgetType::Image => WType::Image,
    }
}

fn frame_to_slint(f: &WidgetFrame) -> WFrame {
    let [s0, s1, s2, s3] = &f.slots;
    WFrame {
        valid: f.valid,
        on: f.on,
        v0: s0.value,
        v1: s1.value,
        v2: s2.value,
        v3: s3.value,
        t0: SharedString::from(s0.text.as_str()),
        t1: SharedString::from(s1.text.as_str()),
        t2: SharedString::from(s2.text.as_str()),
        t3: SharedString::from(s3.text.as_str()),
        c0: color(s0.color),
        c1: color(s1.color),
        c2: color(s2.color),
        c3: color(s3.color),
    }
}

/// Loads images referenced by a dashboard, relative to its folder.
#[derive(Default)]
pub struct ImageCache {
    base_dir: Option<PathBuf>,
    cache: HashMap<String, slint::Image>,
}

impl ImageCache {
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        ImageCache { base_dir, cache: HashMap::new() }
    }

    pub fn get(&mut self, src: &str) -> slint::Image {
        if src.is_empty() {
            return slint::Image::default();
        }
        if let Some(img) = self.cache.get(src) {
            return img.clone();
        }
        #[cfg(not(target_arch = "wasm32"))]
        let img = {
            let path = match &self.base_dir {
                Some(dir) => dir.join(src),
                None => PathBuf::from(src),
            };
            slint::Image::load_from_path(&path).unwrap_or_else(|_| {
                log::warn!("could not load image {}", path.display());
                slint::Image::default()
            })
        };
        #[cfg(target_arch = "wasm32")]
        let img = slint::Image::default();
        self.cache.insert(src.to_owned(), img.clone());
        img
    }
}

pub struct DashboardView {
    eval: Evaluator,
    /// Display order (by z) → widget index.
    order: Vec<usize>,
    statics: Rc<VecModel<WStatic>>,
    frames: Rc<VecModel<WFrame>>,
    last_statics: Vec<WidgetStatic>,
    images: ImageCache,
    since_statics: f32,
}

impl DashboardView {
    pub fn new(compiled: Compiled, units: UnitPrefs, base_dir: Option<PathBuf>) -> Self {
        let mut order: Vec<usize> = (0..compiled.widgets.len()).collect();
        order.sort_by_key(|&i| compiled.layout.widgets[i].z);
        let mut v = DashboardView {
            eval: Evaluator::new(compiled, units),
            order,
            statics: Rc::new(VecModel::default()),
            frames: Rc::new(VecModel::default()),
            last_statics: vec![],
            images: ImageCache::new(base_dir),
            since_statics: 0.0,
        };
        v.refresh_statics(&Snapshot::new(), true);
        v.update(&Snapshot::new(), 0.0);
        v
    }

    pub fn compiled(&self) -> &Compiled {
        self.eval.compiled()
    }

    pub fn name(&self) -> &str {
        &self.compiled().layout.name
    }

    pub fn canvas(&self) -> (f32, f32) {
        let c = self.compiled().layout.canvas;
        (c.width, c.height)
    }

    pub fn statics_model(&self) -> ModelRc<WStatic> {
        ModelRc::from(self.statics.clone())
    }

    pub fn frames_model(&self) -> ModelRc<WFrame> {
        ModelRc::from(self.frames.clone())
    }

    /// Background as (color, gradient end color, gradient angle °, image).
    /// Without a gradient both colors are the same.
    pub fn background(&mut self) -> (Color, Color, f32, slint::Image) {
        let (c0, c1) = self.compiled().background;
        let bg = &self.compiled().layout.background;
        let angle = bg.gradient_angle;
        let image = bg.image.clone().unwrap_or_default();
        (color(c0), color(c1.unwrap_or(c0)), angle, self.images.get(&image))
    }

    pub fn units(&self) -> UnitPrefs {
        self.eval.units()
    }

    pub fn set_units(&mut self, units: UnitPrefs, snap: &Snapshot) {
        self.eval.set_units(units);
        self.refresh_statics(snap, true);
    }

    fn static_to_slint(&mut self, s: &WidgetStatic) -> WStatic {
        let zones: Vec<WZone> = s
            .zones
            .iter()
            .map(|(from, to, c)| WZone { from: from.max(s.min), to: to.min(s.max), color: color(*c) })
            .collect();
        WStatic {
            kind: kind(s.kind),
            x: s.x,
            y: s.y,
            w: s.w,
            h: s.h,
            ink: color(s.colors.ink),
            muted: color(s.colors.muted),
            accent: color(s.colors.accent),
            face: color(s.colors.face),
            danger: color(s.colors.danger),
            edge: color(s.colors.edge),
            font_scale: s.font_scale,
            min: s.min,
            max: s.max,
            major: s.major_ticks as i32,
            minor: s.minor_ticks as i32,
            start_angle: s.start_angle,
            sweep: s.sweep,
            zones: ModelRc::new(VecModel::from(zones)),
            caption: s.caption.as_str().into(),
            label: s.label.as_str().into(),
            unit: s.unit.as_str().into(),
            align: match s.align {
                Align::Left => 0,
                Align::Center => 1,
                Align::Right => 2,
            },
            vertical: s.vertical,
            show_value: s.show_value,
            segments: ModelRc::new(VecModel::from(s.segment_colors.iter().map(|c| color(*c)).collect::<Vec<_>>())),
            color: color(s.color),
            icon: s.icon.as_str().into(),
            names: ModelRc::new(VecModel::from(
                s.names.iter().map(|n| SharedString::from(n.as_str())).collect::<Vec<_>>(),
            )),
            image: self.images.get(&s.image),
            fit: match s.image_fit {
                ImageFit::Contain => 0,
                ImageFit::Cover => 1,
                ImageFit::Fill => 2,
            },
            max_g: s.max_g,
            show_last: s.show_last,
            show_best: s.show_best,
        }
    }

    /// Recomputes widget config; pushes only rows that changed (or all if `force`).
    pub fn refresh_statics(&mut self, snap: &Snapshot, force: bool) {
        self.since_statics = 0.0;
        let new = self.eval.statics(snap);
        if !force && new == self.last_statics {
            return;
        }
        let rebuild = force || self.statics.row_count() != self.order.len();
        let order = self.order.clone();
        if rebuild {
            let rows: Vec<WStatic> = order.iter().map(|&i| self.static_to_slint(&new[i])).collect();
            self.statics.set_vec(rows);
        } else {
            for (row, &i) in order.iter().enumerate() {
                if self.last_statics.get(i) != Some(&new[i]) {
                    let s = self.static_to_slint(&new[i]);
                    self.statics.set_row_data(row, s);
                }
            }
        }
        self.last_statics = new;
    }

    /// Call once per rendered frame.
    pub fn update(&mut self, snap: &Snapshot, dt: f32) {
        self.since_statics += dt;
        if self.since_statics >= STATICS_INTERVAL {
            self.refresh_statics(snap, false);
        }
        let frames = self.eval.frame(snap, dt);
        if self.frames.row_count() != self.order.len() {
            self.frames.set_vec(self.order.iter().map(|&i| frame_to_slint(&frames[i])).collect::<Vec<_>>());
            return;
        }
        for (row, &i) in self.order.iter().enumerate() {
            let f = frame_to_slint(&frames[i]);
            if self.frames.row_data(row).as_ref() != Some(&f) {
                self.frames.set_row_data(row, f);
            }
        }
    }
}
