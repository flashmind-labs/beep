//! The command center: its windows, its app menu, the view each opens on,
//! and Home.

#[cfg(desktop)]
pub(crate) mod app_menu;
pub(crate) mod home;
pub(crate) mod home_frame;
pub(crate) mod sections;
pub(crate) mod targets;
pub(crate) mod usage;
pub(crate) mod window;

pub(crate) use home::{NeedsYou, needs_you};
pub(crate) use sections::section;
pub(crate) use targets::{AgentTarget, CenterTarget, Pending, RunAt, open_on};
pub(crate) use window::{closed, hide_on_close, is_center, open};
