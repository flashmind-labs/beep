//! Sessions held open by a local CLI, including idle prompts.

mod local;
mod registry;
mod stream;

pub use local::listen;
use registry::Registration;
pub(super) use registry::Registry;
pub(super) use stream::follow;

#[cfg(test)]
mod tests;
