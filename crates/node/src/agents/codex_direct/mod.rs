//! Codex without its npm ACP adapter: a task in the node speaks ACP to the
//! session and drives `codex app-server` over its JSON-RPC protocol, when
//! the user picks the Direct driver. The session runs as any other, with
//! the modes, options and tool calls Codex's adapter gives.

mod ask;
mod bridge;
mod items;
mod server;
mod settings;

pub(in crate::agents) use bridge::serve;

#[cfg(test)]
#[path = "../../tests/agents_codex_direct.rs"]
mod tests;
