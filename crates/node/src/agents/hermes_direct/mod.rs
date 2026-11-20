//! Hermes Agent as a session: a task in the node speaks ACP to the session
//! and Hermes's JSON-RPC to Hermes. This holds the pure translation of
//! Hermes's events and requests, and of the answers that go back.

// The bridge that drives these arrives with the rest of the driver.
#[allow(dead_code)]
mod ask;
#[allow(dead_code)]
mod events;

#[cfg(test)]
#[path = "../../tests/agents_hermes_direct.rs"]
mod tests;
