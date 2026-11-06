//! What a mesh's replicated state holds for nodes: the values of the keys
//! `sync/<name>`, `credential/<id>`, `harness/<agent>`, `terminal/<id>` and
//! `session-move/<id>` (and `flsh`, which
//! is a `FlshSettings`). Desktops write them, nodes read them. Encoded as
//! compact msgpack, read by position, like the node link: a field is added
//! at the end with `#[serde(default)]`.

use super::{EnvSecret, ForgejoLogin, harness::HarnessConfig};
use crate::AgentSlug;
use iroh::EndpointId;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;

/// `sync/<name>`: the devices that keep a project in step, and the folder
/// the project has on each. A device not listed does not sync it, and a
/// record that lists none is a pinned project, which never syncs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSpec {
    pub folders: BTreeMap<EndpointId, String>,
    #[serde(default)]
    pub paused: bool,
}

/// `project/<org/repo>`: that a project exists, and its repository's web
/// page. Where it is checked out is one `Place` per machine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    /// The repository's web page.
    pub url: String,
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
    /// The session works in a linked worktree: `path` is then relative to
    /// that worktree, not to the project's checkout. Added after the others,
    /// so a node from before it reads the rest.
    #[serde(default)]
    pub worktree: Option<MovedWorktree>,
}

/// The linked worktree a session moves out of: the target makes a worktree
/// on the same branch in its own checkout.
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
impl StateValue for Place {}
impl StateValue for ProjectRecord {}
impl StateValue for Credential {}
impl StateValue for TerminalRecord {}
impl StateValue for SessionMove {}
impl StateValue for HarnessConfig {}
impl StateValue for crate::FlshSettings {}
