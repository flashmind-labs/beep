//! Claude Code without its npm ACP adapter: a task in the node speaks ACP to
//! the session and drives the `claude` CLI over the CLI's stream-json
//! protocol, when the user picks the Direct driver. The session runs as any
//! other.

mod ask;
mod bridge;
mod cli;
mod updates;

pub(in crate::agents) use bridge::serve;

#[cfg(test)]
#[path = "../../tests/agents_claude_direct.rs"]
mod tests;
