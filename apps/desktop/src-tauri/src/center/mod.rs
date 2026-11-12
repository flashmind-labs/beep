//! The command center: its windows, its app menu, the view each opens on,
//! Home, the views kept for the next window and which agent session each
//! one shows.

#[cfg(desktop)]
pub(crate) mod app_menu;
pub(crate) mod home;
pub(crate) mod home_sessions;
pub(crate) mod shown;
pub(crate) mod targets;
pub(crate) mod usage;
pub(crate) mod view_cache;
pub(crate) mod window;

pub(crate) use home::{NeedsYou, needs_you, section};
pub(crate) use shown::AgentsShown;
pub(crate) use targets::{AgentTarget, CenterTarget, Pending, RunAt, open_on};
pub(crate) use view_cache::{cache_view, kept};
pub(crate) use window::{closed, hide_on_close, is_center, open};
