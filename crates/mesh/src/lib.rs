//! The mesh: a signed list of the devices that belong together, and the
//! tickets a desktop mints to bring a new one in.

mod devices;
mod state;
mod ticket;

pub use devices::{Device, DeviceKind, DeviceOp, Devices, Merged, MeshError, WriteLimit};
pub use state::{Applied, Entry, Heads, Key, State};
pub use ticket::{JoinSecret, JoinTicket};
