//! The node's identity and its connection to the server: key and settings
//! (`state`), linking and the control stream (`connection`), staying
//! connected (`reconnect`), and what carries over a restart (`restart`).

mod connection;
pub(crate) mod from_mesh;
pub(crate) use from_mesh::Membership;
pub mod local;
pub(crate) mod local_socket;
pub mod local_status;
pub(crate) mod note;
pub(crate) mod peer_tools;
pub mod peers;
pub mod reconnect;
mod report;
pub(crate) mod restart;
pub mod state;
mod traffic;
pub(crate) mod watchdog;

pub use connection::{
    Claimed, Linked, Offers, Protocol, Session, claim, connect, listen_hooks, open, print_offer,
    serve, stay,
};
pub use traffic::Traffic;
