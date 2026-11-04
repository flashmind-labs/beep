//! The Beep protocol: messages, framing and client, shared by the server and
//! the Android app.
//!
//! Each request is a struct declared in `requests!` with its op and reply
//! type. Enums made with `wire_enum!` encode as `[op, field...]`; structs
//! encode as their fields in declaration order.
//!
//! With the `uniffi` feature, the types a client sees also derive UniFFI
//! bindings.
#[cfg(test)]
#[macro_use]
mod compat;
#[macro_use]
mod macros;

mod account;
mod agents;
mod apps;
mod chat;
mod error;
#[cfg(feature = "uniffi")]
mod ffi;
pub mod link;
#[cfg(feature = "logging")]
pub mod logging;
mod machines;
mod memories;
pub mod names;
pub mod node;
mod projects;
mod requests;
mod routines;
pub mod secret;
pub mod setup;
mod transport;
mod urls;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use account::*;
pub use agents::*;
pub use apps::*;
pub use chat::*;
pub use error::*;
pub use flashmind_types::ReasoningLevel;
pub use machines::*;
pub use memories::*;
pub use projects::*;
pub use requests::*;
pub use routines::*;
pub use secret::SecretText;
pub use transport::*;
pub use urls::*;
