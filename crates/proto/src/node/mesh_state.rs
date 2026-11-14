//! What a mesh's replicated state holds for nodes: the values of the keys
//! `sync/<name>`, `credential/<id>`, `harness/<agent>`, `terminal/<id>` and
//! `session-move/<id>` (and `flsh`, which
//! is a `FlshSettings`). Desktops write them, nodes read them. Encoded as
//! compact msgpack, read by position, like the node link: a field is added
//! at the end with `#[serde(default)]`.

use super::{EnvSecret, ForgejoLogin, harness::HarnessConfig};
use crate::{AgentSlug, ModelId};
use iroh::EndpointId;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};

/// `sync/<name>`: the devices that keep a project in step, and the folder
/// the project has on each. A device not listed does not sync it, and a
/// record that lists none is a pinned project, which never syncs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSpec {
    pub folders: BTreeMap<EndpointId, String>,
    #[serde(default)]
    pub paused: bool,
    /// Selected ignored folders use independent file sync; off by default.
    #[serde(default)]
    pub ignored: super::sync::IgnoredSync,
}

/// `skills/sync`: whether coding agent skills sync across the mesh.
/// An absent record keeps syncing off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSync {
    pub enabled: bool,
}

/// `skill-library/<name>`: a library of agent skills from a Git repository, the commit every
/// machine checks out, and where its skills are installed. Desktops write it after a node
/// fetched the repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillLibrary {
    /// The repository each machine clones.
    pub url: String,
    /// The commit every machine checks out.
    pub revision: String,
    /// The skills at `revision`, by folder name.
    pub skills: Vec<LibrarySkill>,
    /// The skills installed in every agent's own skills folder on every machine.
    pub global: Pick,
    /// The skills installed in each project's checkouts, by project (`org/repo`).
    pub projects: BTreeMap<String, Pick>,
}

/// One skill of a library: its folder name and the description in its SKILL.md.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibrarySkill {
    pub name: String,
    pub description: String,
}

/// Which of a library's skills a place gets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pick {
    #[default]
    None,
    /// Every skill, including ones a later revision adds.
    All,
    /// These skills, by name.
    Only(BTreeSet<String>),
}

impl Pick {
    /// Whether this pick includes the skill `name`.
    pub fn has(&self, name: &str) -> bool {
        match self {
            Pick::None => false,
            Pick::All => true,
            Pick::Only(names) => names.contains(name),
        }
    }
}

/// `project-skills/<org/repo>`: how library skills are installed in a project's checkouts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSkills {
    /// Lists the installed skills in `.git/info/exclude`, so Git never sees them.
    pub keep_out_of_git: bool,
}

impl Default for ProjectSkills {
    fn default() -> Self {
        Self {
            keep_out_of_git: true,
        }
    }
}

/// `settings/project-sync`: a master pause without discarding project preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSyncSettings {
    pub enabled: bool,
    /// Projects individually paused before the master switch was turned off.
    #[serde(default)]
    pub paused_projects: BTreeSet<String>,
    /// Every project, and each one imported later, is excluded from Syncthing.
    #[serde(default)]
    pub beep_sync_over_syncthing: bool,
}

impl Default for ProjectSyncSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            paused_projects: BTreeSet::new(),
            beep_sync_over_syncthing: false,
        }
    }
}

/// `project/<org/repo>`: that a project exists, and its repository's web
/// page. Where it is checked out is one `Place` per machine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    /// The repository's web page.
    pub url: String,
    /// Project-wide Syncthing preference; absent preserves existing settings.
    #[serde(default)]
    pub syncthing_files: Option<bool>,
    /// Preserve local edits before an automatic fast-forward, on idle machines only.
    #[serde(default)]
    pub git_auto_recovery: bool,
    /// The displayed name; the mesh key remains the project's identity.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Repository URL changes, replayed on machines that were offline.
    #[serde(default)]
    pub remote_renames: Vec<RemoteRename>,
}

/// A hosted repository rename; matching fetch and push URLs keep their transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteRename {
    pub from: String,
    pub to: String,
}

/// `project/<org/repo>/<machine>`: one machine's checkout of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub folder: String,
    pub imported_ms: u64,
    /// The user's fork cloned as `origin`, as `org/repo`.
    #[serde(default)]
    pub fork: Option<String>,
}

/// `credential/<id>`: one secret every machine of the mesh holds. Its
/// `Debug` prints nothing of the secret.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Credential {
    /// An env var every process the node starts gets.
    Env(EnvSecret),
    /// A Forgejo or Gitea server teacli signs in to.
    Forgejo(ForgejoLogin),
}

/// `terminal/<id>`: a terminal a desktop keeps running on a machine. The
/// node knows its tasks by number; this says which one is which, so every
/// desktop lists the same terminals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRecord {
    /// `org/repo`, or empty for a terminal of no project.
    pub project: String,
    /// The machine it runs on.
    pub machine: EndpointId,
    pub task: u32,
    /// An interactive terminal, not a view-only task.
    pub pty: bool,
    pub name: String,
    /// What it ran; tells apart a restarted node's task under the same number.
    pub command: String,
    pub cwd: String,
    /// Milliseconds since the Unix epoch.
    pub started_at: u64,
}

/// `session-move/<id>`: a coding agent session a desktop moves from one
/// machine to another. The target reads it, waits for the source to end the
/// session, brings the conversation and the project files, and resumes the
/// session in its own checkout. The latest record of a session names the
/// machine that owns it; deleting it before the target resumes cancels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMove {
    pub agent: AgentSlug,
    /// The agent's own id for the session.
    pub id: String,
    pub from: EndpointId,
    pub to: EndpointId,
    /// The project, by the name `sync/<name>` has.
    pub project: String,
    /// The folder it works in, relative to the project's checkout; empty for
    /// the checkout itself.
    pub path: String,
    /// The session mode it was in, when it said.
    pub mode: Option<String>,
    /// Stop the turn the source is running instead of waiting for it.
    pub stop: bool,
    /// Milliseconds since the Unix epoch.
    pub at: u64,
    /// Was the linked worktree a session moved out of. Sessions run in the
    /// project folder now: always `None` when written, and a node resumes a
    /// record that has one in its project folder. Kept for its place on the
    /// wire.
    #[serde(default)]
    pub worktree: Option<MovedWorktree>,
    /// The model the session runs on, which the target resumes it on.
    /// Added after `worktree`.
    #[serde(default)]
    pub model: Option<ModelId>,
}

/// The linked worktree in a `SessionMove` from an app from before worktree
/// moves were removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovedWorktree {
    pub branch: String,
    /// The worktree's folder on the source.
    pub from: String,
}

/// A value that is stored in the mesh's state as bytes.
pub trait StateValue: Serialize + DeserializeOwned {
    fn to_bytes(&self) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec(self)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
        crate::lenient::from_slice(bytes)
    }
}

impl StateValue for SyncSpec {}
impl StateValue for SkillSync {}
impl StateValue for SkillLibrary {}
impl StateValue for ProjectSkills {}
impl StateValue for ProjectSyncSettings {}
impl StateValue for Place {}
impl StateValue for ProjectRecord {}
impl StateValue for Credential {}
impl StateValue for TerminalRecord {}
impl StateValue for SessionMove {}
impl StateValue for HarnessConfig {}
impl StateValue for crate::FlshSettings {}

/// One saved version of the user's instructions. Each is its own key, `instructions/<ms>.<id>`, so
/// two desktops saving at once keep both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedVersion {
    pub text: String,
    pub note: String,
    pub saved_ms: u64,
}

impl StateValue for SavedVersion {}

#[cfg(test)]
#[path = "../tests/node_mesh_state.rs"]
mod tests;
