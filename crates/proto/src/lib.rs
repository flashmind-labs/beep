//! The Beep wire: the app protocol (messages, framing, client), the node
//! link and the node and mesh messages, the lenient reader, and the Android
//! bindings.
//!
//! Each request is a struct declared in `requests!` with its op and reply
//! type, sent over HTTP (see `http`). Enums made with `wire_enum!` encode as
//! `[op, field...]`; structs encode with their field names on the apps'
//! wire and as their fields in declaration order on the node link.
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
mod assistant;
mod chat;
mod error;
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

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

#[cfg(feature = "uniffi")]
mod ffi {
    //! What Kotlin and Swift need for the foreign types on the wire.

    use flashmind_types::ReasoningLevel;
    use uuid::Uuid;

    // Kotlin and Swift see ids as their text.
    uniffi::custom_type!(Uuid, String, {
        remote,
        try_lift: |text| Ok(Uuid::parse_str(&text)?),
        lower: |id| id.to_string(),
    });

    /// How long the model reasons before it answers. `Off` sends no reasoning
    /// setting, so the model uses its own default.
    #[uniffi::remote(Enum)]
    pub enum ReasoningLevel {
        Off,
        Low,
        Medium,
        High,
    }
}

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
