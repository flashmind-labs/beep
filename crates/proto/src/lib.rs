//! The Beep wire: the node link, the node and mesh messages and the data
//! types they carry, and the lenient reader.
//!
//! Enums made with `wire_enum!` encode as `[op, field...]`; structs encode
//! as their fields in declaration order on the node link.
#[macro_use]
mod macros;

mod agents;
mod credentials;
mod error;
#[cfg(feature = "https")]
pub mod https;
pub mod lenient;
#[cfg(feature = "logging")]
pub mod logging;
mod machines;
mod media;
pub mod names;
pub mod node;
pub mod port;
mod projects;
pub mod report;
pub mod secret;

pub use agents::*;
pub use credentials::*;
pub use error::*;
pub use flashmind_types::{ApiKey, ReasoningLevel};
pub use machines::*;
pub use media::*;
pub use projects::*;
pub use secret::SecretText;
