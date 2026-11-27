//! How a session stands, and what another session saw of it.

use super::Live;
use beep_proto::node::agent::{Status, StopReason};

/// How a relative stands, as the context says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::AsRefStr)]
#[strum(serialize_all = "snake_case")]
pub(in crate::agents) enum Stand {
    Working,
    WaitingOnUser,
    Idle,
    /// Idle since the user stopped its turn.
    Stopped,
    Ended,
}

impl From<&Live> for Stand {
    fn from(live: &Live) -> Self {
        let state = &live.info.state;

        match state.status {
            Status::Ended => Self::Ended,
            Status::Asking => Self::WaitingOnUser,
            Status::Idle
                if live.stopped_by_user && state.last_stop == Some(StopReason::Cancelled) =>
            {
                Self::Stopped
            }
            Status::Idle if state.subagents == 0 => Self::Idle,
            _ => Self::Working,
        }
    }
}

/// What a session saw of a relative: told again once it differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mark {
    pub(in crate::agents) turns: u32,
    pub(in crate::agents) stand: Stand,
}

impl From<&Live> for Mark {
    fn from(live: &Live) -> Self {
        Self {
            turns: live.info.turns,
            stand: Stand::from(live),
        }
    }
}
