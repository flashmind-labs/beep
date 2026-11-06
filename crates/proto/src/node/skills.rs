//! `skills/1`: the coding agent skills a user made on a node, for the other
//! nodes of the mesh to copy. A stream starts with `Open`, then carries one
//! `Request` and its `Reply`. A node from before it declines the protocol,
//! which reads as a node with no skills.

use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

pub const PROTOCOL: &str = "skills/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    /// The user's skills on the node: `Reply::Skills`.
    List {},
    /// The files of one skill while it still hashes to `hash`:
    /// `Reply::Files`, empty when it is gone or changed.
    Get { name: String, hash: String },
    /// The skills the node found differing across the mesh:
    /// `Reply::Conflicts`.
    Conflicts {},
    /// The caller's skills changed: the node compares again soon.
    /// `Reply::Skills`, empty.
    Changed {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    Skills { skills: Vec<Listed> },
    Files { files: Vec<SkillFile> },
    Conflicts { conflicts: Vec<Conflict> },
}

/// One copy of a skill. A name the node holds in two agents' folders with
/// other files is listed once for each.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    /// The skill's folder name.
    pub name: String,
    /// SHA-256 of the folder's files, in hex.
    pub hash: String,
    /// The skills folders that hold this copy, such as `~/.claude/skills`.
    pub folders: Vec<String>,
    /// How many edits the copy the node last synced has been through.
    #[serde(default)]
    pub ver: u64,
    /// Whether this copy was edited on the node since it last synced: it is
    /// then one edit past `ver`.
    #[serde(default)]
    pub edited: bool,
}

/// A file of a skill, at its path in the skill's folder, `/` between parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillFile {
    pub path: String,
    pub data: ByteBuf,
}

/// A skill name with more than one content in the mesh. Nothing is changed
/// for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub name: String,
    /// One line per copy, such as `build-box ~/.claude/skills`.
    pub copies: Vec<String>,
    /// Where each differing content is, one label per content: a machine's
    /// name, with its folder when one machine holds two.
    pub places: Vec<String>,
    /// The lines of `copies` that are on the machine asking.
    #[serde(default)]
    pub here: Vec<String>,
}

impl Conflict {
    /// `Skill notes differs on box-a and box-b`, as the log and the bell say it.
    pub fn title(&self) -> String {
        let places = match self.places.as_slice() {
            [] => String::new(),
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        };
        format!("Skill {} differs on {places}", self.name)
    }
}
