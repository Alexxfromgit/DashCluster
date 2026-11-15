//! Local shared-memory telemetry for the Windows releases of ACC and Le Mans Ultimate.

#[cfg(windows)]
mod acc;
#[cfg(windows)]
mod lmu;
#[cfg(windows)]
mod windows;

use cluster_core::{SourceContext, TelemetrySource};

pub enum SharedMemorySource {
    Acc,
    Lmu,
}

impl TelemetrySource for SharedMemorySource {
    fn name(&self) -> String {
        match self {
            Self::Acc => "Assetto Corsa Competizione",
            Self::Lmu => "Le Mans Ultimate",
        }
        .into()
    }

    fn run(&mut self, ctx: &SourceContext) -> anyhow::Result<()> {
        #[cfg(windows)]
        {
            match self {
                Self::Acc => windows::poll(ctx, acc::Acc::open, acc::Acc::sample),
                Self::Lmu => windows::poll(ctx, lmu::Lmu::open, lmu::Lmu::sample),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = ctx;
            anyhow::bail!("{} shared memory requires DashCluster on the Windows PC running the game", self.name());
        }
    }
}
