//! The mesh: a signed list of the devices that belong together, the tickets
//! a desktop mints to bring a new one in, and the service that keeps every
//! device's list and state current over iroh.

mod bin;
mod client;
mod devices;
mod frame;
mod join;
mod link;
pub mod machine;
mod mesh;
mod state;
mod store;
mod ticket;

pub use client::{
    CALL_TIMEOUT, DEADLINE, OPEN_TIMEOUT, Offline, PeerClient, SessionFn, with_deadline,
};
pub use devices::{Device, DeviceKind, DeviceOp, Devices, Merged, MeshError, WriteLimit};
pub use mesh::{
    DEFAULT_TICKET_TTL, DeviceStatus, InUse, JOIN_ALPN, JoinProtocol, Mesh, MeshProtocol,
    PEER_ALPN, PeerStream, StreamHandler, transport_config,
};
pub use state::{Applied, Entry, Heads, Key, State};
pub use ticket::{JoinSecret, JoinTicket, TicketError};

#[cfg(test)]
mod wire_tests;
