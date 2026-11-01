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
    /// What raises the watch limit, said once per node.
    pub hint: Option<String>,
    /// Why the last scan or fetch failed.
    pub error: Option<String>,
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
    /// `.git` never syncs.
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
    use iroh::SecretKey;

    fn round_trip<T: Serialize + for<'de> Deserialize<'de>>(value: &T) -> T {
        let bytes = rmp_serde::to_vec_named(value).expect("encodes");
        rmp_serde::from_slice(&bytes).expect("decodes")
    }

    #[test]
    fn requests_and_replies_round_trip() {
        let configure = Request::Configure {
            projects: vec![Project {
                name: "game".into(),
                folder: "/srv/project".into(),
                peers: vec![EndpointAddr::new(SecretKey::generate().public())],
            }],
        };
        assert_eq!(round_trip(&configure), configure);
        let status = Reply::Status {
            projects: vec![ProjectStatus {
                name: "game".into(),
                state: State::Syncing,
                files: 3,
                conflicts: vec!["notes (conflict 1a2b3c4d).txt".into()],
                rescan_secs: Some(45),
                ..ProjectStatus::default()
            }],
        };
        assert_eq!(round_trip(&status), status);
        let skipped = Reply::Skipped {
            skipped: Skipped {
                paths: vec![SkippedPath {
                    path: "target".into(),
                    folder: true,
                    rule: Rule::Default,
                    marker: Some("Cargo.toml".into()),
                }],
                through_git: 12,
                truncated: false,
            },
        };
        assert_eq!(round_trip(&skipped), skipped);
        let rule: &'static str = Rule::BeepIgnore.into();
        assert_eq!(rule, "beep_ignore");
    }

    #[test]
    fn a_newer_state_or_rule_still_reads() {
        #[derive(Serialize)]
        #[serde(rename_all = "snake_case")]
        enum Newer {
            Paused,
        }
        let bytes = rmp_serde::to_vec_named(&Newer::Paused).expect("encodes");
        assert_eq!(
            rmp_serde::from_slice::<State>(&bytes).expect("decodes"),
            State::Other
        );
        assert_eq!(
            rmp_serde::from_slice::<Rule>(&bytes).expect("decodes"),
            Rule::Other
        );
    }
}
