//! The node's identity and how it serves the mesh: key and settings
//! (`state`), serving until stopped (`serve`), the mesh's peers (`peers`),
//! and what carries over a restart (`restart`).

mod protocol;

pub use crate::protocol::{Protocol, Session};

host! {
    mod channels;
    pub mod commands;
    pub mod forward;
    pub mod subscriptions;
    mod events;
    pub(crate) use events::sources::Registry as EventSources;
    pub(crate) mod from_mesh;
    mod project_remotes;
    pub mod peers;
    mod report;
    pub(crate) mod search;
    mod metrics;
    pub(crate) use crate::upkeep::restart;
    pub use restart::{note_on_quit, reopen_and_keep};
    pub mod diagnostics;
    mod serve;
    pub mod state;
    mod sampler;
    mod traffic;

    pub use peers::{Bound, Offers};
    pub use serve::serve;
    pub use traffic::Traffic;
}

#[cfg(all(test, unix, feature = "host"))]
pub(crate) use events::serve as serve_events;
