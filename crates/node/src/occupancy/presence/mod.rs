//! Sessions held open by a local CLI, including idle prompts.

mod local;
mod registry;

pub(crate) use local::{listen, listen_in};
pub(crate) use registry::{Registration, Registry};

#[cfg(test)]
#[path = "../../tests/occupancy_presence.rs"]
mod tests;
