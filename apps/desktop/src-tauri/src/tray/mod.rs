//! The tray icon and its menu: what waits on the user.
//! Entry points: `install`, `refresh`.

mod follow;
#[cfg(desktop)]
pub(crate) mod menu;
#[cfg(mobile)]
#[path = "mobile.rs"]
pub(crate) mod menu;
pub(crate) mod waiting;

#[cfg(desktop)]
pub(crate) use follow::follow_update;
pub(crate) use follow::follow_waiting;
pub(crate) use menu::{install, refresh};
