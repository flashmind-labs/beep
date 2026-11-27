//! The user's files: shared folders (`folders`), project sync (`sync`,
//! `syncthing`) and node-to-node transfers (`transfer`).

#[cfg(feature = "host")]
mod folders;
#[cfg(feature = "host")]
pub(crate) mod grants;
#[cfg(feature = "host")]
pub(crate) mod place;
#[cfg(feature = "host")]
pub mod sync;
#[cfg(feature = "host")]
pub mod syncthing;
#[cfg(feature = "host")]
pub mod transfer;

#[cfg(feature = "host")]
pub use folders::Folders;
