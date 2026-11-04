//! Getting messages between the apps, the server and a phone's notifications.

pub mod client;
pub mod port;
pub mod seal;
pub mod version;
pub mod wire;

pub use version::{ClientInfo, Feature, WIRE};
