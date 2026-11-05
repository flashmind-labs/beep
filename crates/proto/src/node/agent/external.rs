//! Sessions started outside Beep, and the folders agents worked in.

use super::{PlanEntry, WorkPlace};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A git checkout coding agents worked in, as `SessionsRequest::Folders`
/// finds it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionFolder {
    /// The checkout's main folder, absolute.
    pub path: String,
    /// Its `origin` remote; empty when it has none.
    pub origin: String,
    /// Sessions per agent, most first.
    pub agents: Vec<AgentSessions>,
    /// When a session there last changed or its checked-out commit was
    /// made, whichever is newer, Unix seconds. Nodes from before it give
    /// only the session's.
    pub last_activity: u64,
}

/// How many sessions one agent ran in a folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSessions {
    /// Such as `claude` or `codex`.
    pub agent: String,
    pub sessions: u32,
}

/// A session the user started on the machine outside Beep, such as Claude
/// Code in a terminal or an IDE.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExternalSession {
    /// The agent's own id for it, such as Claude Code's session UUID.
    pub id: String,
    pub agent: String,
    pub cwd: String,
    pub title: Option<String>,
    /// Unix seconds.
    pub started_at: u64,
    /// When its transcript last changed, Unix seconds.
    pub last_activity: u64,
    pub presence: Presence,
    /// The process that runs it, when one does.
    pub pid: Option<u32>,
    /// The user's last prompt, cut short.
    pub last_prompt: String,
    /// The agent's last text, cut short.
    pub last_message: String,
    /// The node session it runs as, once Beep took it over.
    pub taken_over: Option<String>,
    /// Where it works now, as the node worked it out from the agent's
    /// records; `None` when it does not know.
    pub place: Option<WorkPlace>,
}

/// An external Claude Code session as its hooks report it, for
/// `SessionsRequest::Live`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveStatus {
    pub session: ExternalSession,
    pub state: LiveState,
    /// What it waits for, as Claude Code put it, while `Asking`.
    pub waiting_for: String,
    /// The tool call running now.
    pub tool: Option<LiveTool>,
    /// The last tool call that failed.
    pub error: Option<String>,
    /// Its todo list, when it keeps one.
    pub todos: Vec<PlanEntry>,
    /// Files its tools wrote since the node heard of it.
    pub files: Vec<String>,
    /// What waits for the next hook to deliver it.
    pub queued: Option<Steer>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveTool {
    pub name: String,
    /// What it works on: a command, a file, a pattern.
    pub detail: String,
    pub secs: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum LiveState {
    /// It runs a turn.
    Working,
    /// It waits for the user to answer in the terminal, such as a
    /// permission prompt.
    Asking,
    /// It waits at its prompt: no hook is left to fire, so it cannot be
    /// steered.
    Idle,
    Ended,
    /// No hook reported on it since the node started.
    #[default]
    #[serde(other)]
    Unknown,
}

/// What Beep sends a running external session through its hooks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Steer {
    /// Delivered after the current tool call, or instead of ending the turn.
    Message { text: String },
    /// Denies every tool call until the turn ends.
    Stop {},
}

/// Whether anything runs an external session now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Presence {
    /// Its process runs a turn.
    Working,
    /// Its process waits for the user.
    Waiting,
    /// Its process runs; what it does is not known.
    Running,
    /// No process was found, but its transcript changed a moment ago.
    Recent,
    /// Nothing runs it.
    Stopped,
    #[default]
    #[serde(other)]
    Unknown,
}

impl Presence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Waiting => "waiting for the user",
            Self::Running => "running",
            Self::Recent => "active a moment ago",
            Self::Stopped => "stopped",
            Self::Unknown => "unknown",
        }
    }

    /// A process may run it, so taking it over could clash.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Working | Self::Waiting | Self::Running | Self::Recent
        )
    }
}

impl fmt::Display for Presence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Something a followed external session did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    pub session: String,
    pub kind: ActivityKind,
    /// The agent's last text or the notice it showed, cut short.
    #[serde(default)]
    pub message: String,
    /// It came from a hook rather than from watching the transcript.
    #[serde(default)]
    pub from_hook: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ActivityKind {
    /// A turn ended.
    TurnDone,
    /// It waits for the user: a permission prompt or an idle prompt.
    NeedsInput,
    /// A process started or resumed it.
    Started,
    /// Its process ended.
    Ended,
    #[serde(other)]
    Other,
}

impl ActivityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TurnDone => "finished a turn",
            Self::NeedsInput => "is waiting for the user",
            Self::Started => "started",
            Self::Ended => "ended",
            Self::Other => "changed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader from before `place`: it reads with `lenient`, as the
    /// node's streams and mesh frames do, and skips the field it lacks.
    #[test]
    fn a_session_with_a_place_still_reads_for_an_older_reader() {
        #[derive(Deserialize)]
        struct Before {
            id: String,
            agent: String,
            cwd: String,
            title: Option<String>,
            started_at: u64,
            last_activity: u64,
            presence: Presence,
            pid: Option<u32>,
            last_prompt: String,
            last_message: String,
            taken_over: Option<String>,
        }
        let sent = ExternalSession {
            id: "s1".into(),
            agent: "codex".into(),
            cwd: "/srv/project".into(),
            taken_over: Some("a3f9c1".into()),
            place: Some(WorkPlace {
                folder: "/srv/project/.worktrees/fix".into(),
                main: "/srv/project".into(),
                linked: true,
                branch: Some("fix".into()),
                origin: String::new(),
            }),
            ..ExternalSession::default()
        };
        let bytes = rmp_serde::to_vec(&sent).expect("encodes");
        let read: Before = crate::lenient::from_slice(&bytes).expect("reads");
        assert_eq!(read.id, "s1");
        assert_eq!(read.agent, "codex");
        assert_eq!(read.cwd, "/srv/project");
        assert_eq!(read.title, None);
        assert_eq!((read.started_at, read.last_activity), (0, 0));
        assert_eq!(read.presence, Presence::default());
        assert_eq!(read.pid, None);
        assert!(read.last_prompt.is_empty() && read.last_message.is_empty());
        assert_eq!(read.taken_over.as_deref(), Some("a3f9c1"));
        // And one without it reads for this build.
        let old = rmp_serde::to_vec(&(
            "s2",
            "claude",
            "/srv/project",
            None::<String>,
            1u64,
            2u64,
            "Working",
            None::<u32>,
            "",
            "",
            None::<String>,
        ))
        .expect("encodes");
        let now: ExternalSession = crate::lenient::from_slice(&old).expect("reads");
        assert_eq!(now.place, None);
    }

    #[test]
    fn an_external_session_from_a_newer_node_still_reads() {
        #[derive(Serialize)]
        struct Newer {
            id: &'static str,
            presence: &'static str,
            branch: &'static str,
        }
        let body = rmp_serde::to_vec_named(&Newer {
            id: "s1",
            presence: "Compacting",
            branch: "main",
        })
        .unwrap();
        let session: ExternalSession = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(session.id, "s1");
        assert_eq!(session.presence, Presence::Unknown);
    }
}
