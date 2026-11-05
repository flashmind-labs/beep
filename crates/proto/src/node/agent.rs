//! Coding agents on a node, such as Claude Code or Codex, driven over ACP by
//! the daemon.
//!
//! Each agent is its own protocol, `<agent>/1` (`claude/1`, `codex/1`), and
//! all of them speak the messages here. A stream is one agent session: the
//! server opens it with `Start` or `Attach`, the node answers `Attached` with
//! the session as it stands, then the server steers it with `Prompt`,
//! `Answer`, `Cancel` and `Close` (each answered by one `Ack`) while the node
//! sends `State`, `Question` and `TurnDone` as they happen. Sessions live in
//! the daemon, not in the stream: when the stream drops the agent keeps
//! working, and a later `Attach` picks it up again.
//!
//! `agents/1` lists the sessions and reads their transcripts, one request per
//! stream. It also lists the sessions the user started on the machine
//! outside Beep (`External`), such as Claude Code in a terminal, and
//! `Watch` follows them: one request, then `Activity` replies until the
//! server closes the stream. `Resume` takes one of them over once nothing
//! else runs it.
mod checkout;
mod external;
mod models;
mod moving;
mod session;
mod sessions;

pub use checkout::*;
pub use external::*;
pub use models::*;
pub use moving::*;
pub use session::*;
pub use sessions::*;

/// The version every agent protocol shares.
const VERSION: &str = "1";

/// Lists sessions and reads transcripts.
pub const SESSIONS_PROTOCOL: &str = "agents/1";

/// The protocol for the agent `name`, such as `claude/1`.
pub fn protocol(name: &str) -> String {
    format!("{name}/{VERSION}")
}

/// The agent a protocol name serves, such as `claude` for `claude/1`.
pub fn agent_of(protocol: &str) -> Option<&str> {
    let (name, version) = protocol.split_once('/')?;
    (version == VERSION && super::harness::harness(name).is_some()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocols_name_their_agent() {
        assert_eq!(protocol("claude"), "claude/1");
        assert_eq!(agent_of("codex/1"), Some("codex"));
        assert_eq!(agent_of("shell/1"), None);
        assert_eq!(agent_of("files/1"), None);
        assert_eq!(agent_of("process/1"), None);
        assert_eq!(agent_of(crate::node::sync::PROTOCOL), None);
        assert_eq!(agent_of(crate::node::terminal::PROTOCOL), None);
        assert_eq!(agent_of(crate::node::transfer::PROTOCOL), None);
        assert_eq!(agent_of(crate::node::tool_update::PROTOCOL), None);
        assert_eq!(agent_of(crate::node::update::PROTOCOL), None);
        assert_eq!(agent_of(SESSIONS_PROTOCOL), None);
        assert_eq!(agent_of("claude/2"), None);
    }
}
