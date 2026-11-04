//! Crons and the places they can start on.

use serde::{Deserialize, Serialize};

wire_enum! {
    /// Arriving at a saved place or leaving it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
    pub enum PlaceCrossing {
        0 Arrive {}
        1 Leave {}
    }
}

/// A place the user saved by name, such as Home or Gym, that routines can
/// start on arriving at or leaving.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    /// Within this many metres counts as being there.
    pub radius: u32,
    /// A street address or area name, when known.
    pub address: Option<String>,
    pub created_at: String,
    /// Whether the phone's last location was there.
    pub here: bool,
    /// When the phone was last there.
    pub last_here_at: Option<String>,
    /// How many routines start on arriving at or leaving it.
    pub routines: u32,
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cron {
    pub id: String,
    pub label: String,
    pub prompt: String,
    /// RFC 3339 for a one-off, a place trigger as `CreateCron` describes,
    /// else a cron expression.
    pub schedule: String,
    pub enabled: bool,
    pub created_at: String,
    pub run_once: bool,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
}
