mod library;
mod sources;

use clap::{Parser, Subcommand, ValueEnum};
use cluster_core::{SignalBus, SignalId, SourceState, UnitPrefs};
use cluster_ui::{DashboardView, MainWindow};
use library::Library;
use notify::Watcher;
use slint::ComponentHandle;
use sources::{SourceConfig, SourceKind, Sources};
use std::cell::RefCell;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
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
    },
}

fn source_kind(cli: &Cli) -> anyhow::Result<SourceKind> {
    Ok(match cli.source {
        Source::Forza => SourceKind::Forza,
        Source::Demo => SourceKind::Demo,
        Source::Replay => SourceKind::Replay(
            cli.replay.clone().ok_or_else(|| anyhow::anyhow!("--source replay needs --replay FILE"))?,
        ),
    })
}

fn source_config(cli: &Cli) -> SourceConfig {
    SourceConfig {
        forza_port: cli.port,
        record: cli.record.clone(),
        forward: cli.forward.clone(),
        replay_speed: cli.replay_speed,
    }
}

fn library(cli: &Cli) -> anyhow::Result<Library> {
    let mut lib = Library::new(&cli.dashboards);
    if let Some(name) = &cli.dashboard {
        anyhow::ensure!(lib.select(name), "no dashboard named {name:?}");
    } else if !cli.dashboards.is_empty() {
        lib.current = cluster_layout::BUILTIN.len().min(lib.entries.len() - 1);
    }
    Ok(lib)
}

fn snapshot(cli: &Cli, out: &std::path::Path, seconds: f32, width: u32, live: bool) -> anyhow::Result<()> {
    let lib = library(cli)?;
    let (compiled, base) = lib.current().load()?;
    let (cw, ch) = (compiled.layout.canvas.width, compiled.layout.canvas.height);
    let height = (width as f32 * ch / cw).round() as u32;
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
    ui.show()?;
    off.render_png(out)?;
    println!("wrote {} ({width}×{height})", out.display());
    Ok(())
}

/// Points the window at a view's models and canvas.
fn bind_view(ui: &MainWindow, view: &mut DashboardView) {
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

struct App {
    ui: slint::Weak<MainWindow>,
    lib: Library,
    view: DashboardView,
    units: UnitPrefs,
    watcher: Option<notify::RecommendedWatcher>,
    reload_rx: mpsc::Receiver<()>,
    reload_tx: mpsc::Sender<()>,
    pending_reload: Option<Instant>,
    toast_until: Option<Instant>,
}

impl App {
    fn ui(&self) -> MainWindow {
        self.ui.upgrade().expect("window alive")
    }

    fn toast(&mut self, text: &str) {
        self.ui().set_toast_text(text.into());
        self.toast_until = Some(Instant::now() + Duration::from_millis(1500));
    }

    /// Loads the library's current entry. On error, keeps the old dashboard and shows
    /// the problem; returns whether it loaded.
    fn load_current(&mut self) -> bool {
        let entry = self.lib.current().clone();
        self.watch(entry.path().map(PathBuf::from));
        match entry.load() {
            Ok((compiled, base)) => {
                self.view = DashboardView::new(compiled, self.units, base);
                let ui = self.ui();
                bind_view(&ui, &mut self.view);
                ui.set_error_text("".into());
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
                let dir = path.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
                if let Err(e) = w.watch(&dir, notify::RecursiveMode::NonRecursive) {
                    log::warn!("can't watch {}: {e}", dir.display());
                }
                self.watcher = Some(w);
            }
            Err(e) => log::warn!("file watching unavailable: {e}"),
        }
    }

    fn poll_reload(&mut self) {
        while self.reload_rx.try_recv().is_ok() {
            self.pending_reload = Some(Instant::now());
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
}

fn run(cli: &Cli) -> anyhow::Result<()> {
    let bus = SignalBus::new();
    let sources = Rc::new(RefCell::new(Sources::new(bus.clone(), source_config(cli), source_kind(cli)?)));

    let ui = MainWindow::new()?;
    let lib = library(cli)?;
    let (compiled, base) = cluster_layout::BUILTIN
        .first()
        .map(|(_, src)| (cluster_layout::Compiled::from_json5(src).expect("built-in dashboard"), None))
        .expect("built-ins exist");
    let units = cli.units.prefs();
    let (reload_tx, reload_rx) = mpsc::channel();
    let app = Rc::new(RefCell::new(App {
        ui: ui.as_weak(),
        lib,
        view: DashboardView::new(compiled, units, base),
        units,
        watcher: None,
        reload_rx,
        reload_tx,
        pending_reload: None,
        toast_until: None,
    }));
    app.borrow_mut().load_current();

    let step = |delta: isize| {
        let app = app.clone();
        move || {
            let mut a = app.borrow_mut();
            a.lib.step(delta);
            if a.load_current() {
                let name = a.view.name().to_owned();
                a.toast(&name);
            }
        }
    };
    ui.on_next_dashboard(step(1));
    ui.on_previous_dashboard(step(-1));
    {
        let app = app.clone();
        let bus = bus.clone();
        ui.on_toggle_units(move || {
            let mut a = app.borrow_mut();
            a.units = if a.units == UnitPrefs::default() { UnitPrefs::imperial() } else { UnitPrefs::default() };
            let units = a.units;
            a.view.set_units(units, &bus.latest().snapshot);
            let label = if units == UnitPrefs::default() { "Units: metric" } else { "Units: imperial" };
            a.toast(label);
        });
    }
    {
        let sources = sources.clone();
        let app = app.clone();
        let bus = bus.clone();
        ui.on_toggle_demo(move || {
            sources.borrow_mut().toggle_demo();
            let name = bus.status().source.clone();
            app.borrow_mut().toast(&name);
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_toggle_fullscreen(move || {
            if let Some(ui) = weak.upgrade() {
                let w = ui.window();
                w.set_fullscreen(!w.is_fullscreen());
            }
        });
        let weak = ui.as_weak();
        ui.on_toggle_status(move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_show_status(!ui.get_show_status());
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
            let (title, detail) = match &st.state {
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
            ui.set_stale(stale || paused);
            ui.set_status_text(
                format!(
                    "{} · {} · {} · {}/{}  —  ←/→ dashboards · D demo · U units · F fullscreen · S hide",
                    st.source,
                    state,
                    a.view.name(),
                    a.lib.current + 1,
                    a.lib.entries.len()
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
        Some(Command::Snapshot { out, seconds, width, live }) => snapshot(&cli, out, *seconds, *width, *live),
        None => run(&cli),
    }
}
