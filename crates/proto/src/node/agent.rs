//! Coding agents on a node, such as Claude Code or Codex, driven over ACP by
//! the daemon.
//!
//! Each agent is its own protocol, `<agent>/1` (`claude/1`, `codex/1`), and
//! all of them speak the messages here. A stream is one agent session: the
//! server opens it with `Start` or `Attach`, the node answers `Attached` with
//! the session as it stands, then the server steers it with `Prompt`,
//! `Answer`, `Cancel` and `Close` (each answered by one `Ack`) while the node
//! sends `State`, `Question` and `TurnDone` as they happen. Sessions live in
//! the daemon, not in the stream: when the stream drops the agent keeps
//! working, and a later `Attach` picks it up again.
//!
//! `agents/1` lists the sessions and reads their transcripts, one request per
//! stream. It also lists the sessions the user started on the machine
//! outside Beep (`External`), such as Claude Code in a terminal, and
//! `Watch` follows them: one request, then `Activity` replies until the
//! server closes the stream. `Resume` takes one of them over once nothing
//! else runs it.

use crate::Upload;
use serde::{Deserialize, Serialize};
use std::fmt;

/// The version every agent protocol shares.
const VERSION: &str = "1";

/// Lists sessions and reads transcripts.
pub const SESSIONS_PROTOCOL: &str = "agents/1";

/// The protocol for the agent `name`, such as `claude/1`.
pub fn protocol(name: &str) -> String {
    format!("{name}/{VERSION}")
}

/// The agent a protocol name serves, such as `claude` for `claude/1`.
pub fn agent_of(protocol: &str) -> Option<&str> {
    let (name, version) = protocol.split_once('/')?;
    (version == VERSION
        && !matches!(
            name,
            "shell" | "agents" | "desktop" | "files" | "process" | "sync" | "transfer"
        ))
    .then_some(name)
}

/// What the server sends on an agent stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// The first message: starts a session in `cwd` (the home folder when
    /// `None`) on `prompt`.
    Start { cwd: Option<String>, prompt: String },
    /// The first message: follows a session that already runs.
    Attach { session: String },
    /// The first message: takes over a session started outside Beep, by its
    /// agent's own id (see `ExternalSession`), once nothing else runs it.
    /// It then runs here like a started one, under a new id, on `prompt`
    /// when there is one.
    Resume {
        session: String,
        #[serde(default)]
        prompt: Option<String>,
    },
    /// Sends the agent a message. While a turn runs it waits for the turn
    /// to end.
    Prompt { text: String },
    /// `Prompt` with images, sent to the agent after the text. Refused,
    /// not sent without them, when the agent takes no images. A node from
    /// before it answers that it does not know the request.
    PromptImages { text: String, images: Vec<Upload> },
    /// Answers the open question. `reply` is one of its options, by id,
    /// number or name; anything else declines and goes to the agent as the
    /// next prompt.
    Answer { question: u32, reply: String },
    /// Stops the running turn and drops waiting prompts.
    Cancel {},
    /// Ends the session and stops the agent.
    Close {},
}

/// What the node sends on an agent stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    /// The answer to `Start` or `Attach`.
    Attached { session: Box<SessionInfo> },
    /// The answer to `Start` or `Attach` when there is no session.
    Refused { reason: String },
    /// One per request after the first, in order. `error` says why it did
    /// nothing.
    Ack {
        #[serde(default)]
        error: Option<String>,
    },
    /// The session's state changed.
    State { state: State },
    /// The agent asks before it acts.
    Question { question: Question },
    /// A turn ended. `turn` counts the turns so far.
    TurnDone {
        turn: u32,
        stop_reason: StopReason,
        last_message: String,
    },
    /// The session ended; the stream ends after this.
    Ended { reason: String },
}

/// A session as the node keeps it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionInfo {
    /// Short and unique on the node, such as `a3f9c1`.
    pub id: String,
    /// The agent, such as `claude`.
    pub agent: String,
    pub cwd: String,
    /// The title the agent gave the session, when it gives one.
    pub title: Option<String>,
    /// Unix seconds.
    pub started_at: u64,
    pub state: State,
    pub question: Option<Question>,
    /// The agent's last message, cut short.
    pub last_message: String,
    pub turns: u32,
}

/// What a session is doing, small enough to send on every change.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub status: Status,
    /// What the agent is doing now, such as "Edit src/main.rs".
    pub summary: String,
    pub plan: Vec<PlanEntry>,
    /// Files the agent changed, newest last.
    pub files_touched: Vec<String>,
    /// Prompts waiting for the running turn to end.
    pub queued: u32,
    /// Why the agent failed or the last turn went wrong.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Status {
    /// The agent is starting up.
    #[default]
    Starting,
    /// A turn is running.
    Working,
    /// A turn waits for an answer to `question`.
    Asking,
    /// Waiting for a prompt.
    Idle,
    /// The agent stopped: it failed, exited or was closed.
    Ended,
    /// A status from a newer node.
    #[serde(other)]
    Unknown,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Working => "working",
            Self::Asking => "waiting for an answer",
            Self::Idle => "idle",
            Self::Ended => "ended",
            Self::Unknown => "unknown",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanEntry {
    pub content: String,
    #[serde(default)]
    pub status: PlanStatus,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PlanStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    #[serde(other)]
    Unknown,
}

/// The longest question `detail` the node sends: it cuts longer inputs
/// without a marker, so a question this long may be only the start of a
/// command.
pub const MAX_QUESTION_LINE: usize = 300;

/// A permission question from the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    /// Counts up within the session.
    pub id: u32,
    /// What the agent wants to do, such as "Run cargo test".
    pub text: String,
    /// The command, path or other input it would act on, cut short.
    #[serde(default)]
    pub detail: Option<String>,
    pub options: Vec<AnswerOption>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerOption {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: OptionKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    #[default]
    #[serde(other)]
    Other,
}

impl OptionKind {
    /// The kind ACP names `name`, such as `allow_once`.
    pub fn from_acp(name: &str) -> Self {
        match name {
            "allow_once" => Self::AllowOnce,
            "allow_always" => Self::AllowAlways,
            "reject_once" => Self::RejectOnce,
            "reject_always" => Self::RejectAlways,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllowOnce => "allow_once",
            Self::AllowAlways => "allow_always",
            Self::RejectOnce => "reject_once",
            Self::RejectAlways => "reject_always",
            Self::Other => "other",
        }
    }

    pub fn rejects(self) -> bool {
        matches!(self, Self::RejectOnce | Self::RejectAlways)
    }
}

/// Why a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
    /// The agent answered the prompt with an error.
    Failed,
    #[serde(other)]
    Other,
}

impl StopReason {
    /// The reason ACP names `name`, such as `end_turn`.
    pub fn from_acp(name: &str) -> Self {
        match name {
            "end_turn" => Self::EndTurn,
            "max_tokens" => Self::MaxTokens,
            "max_turn_requests" => Self::MaxTurnRequests,
            "refusal" => Self::Refusal,
            "cancelled" => Self::Cancelled,
            _ => Self::Other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::EndTurn => "finished",
            Self::MaxTokens => "hit the token limit",
            Self::MaxTurnRequests => "hit the request limit",
            Self::Refusal => "refused",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Other => "stopped",
        }
    }
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the server sends on an `agents/1` stream: one request, one reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SessionsRequest {
    List {},
    /// The last `last` entries of a session's transcript. `session` may
    /// also be an external session's id.
    Transcript {
        session: String,
        last: u32,
    },
    /// Sessions of `agent` started outside Beep, newest first: at most
    /// `limit`, or only `session` when given.
    External {
        agent: String,
        #[serde(default)]
        limit: u32,
        #[serde(default)]
        session: Option<String>,
    },
    /// Follows these external sessions: `Watching` first, then `Activity`
    /// as they finish turns or wait for the user, until the server closes
    /// the stream.
    Watch {
        sessions: Vec<String>,
    },
    /// The git checkouts coding agents worked in, from the sessions Claude
    /// Code, Codex, pi, flsh and OpenCode keep: at most `limit`, newest activity
    /// first. Answered with `Folders`. A node from before it answers
    /// `Error` or closes the stream.
    Folders {
        #[serde(default)]
        limit: u32,
    },
    /// The sessions those agents ran in any of `folders` (or a subfolder or
    /// worktree of one), newest first, at most `limit`. Answered with
    /// `External`, `title` falling back to the first prompt.
    InFolders {
        folders: Vec<String>,
        #[serde(default)]
        limit: u32,
    },
    /// The picture of the `[image: image-<key>]` line in `session`'s
    /// transcript, `key` being its `image_key`. Answered with `Image`, or
    /// `Error` when the node no longer has it. A node from before it
    /// closes the stream.
    Image {
        session: String,
        key: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SessionsReply {
    Sessions {
        sessions: Vec<SessionInfo>,
    },
    Transcript {
        entries: Vec<Entry>,
        /// Entries before these ones.
        earlier: u32,
    },
    Error {
        message: String,
    },
    External {
        sessions: Vec<ExternalSession>,
    },
    /// The answer to `Watch`: the sessions it could not find.
    Watching {
        unknown: Vec<String>,
    },
    Activity {
        activity: Activity,
    },
    Folders {
        folders: Vec<SessionFolder>,
    },
    /// The answer to `Image`.
    Image {
        mime: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
}

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
    /// When a session there last changed, Unix seconds.
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

/// One entry of a transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub kind: EntryKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EntryKind {
    /// A prompt sent to the agent.
    User,
    /// What the agent said.
    Agent,
    /// A tool call and how it went.
    Tool,
    /// A permission question and its answer.
    Question,
    #[serde(other)]
    Other,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Agent => "agent",
            Self::Tool => "tool",
            Self::Question => "question",
            Self::Other => "other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocols_name_their_agent() {
        assert_eq!(protocol("claude"), "claude/1");
        assert_eq!(agent_of("codex/1"), Some("codex"));
        assert_eq!(agent_of("shell/1"), None);
        assert_eq!(agent_of("files/1"), None);
        assert_eq!(agent_of("process/1"), None);
        assert_eq!(agent_of(crate::node::sync::PROTOCOL), None);
        assert_eq!(agent_of(crate::node::transfer::PROTOCOL), None);
        assert_eq!(agent_of(SESSIONS_PROTOCOL), None);
        assert_eq!(agent_of("claude/2"), None);
    }

    #[test]
    fn a_newer_status_still_reads() {
        #[derive(Serialize)]
        struct Newer {
            status: &'static str,
            summary: &'static str,
            mood: u8,
        }
        let body = rmp_serde::to_vec_named(&Newer {
            status: "Compacting",
            summary: "Thinking",
            mood: 3,
        })
        .unwrap();
        let state: State = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(state.status, Status::Unknown);
        assert_eq!(state.summary, "Thinking");
    }

    #[test]
    fn external_requests_and_replies_round_trip() {
        let request = SessionsRequest::External {
            agent: "claude".into(),
            limit: 20,
            session: None,
        };
        let body = rmp_serde::to_vec_named(&request).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsRequest>(&body).unwrap(),
            request
        );
        let reply = SessionsReply::Activity {
            activity: Activity {
                session: "0f3c2a9e-1111-4222-8333-944455556666".into(),
                kind: ActivityKind::NeedsInput,
                message: "Claude needs your permission to use Bash".into(),
                from_hook: true,
            },
        };
        let body = rmp_serde::to_vec_named(&reply).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<SessionsReply>(&body).unwrap(),
            reply
        );
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

    #[test]
    fn events_round_trip() {
        let event = Event::Question {
            question: Question {
                id: 1,
                text: "Run cargo test".into(),
                detail: None,
                options: vec![AnswerOption {
                    id: "allow".into(),
                    name: "Allow".into(),
                    kind: OptionKind::AllowOnce,
                }],
            },
        };
        let body = rmp_serde::to_vec_named(&event).unwrap();
        assert_eq!(rmp_serde::from_slice::<Event>(&body).unwrap(), event);
    }
}
