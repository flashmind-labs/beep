use thiserror::Error;

/// Why a call to an MCP server failed. The text is what the user and the model read.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("the server did not answer in time")]
    Timeout,
    /// The registry's own message: a spawn or handshake failure, or the server's error.
    #[error("{0}")]
    Failed(String),
}
