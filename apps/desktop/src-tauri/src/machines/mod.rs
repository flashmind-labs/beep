//! The account's machines: listing, adding and updating them, their
//! terminals and emulators, and this Mac as a node (`this_mac`, `desktop`, `mcp_command`).

pub(crate) mod add;
pub(crate) mod desktop;
pub(crate) mod devices;
pub(crate) mod list;
#[cfg(desktop)]
pub(crate) mod mcp_command;
pub(crate) mod sign_in;
pub(crate) mod terminals;
#[cfg(desktop)]
pub(crate) mod this_mac;
#[cfg(mobile)]
#[path = "phone.rs"]
pub(crate) mod this_mac;
pub(crate) mod tools;
