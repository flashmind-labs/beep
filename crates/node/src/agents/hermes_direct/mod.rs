//! Hermes Agent as a session: a task in the node speaks ACP to the session
//! and JSON-RPC to Hermes's dashboard socket, as `codex_direct` does for
//! Codex's app-server.

mod ask;
mod bridge;
mod events;
mod link;

#[allow(unused_imports)] // the launch path uses them in the next change
pub(in crate::agents) use bridge::{NAME, serve};

#[cfg(test)]
#[path = "../../tests/agents_hermes_direct.rs"]
mod tests;
