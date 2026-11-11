use clap::{Parser, Subcommand};
use cluster_core::{SignalBus, SignalId as S, Snapshot, SourceRunner, UnitPrefs, format_gear};
use cluster_ui::MainWindow;
use slint::ComponentHandle;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "dashcluster", about = "Digital instrument cluster")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Render the dashboard headlessly to a PNG after simulating the demo car.
    Snapshot {
        #[arg(long, default_value = "snapshot.png")]
        out: PathBuf,
        /// Seconds of demo driving to simulate before rendering.
        #[arg(long, default_value_t = 20.0)]
        seconds: f32,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
    },
}

fn apply_frame(ui: &MainWindow, s: &Snapshot, units: &UnitPrefs, status: &str) {
    let rpm = s.get(S::Rpm).unwrap_or(0.0);
    let rpm_max = s.get(S::RpmMax).unwrap_or(8000.0);
    ui.set_rpm(rpm / 1000.0);
    ui.set_rpm_max((rpm_max / 1000.0).ceil());
    ui.set_rpm_red(rpm_max * 0.95 / 1000.0);
    let speed = s
        .get(S::Speed)
        .map(|v| units.convert(S::Speed.quantity(), v));
    ui.set_speed(speed.unwrap_or(0.0));
    ui.set_speed_text(speed.map_or("--".into(), |v| format!("{v:.0}")).into());
    ui.set_speed_unit(units.unit_label(S::Speed.quantity()).into());
    ui.set_gear_text(s.get(S::Gear).map_or("-".into(), format_gear).into());
    ui.set_status_text(status.into());
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    let units = UnitPrefs::default();

    if let Some(Command::Snapshot {
        out,
        seconds,
        width,
        height,
    }) = cli.command
    {
        let off = cluster_ui::offscreen::install(width, height)?;
        let ui = MainWindow::new()?;
        let mut car = source_demo::DemoCar::new();
        for _ in 0..(seconds * 60.0) as usize {
            car.step(1.0 / 60.0);
        }
        apply_frame(&ui, &car.snapshot(), &units, "snapshot");
        ui.show()?;
        off.render_png(&out)?;
        println!("wrote {}", out.display());
        return Ok(());
    }

    let bus = SignalBus::new();
    let _runner = SourceRunner::start(bus.clone(), Box::new(source_demo::DemoSource::default()));
    let ui = MainWindow::new()?;
    let weak = ui.as_weak();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let st = bus.status();
            let status = format!("{} · {:?} · {}", st.source, st.state, st.detail);
            apply_frame(&ui, &bus.latest().snapshot, &units, &status);
        },
    );
    ui.run()?;
    Ok(())
}
