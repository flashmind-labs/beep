//! The node's identity and how it serves the mesh: key and settings
//! (`state`), serving until stopped (`serve`), the mesh's peers (`peers`),
//! and what carries over a restart (`restart`).

mod protocol;

pub use crate::protocol::{Protocol, Session};

#[cfg(feature = "host")]
mod channels;
#[cfg(feature = "host")]
pub mod commands;
#[cfg(feature = "host")]
pub mod forward;
#[cfg(feature = "host")]
pub mod subscriptions;
#[cfg(feature = "host")]
mod events;
#[cfg(feature = "host")]
pub(crate) use events::sources::Registry as EventSources;
#[cfg(feature = "host")]
pub(crate) mod from_mesh;
#[cfg(feature = "host")]
mod project_remotes;
#[cfg(feature = "host")]
pub mod peers;
#[cfg(feature = "host")]
mod report;
#[cfg(feature = "host")]
pub(crate) mod search;
#[cfg(feature = "host")]
mod metrics;
#[cfg(feature = "host")]
pub(crate) use crate::upkeep::restart;
#[cfg(feature = "host")]
pub use restart::{note_on_quit, reopen_and_keep};
#[cfg(feature = "host")]
pub mod diagnostics;
#[cfg(feature = "host")]
mod serve;
#[cfg(feature = "host")]
pub mod state;
#[cfg(feature = "host")]
mod sampler;
#[cfg(feature = "host")]
mod traffic;

#[cfg(feature = "host")]
pub use peers::{Bound, Offers};
#[cfg(feature = "host")]
pub use serve::serve;
#[cfg(feature = "host")]
pub use traffic::Traffic;

#[cfg(all(test, unix, feature = "host"))]
pub(crate) use events::serve as serve_events;
