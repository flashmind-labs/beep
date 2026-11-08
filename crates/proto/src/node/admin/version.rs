//! `version/1`: the node says which beep-node it runs and what time its
//! clock shows, in one message, and finishes the stream. A node from before
//! it declines the protocol, which reads as a version not known. `status/1`
//! is not changed to carry it.

use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "version/1";

/// A sample whose round trip took longer than this says nothing about the
/// clock.
const MAX_ROUND_TRIP_MS: u64 = 2_000;
/// A clock this far off, either way, is worth a warning.
const WARN_OFFSET_MS: i64 = 5_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    /// Such as "0.2.3".
    pub version: String,
    /// The node's Unix time in milliseconds when it answered; none from a
    /// node that does not say.
    #[serde(default)]
    pub now_ms: Option<u64>,
}

/// How far the remote clock is ahead of ours, in milliseconds (negative:
/// behind). `before_ms` and `after_ms` are our Unix time around the round
/// trip and `remote_ms` is what the other side answered; the answer is taken
/// to be from the middle of the trip. None when the trip took too long or
/// our clock went backwards during it.
pub fn clock_offset_ms(before_ms: u64, after_ms: u64, remote_ms: u64) -> Option<i64> {
    let trip = after_ms.checked_sub(before_ms)?;
    if trip > MAX_ROUND_TRIP_MS {
        return None;
    }
    let middle = i64::try_from(before_ms.checked_add(trip / 2)?).ok()?;
    i64::try_from(remote_ms).ok()?.checked_sub(middle)
}

/// Whether an offset is large enough to warn about.
pub fn clock_is_off(offset_ms: i64) -> bool {
    offset_ms.unsigned_abs() >= WARN_OFFSET_MS.unsigned_abs()
}

#[cfg(test)]
#[path = "../../tests/node_admin_version.rs"]
mod tests;
