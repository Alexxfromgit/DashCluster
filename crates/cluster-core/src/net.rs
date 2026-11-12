//! Small networking helpers shared by the hub, the hub client and the game sources.

use std::net::{IpAddr, UdpSocket};
use std::sync::OnceLock;

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
