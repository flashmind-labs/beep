//! `skill-library/1`: a node fetches a skill library's repository, finds libraries already in
//! the agents' skills folders, and says what it could not install. A stream starts with `Open`, then carries one `Request` and its `Reply`.

use super::mesh_state::LibrarySkill;
use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "skill-library/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    /// Fetch the repository at `url` and list its skills at the default branch's newest
    /// commit: `Reply::Inspected`, or `Reply::Failed`.
    Inspect { url: String },
    /// Skills in the agents' own skills folders that come from a Git repository, grouped by
    /// repository: `Reply::Found`.
    Find {},
    /// What the node could not fetch or install: `Reply::Problems`.
    Problems {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    Inspected {
        revision: String,
        skills: Vec<LibrarySkill>,
    },
    Failed {
        message: String,
    },
    Found {
        libraries: Vec<FoundLibrary>,
    },
    Problems {
        problems: Vec<Problem>,
    },
}

/// Skills in an agent's skills folder that are checkouts of, or links into, one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoundLibrary {
    /// The repository's remote URL.
    pub url: String,
    /// The skill folder names.
    pub skills: Vec<String>,
    /// The skills folders that hold them, such as `~/.claude/skills`.
    pub folders: Vec<String>,
}

/// A library the node could not fetch, or a skill it could not install in one place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    pub library: String,
    /// The skill, when the problem is one skill's.
    pub skill: Option<String>,
    /// Where: `global` or a project name.
    pub place: String,
    pub message: String,
}
