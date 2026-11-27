//! What the desktop app gives its node to answer the home assistant's project
//! tools.

use beep_proto::node::tools::Reply;
use futures::future::BoxFuture;

/// What the desktop app gives its node to answer `Tool::Projects`.
pub trait ProjectTools: Send + Sync {
    /// The reply to one call, from its arguments as JSON text with the
    /// action under `action`.
    fn call(&self, args: String) -> BoxFuture<'_, Reply>;
}
