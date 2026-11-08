#[cfg(target_os = "macos")]
mod keys;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
mod session;
#[cfg(target_os = "macos")]
mod tree;

#[cfg(target_os = "macos")]
pub use session::{Desktop, permissions, request};

#[cfg(all(desktop, not(target_os = "macos")))]
pub fn permissions() -> beep_proto::node::desktop::Permissions {
    beep_proto::node::desktop::Permissions::default()
}

#[cfg(all(desktop, not(target_os = "macos")))]
pub fn request(_: beep_proto::node::desktop::Permission) {}
