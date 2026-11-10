//! Sessions held open by a local CLI, including idle prompts.

mod local;
mod registry;
mod stream;

pub use local::{listen, listen_in};
use registry::Registration;
pub(super) use registry::Registry;
pub(super) use stream::follow;

#[cfg(test)]
#[path = "../../tests/agents_presence.rs"]
mod tests;
