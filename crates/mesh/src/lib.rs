//! The mesh: a signed list of the devices that belong together, the tickets
//! a desktop mints to bring a new one in, and the service that keeps every
//! device's list and state current over iroh.

mod client;
mod clock;
mod devices;
mod error;
mod inner;
mod join;
mod link;
pub mod machine;
mod mesh;
mod state;
mod store;
mod team;
mod ticket;
mod timeouts;
mod wire;

pub(crate) use client::CALL_TIMEOUT;
pub use client::{
    DEADLINE, FollowFuture, Followed, Hold, Offline, PeerClient, SessionFn, SnapshotFuture,
    SnapshotReader, with_deadline,
};
pub use devices::{Device, DeviceKind};
pub(crate) use devices::{DeviceOp, Devices, Merged, WriteLimit};
pub use error::MeshError;
pub use mesh::values;
pub use mesh::{
    DEFAULT_TICKET_TTL, DeviceStatus, InUse, JOIN_ALPN, JoinProtocol, Lease, Mesh, MeshProtocol,
    PEER_ALPN, PeerStream, router, transport_config, ways,
};
pub use state::Key;
pub(crate) use state::{Applied, Entry, Heads, State};
pub use team::{TeamTicket, TeamWrites, keep_contacts, secret_hash, take_roster};
pub use ticket::{Invitation, JoinSecret, JoinTicket, ShortTicket, TicketError};
pub use timeouts::{OPEN_TIMEOUT, RETRY};

#[cfg(test)]
mod tests;
