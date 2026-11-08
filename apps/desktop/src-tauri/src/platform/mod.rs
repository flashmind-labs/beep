//! The desktop app: its log, login `PATH`, installed editors and updates.

pub(crate) mod editors;

mod open;
pub(crate) use open::{open, reveal};

pub(crate) mod logfile;
#[cfg(all(unix, desktop))]
pub(crate) mod login_path;
pub(crate) mod metrics;
#[cfg(desktop)]
pub(crate) mod update;
#[cfg(mobile)]
#[path = "update_mobile.rs"]
pub(crate) mod update;
