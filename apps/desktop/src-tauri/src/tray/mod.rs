//! The tray icon and its menu: Recent chats and what waits on the user.
//! Entry points: `install`, `refresh`, `became_linked`.

#[cfg(desktop)]
mod menu;
#[cfg(mobile)]
#[path = "mobile.rs"]
mod menu;
pub(crate) mod waiting;

#[cfg(all(desktop, not(target_os = "windows")))]
pub(crate) use menu::REPORT;
pub(crate) use menu::{Recent, became_linked, install, refresh, refresh_recent};
