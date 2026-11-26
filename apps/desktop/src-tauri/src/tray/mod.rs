//! The tray icon and its menu: what waits on the user.
//! Entry points: `install`, `refresh`.

#[cfg(desktop)]
mod menu;
#[cfg(mobile)]
#[path = "mobile.rs"]
mod menu;
pub(crate) mod waiting;

#[cfg(all(desktop, not(target_os = "windows")))]
pub(crate) use menu::REPORT;
#[cfg(desktop)]
pub(crate) use menu::follow_update;
pub(crate) use menu::{install, refresh};
