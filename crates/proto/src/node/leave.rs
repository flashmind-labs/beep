//! `leave/1`: a temporary device, such as a container, asks a desktop to
//! remove it from the mesh as it stops. A stream starts with `Open`, then
//! carries one `Request` and its `Reply`. A desktop takes it only from a
//! temporary device about itself; a node, or a desktop from before it,
//! refuses the protocol.

use iroh::EndpointId;
use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "leave/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    /// Remove `id`, which is the caller: `Reply::Removed` or `Reply::Refused`.
    Remove { id: EndpointId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    /// Out of the mesh, now or before.
    Removed {},
    Refused {
        reason: String,
    },
}
