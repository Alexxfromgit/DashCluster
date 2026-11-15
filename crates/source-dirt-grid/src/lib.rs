//! DiRT and GRID telemetry over the legacy Codemasters UDP interface.

mod codec;

use cluster_core::{SourceContext, TelemetrySource, net::receive_snapshots};

pub const DEFAULT_PORT: u16 = 20777;

pub struct DirtGridSource {
    pub port: u16,
}

impl TelemetrySource for DirtGridSource {
    fn name(&self) -> String {
        format!("DiRT / GRID (UDP {})", self.port)
    }

    fn run(&mut self, ctx: &SourceContext) -> anyhow::Result<()> {
        let hint = format!("Listening on UDP {} · enable legacy UDP in the game's hardware settings", self.port);
        receive_snapshots(ctx, self.port, &hint, "use extradata 3, or 1 for GRID 2", |packet, _| {
            codec::parse(packet).map(Some).ok_or(())
        })
    }
}
