//! MCP servers the user lists: their tools are offered to the model in local chats. A server is
//! a command run on this Mac or an HTTP address. `config` is the list on disk, `hub` owns the
//! connections, `call` runs one call of the model.

mod call;
mod config;
mod conn;
mod error;
mod http;
mod hub;
mod stdio;

pub(crate) use call::run;
pub use call::{Approval, ApprovalField};
pub use config::{
    EnvVar, McpServer, ServerSummary, Transport, add_server, load_servers, remove_server,
};
pub use error::McpError;
pub use hub::{McpHub, McpStatus, McpTool};

use conn::answer;
