//! The command center: its windows, the view each opens on, Home, the
//! views kept for the next window and which chat each one shows.

pub(crate) mod home;
pub(crate) mod home_sessions;
pub(crate) mod shown;
pub(crate) mod targets;
pub(crate) mod usage;
pub(crate) mod view_cache;
pub(crate) mod window;

pub(crate) use home::{NeedsYou, is_unsupported, needs_you, section};
pub(crate) use shown::{AgentsShown, Chat, Seen, Showing, Shown, on_screen, remember_for};
pub(crate) use targets::{AgentTarget, CenterTarget, Pending, open_on};
pub(crate) use view_cache::{cache_view, kept};
pub(crate) use window::{hide_on_close, is_center, open};
