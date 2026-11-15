//! Claude Code without its npm ACP adapter: `beep-node claude-acp` speaks
//! ACP on its stdio and drives the `claude` CLI over the CLI's stream-json
//! protocol. The node starts it as Claude Code's adapter when the user picks
//! the Direct driver; the session runs as any other.

mod ask;
mod bridge;
mod cli;
mod updates;

pub use bridge::serve;

#[cfg(test)]
#[path = "../../tests/agents_claude_direct.rs"]
mod tests;
