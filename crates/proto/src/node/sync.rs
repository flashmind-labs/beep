//! `sync/1`: keeping a project's folder in step across the user's machines.
//! The server says which projects a node syncs, in which folder, and with
//! which other nodes; the nodes then exchange their file lists and the
//! files themselves directly, over iroh, so sync goes on while the server is
//! down. A node offers it only when it shares folders, and a project's
//! folder must lie in them.
//!
//! Each session is one request and its reply.

use iroh::EndpointAddr;
use serde::{Deserialize, Serialize};
use strum::IntoStaticStr;

pub const PROTOCOL: &str = "sync/1";

/// Offered beside `PROTOCOL` by a node that also pauses a project
/// (`Project::paused`), settles conflicts (`Request::Resolve`) and reports
/// progress. Sessions still open with `PROTOCOL`; an older node refuses
/// what it does not know.
pub const CONTROLS: &str = "sync-controls/1";

/// Offered beside `PROTOCOL` by a node that takes `Request::Rescan`.
pub const RESCAN: &str = "sync-rescan/1";

/// How `ProjectStatus::error` starts while the project's folder is not there.
/// The node makes it again and fills it from its peers.
pub const FOLDER_MISSING: &str = "The project's folder is missing";

/// How `ProjectStatus::error` starts when the folder above the project's is
/// missing too, as with a disk that is not mounted: nothing is made until it
/// is back.
pub const PARENT_MISSING: &str = "The folder above the project's folder is missing";

/// The most paths `ProjectStatus::waiting` lists.
pub const MAX_WAITING: usize = 50;

/// The most paths one `Skipped` reply lists.
pub const MAX_SKIPPED: u32 = 1000;

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// Every project this node syncs, replacing the last list. A project
    /// left out stops syncing; its files stay.
    Configure { projects: Vec<Project> },
    /// How each project's sync stands.
    Status {},
    /// The paths a project's sync leaves out, at most `limit`.
    Skipped { project: String, limit: u32 },
    /// Settles the conflict copy at `path`: keeps it in place of the file
    /// it stands beside, or deletes it. Only a node offering `CONTROLS`.
    Resolve {
        project: String,
        path: String,
        keep: Keep,
    },
    /// Scans `project`'s folder now and brings it in line with what the
    /// node has from its peers, then answers `Rescanned`. Only a node
    /// offering `RESCAN`.
    Rescan { project: String },
}

/// Which side of a conflict `Request::Resolve` keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    /// The file the copy stands beside; the copy is deleted.
    Original,
    /// The copy, which replaces that file.
    Copy,
}

/// One project a node syncs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// The name Beep knows the project by, the same on every machine.
    pub name: String,
    /// Its absolute folder on this node, inside the shared folders.
    pub folder: String,
    /// The other nodes that sync it.
    pub peers: Vec<EndpointAddr>,
    /// Kept on the list but not synced: no scans, fetches or pulls until
    /// it is resumed.
    #[serde(default)]
    pub paused: bool,
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    /// The projects are set; `addr` is how other nodes reach this one.
    Configured {
        addr: EndpointAddr,
        #[serde(default)]
        refused: Vec<Refused>,
    },
    Status {
        projects: Vec<ProjectStatus>,
    },
    Skipped {
        skipped: Skipped,
    },
    /// The conflict is settled; the next scan syncs the result.
    Resolved {},
    /// The scan `Request::Rescan` asked for is done; `status` is how the
    /// project stands after it.
    Rescanned {
        status: ProjectStatus,
    },
    Error {
        message: String,
    },
}

/// A project the node will not sync, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Refused {
    pub project: String,
    pub message: String,
}

/// How one project's sync stands on a node.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectStatus {
    pub name: String,
    pub folder: String,
    pub state: State,
    /// Files kept in step, and their bytes.
    pub files: u64,
    pub bytes: u64,
    /// Files another machine changed that this one does not have yet.
    pub pending: u64,
    /// The copies two machines' edits of one file left, by path.
    pub conflicts: Vec<String>,
    /// Set when the folder is rescanned this often instead of watched.
    pub rescan_secs: Option<u32>,
    /// What raises the watch limit, set while the folder is rescanned.
    pub hint: Option<String>,
    /// Why the last scan or fetch failed.
    pub error: Option<String>,
    /// Paused by the server: nothing moves until it is resumed.
    pub paused: bool,
    /// How far it is through what it brings in line now, while it syncs.
    pub progress: Option<Progress>,
    /// Some paths it still has to bring, or that failed and wait to be
    /// tried again, at most `MAX_WAITING`.
    pub waiting: Vec<String>,
}

/// How far a node is through one round of bringing files in line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// Bytes a second over the last few seconds; 0 before it can tell.
    pub rate: u64,
    /// Seconds left at that rate; `None` before it can tell.
    pub eta_secs: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum State {
    /// Looking for changes on disk.
    #[default]
    Scanning,
    /// Fetching or writing what another machine changed.
    Syncing,
    /// Every file is in step with what this node knows.
    Idle,
    /// It stopped; `error` says why.
    Failed,
    /// Paused by the server.
    Paused,
    /// A state a newer node reports.
    #[serde(other)]
    Other,
}

/// What a project's sync leaves out.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Skipped {
    pub paths: Vec<SkippedPath>,
    /// Files in a git repository that git carries rather than sync.
    pub through_git: u64,
    /// More paths are left out than `paths` lists.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkippedPath {
    /// Relative to the project folder, parts joined with `/`.
    pub path: String,
    pub folder: bool,
    pub rule: Rule,
    /// The marker file a default rule came from, such as `Cargo.toml`.
    #[serde(default)]
    pub marker: Option<String>,
}

/// Why a path is left out, in the order the rules apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Rule {
    /// A `.git` that stays on its machine: one below the project's root, a
    /// linked worktree's or a submodule's.
    Git,
    /// The project's `.gitignore`.
    GitIgnore,
    /// A default picked from a marker file, such as `target/` for
    /// `Cargo.toml`.
    Default,
    /// The project's `.beepignore`.
    BeepIgnore,
    /// A symlink or special file.
    Link,
    /// A rule a newer node applies.
    #[serde(other)]
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_state_or_rule_still_reads() {
        #[derive(Serialize)]
        #[serde(rename_all = "snake_case")]
        enum Newer {
            Frozen,
        }
        let bytes = rmp_serde::to_vec_named(&Newer::Frozen).expect("encodes");
        assert_eq!(
            rmp_serde::from_slice::<State>(&bytes).expect("decodes"),
            State::Other
        );
        assert_eq!(
            rmp_serde::from_slice::<Rule>(&bytes).expect("decodes"),
            Rule::Other
        );
    }

    #[test]
    fn an_older_node_reads_a_paused_project_and_a_newer_status() {
        /// `Project` and `ProjectStatus` as a node before pausing read them.
        #[derive(Deserialize)]
        struct OldProject {
            name: String,
        }
        #[derive(Deserialize)]
        struct OldStatus {
            files: u64,
        }
        let project = Project {
            name: "game".into(),
            folder: "/srv/project".into(),
            peers: Vec::new(),
            paused: true,
        };
        let bytes = rmp_serde::to_vec_named(&project).expect("encodes");
        let old: OldProject = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(old.name, "game");
        let status = ProjectStatus {
            files: 3,
            paused: true,
            waiting: vec!["a".into()],
            ..ProjectStatus::default()
        };
        let bytes = rmp_serde::to_vec_named(&status).expect("encodes");
        let old: OldStatus = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(old.files, 3);
        // And a newer server reads an older node's status.
        #[derive(Serialize)]
        struct Before {
            name: String,
            files: u64,
        }
        let before = Before {
            name: "game".into(),
            files: 2,
        };
        let bytes = rmp_serde::to_vec_named(&before).expect("encodes");
        let now: ProjectStatus = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(now.files, 2);
        assert!(!now.paused && now.progress.is_none());
    }
}
