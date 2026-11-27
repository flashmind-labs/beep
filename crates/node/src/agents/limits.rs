//! Limits and timings the agents' requests share.

use std::time::Duration;

/// The most sessions kept, ended ones included; the oldest ended go first.
pub(crate) const MAX_KEPT: usize = 64;
/// The most transcript entries one request reads.
pub(crate) const MAX_TRANSCRIPT: u32 = 200;
/// The most folders `Folders` answers, and how many when the caller names
/// no number.
pub(crate) const MAX_FOLDERS: usize = 800;
pub(crate) const DEFAULT_FOLDERS: usize = 100;
/// The most sessions `InFolders` answers, and how many by default.
pub(crate) const MAX_IN_FOLDERS: usize = crate::occupancy::MAX_IN_FOLDERS;
pub(crate) const DEFAULT_IN_FOLDERS: usize = 50;
/// The most folders one `InFolders` names.
pub(crate) const MAX_ASKED_FOLDERS: usize = 32;
/// The largest picture `Image` answers, under the link's frame limit.
pub(crate) const MAX_IMAGE: usize = 7 * 1024 * 1024;
/// How often the plans' usage is read while the caller follows it, and
/// the least time between two reads it asks for.
pub(crate) const PLAN_EVERY: Duration = Duration::from_secs(5 * 60);
pub(crate) const PLAN_REFRESH_GAP: Duration = Duration::from_secs(30);
/// The least time between two reads of the agents' logs for what sessions
/// spent: a busy agent changes its log on every line.
pub(crate) const SPEND_GAP: Duration = Duration::from_secs(10);
/// How long a file request may wait for a look in the agents' logs.
pub(crate) const SIGHTING_WAIT: Duration = Duration::from_secs(5);
