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
mod session;
mod sessions;

pub use checkout::*;
pub use external::*;
pub use models::*;
pub use session::*;
pub use sessions::*;
