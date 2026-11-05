use thiserror::Error;

/// Why a call failed. The text is what the user reads.
#[derive(Debug, Error)]
pub enum AssistantError {
    /// A service answered with an error: its own text (bad key, no credit, model gone).
    #[error("{0}")]
    Api(String),
    #[error("could not reach the service: {0}")]
    Network(#[from] reqwest::Error),
    #[error("could not read or write the chat: {0}")]
    Store(#[from] std::io::Error),
    #[error("the chat file is not valid: {0}")]
    Format(#[from] serde_json::Error),
}
