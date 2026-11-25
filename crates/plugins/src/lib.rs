//! Runs Beep plugins: JavaScript in a QuickJS sandbox that places UI in named
//! slots. It has no Tauri dependency, so `beep-node` and phones can host it too.

mod asset;
mod console;
mod event;
mod host;
mod install;
mod instance;
mod job;
mod lock;
mod managed;
mod manifest;
mod meter;
mod sandbox;
mod source;
mod status;
mod store;
mod style;
mod tool;
mod topic;
mod ui;

pub use asset::{Asset, AssetHash, AssetRef, InvalidAssetHash};
pub use event::{Action, EventError, UiEvent};
pub use host::{Handler, Host, HostError};
pub use install::{Builtin, MAX_PLUGIN_BYTES, ManageError, Preview, PreviewKind, check_tree};
pub use instance::{
    ComposerContext, Instance, InstanceBatch, InstanceError, InstanceKey, InstanceRef,
    SessionHeaderContext, SessionRowContext, SlotContext,
};
pub use managed::{Commit, GitOrigin, InvalidCommit, Origin, Policy, Record, STATE_FILE};
pub use manifest::{
    API_VERSION, Activation, InvalidActivation, InvalidPluginId, Manifest, ManifestError,
    Permission, PluginId,
};
pub use status::{PluginState, PluginStatus};
pub use style::{
    InvalidStyleValue, MAX_STYLE_VALUE, Style, StyleToken, StyleValue, UnknownStyleToken,
};
pub use tool::{
    InvalidToolName, MAX_DESCRIPTION, MAX_TOOLS, ToolError, ToolName, ToolSpec, ToolsError,
};
pub use topic::{Publication, Topic, TopicError};
pub use ui::{Align, CallbackId, Motion, Node, Placement, Position, Repeat, Slot, Step, Tone};
