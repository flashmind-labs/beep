//! Runs Beep plugins: TypeScript in a QuickJS sandbox that places UI in named
//! slots. It has no Tauri dependency, so `beep-node` and phones can host it too.

mod asset;
mod event;
mod host;
mod instance;
mod job;
mod manifest;
mod meter;
mod sandbox;
mod source;
mod status;
mod topic;
mod ui;

pub use asset::{Asset, AssetHash, AssetRef, InvalidAssetHash};
pub use event::{Action, EventError, UiEvent};
pub use host::{Handler, Host, HostError};
pub use instance::{
    ComposerContext, Instance, InstanceBatch, InstanceError, InstanceKey, InstanceRef,
    SessionHeaderContext, SessionRowContext, SlotContext,
};
pub use manifest::{
    Activation, InvalidActivation, InvalidPluginId, Manifest, ManifestError, Permission, PluginId,
};
pub use status::{PluginState, PluginStatus};
pub use topic::{Publication, Topic, TopicError};
pub use ui::{Align, CallbackId, Motion, Node, Placement, Position, Repeat, Slot, Step, Tone};
