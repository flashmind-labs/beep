//! The mesh: a signed list of the devices that belong together, the tickets
//! a desktop mints to bring a new one in, and the service that keeps every
//! device's list and state current over iroh.

mod devices;
mod frame;
mod join;
mod link;
mod mesh;
mod state;
mod store;
mod ticket;

pub use devices::{Device, DeviceKind, DeviceOp, Devices, Merged, MeshError, WriteLimit};
pub use mesh::{
    DeviceStatus, JOIN_ALPN, JoinProtocol, Mesh, MeshProtocol, PEER_ALPN, PeerStream, StreamHandler,
};
pub use state::{Applied, Entry, Heads, Key, State};
pub use ticket::{JoinSecret, JoinTicket};
