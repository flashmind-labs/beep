use thiserror::Error;

/// Why a call to an MCP server failed. The text is what the user and the model read.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("could not start {command}: {reason}")]
    Start { command: String, reason: String },
    #[error("the server closed the connection")]
    Closed,
    #[error("the server did not answer in time")]
    Timeout,
    #[error("the server broke the protocol: {0}")]
    Protocol(String),
    /// The server answered with an error of its own.
    #[error("{0}")]
    Server(String),
    #[error("could not reach the server: {0}")]
    Http(String),
    #[error("the server's message is too long")]
    TooLong,
}

impl From<reqwest::Error> for McpError {
    fn from(e: reqwest::Error) -> Self {
        // The address may carry a key in its query, and the error prints it.
        Self::Http(e.without_url().to_string())
    }
}

impl From<std::io::Error> for McpError {
    fn from(_: std::io::Error) -> Self {
        // A pipe to a program that ended: every write or read error means it is gone.
        Self::Closed
    }
}
