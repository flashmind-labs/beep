//! What the node keeps of its sessions across a stop: the note the next
//! start resumes from, and whether it has done so.

use super::{
    acp::Prompt,
    family::{RemoteKid, links::Links},
    sessions::Sessions,
};
use beep_proto::{AccountName, SessionId};
use beep_proto::{
    ModelId,
    node::agent::{Effort, Entry, ParentLink, Persona, SessionDevice, SessionInfo},
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

/// The sessions kept from before the last stop, and whether they were
/// resumed.
#[derive(Default)]
pub struct Reopen {
    /// What sessions were, ended ones too, from the restart note.
    pub(super) saved: Mutex<Vec<SavedSession>>,
    /// Set once the sessions open before the last stop have been resumed, or
    /// there were none.
    pub(super) reopened: watch::Sender<bool>,
    pub(super) sessions: Arc<Sessions>,
    /// The remote children a restart keeps.
    pub(super) links: Arc<Links>,
}

/// What the node keeps of a session, also after it ended, so it goes on as
/// it was when it is resumed, taken over or moved: its `SessionInfo` as
/// listed, and the thinking level, which the info does not carry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedSession {
    /// By the agent's own id.
    pub(crate) info: SessionInfo,
    #[serde(default)]
    pub(crate) effort: Option<Effort>,
    #[serde(default)]
    pub(crate) fast: Option<bool>,
    /// The named account it ran as; `None` for the default one.
    #[serde(default)]
    pub(crate) account: Option<AccountName>,
    /// Its parent gave its title, which its agent's own names leave alone.
    #[serde(default)]
    pub(crate) named: bool,
}

/// A session the node stopped, to resume when it starts again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct OpenSession {
    /// The agent, such as `claude`.
    pub(super) agent: String,
    /// The agent's own id for the session.
    pub(crate) session: SessionId,
    pub(super) cwd: PathBuf,
    #[serde(default)]
    pub(super) started_at: u64,
    #[serde(default)]
    pub(super) last_activity: u64,
    /// Prompts that had not gone to the agent yet.
    pub(super) held: Vec<Prompt>,
    /// The session mode it was in.
    #[serde(default)]
    pub(super) mode: Option<String>,
    /// The session that started it with `start_agent`.
    #[serde(default)]
    pub(super) parent: Option<ParentLink>,
    /// The children it started on other machines.
    #[serde(default)]
    pub(super) kids: Vec<RemoteKid>,
    /// Its last transcript entries, kept when the node stopped, for an
    /// agent that resumes with `session/resume`, which replays nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) transcript: Vec<Entry>,
    /// The model it ran on. A resumed agent starts on its own default otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) model: Option<ModelId>,
    #[serde(default)]
    pub(super) title: Option<String>,
    /// The device it used, kept as `SessionInfo::device`.
    #[serde(default)]
    pub(super) device: Option<SessionDevice>,
    /// The linked worktrees it created, kept as `SessionInfo::worktrees`.
    #[serde(default)]
    pub(super) worktrees: Vec<String>,
    /// Whether it is the home assistant.
    #[serde(default)]
    pub(super) assistant: bool,
    /// The thinking level it ran at.
    #[serde(default)]
    pub(super) effort: Option<Effort>,
    #[serde(default)]
    pub(super) fast: Option<bool>,
    /// The named account it ran as; `None` for the default one.
    #[serde(default)]
    pub(super) account: Option<AccountName>,
    /// Its parent gave its title.
    #[serde(default)]
    pub(super) named: bool,
    /// The custom agent it runs as.
    #[serde(default)]
    pub(super) persona: Option<Persona>,
    /// The session it carries on.
    #[serde(default)]
    pub(super) continues: Option<SessionId>,
    /// The team issue its first prompt named.
    #[serde(default)]
    pub(super) issue: Option<u64>,
}

impl From<&OpenSession> for SavedSession {
    /// What a note from before saved sessions knows of open session `open`.
    fn from(open: &OpenSession) -> Self {
        let cwd = open.cwd.to_string_lossy().into_owned();

        Self {
            info: SessionInfo {
                id: open.session.clone(),
                agent: open.agent.clone(),
                cwd,
                title: open.title.clone(),
                mode: open.mode.clone(),
                parent: open.parent.clone(),
                model: open.model.clone(),
                device: open.device.clone(),
                worktrees: open.worktrees.clone(),
                account: open.account.clone(),
                assistant: open.assistant,
                persona: open.persona.clone(),
                continues: open.continues.clone(),
                issue: open.issue,
                ..SessionInfo::default()
            },
            effort: open.effort,
            fast: open.fast,
            account: open.account.clone(),
            named: open.named,
        }
    }
}
