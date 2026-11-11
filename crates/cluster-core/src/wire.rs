//! Hub ↔ display protocol over WebSocket.
//!
//! - **Binary messages** carry telemetry: `b'F'`, `u32 LE seq`, then `(u8 signal index,
//!   f32 LE value)` per present signal. ~5 bytes per signal, so a full frame is < 300 B.
//! - **Text messages** carry JSON control messages ([`ServerMsg`], [`ClientMsg`]).

use crate::signal::SignalId;
use crate::snapshot::Snapshot;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;
const FRAME_TAG: u8 = b'F';

pub fn encode_frame(s: &Snapshot) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + 5 * SignalId::COUNT);
    out.push(FRAME_TAG);
    out.extend_from_slice(&(s.seq as u32).to_le_bytes());
    for (id, v) in s.iter() {
        out.push(id.index() as u8);
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

pub fn decode_frame(b: &[u8]) -> Option<Snapshot> {
    if b.len() < 5 || b[0] != FRAME_TAG || !(b.len() - 5).is_multiple_of(5) {
        return None;
    }
    let mut s = Snapshot::new();
    s.seq = u64::from(u32::from_le_bytes([b[1], b[2], b[3], b[4]]));
    for c in b[5..].as_chunks::<5>().0 {
        if let Some(id) = SignalId::from_index(c[0] as usize) {
            s.set(id, f32::from_le_bytes([c[1], c[2], c[3], c[4]]));
        }
    }
    Some(s)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Hello {
        version: u32,
        /// Dashboards the hub offers, by name.
        dashboards: Vec<String>,
        /// The hub's current dashboard.
        current: String,
        /// "metric" or "imperial".
        units: String,
    },
    /// A dashboard's JSON5 source.
    Layout {
        name: String,
        source: String,
    },
    Status {
        source: String,
        state: String,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Ask for a dashboard by name (sent back as [`ServerMsg::Layout`]).
    Select { name: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip() {
        let mut s = Snapshot::new();
        s.seq = 77;
        s.set(SignalId::Rpm, 6500.5);
        s.set(SignalId::PosZ, -12.25);
        let b = encode_frame(&s);
        assert_eq!(b.len(), 5 + 10);
        assert_eq!(decode_frame(&b), Some(s));
        assert_eq!(decode_frame(&b[..7]), None);
        assert_eq!(
            decode_frame(b"Fxxxx"),
            Some({
                let mut e = Snapshot::new();
                e.seq = u64::from(u32::from_le_bytes(*b"xxxx"));
                e
            })
        );
    }

    #[test]
    fn control_messages_are_tagged_json() {
        let m = ClientMsg::Select { name: "race".into() };
        assert_eq!(serde_json::to_string(&m).unwrap(), r#"{"type":"select","name":"race"}"#);
        let h: ServerMsg =
            serde_json::from_str(r#"{"type":"status","source":"Forza","state":"live","detail":""}"#).unwrap();
        assert!(matches!(h, ServerMsg::Status { .. }));
    }
}
