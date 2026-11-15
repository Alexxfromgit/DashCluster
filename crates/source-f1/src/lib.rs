//! UDP telemetry from the F1 games.

mod codec;

use cluster_core::{SourceContext, TelemetrySource, net::receive_snapshots};

pub const DEFAULT_PORT: u16 = 20777;

pub struct F1Source {
    pub port: u16,
}

impl TelemetrySource for F1Source {
    fn name(&self) -> String {
        format!("F1 2019–25 (UDP {})", self.port)
    }

    fn run(&mut self, ctx: &SourceContext) -> anyhow::Result<()> {
        let mut decoder = codec::Decoder::default();
        let hint = format!("Listening on UDP {} · enable UDP Telemetry in the game's settings", self.port);
        receive_snapshots(ctx, self.port, &hint, "select an F1 UDP format from 2019 through 2025", |packet, sender| {
            decoder.decode(packet, sender)
        })
    }
}
