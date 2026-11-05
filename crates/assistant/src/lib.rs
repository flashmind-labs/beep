//! The desktop's local assistant: a chat with an OpenRouter model, optional web tools through
//! Firecrawl, and read-only questions about the user's machines and coding agents. Chats are one
//! JSON file each. Nothing here knows about the desktop: it gets its keys, its folder and a
//! function that answers work questions.

mod error;
mod firecrawl;
mod message;
mod models;
mod openrouter;
mod prefs;
mod run;
mod store;
mod tools;
mod view;

pub use error::AssistantError;
pub use message::{Call, Message};
pub use models::{ModelInfo, default_model, list_models};
pub use prefs::{Prefs, load_prefs, save_prefs};
pub use run::{Event, Outcome, Setup, run_turn};
pub use store::{
    Chat, ChatMatch, ChatSummary, delete_chat, list_chats, load_chat, pin_chat, rename_chat,
    save_chat, search_chats,
};
pub use tools::{MAX_TOOL_ROUNDS, WorkQuery};
pub use view::{Step, TurnView, turns};

use secrecy::SecretString;
use serde::{Deserialize, Serialize};

/// How hard the model thinks before it answers.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, strum::IntoStaticStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Effort {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

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

/// The HTTP client for both services: no limit on a whole reply, which streams for as long as the
/// model takes, but one on connecting and on silence.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_default()
}
