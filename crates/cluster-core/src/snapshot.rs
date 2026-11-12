//! A point-in-time set of signal values.

use crate::signal::{SignalId, SignalSet};

/// One frame of telemetry. Values are in base units (see [`crate::Quantity`]).
/// A missing value means the source didn't provide that signal in this frame.
#[derive(Clone)]
pub struct Snapshot {
    /// Monotonic frame counter set by the source.
    pub seq: u64,
    values: [f32; SignalId::COUNT],
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot { seq: 0, values: [f32::NAN; SignalId::COUNT] }
    }
}

impl Snapshot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: SignalId) -> Option<f32> {
        let v = self.values[id.index()];
        (!v.is_nan()).then_some(v)
    }

    /// Sets a value. Non-finite values are treated as "missing".
    pub fn set(&mut self, id: SignalId, value: f32) {
        self.values[id.index()] = if value.is_finite() { value } else { f32::NAN };
    }

    pub fn set_opt(&mut self, id: SignalId, value: Option<f32>) {
        match value {
            Some(v) => self.set(id, v),
            None => self.clear(id),
        }
    }

    pub fn set_flag(&mut self, id: SignalId, on: bool) {
        self.set(id, if on { 1.0 } else { 0.0 });
    }

    pub fn clear(&mut self, id: SignalId) {
        self.values[id.index()] = f32::NAN;
    }

    pub fn present(&self) -> SignalSet {
        let mut s = SignalSet::EMPTY;
        for (id, _) in self.iter() {
            s.insert(id);
        }
        s
    }

    /// Present values in ID order.
    pub fn iter(&self) -> impl Iterator<Item = (SignalId, f32)> + '_ {
        self.values
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_nan())
            .filter_map(|(i, &v)| SignalId::from_index(i).map(|id| (id, v)))
    }
}

impl PartialEq for Snapshot {
    /// Equal when the same signals are present with the same values (missing == missing).
    fn eq(&self, other: &Self) -> bool {
        self.seq == other.seq
            && self
                .values
                .iter()
                .zip(other.values.iter())
                .all(|(a, b)| a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()))
    }
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut m = f.debug_map();
        m.entry(&"seq", &self.seq);
        for (id, v) in self.iter() {
            m.entry(&id.name(), &v);
        }
        m.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_by_default_and_rejects_non_finite() {
        let mut s = Snapshot::new();
        assert_eq!(s.get(SignalId::Rpm), None);
        s.set(SignalId::Rpm, 4200.0);
        assert_eq!(s.get(SignalId::Rpm), Some(4200.0));
        s.set(SignalId::Rpm, f32::INFINITY);
        assert_eq!(s.get(SignalId::Rpm), None);
    }
}
