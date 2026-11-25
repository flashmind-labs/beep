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
pub(in crate::agents) struct Begin {
    pub(in crate::agents) prompt: Option<String>,
    /// Images that go with `prompt`.
    pub(in crate::agents) images: Vec<Upload>,
    pub(in crate::agents) resume: Option<SessionId>,
    /// The resumed session's transcript from before the node restarted.
    pub(in crate::agents) earlier: Vec<agent::Entry>,
    pub(in crate::agents) timestamps: Option<(u64, u64)>,
    pub(in crate::agents) title: Option<String>,
    /// Its parent gave `title`, which its agent's own names leave alone.
    pub(in crate::agents) named: bool,
    pub(in crate::agents) model: Option<ModelId>,
    pub(in crate::agents) effort: Option<agent::Effort>,
    pub(in crate::agents) fast: Option<bool>,
    /// The session mode it was in before the node restarted.
    pub(in crate::agents) mode: Option<String>,
    /// A first line for the transcript, such as the branch it started on.
    pub(in crate::agents) note: Option<String>,
    /// Beep's word to the agent ahead of the first prompt.
    pub(in crate::agents) brief: Option<Cow<'static, str>>,
    /// The linked worktrees it created, kept across restarts and moves.
    pub(in crate::agents) worktrees: Vec<String>,
    /// The session that started this one with `start_agent`.
    pub(in crate::agents) parent: Option<ParentLink>,
    /// The parent's state when it runs here.
    pub(in crate::agents) follows: Option<watch::Receiver<Live>>,
    /// A temporary reviewer (`agents::reviews`).
    pub(in crate::agents) review: bool,
    /// The home assistant, as asked for (Cmd+N on Home) or as saved.
    pub(in crate::agents) assistant: bool,
    /// The named account to sign in as; `None` for the default one.
    pub(in crate::agents) account: Option<AccountName>,
    /// The custom agent it runs as.
    pub(in crate::agents) persona: Option<Persona>,
    /// The custom agent's instructions, said once after Beep's brief.
    pub(in crate::agents) instructions: Option<String>,
    /// The session this one carries on (`SessionInfo::continues`).
    pub(in crate::agents) continues: Option<SessionId>,
    /// The team issue its first prompt names (`SessionInfo::issue`).
    pub(in crate::agents) issue: Option<u64>,
}
