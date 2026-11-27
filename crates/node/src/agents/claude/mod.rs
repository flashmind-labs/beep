//! Claude Code sessions started outside Beep: watching them for a stream,
//! and whether one can be taken over through the ACP adapter. What their
//! hooks report is the occupancy's.

pub use crate::occupancy::claude::hooks;
pub(crate) mod watch;

use beep_proto::node::agent::{ExternalSession, Presence};

/// Why `found` cannot be taken over, when it cannot.
pub(crate) fn refuse_takeover(found: &ExternalSession) -> Option<String> {
    if found.presence.is_live() {
        let process = found
            .pid
            .map(|pid| format!(" (process {pid})"))
            .unwrap_or_default();
        let how = match found.presence {
            Presence::Recent => "was active a moment ago".to_owned(),
            presence => format!("is {presence}"),
        };
        return Some(format!(
            "session {} {how} in a terminal or IDE{process}, so Beep can only follow it. \
             Once the user exits it there, it can be taken over.",
            found.id
        ));
    }

    if found.cwd.is_empty() {
        return Some(format!(
            "session {} does not record the folder it ran in",
            found.id
        ));
    }

    None
}
