//! The tray icon and its menu: Recent chats and what waits on the user.
//! Entry points: `install`, `refresh`, `became_linked`.

mod menu;
pub(crate) mod waiting;

#[cfg(not(target_os = "windows"))]
pub(crate) use menu::REPORT;
pub(crate) use menu::{Recent, became_linked, install, refresh, refresh_recent};
