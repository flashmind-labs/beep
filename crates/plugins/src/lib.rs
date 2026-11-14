//! Runs Beep plugins: TypeScript in a QuickJS sandbox that places UI in named
//! slots. It has no Tauri dependency, so `beep-node` and phones can host it too.

mod host;
mod manifest;
mod sandbox;
mod source;
mod topic;
mod ui;

pub use host::{Handler, Host, HostError};
pub use manifest::{InvalidPluginId, Manifest, ManifestError, Permission, PluginId};
pub use topic::{Publication, Topic, TopicError};
pub use ui::{Align, Node, Placement, Slot};
