//! What Claude Code's hooks report of sessions started outside Beep: each
//! session's state and the message Beep queued for it.

pub mod hooks;
mod live;
mod watch;

pub(crate) use live::Live;
pub(crate) use watch::{Hub, listen};

/// What the hooks report: the events every stream follows, and each
/// session's state.
#[derive(Default)]
pub(crate) struct Hooks {
    pub(super) hub: Hub,
    pub(super) live: Live,
}
