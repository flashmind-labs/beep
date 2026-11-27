//! How a session begins, and what a resumed one keeps of its history.

use crate::agents::live::Live;
use beep_proto::node::agent::{self, ParentLink, Persona};
use beep_proto::{AccountName, ModelId, SessionId, Upload};
use std::{borrow::Cow, path::PathBuf};
use tokio::sync::watch;

#[derive(Clone)]
pub(in crate::agents) struct History {
    pub(in crate::agents) path: PathBuf,
    pub(in crate::agents) native: u64,
    pub(in crate::agents) dropped: u64,
}

/// How a session begins: on a prompt, and as a new session (on a model
/// and thinking level when given) or an earlier one resumed by the agent's
/// id.
#[derive(Default)]
pub(crate) struct Begin {
    pub(crate) prompt: Option<String>,
    /// Images that go with `prompt`.
    pub(crate) images: Vec<Upload>,
    pub(crate) resume: Option<SessionId>,
    /// The resumed session's transcript from before the node restarted.
    pub(crate) earlier: Vec<agent::Entry>,
    pub(crate) timestamps: Option<(u64, u64)>,
    pub(crate) title: Option<String>,
    /// Its parent gave `title`, which its agent's own names leave alone.
    pub(crate) named: bool,
    pub(crate) model: Option<ModelId>,
    pub(crate) effort: Option<agent::Effort>,
    pub(crate) fast: Option<bool>,
    /// The session mode it was in before the node restarted.
    pub(crate) mode: Option<String>,
    /// A first line for the transcript, such as the branch it started on.
    pub(crate) note: Option<String>,
    /// Beep's word to the agent ahead of the first prompt.
    pub(crate) brief: Option<Cow<'static, str>>,
    /// The linked worktrees it created, kept across restarts and moves.
    pub(crate) worktrees: Vec<String>,
    /// The session that started this one with `start_agent`.
    pub(crate) parent: Option<ParentLink>,
    /// The parent's state when it runs here.
    pub(crate) follows: Option<watch::Receiver<Live>>,
    /// A temporary reviewer (`agents::reviews`).
    pub(crate) review: bool,
    /// The home assistant, as asked for (Cmd+N on Home) or as saved.
    pub(crate) assistant: bool,
    /// The named account to sign in as; `None` for the default one.
    pub(crate) account: Option<AccountName>,
    /// The custom agent it runs as.
    pub(crate) persona: Option<Persona>,
    /// The custom agent's instructions, said once after Beep's brief.
    pub(crate) instructions: Option<String>,
    /// The session this one carries on (`SessionInfo::continues`).
    pub(crate) continues: Option<SessionId>,
    /// The team issue its first prompt names (`SessionInfo::issue`).
    pub(crate) issue: Option<u64>,
}
