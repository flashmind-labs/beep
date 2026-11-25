//! The node's identity and how it serves the mesh: key and settings
//! (`state`), serving until stopped (`serve`), the mesh's peers (`peers`),
//! and what carries over a restart (`restart`).

mod protocol;

pub use protocol::{Protocol, Session};

host! {
    mod channels;
    pub mod commands;
    pub mod forward;
    pub mod subscriptions;
    mod events;
    pub(crate) use events::HEARTBEAT;
    pub(crate) use events::sources::Registry as EventSources;
    pub(crate) mod from_mesh;
    mod project_remotes;
    pub use from_mesh::Membership;
    pub(crate) mod local_socket;
    pub mod local_status;
    pub(crate) mod note;
    pub(crate) mod peer_tools;
    pub(crate) mod peer_watch;
    pub mod peers;
    mod report;
    mod metrics;
    pub(crate) mod restart;
    pub use restart::{note_on_quit, reopen_and_keep};
    pub mod diagnostics;
    mod serve;
    pub mod state;
    mod sampler;
    mod traffic;
    pub(crate) mod watchdog;

    pub use crate::run::device::track_phones;
    pub use serve::{Offers, listen_hooks, serve};
    pub use traffic::Traffic;
}

#[cfg(all(test, unix, feature = "host"))]
pub(crate) use events::serve as serve_events;
