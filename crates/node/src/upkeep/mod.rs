//! Keeping the node itself going: updating it (`update`), watching the CI
//! runs of pinned projects (`ci`), what carries over a restart and the wait
//! for idle (`restart`), and asking a running node how it stands
//! (`local_status`).

#[cfg(feature = "cli")]
mod local_status;

#[cfg(feature = "cli")]
pub use local_status::local_status;
