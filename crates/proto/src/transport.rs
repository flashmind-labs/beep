//! Getting messages between the apps, the server and a phone's notifications.

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "client")]
pub mod follow;
pub mod http;
pub mod port;
#[cfg(feature = "seal")]
pub mod seal;

pub use http::AppEvent;
