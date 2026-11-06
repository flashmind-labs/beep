//! The user's files: shared folders (`folders`), project sync (`sync`,
//! `syncthing`), node-to-node transfers (`transfer`) and the search for
//! git checkouts (`search`).

mod folders;
pub(crate) mod fsat;
pub(crate) mod paths;
pub(crate) mod search;
pub mod sync;
pub mod syncthing;
pub mod transfer;

pub use folders::Folders;
