//! The desktop app: its log, login `PATH`, installed editors and updates.

pub(crate) mod editors;
#[cfg(all(desktop, feature = "resource-profiling"))]
pub(crate) mod energy;

#[cfg(target_os = "ios")]
pub mod ios_keyboard;
#[cfg(target_os = "ios")]
pub(crate) mod ios_share;
mod open;
pub(crate) use open::{open, reveal, saves_dir};

pub(crate) mod logfile;
#[cfg(all(unix, desktop))]
pub(crate) mod login_path;
pub(crate) mod metrics;
pub(crate) mod startup;
#[cfg(all(desktop, not(target_os = "macos")))]
pub(crate) mod update;
#[cfg(target_os = "macos")]
#[path = "update_macos.rs"]
pub(crate) mod update;
#[cfg(mobile)]
#[path = "update_mobile.rs"]
pub(crate) mod update;
