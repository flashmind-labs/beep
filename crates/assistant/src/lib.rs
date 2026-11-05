//! The desktop's local assistant: a chat with an OpenRouter model, optional web tools through
//! Firecrawl, and read-only questions about the user's machines and coding agents. Chats are one
//! JSON file each. Nothing here knows about the desktop: it gets its keys, its folder and a
//! function that answers work questions.

mod error;
mod firecrawl;
mod message;
mod models;
mod openrouter;
mod run;
mod store;
mod tools;

pub use error::AssistantError;
pub use message::{Call, Message};
pub use models::{ModelInfo, default_model, list_models};
pub use run::{Event, Outcome, Setup, run_turn};
pub use store::{Chat, ChatSummary, delete_chat, list_chats, load_chat, save_chat};
pub use tools::{MAX_TOOL_ROUNDS, WorkQuery};

use secrecy::SecretString;

/// A service's address and the key for it.
pub struct Endpoint {
    /// Without a trailing slash, such as `https://openrouter.ai/api/v1`.
    pub base: String,
    pub key: SecretString,
}

/// OpenRouter's API.
pub const OPENROUTER_BASE: &str = "https://openrouter.ai/api/v1";
/// Firecrawl's API.
pub const FIRECRAWL_BASE: &str = "https://api.firecrawl.dev/v2";
