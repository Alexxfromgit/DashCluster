//! Small networking helpers shared by the hub, the hub client and the game sources.

use crate::{Snapshot, SourceContext, SourceState};
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// mDNS service type DashCluster hubs advertise.
pub const MDNS_SERVICE: &str = "_dashcluster._tcp.local.";

/// TXT-record key carrying [`instance_id`], so discovery can recognize (and skip) itself.
pub const MDNS_ID_KEY: &str = "id";

/// This machine's LAN address: the interface that would route to the internet. Uses a
/// connected UDP socket, which picks the interface without sending anything.
pub fn lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?; // TEST-NET-1, never routed
    s.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_unspecified())
}

/// Run a UDP listener until stopped. The decoder returns a frame, `Ok(None)` for
/// packets that need no frame, or `Err(())` for invalid packets.
pub fn receive_snapshots(
    ctx: &SourceContext,
    port: u16,
    hint: &str,
    invalid_hint: &str,
    mut decode: impl FnMut(&[u8], SocketAddr) -> Result<Option<Snapshot>, ()>,
) -> anyhow::Result<()> {
    const READ_TIMEOUT: Duration = Duration::from_millis(100);
    const STALE_AFTER: Duration = Duration::from_secs(2);
    let socket =
        UdpSocket::bind(("0.0.0.0", port)).map_err(|e| anyhow::anyhow!("can't listen on UDP port {}: {e}", port))?;
    socket.set_read_timeout(Some(READ_TIMEOUT))?;
    ctx.set_state(SourceState::Waiting, hint);

    // Receive entire datagrams so an oversized packet cannot be mistaken for a supported layout.
    let mut buffer = vec![0; 65_536];
    let mut last_valid: Option<Instant> = None;
    let mut last_sender = None;
    let mut invalid = 0_u64;
    let mut receive_errors = 0_u64;
    let mut seq = 0_u64;
    while !ctx.should_stop() {
        if last_valid.is_some_and(|t| t.elapsed() >= STALE_AFTER) {
            last_valid = None;
            last_sender = None;
            invalid = 0;
            let mut clear = Snapshot::new();
            seq += 1;
            clear.seq = seq;
            ctx.publish(clear);
            ctx.set_state(SourceState::Waiting, hint);
        }
        let (len, sender) = match socket.recv_from(&mut buffer) {
            Ok(packet) => packet,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted) => {
                continue;
            }
            Err(e) => {
                receive_errors += 1;
                if receive_errors == 1 || receive_errors.is_multiple_of(100) {
                    log::warn!("UDP receive error on port {port} ({receive_errors} so far): {e}");
                }
                std::thread::sleep(READ_TIMEOUT);
                continue;
            }
        };
        let mut snapshot = match decode(&buffer[..len], sender) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => continue,
            Err(()) => {
                invalid += 1;
                if last_valid.is_none() && (invalid == 1 || invalid.is_multiple_of(300)) {
                    ctx.set_state(
                        SourceState::Waiting,
                        format!("Ignoring {len}-byte packet from {} · {invalid_hint}", sender.ip()),
                    );
                }
                continue;
            }
        };
        last_valid = Some(Instant::now());
        seq += 1;
        snapshot.seq = seq;
        ctx.publish(snapshot);
        let sender = (sender.ip(), len);
        if last_sender != Some(sender) {
            last_sender = Some(sender);
            ctx.set_state(SourceState::Live, format!("{}-byte packet from {}", sender.1, sender.0));
        }
    }
    Ok(())
}

/// A random ID for this running app, advertised by its hub.
pub fn instance_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u32(std::process::id());
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
        format!("{:016x}", h.finish())
    })
}

/// A short host name for display (no process spawning, so no console window on Windows).
pub fn host_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
        })
        .or_else(hostname_command)
        .map(|h| h.trim_end_matches(".local").to_owned())
        .unwrap_or_else(|| "dashcluster".into())
}

/// macOS has no `/etc/hostname` and usually no `HOSTNAME` variable.
#[cfg(not(windows))]
fn hostname_command() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Windows always has `COMPUTERNAME`; spawning a program would flash a console window.
#[cfg(windows)]
fn hostname_command() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn instance_id_is_stable_and_nonempty() {
        assert_eq!(super::instance_id(), super::instance_id());
        assert_eq!(super::instance_id().len(), 16);
        assert!(!super::host_name().is_empty());
    }
}
