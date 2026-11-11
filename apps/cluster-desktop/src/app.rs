//! Desktop app state: the dashboard library, the live view, hot reload, the hub
//! catalog, and the visual editor.

use crate::editor::{self, Change, Editor};
use crate::library::{self, Library};
use crate::settings::Settings;
use crate::sources::{SourceKind, Sources};
use cluster_core::SignalBus;
use cluster_core::{Snapshot, UnitPrefs};
use cluster_layout::edit::Doc;
use cluster_ui::UiSettings;
use cluster_ui::{DashboardView, MainWindow, PropRow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

pub fn units_name(u: UnitPrefs) -> &'static str {
    if u == UnitPrefs::default() { "metric" } else { "imperial" }
}

/// Points the window at a view's models and canvas.
pub fn bind_view(ui: &MainWindow, view: &mut DashboardView) {
    let (w, h) = view.canvas();
    ui.set_canvas_w(w);
    ui.set_canvas_h(h);
    ui.set_statics(view.statics_model());
    ui.set_frames(view.frames_model());
    let (c0, c1, angle, image) = view.background();
    ui.set_bg(c0);
    ui.set_bg2(c1);
    ui.set_bg_angle(angle);
    ui.set_bg_image(image);
    ui.set_dashboard_name(view.name().into());
}

pub struct App {
    pub ui: slint::Weak<MainWindow>,
    pub lib: Library,
    pub view: DashboardView,
    pub units: UnitPrefs,
    pub hub: Option<cluster_net::Hub>,
    pub editor: Option<Editor>,
    pub last_snapshot: Snapshot,
    watcher: Option<notify::RecommendedWatcher>,
    reload_rx: mpsc::Receiver<()>,
    reload_tx: mpsc::Sender<()>,
    pending_reload: Option<Instant>,
    pub toast_until: Option<Instant>,
    selected_model: Rc<VecModel<bool>>,
    props_model: Rc<VecModel<PropRow>>,
    pub settings: Settings,
    /// Settings as stored on disk (command-line overrides apply only to this run).
    pub saved_settings: Settings,
    pub sources: Sources,
    pub bus: Arc<SignalBus>,
    pub web_dir: Option<PathBuf>,
}

enum Confirm {
    Save,
    Discard,
    Cancel,
}

fn confirm_unsaved(name: &str) -> Confirm {
    match rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title("Unsaved changes")
        .set_description(format!("Save your changes to “{name}”?"))
        .set_buttons(rfd::MessageButtons::YesNoCancelCustom("Save".into(), "Don't save".into(), "Cancel".into()))
        .show()
    {
        rfd::MessageDialogResult::Yes => Confirm::Save,
        rfd::MessageDialogResult::No => Confirm::Discard,
        rfd::MessageDialogResult::Custom(s) if s == "Save" => Confirm::Save,
        rfd::MessageDialogResult::Custom(s) if s == "Don't save" => Confirm::Discard,
        _ => Confirm::Cancel,
    }
}

fn open_in_file_manager(dir: &Path) {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let cmd = "xdg-open";
    if let Err(e) = std::process::Command::new(cmd).arg(dir).spawn() {
        log::warn!("couldn't open {}: {e}", dir.display());
    }
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ui: &MainWindow,
        lib: Library,
        settings: Settings,
        saved_settings: Settings,
        sources: Sources,
        bus: Arc<SignalBus>,
        web_dir: Option<PathBuf>,
    ) -> App {
        let units = settings.units;
        let (compiled, base) =
            (cluster_layout::Compiled::from_json5(cluster_layout::BUILTIN[0].1).expect("built-in dashboard"), None);
        let (reload_tx, reload_rx) = mpsc::channel();
        let selected_model = Rc::new(VecModel::default());
        let props_model = Rc::new(VecModel::default());
        ui.set_selected(ModelRc::from(selected_model.clone()));
        ui.set_props(ModelRc::from(props_model.clone()));
        ui.set_palette_items(ModelRc::new(VecModel::from(editor::palette_items())));
        App {
            ui: ui.as_weak(),
            lib,
            view: DashboardView::new(compiled, units, base),
            units,
            hub: None,
            editor: None,
            last_snapshot: Snapshot::new(),
            watcher: None,
            reload_rx,
            reload_tx,
            pending_reload: None,
            toast_until: None,
            selected_model,
            props_model,
            settings,
            saved_settings,
            sources,
            bus,
            web_dir,
        }
    }

    /// (Re)starts this app's hub per settings and updates the QR code.
    pub fn restart_hub(&mut self) {
        self.hub = None; // stop the old one first (frees the port)
        let ui = self.ui();
        if !self.settings.hub_enabled {
            ui.set_qr_url("Sharing is off".into());
            ui.set_qr_detail("Turn on “Share telemetry” in Settings (P) to use a phone.".into());
            ui.set_qr_image(slint::Image::default());
            return;
        }
        let config = cluster_net::HubConfig {
            port: self.settings.hub_port,
            web_dir: self.web_dir.clone(),
            ..Default::default()
        };
        match cluster_net::Hub::start(self.bus.clone(), cluster_net::Catalog::default(), config) {
            Ok(h) => {
                let url = h.url();
                log::info!("phones can open {url}");
                ui.set_qr_url(url.clone().into());
                ui.set_qr_image(crate::qr_image(&url));
                ui.set_qr_detail("".into());
                self.hub = Some(h);
                self.publish_catalog();
            }
            Err(e) => {
                log::warn!("hub not started: {e:#}");
                ui.set_qr_url("Hub couldn't start".into());
                ui.set_qr_detail(format!("{e:#}. Pick another hub port in Settings (P).").into());
                ui.set_qr_image(slint::Image::default());
            }
        }
    }

    pub fn source_kind(&self) -> SourceKind {
        match self.settings.source.as_str() {
            "demo" => SourceKind::Demo,
            "hub" => SourceKind::Hub(
                (!self.settings.hub_address.trim().is_empty()).then(|| self.settings.hub_address.trim().to_owned()),
            ),
            _ => SourceKind::Forza,
        }
    }

    pub fn ui_settings(&self) -> UiSettings {
        use cluster_core::units::*;
        let s = &self.settings;
        let u = &s.units;
        UiSettings {
            source: match s.source.as_str() {
                "demo" => 1,
                "hub" => 2,
                _ => 0,
            },
            forza_port: s.forza_port.to_string().into(),
            hub_address: s.hub_address.clone().into(),
            hub_enabled: s.hub_enabled,
            hub_port: s.hub_port.to_string().into(),
            speed: match u.speed {
                SpeedUnit::Kmh => 0,
                SpeedUnit::Mph => 1,
                SpeedUnit::Ms => 2,
            },
            temperature: match u.temperature {
                TempUnit::Celsius => 0,
                TempUnit::Fahrenheit => 1,
            },
            pressure: match u.pressure {
                PressureUnit::Bar => 0,
                PressureUnit::Psi => 1,
                PressureUnit::Kpa => 2,
            },
            power: match u.power {
                PowerUnit::Kw => 0,
                PowerUnit::Hp => 1,
                PowerUnit::Ps => 2,
            },
            torque: match u.torque {
                TorqueUnit::Nm => 0,
                TorqueUnit::LbFt => 1,
            },
            start_fullscreen: s.start_fullscreen,
            always_on_top: s.always_on_top,
        }
    }

    pub fn open_settings(&mut self) {
        let ui = self.ui();
        ui.set_ui_settings(self.ui_settings());
        ui.set_settings_message("".into());
        ui.set_show_settings(true);
    }

    /// Applies the settings panel; restarts the source/hub only if their settings changed.
    pub fn apply_ui_settings(&mut self, v: UiSettings) {
        use cluster_core::units::*;
        let ui = self.ui();
        let port = |t: &str, what: &str| {
            t.trim().parse::<u16>().ok().filter(|p| *p > 0).ok_or(format!("{what} must be a number from 1 to 65535"))
        };
        let (forza_port, hub_port) = match (port(&v.forza_port, "Forza port"), port(&v.hub_port, "Hub port")) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => {
                ui.set_settings_message(e.into());
                return;
            }
        };
        let mut s = self.settings.clone();
        s.source = ["forza", "demo", "hub"][v.source.clamp(0, 2) as usize].into();
        s.forza_port = forza_port;
        s.hub_address = v.hub_address.trim().to_owned();
        s.hub_enabled = v.hub_enabled;
        s.hub_port = hub_port;
        s.units = UnitPrefs {
            speed: [SpeedUnit::Kmh, SpeedUnit::Mph, SpeedUnit::Ms][v.speed.clamp(0, 2) as usize],
            temperature: [TempUnit::Celsius, TempUnit::Fahrenheit][v.temperature.clamp(0, 1) as usize],
            pressure: [PressureUnit::Bar, PressureUnit::Psi, PressureUnit::Kpa][v.pressure.clamp(0, 2) as usize],
            power: [PowerUnit::Kw, PowerUnit::Hp, PowerUnit::Ps][v.power.clamp(0, 2) as usize],
            torque: [TorqueUnit::Nm, TorqueUnit::LbFt][v.torque.clamp(0, 1) as usize],
            acceleration: s.units.acceleration,
        };
        s.start_fullscreen = v.start_fullscreen;
        s.always_on_top = v.always_on_top;

        let old = std::mem::replace(&mut self.settings, s.clone());
        if old.hub_enabled != s.hub_enabled || old.hub_port != s.hub_port {
            self.restart_hub();
        }
        let source_changed =
            old.source != s.source || old.forza_port != s.forza_port || old.hub_address != s.hub_address;
        if source_changed || old.hub_port != s.hub_port {
            self.sources.config.forza_port = s.forza_port;
            self.sources.config.own_hub_port = s.hub_enabled.then_some(s.hub_port);
            let kind = self.source_kind();
            self.sources.start(kind);
        }
        if old.units != s.units {
            self.units = s.units;
            let snap = self.last_snapshot.clone();
            self.view.set_units(s.units, &snap);
            self.publish_catalog();
        }
        ui.set_on_top(s.always_on_top);
        ui.set_forza_port_text(s.forza_port.to_string().into());
        // Persist only what was changed in the panel, so a one-off command-line override
        // (e.g. --source demo) doesn't become the saved default.
        let saved = &mut self.saved_settings;
        macro_rules! keep_changes {
            ($($f:ident),*) => { $( if old.$f != s.$f { saved.$f = s.$f.clone(); } )* };
        }
        keep_changes!(source, forza_port, hub_address, hub_enabled, hub_port, units, start_fullscreen, always_on_top);
        saved.save();
        ui.set_show_settings(false);
        self.toast("Settings saved");
    }

    /// Remembers the dashboard being shown for next launch.
    pub fn remember_dashboard(&mut self) {
        let label = self.lib.current().label();
        if self.saved_settings.dashboard != label {
            self.saved_settings.dashboard = label;
            self.saved_settings.save();
        }
    }

    pub fn ui(&self) -> MainWindow {
        self.ui.upgrade().expect("window alive")
    }

    pub fn toast(&mut self, text: &str) {
        self.ui().set_toast_text(text.into());
        self.toast_until = Some(Instant::now() + Duration::from_millis(1800));
    }

    /// Tells the hub which dashboards exist and which one is showing here. While
    /// editing, phones following this screen see the edits live.
    pub fn publish_catalog(&self) {
        let Some(hub) = &self.hub else { return };
        let current = self.lib.current().clone();
        let dashboards = self
            .lib
            .entries
            .iter()
            .filter_map(|e| {
                let text = match &self.editor {
                    Some(ed) if *e == current && ed.path.as_deref() == e.path() => Some(ed.doc.to_text()),
                    _ => e.text().ok(),
                };
                text.map(|t| (e.label(), t))
            })
            .collect();
        hub.set_catalog(cluster_net::Catalog {
            dashboards,
            current: current.label(),
            units: units_name(self.units).into(),
        });
    }

    /// Loads the library's current entry. On error, keeps the old dashboard and shows
    /// the problem; returns whether it loaded.
    pub fn load_current(&mut self) -> bool {
        let entry = self.lib.current().clone();
        self.watch(entry.path().map(PathBuf::from));
        match entry.load() {
            Ok((compiled, base)) => {
                self.view = DashboardView::new(compiled, self.units, base);
                let ui = self.ui();
                bind_view(&ui, &mut self.view);
                ui.set_error_text("".into());
                self.publish_catalog();
                true
            }
            Err(e) => {
                log::warn!("{e:#}");
                self.ui().set_error_text(format!("{e:#}").into());
                false
            }
        }
    }

    fn watch(&mut self, path: Option<PathBuf>) {
        self.watcher = None;
        let Some(path) = path else { return };
        let tx = self.reload_tx.clone();
        let target = path.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res
                && ev.paths.iter().any(|p| p.file_name() == target.file_name())
                && !matches!(ev.kind, notify::EventKind::Access(_))
            {
                let _ = tx.send(());
            }
        });
        match watcher {
            Ok(mut w) => {
                // Watch the folder: editors often save by replacing the file.
                use notify::Watcher;
                let dir = path.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
                if let Err(e) = w.watch(&dir, notify::RecursiveMode::NonRecursive) {
                    log::warn!("can't watch {}: {e}", dir.display());
                }
                self.watcher = Some(w);
            }
            Err(e) => log::warn!("file watching unavailable: {e}"),
        }
    }

    pub fn poll_reload(&mut self) {
        while self.reload_rx.try_recv().is_ok() {
            self.pending_reload = Some(Instant::now());
        }
        if self.editor.is_some() {
            // The editor owns the document; our own saves also land here.
            self.pending_reload = None;
            return;
        }
        // Debounce: editors write several events per save.
        if self.pending_reload.is_some_and(|t| t.elapsed() > Duration::from_millis(150)) {
            self.pending_reload = None;
            if self.load_current() {
                log::info!("reloaded {}", self.lib.current().label());
                self.toast("Reloaded");
            }
        }
    }

    pub fn step_dashboard(&mut self, delta: isize) {
        if self.editor.is_some() {
            return;
        }
        self.lib.step(delta);
        if self.load_current() {
            let name = self.view.name().to_owned();
            self.toast(&name);
            self.remember_dashboard();
        }
    }

    // ------------------------------------------------------------ editor

    pub fn toggle_edit(&mut self) {
        if self.editor.is_some() {
            self.exit_edit();
        } else {
            self.enter_edit();
        }
    }

    fn enter_edit(&mut self) {
        let entry = self.lib.current().clone();
        let value = match entry.text().and_then(|t| json5::from_str::<serde_json::Value>(&t).map_err(Into::into)) {
            Ok(v) => v,
            Err(e) => {
                self.ui().set_error_text(format!("Can't edit: {e:#}").into());
                return;
            }
        };
        let mut ed = Editor::new(Doc::new(value), entry.path().map(PathBuf::from));
        ed.from_builtin = entry.path().is_none();
        if entry.path().is_none() {
            ed.message = "Built-in dashboard — Save will make your own copy.".into();
        }
        self.start_editor(ed);
    }

    fn start_editor(&mut self, ed: Editor) {
        self.editor = Some(ed);
        let ui = self.ui();
        ui.set_edit_mode(true);
        ui.set_show_qr(false);
        self.apply_doc();
    }

    /// Leaves edit mode; asks about unsaved changes. Returns false if cancelled.
    pub fn exit_edit(&mut self) -> bool {
        let Some(ed) = &self.editor else { return true };
        if ed.doc.is_dirty() {
            match confirm_unsaved(&ed.name()) {
                Confirm::Save => {
                    if !self.save(false) {
                        return false;
                    }
                }
                Confirm::Discard => {}
                Confirm::Cancel => return false,
            }
        }
        self.editor = None;
        self.ui().set_edit_mode(false);
        self.load_current();
        true
    }

    /// Recompiles the edited document into the view and refreshes all editor UI.
    fn apply_doc(&mut self) {
        let Some(ed) = &self.editor else { return };
        let base = ed.base_dir();
        match ed.doc.compile() {
            Ok(compiled) => {
                let snap = self.last_snapshot.clone();
                self.view.replace(compiled, base, &snap);
                let ui = self.ui();
                bind_view(&ui, &mut self.view);
                ui.set_error_text("".into());
            }
            Err(e) => self.ui().set_error_text(e.to_string().into()),
        }
        self.sync_editor(true);
        self.publish_catalog();
    }

    pub fn on_change(&mut self, change: Change) {
        match change {
            Change::None => self.sync_editor(false),
            Change::Selection => self.sync_editor(true),
            Change::Rects(rects) => {
                for (i, r) in rects {
                    self.view.set_rect(i, r);
                }
                self.sync_editor(false);
            }
            Change::Doc => self.apply_doc(),
        }
    }

    /// Pushes selection, inspector rows and toolbar state to the window.
    fn sync_editor(&mut self, rows_changed: bool) {
        let Some(ed) = &self.editor else { return };
        let ui = self.ui();
        let order = self.view.order();
        let sel: Vec<bool> = order.iter().map(|i| ed.selected.contains(i)).collect();
        if self.selected_model.iter().collect::<Vec<_>>() != sel {
            self.selected_model.set_vec(sel);
        }
        let palette = self.view.compiled().palette;
        let rows = ed.rows(&palette);
        let same_keys = self.props_model.row_count() == rows.len()
            && self.props_model.iter().zip(&rows).all(|(a, b)| a.key == b.key && a.kind == b.kind);
        if same_keys {
            // Update in place so a field being typed into keeps focus.
            for (i, r) in rows.into_iter().enumerate() {
                let cur = self.props_model.row_data(i);
                if cur.as_ref().is_none_or(|c| c.value != r.value || c.error != r.error || c.swatch != r.swatch) {
                    self.props_model.set_row_data(i, r);
                }
            }
        } else if rows_changed || !same_keys {
            self.props_model.set_vec(rows);
        }
        let (swatches, names) = editor::theme_swatches(&palette);
        ui.set_theme_swatches(ModelRc::new(VecModel::from(swatches)));
        ui.set_theme_names(ModelRc::new(VecModel::from(names)));
        ui.set_selection_count(ed.selected.len() as i32);
        ui.set_inspector_title(ed.inspector_title().into());
        ui.set_can_undo(ed.doc.can_undo());
        ui.set_can_redo(ed.doc.can_redo());
        ui.set_dirty(ed.doc.is_dirty());
        ui.set_snap(ed.snap);
        let where_ = ed.path.as_ref().map_or_else(|| "not saved yet".to_owned(), |p| p.display().to_string());
        ui.set_editor_title(format!("{} — {where_}", ed.name()).into());
        ui.set_editor_message(ed.message.clone().into());
    }

    pub fn with_editor(&mut self, f: impl FnOnce(&mut Editor) -> Change) {
        let Some(ed) = &mut self.editor else { return };
        let change = f(ed);
        self.on_change(change);
    }

    /// Saves the edited dashboard (asking for a file when needed). Returns true if saved.
    pub fn save(&mut self, save_as: bool) -> bool {
        let Some(ed) = &mut self.editor else { return false };
        let path = match (&ed.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let dir = library::user_dir();
                let _ = std::fs::create_dir_all(&dir);
                let name = if ed.path.is_none() && ed.from_builtin { format!("{} copy", ed.name()) } else { ed.name() };
                match rfd::FileDialog::new()
                    .set_title("Save dashboard")
                    .set_directory(&dir)
                    .set_file_name(format!("{}.json5", library::slug(&name)))
                    .add_filter("Dashboard", &["json5", "json"])
                    .save_file()
                {
                    Some(p) => p,
                    None => return false,
                }
            }
        };
        let old_base = ed.base_dir();
        if let Err(e) = ed.save_to(&path) {
            ed.message = format!("Couldn't save: {e:#}");
            self.sync_editor(false);
            return false;
        }
        // Bring images along when saving somewhere new.
        if let (Some(old), Some(new)) = (old_base, ed.base_dir())
            && old != new
        {
            for rel in editor::referenced_assets(ed.doc.value()) {
                let (from, to) = (old.join(&rel), new.join(&rel));
                if from.is_file() && !to.exists() {
                    let _ = to.parent().map(std::fs::create_dir_all);
                    let _ = std::fs::copy(&from, &to);
                }
            }
        }
        self.lib.rescan();
        self.lib.select_path(&path);
        self.watch(Some(path));
        self.sync_editor(false);
        self.publish_catalog();
        true
    }

    pub fn new_dashboard(&mut self) {
        if let Some(ed) = &self.editor
            && ed.doc.is_dirty()
        {
            match confirm_unsaved(&ed.name()) {
                Confirm::Save => {
                    if !self.save(false) {
                        return;
                    }
                }
                Confirm::Discard => {}
                Confirm::Cancel => return,
            }
        }
        let mut ed = Editor::new(Doc::new(cluster_layout::edit::blank_layout("New dashboard")), None);
        ed.message = "Add widgets from the left, then Save.".into();
        self.start_editor(ed);
    }

    pub fn import(&mut self) {
        let Some(file) = rfd::FileDialog::new()
            .set_title("Import dashboard")
            .add_filter("DashCluster dashboard", &["cluster", "json5", "json"])
            .pick_file()
        else {
            return;
        };
        if let Some(ed) = &self.editor
            && ed.doc.is_dirty()
            && !self.exit_edit()
        {
            return;
        }
        let dir = library::user_dir();
        let result = (|| -> anyhow::Result<PathBuf> {
            std::fs::create_dir_all(&dir)?;
            if file.extension().is_some_and(|e| e == "cluster") {
                editor::import_package(&file, &dir)
            } else {
                let text = std::fs::read_to_string(&file)?;
                cluster_layout::Compiled::from_json5(&text)?;
                let stem = file.file_stem().map_or_else(|| "imported".into(), |s| library::slug(&s.to_string_lossy()));
                let target = library::unique_path(&dir, &stem, "json5");
                std::fs::write(&target, text)?;
                Ok(target)
            }
        })();
        match result {
            Ok(path) => {
                let was_editing = self.editor.take().is_some();
                self.lib.rescan();
                self.lib.select_path(&path);
                self.load_current();
                self.toast(&format!("Imported {}", self.lib.current().label()));
                if was_editing {
                    self.enter_edit();
                } else {
                    self.ui().set_edit_mode(false);
                }
            }
            Err(e) => self.ui().set_error_text(format!("Import failed: {e:#}").into()),
        }
    }

    pub fn export(&mut self) {
        let (doc, base, name) = match &self.editor {
            Some(ed) => (ed.doc.clone(), ed.base_dir(), ed.name()),
            None => {
                let entry = self.lib.current().clone();
                let Ok(v) = entry.text().and_then(|t| json5::from_str(&t).map_err(Into::into)) else { return };
                (Doc::new(v), entry.base_dir(), entry.label())
            }
        };
        let Some(out) = rfd::FileDialog::new()
            .set_title("Export dashboard")
            .set_file_name(format!("{}.cluster", library::slug(&name)))
            .add_filter("DashCluster package", &["cluster"])
            .save_file()
        else {
            return;
        };
        let msg = match editor::export_package(&doc, base.as_deref(), &out) {
            Ok(n) => format!("Exported {} ({n} image{})", out.display(), if n == 1 { "" } else { "s" }),
            Err(e) => format!("Export failed: {e:#}"),
        };
        match &mut self.editor {
            Some(ed) => {
                ed.message = msg;
                self.sync_editor(false);
            }
            None => self.toast(&msg),
        }
    }

    pub fn reveal_folder(&self) {
        let dir = library::user_dir();
        let _ = std::fs::create_dir_all(&dir);
        open_in_file_manager(&dir);
    }

    pub fn pick_image(&mut self, key: &str) {
        let Some(ed) = &mut self.editor else { return };
        let Some(base) = ed.base_dir() else {
            ed.message = "Save the dashboard first — images are stored next to its file.".into();
            self.sync_editor(false);
            return;
        };
        let Some(file) = rfd::FileDialog::new()
            .set_title("Choose image")
            .add_filter("Images", &["png", "jpg", "jpeg", "svg", "webp"])
            .pick_file()
        else {
            return;
        };
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let target = base.join(&name);
        if file != target
            && let Err(e) = std::fs::copy(&file, &target)
        {
            ed.message = format!("Couldn't copy image: {e}");
            self.sync_editor(false);
            return;
        }
        self.with_editor(|ed| ed.set_prop(key, &name));
    }
}
