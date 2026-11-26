//! The mesh: a signed list of the devices that belong together, the tickets
//! a desktop mints to bring a new one in, and the service that keeps every
//! device's list and state current over iroh.

mod client;
mod devices;
mod join;
mod link;
pub mod machine;
mod mesh;
mod state;
mod store;
mod team;
mod wire;

pub(crate) use client::CALL_TIMEOUT;
pub use client::{
    DEADLINE, Followed, OPEN_TIMEOUT, Offline, PeerClient, RETRY, SessionFn, SnapshotFuture,
    SnapshotReader, with_deadline,
};
pub use devices::{Device, DeviceKind, MeshError};
pub(crate) use devices::{DeviceOp, Devices, Merged, WriteLimit};
pub use join::{Invitation, JoinSecret, JoinTicket, ShortTicket, TicketError};
pub use mesh::values;
pub use mesh::{
    DEFAULT_TICKET_TTL, DeviceStatus, InUse, JOIN_ALPN, JoinProtocol, Mesh, MeshProtocol,
    PEER_ALPN, PeerStream, StreamHandler, router, transport_config, ways,
};
pub use state::Key;
pub(crate) use state::{Applied, Entry, Heads, State};
pub use team::{TeamTicket, TeamWrites, keep_contacts, secret_hash, take_roster};

#[cfg(test)]
mod tests;
