//! Remembered latency results.
//!
//! A delay number is only meaningful with the moment it was measured and the
//! way it was measured, so both travel with it. Keeping the last result on disk
//! is what makes the node list honest after a restart: without it every node
//! would read "未测" again and the automatic sweep would be work with no trace.
//!
//! Two states that must not be confused:
//! * no record at all — this node has never been probed;
//! * a record with `delay_ms: None` — the probe ran and got nothing back
//!   ("测不到"). The reason is deliberately not stored: the next probe replaces
//!   it anyway, and a stale explanation is worse than none.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use specta::Type;

/// How a delay was measured. The number means different things, so the record
/// keeps the distinction even though the list prints only one figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ProbeKind {
    /// TCP handshake straight to the node's own port: no kernel involved.
    Tcp,
    /// One request through the running kernel's Clash API for that outbound.
    Real,
}

impl Default for ProbeKind {
    fn default() -> Self {
        Self::Tcp
    }
}

/// The last latency measurement of one node, keyed by node id in the document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct NodeProbe {
    /// `None` means the probe ran and produced no number.
    #[serde(default)]
    pub delay_ms: Option<u32>,
    /// Unix seconds, not milliseconds: the whole record is one line of JSON and
    /// sub-second precision would only be noise.
    ///
    /// Stored as `i64` because that is what a timestamp is; handed to the
    /// frontend as `f64` because a second count is exact in a double and the
    /// contract cannot carry 64-bit integers.
    #[serde(default)]
    #[specta(type = f64)]
    pub at: i64,
    #[serde(default)]
    pub kind: ProbeKind,
}

impl NodeProbe {
    /// A record stamped with the current wall clock.
    pub fn measured(delay_ms: Option<u32>, kind: ProbeKind) -> Self {
        Self {
            delay_ms,
            at: unix_seconds(),
            kind,
        }
    }
}

/// Seconds since the Unix epoch.
///
/// A clock set before 1970 is not a reason to panic or to drop a result: it
/// becomes `0`, which every reader treats as "no time to show".
pub fn unix_seconds() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_secs().min(i64::MAX as u64) as i64,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_record_carries_the_current_time() {
        let probe = NodeProbe::measured(Some(120), ProbeKind::Tcp);
        // Anything above zero is a usable timestamp; the exact second depends on
        // when the test runs, so only the presence is asserted.
        assert!(probe.at > 0);
        assert_eq!(probe.delay_ms, Some(120));
        assert_eq!(probe.kind, ProbeKind::Tcp);
    }

    #[test]
    fn a_record_without_a_time_is_recognisable() {
        // Writing the document by hand or with a clock set before 1970 must not
        // make the list pretend the result is fresh: `at == 0` is the one value
        // the front end renders as "no time to show".
        let probe = NodeProbe {
            delay_ms: Some(120),
            at: 0,
            kind: ProbeKind::Real,
        };
        assert_eq!(probe.at, 0);
        // A record materialised from an empty object has no time either: every
        // field carries `#[serde(default)]`, so nothing has to be present.
        let blank: NodeProbe = serde_json::from_str("{}").expect("an empty record is valid");
        assert_eq!(blank.at, 0);
        assert_eq!(blank.delay_ms, None);
        assert_eq!(blank.kind, ProbeKind::Tcp);
    }
}
