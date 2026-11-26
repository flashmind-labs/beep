//! Errors shared by mesh operations.

use iroh::EndpointId;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MeshError {
    /// The author is not (yet) a member.
    #[error("{0} is not a member")]
    Stranger(EndpointId),
    /// The key was removed and cannot author until a desktop adds it again.
    #[error("{0} was removed")]
    Removed(EndpointId),
    #[error("{0} is not a desktop")]
    NotDesktop(EndpointId),
    #[error("bad signature")]
    BadSignature,
    #[error("op belongs to another mesh")]
    OtherMesh,
    #[error("no genesis op")]
    NoGenesis,
    #[error("encoding failed: {0}")]
    Encode(#[from] rmp_serde::encode::Error),
    #[error("could not read: {0}")]
    Decode(#[from] rmp_serde::decode::Error),
    #[error("state file: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a state key: {0}")]
    BadKey(String),
    /// A replicated entry is stamped more than a day ahead of this clock.
    #[error("entry from {0} is stamped too far ahead")]
    Future(EndpointId),
    /// The write clock cannot move past what it has seen.
    #[error("write clock is exhausted")]
    Clock,
    #[error("record is too large")]
    TooLarge,
    /// A desktop declined a join. The text is meant for the person joining.
    #[error("refused: {0}")]
    Refused(String),
    #[error("this directory already belongs to a mesh")]
    AlreadyMember,
    /// A mesh can only be left while this device is its only member.
    #[error("this device belongs to a mesh with other devices")]
    NotLone,
    #[error("network: {0}")]
    Net(String),
    #[error("the mesh is closed")]
    Closed,
    /// A stream to a node failed before it opened or while it was in use.
    #[error(transparent)]
    Link(anyhow::Error),
    /// The node said it will not serve the protocol.
    #[error("the node refused {protocol}: {reason}")]
    Declined { protocol: String, reason: String },
    /// The node answered an `Open` with something other than `Ready` or `Refused`.
    #[error("the node answered {0}")]
    Unexpected(String),
}
