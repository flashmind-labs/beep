//! The Beep wire: the app protocol (messages, framing, client), the node
//! link and the node and mesh messages, and the lenient reader.
//!
//! Each request is a struct declared in `requests!` with its op and reply
//! type, sent over HTTP (see `http`). Enums made with `wire_enum!` encode as
//! `[op, field...]`; structs encode with their field names on the apps'
//! wire and as their fields in declaration order on the node link.
#[cfg(test)]
#[macro_use]
mod compat;
#[macro_use]
mod macros;

mod account;
mod agents;
mod apps;
mod assistant;
mod chat;
mod error;
#[cfg(feature = "https")]
pub mod https;
pub mod lenient;
pub mod link;
#[cfg(feature = "logging")]
pub mod logging;
mod machines;
pub mod names;
pub mod node;
mod projects;
pub mod report;
mod requests;
pub mod secret;
pub mod setup;
mod transport;
mod urls;

pub use account::*;
pub use agents::*;
pub use apps::*;
pub use assistant::*;
pub use chat::*;
pub use error::*;
pub use flashmind_types::ReasoningLevel;
pub use machines::*;
pub use projects::*;
pub use requests::*;
pub use secret::SecretText;
pub use transport::*;
pub use urls::*;
