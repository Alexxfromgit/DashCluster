mod app;
mod editor;
mod library;
mod sources;

use app::{App, bind_view};
use clap::{Parser, Subcommand, ValueEnum};
use cluster_core::{SignalBus, SignalId, SourceState, UnitPrefs};
use cluster_ui::{DashboardView, MainWindow};
use library::Library;
use slint::ComponentHandle;
use sources::{SourceConfig, SourceKind, Sources};
use std::cell::RefCell;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "dashcluster", version, about = "Digital instrument cluster")]
struct Cli {
    /// Dashboard to show first: a built-in name (grand-tourer, race, rally,
    /// phone-portrait) or the name/path of one of the --dashboards files.
    #[arg(long, short = 'd', global = true)]
    dashboard: Option<String>,
    /// Extra dashboard files or folders to load (hot-reloaded on save).
    #[arg(long = "dashboards", value_name = "PATH", global = true)]
    dashboards: Vec<PathBuf>,
    #[arg(long, value_enum, default_value_t = Units::Metric, global = true)]
    units: Units,
    /// Where telemetry comes from. Press D in the app to toggle the demo car.
    #[arg(long, value_enum, default_value_t = Source::Forza, global = true)]
    source: Source,
    /// UDP port Forza sends to (Forza: Settings → HUD and Gameplay → Data Out IP Port).
    #[arg(long, default_value_t = source_forza::DEFAULT_PORT, global = true)]
    port: u16,
    /// Record raw Forza packets to this .ctlog file.
    #[arg(long, value_name = "FILE", global = true)]
    record: Option<PathBuf>,
    /// Re-send raw Forza packets to another tool (repeatable), e.g. 127.0.0.1:5301.
    #[arg(long, value_name = "ADDR", global = true)]
    forward: Vec<SocketAddr>,
    /// Recording to play with --source replay.
    #[arg(long, value_name = "FILE", global = true)]
    replay: Option<PathBuf>,
    /// Replay speed multiplier.
    #[arg(long, default_value_t = 1.0, global = true)]
    replay_speed: f32,
    /// Hub to mirror with --source hub (host:port). Omit to find one on the network.
    #[arg(long, value_name = "ADDR", global = true)]
    hub: Option<String>,
    /// Port for this app's hub (phones and other displays connect here).
    #[arg(long, default_value_t = cluster_net::DEFAULT_PORT, global = true)]
    hub_port: u16,
    /// Don't run a hub (no phone/network displays).
    #[arg(long, global = true)]
    no_hub: bool,
    /// Serve the web display from this folder instead of the built-in copy (development).
    #[arg(long, value_name = "DIR", global = true)]
    web_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Source {
    /// Forza Motorsport / Horizon Data Out (UDP)
    Forza,
    /// Built-in simulated car
    Demo,
    /// Play a .ctlog recording (see --replay)
    Replay,
    /// Mirror another DashCluster (see --hub)
    Hub,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
enum Units {
    Metric,
    Imperial,
}

impl Units {
    fn prefs(self) -> UnitPrefs {
        match self {
            Units::Metric => UnitPrefs::default(),
            Units::Imperial => UnitPrefs::imperial(),
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Render a dashboard headlessly to a PNG after simulating the demo car.
    Snapshot {
        #[arg(long, default_value = "snapshot.png")]
        out: PathBuf,
        /// Seconds of demo driving to simulate before rendering.
        #[arg(long, default_value_t = 20.0)]
        seconds: f32,
        /// Image width; height follows the dashboard's aspect ratio.
        #[arg(long, default_value_t = 1600)]
        width: u32,
        /// Run the configured --source in real time for --seconds instead of
        /// simulating the demo car (end-to-end check).
        #[arg(long)]
        live: bool,
        /// Render the editor UI with this widget selected (by id), for checking the editor.
        #[arg(long, value_name = "WIDGET_ID")]
        edit: Option<String>,
        /// Image height (default: follows the dashboard's aspect ratio).
        #[arg(long)]
        height: Option<u32>,
    },
}

fn source_kind(cli: &Cli) -> anyhow::Result<SourceKind> {
    Ok(match cli.source {
        Source::Forza => SourceKind::Forza,
        Source::Demo => SourceKind::Demo,
        Source::Replay => SourceKind::Replay(
            cli.replay.clone().ok_or_else(|| anyhow::anyhow!("--source replay needs --replay FILE"))?,
        ),
        Source::Hub => SourceKind::Hub(cli.hub.clone()),
    })
}

fn source_config(cli: &Cli) -> SourceConfig {
    SourceConfig {
        forza_port: cli.port,
        record: cli.record.clone(),
        forward: cli.forward.clone(),
        replay_speed: cli.replay_speed,
        own_hub_port: (!cli.no_hub).then_some(cli.hub_port),
    }
}

/// Renders a QR code for `text` as an image.
fn qr_image(text: &str) -> slint::Image {
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else { return slint::Image::default() };
    let n = code.width();
    let colors = code.to_colors();
    let (quiet, scale) = (4usize, 8usize);
    let side = (n + 2 * quiet) * scale;
    let mut buf = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(side as u32, side as u32);
    let px = buf.make_mut_slice();
    for y in 0..side {
        for x in 0..side {
            let (mx, my) = ((x / scale) as isize - quiet as isize, (y / scale) as isize - quiet as isize);
            let dark = mx >= 0
                && my >= 0
                && (mx as usize) < n
                && (my as usize) < n
                && colors[my as usize * n + mx as usize] == qrcode::Color::Dark;
            let v = if dark { 0 } else { 255 };
            px[y * side + x] = slint::Rgb8Pixel::new(v, v, v);
        }
    }
    slint::Image::from_rgb8(buf)
}

fn library(cli: &Cli) -> anyhow::Result<Library> {
    let mut lib = Library::new(&cli.dashboards);
    if let Some(name) = &cli.dashboard {
        anyhow::ensure!(lib.select(name), "no dashboard named {name:?}");
    } else if let Some(i) = lib.first_extra() {
        lib.current = i;
    }
    Ok(lib)
}

fn snapshot(
    cli: &Cli,
    out: &std::path::Path,
    seconds: f32,
    width: u32,
    live: bool,
    edit: Option<&str>,
    height: Option<u32>,
) -> anyhow::Result<()> {
    let lib = library(cli)?;
    let (compiled, base) = lib.current().load()?;
    let (cw, ch) = (compiled.layout.canvas.width, compiled.layout.canvas.height);
    let height = height.unwrap_or((width as f32 * ch / cw).round() as u32);
    let off = cluster_ui::offscreen::install(width, height)?;
    let ui = MainWindow::new()?;
    let mut view = DashboardView::new(compiled, cli.units.prefs(), base);
    let dt = 1.0 / 60.0;
    let snap = if live {
        let bus = SignalBus::new();
        let _sources = Sources::new(bus.clone(), source_config(cli), source_kind(cli)?);
        let end = Instant::now() + Duration::from_secs_f32(seconds);
        while Instant::now() < end {
            view.update(&bus.latest().snapshot, dt);
            std::thread::sleep(Duration::from_secs_f32(dt));
        }
        let st = bus.status();
        println!("source: {} · {:?} · {} · {} frames", st.source, st.state, st.detail, bus.publish_count());
        bus.latest().snapshot.clone()
    } else {
        let mut car = source_demo::DemoCar::new();
        for _ in 0..(seconds / dt) as usize {
            car.step(dt);
            view.update(&car.snapshot(), dt);
        }
        car.snapshot()
    };
    view.refresh_statics(&snap, false);
    bind_view(&ui, &mut view);
    ui.set_show_status(false);
    if let Some(id) = edit {
        let text = lib.current().text()?;
        let doc = cluster_layout::edit::Doc::new(json5::from_str(&text)?);
        let mut ed = editor::Editor::new(doc, None);
        ed.selected = ed.doc.widgets().iter().position(|w| w["id"] == id).into_iter().collect();
        let order = view.order().to_vec();
        let palette = view.compiled().palette;
        let sel: Vec<bool> = order.iter().map(|i| ed.selected.contains(i)).collect();
        ui.set_selected(slint::ModelRc::new(slint::VecModel::from(sel)));
        ui.set_props(slint::ModelRc::new(slint::VecModel::from(ed.rows(&palette))));
        ui.set_palette_items(slint::ModelRc::new(slint::VecModel::from(editor::palette_items())));
        let (sw, names) = editor::theme_swatches(&palette);
        ui.set_theme_swatches(slint::ModelRc::new(slint::VecModel::from(sw)));
        ui.set_theme_names(slint::ModelRc::new(slint::VecModel::from(names)));
        ui.set_selection_count(ed.selected.len() as i32);
        ui.set_inspector_title(ed.inspector_title().into());
        ui.set_editor_title(format!("{} — built-in", ed.name()).into());
        ui.set_editor_message("Built-in dashboard — Save will make your own copy.".into());
        ui.set_can_undo(true);
        ui.set_dirty(true);
        ui.set_edit_mode(true);
    }
    ui.show()?;
    off.render_png(out)?;
    println!("wrote {} ({width}×{height})", out.display());
    Ok(())
}

fn run(cli: &Cli) -> anyhow::Result<()> {
    let bus = SignalBus::new();
    let sources = Rc::new(RefCell::new(Sources::new(bus.clone(), source_config(cli), source_kind(cli)?)));

    let ui = MainWindow::new()?;
    let lib = library(cli)?;
    let hub = if cli.no_hub {
        None
    } else {
        let config = cluster_net::HubConfig { port: cli.hub_port, web_dir: cli.web_dir.clone(), ..Default::default() };
        match cluster_net::Hub::start(bus.clone(), cluster_net::Catalog::default(), config) {
            Ok(h) => {
                log::info!("phones can open {}", h.url());
                Some(h)
            }
            Err(e) => {
                log::warn!("hub not started: {e:#}");
                None
            }
        }
    };
    if let Some(h) = &hub {
        let url = h.url();
        ui.set_qr_url(url.clone().into());
        ui.set_qr_image(qr_image(&url));
    } else {
        ui.set_qr_url("Hub is off".into());
        ui.set_qr_detail("Start without --no-hub, or check that the port is free.".into());
    }
    let app = Rc::new(RefCell::new(App::new(&ui, lib, cli.units.prefs(), hub)));
    app.borrow_mut().load_current();

    // Small helper: run `f` on the app from a callback.
    macro_rules! on {
        ($setter:ident, |$a:ident $(, $arg:ident)*| $body:expr) => {{
            let app = app.clone();
            ui.$setter(move |$($arg),*| {
                let mut guard = app.borrow_mut();
                let $a = &mut *guard;
                $body
            });
        }};
    }

    on!(on_next_dashboard, |a| a.step_dashboard(1));
    on!(on_previous_dashboard, |a| a.step_dashboard(-1));
    {
        let bus = bus.clone();
        on!(on_toggle_units, |a| {
            a.units = if a.units == UnitPrefs::default() { UnitPrefs::imperial() } else { UnitPrefs::default() };
            let units = a.units;
            a.view.set_units(units, &bus.latest().snapshot);
            a.publish_catalog();
            a.toast(if units == UnitPrefs::default() { "Units: metric" } else { "Units: imperial" });
        });
    }
    {
        let sources = sources.clone();
        let bus = bus.clone();
        on!(on_toggle_demo, |a| {
            sources.borrow_mut().toggle_demo();
            let name = bus.status().source.clone();
            a.toast(&name);
        });
    }
    on!(on_toggle_qr, |a| {
        let ui = a.ui();
        ui.set_show_qr(!ui.get_show_qr());
    });
    on!(on_toggle_fullscreen, |a| {
        let ui = a.ui();
        let w = ui.window();
        w.set_fullscreen(!w.is_fullscreen());
    });
    on!(on_toggle_status, |a| {
        let ui = a.ui();
        ui.set_show_status(!ui.get_show_status());
    });

    // Editor
    on!(on_toggle_edit, |a| a.toggle_edit());
    on!(on_editor_pointer_down, |a, x, y, additive, handle| a.with_editor(|e| e.pointer_down(x, y, additive, handle)));
    on!(on_editor_pointer_move, |a, x, y| a.with_editor(|e| e.pointer_move(x, y)));
    on!(on_editor_pointer_up, |a| a.with_editor(|e| e.pointer_up()));
    on!(on_add_widget, |a, kind| a.with_editor(|e| e.add(&kind)));
    on!(on_prop_changed, |a, key, value| a.with_editor(|e| e.set_prop(&key, &value)));
    on!(on_undo, |a| a.with_editor(|e| e.undo()));
    on!(on_redo, |a| a.with_editor(|e| e.redo()));
    on!(on_delete_selected, |a| a.with_editor(|e| e.delete()));
    on!(on_duplicate_selected, |a| a.with_editor(|e| e.duplicate()));
    on!(on_clear_selection, |a| a.with_editor(|e| {
        e.selected.clear();
        editor::Change::Selection
    }));
    on!(on_nudge, |a, dx, dy| a.with_editor(|e| e.nudge(dx, dy)));
    on!(on_restack, |a, d| a.with_editor(|e| e.restack(i64::from(d))));
    on!(on_align, |a, how| a.with_editor(|e| e.align(&how)));
    on!(on_toggle_snap, |a| a.with_editor(|e| {
        e.snap = !e.snap;
        editor::Change::None
    }));
    on!(on_save, |a| {
        a.save(false);
    });
    on!(on_save_as, |a| {
        a.save(true);
    });
    on!(on_new_dashboard, |a| a.new_dashboard());
    on!(on_import_dashboard, |a| a.import());
    on!(on_export_dashboard, |a| a.export());
    on!(on_reveal_folder, |a| a.reveal_folder());
    on!(on_pick_image, |a, key| a.pick_image(&key));

    {
        let app = app.clone();
        ui.window().on_close_requested(move || {
            if app.borrow_mut().exit_edit() {
                slint::CloseRequestResponse::HideWindow
            } else {
                slint::CloseRequestResponse::KeepWindowShown
            }
        });
    }

    let timer = slint::Timer::default();
    let mut last = Instant::now();
    let mut rate_at = Instant::now();
    let mut rate_count = bus.publish_count();
    let mut rate = 0.0f32;
    {
        let app = app.clone();
        timer.start(slint::TimerMode::Repeated, Duration::from_millis(16), move || {
            let now = Instant::now();
            let dt = (now - last).as_secs_f32();
            last = now;
            let mut a = app.borrow_mut();
            a.poll_reload();
            let frame = bus.latest();
            a.view.update(&frame.snapshot, dt);
            a.last_snapshot = frame.snapshot.clone();

            if rate_at.elapsed() >= Duration::from_secs(1) {
                let c = bus.publish_count();
                rate = c.saturating_sub(rate_count) as f32 / rate_at.elapsed().as_secs_f32();
                rate_count = c;
                rate_at = Instant::now();
            }
            let st = bus.status();
            let stale = match st.state {
                SourceState::Live | SourceState::Waiting => bus.age_secs().is_none_or(|age| age > 0.5),
                _ => false,
            };
            let state = match &st.state {
                SourceState::Idle => "idle".to_owned(),
                SourceState::Waiting => "waiting".to_owned(),
                SourceState::Live => format!("{rate:.0} Hz"),
                SourceState::Error(e) => format!("error: {e}"),
            };
            let paused = frame.snapshot.get(SignalId::RaceOn) == Some(0.0);
            let editing = a.editor.is_some();
            let (title, detail) = match &st.state {
                _ if editing => (String::new(), String::new()),
                SourceState::Error(e) => ("Telemetry source stopped".to_owned(), e.clone()),
                _ if stale => {
                    ("Waiting for telemetry…".to_owned(), format!("{}\nPress D for the demo car.", st.detail))
                }
                _ if paused => ("Game paused or in a menu".to_owned(), String::new()),
                _ => (String::new(), String::new()),
            };
            let ui = a.ui();
            ui.set_overlay_title(title.into());
            ui.set_overlay_detail(detail.into());
            ui.set_stale(!editing && (stale || paused));
            ui.set_status_text(
                format!(
                    "{} · {} · {} · {}/{}{}  —  ←/→ dashboards · E edit · D demo · Q phone · U units · F fullscreen · S hide",
                    st.source,
                    state,
                    a.view.name(),
                    a.lib.current + 1,
                    a.lib.entries.len(),
                    a.hub.as_ref().map_or(String::new(), |h| match h.client_count() {
                        0 => String::new(),
                        1 => " · 1 display".into(),
                        n => format!(" · {n} displays"),
                    })
                )
                .into(),
            );
            if a.toast_until.is_some_and(|t| now >= t) {
                a.toast_until = None;
                ui.set_toast_text("".into());
            }
        });
    }
    ui.run()?;
    drop(sources);
    Ok(())
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match &cli.command {
        Some(Command::Snapshot { out, seconds, width, live, edit, height }) => {
            snapshot(&cli, out, *seconds, *width, *live, edit.as_deref(), *height)
        }
        None => run(&cli),
    }
}
