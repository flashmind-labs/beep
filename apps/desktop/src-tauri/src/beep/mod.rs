//! This computer's key and the choices the app saves between launches.

pub(crate) mod identity;
pub(crate) mod state;

use std::time::SystemTime;

/// `at` in milliseconds since the Unix epoch, as the pages read times.
/// A time before the epoch reads as the epoch.
pub fn epoch_ms(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}
