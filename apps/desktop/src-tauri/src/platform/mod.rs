//! The desktop app: its log, login `PATH`, installed editors and updates.

pub(crate) mod editors;

mod open;
pub(crate) use open::{open, reveal};

pub(crate) mod logfile;
#[cfg(unix)]
pub(crate) mod login_path;
pub(crate) mod metrics;
pub(crate) mod update;
