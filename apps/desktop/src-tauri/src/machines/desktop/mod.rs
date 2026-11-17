#[cfg(all(target_os = "macos", feature = "desktop-control"))]
mod keys;
#[cfg(all(target_os = "macos", feature = "desktop-control"))]
mod mac;
#[cfg(all(
    target_os = "macos",
    feature = "desktop-control",
    feature = "local-host"
))]
mod session;
#[cfg(all(target_os = "macos", feature = "desktop-control"))]
mod tree;

#[cfg(all(target_os = "macos", feature = "desktop-control"))]
pub use mac::{permissions, request};

#[cfg(all(
    target_os = "macos",
    feature = "desktop-control",
    feature = "local-host"
))]
pub use session::Desktop;

#[cfg(all(desktop, not(all(target_os = "macos", feature = "desktop-control"))))]
pub fn permissions() -> beep_proto::node::desktop::Permissions {
    beep_proto::node::desktop::Permissions::default()
}

#[cfg(all(desktop, not(all(target_os = "macos", feature = "desktop-control"))))]
pub fn request(_: beep_proto::node::desktop::Permission) {}
