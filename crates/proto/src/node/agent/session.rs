//! One agent session's stream: what the server asks and what the node sends.

use super::{CheckoutPlan, Effort};
use crate::{AgentContext, ModelId, Upload};
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::fmt;

/// What the server sends on an agent stream.
/// A node older than a request answers that it does not know it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// The first message: starts a session in `cwd` (the home folder when
    /// `None`) on `prompt`, on `model` when given (an id from
    /// `SessionsRequest::Models`) and thinking at `effort` when given.
    /// With `checkout`, it first makes the branch or worktree it names in
    /// `cwd`'s git checkout and starts there. A node from before any of
    /// these ignores it; one that makes checkouts says so with
    /// `NodeHello::checkouts`.
    Start {
        cwd: Option<String>,
        prompt: String,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        effort: Option<Effort>,
        #[serde(default)]
        checkout: Option<CheckoutPlan>,
    },
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
    /// `Prompt` with images, sent to the agent after the text. Refused, not
    /// sent without them, when the agent takes no images.
    PromptImages { text: String, images: Vec<Upload> },
    /// Answers the open question. `reply` is one of its options, by id,
    /// number or name; anything else declines and goes to the agent as the
    /// next prompt.
    Answer { question: u32, reply: String },
    /// Stops the running turn and drops waiting prompts.
    Cancel {},
    /// Ends the session and stops the agent.
    Close {},
    /// Switches the running session to `model` and thinking at `effort` (`None`
    /// keeps it), from its next turn. Refused when the agent cannot switch.
    Configure {
        model: Option<ModelId>,
        effort: Option<Effort>,
    },
    /// A message for the running turn: sent once the agent's current tool call
    /// finishes, by stopping the turn there and prompting again, and ahead of
    /// any `Prompt` waiting. With no turn running it goes as a `Prompt`.
    Steer { text: String, images: Vec<Upload> },
    /// Takes back the `Steer`s not yet sent to the agent.
    DropSteers {},
    /// Switches the running session to `mode`, one of `SessionInfo::modes`.
    SetMode { mode: String },
    /// Stops the background task `task` (one of `State::tasks`) without
    /// ending the turn. Refused when it is not running or cannot be stopped.
    StopTask { task: String },
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
    /// How full the context window is, as the agent last said.
    pub context: Option<AgentContext>,
    /// Every token the finished turns used.
    pub tokens: u64,
    /// What the finished turns cost at API prices, in US dollars, when the
    /// agent reports a cost.
    pub cost_usd: Option<f64>,
    /// The modes it can be switched between, in the order a switch goes
    /// through them; empty for an agent that offers none.
    pub modes: Vec<ModeOption>,
    /// The id of the mode it is in, once the agent said.
    pub mode: Option<String>,
    /// The plan `question` asks to approve, in Markdown.
    pub plan_ready: Option<String>,
    /// Where it works now, as the node worked it out; `None` before the
    /// node knows or when the folder is in no git checkout.
    pub place: Option<WorkPlace>,
    /// The session that started this one with `start_agent`, when one did.
    /// Always the last field: readers skip what they do not know.
    pub parent: Option<ParentLink>,
}

/// Who started a session and how deep the chain goes, as the node that
/// started it recorded and as it passes it across machines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParentLink {
    /// The machine the parent runs on; `None` for a node out of a mesh.
    #[serde(default)]
    pub machine: Option<EndpointId>,
    /// The parent's id: the agent's own when it has one.
    pub session: String,
    /// The id the parent's node listed it under when this was made, which
    /// the agent's own id replaces once the node restarts.
    #[serde(default)]
    pub key: String,
    /// The session at the top of the chain, and its machine.
    #[serde(default)]
    pub root_machine: Option<EndpointId>,
    pub root: String,
    /// How many starts deep this session is: 1 for a child of a root.
    pub depth: u8,
    /// Not read: the node that starts a child counts the root's agents on
    /// every machine itself. Kept so the fields after it keep their place.
    #[serde(default)]
    pub others: u8,
    /// The parent's agent, such as `claude`.
    #[serde(default)]
    pub agent: String,
    /// The mode the parent was held to when it started this one, when it
    /// was not in full access: a child may not run above it.
    #[serde(default)]
    pub mode: Option<String>,
}

/// The git checkout a session works in, as its node works it out from what
/// the agent records: its latest folder, else its last tool call's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkPlace {
    /// The top of the checkout holding the folder.
    pub folder: String,
    /// The main folder of the repository: the checkout `folder` was added
    /// to when it is a linked worktree, else `folder`.
    pub main: String,
    /// `folder` is a linked worktree, not the repository's main checkout.
    pub linked: bool,
    /// The branch checked out; `None` when the head is detached.
    pub branch: Option<String>,
    /// The `origin` remote's URL; empty when there is none.
    pub origin: String,
}

/// A session mode an agent offers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModeOption {
    pub id: String,
    pub name: String,
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
    /// The commands the agent runs in the background, oldest first. Only
    /// an agent that reports them has any; a task stays after it ended.
    pub tasks: Vec<Task>,
}

/// A command the agent left running in the background.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Task {
    /// The agent's own id for it.
    pub id: String,
    pub name: String,
    pub description: String,
    pub state: TaskState,
    /// The agent can stop it.
    pub can_stop: bool,
    /// The file its output goes to, on the node.
    pub output_file: Option<String>,
    /// The tool call that started it, by the agent's id for the call.
    pub tool_call: Option<String>,
    /// How it ended, when the agent says.
    pub summary: Option<String>,
    /// The transcript entry of the tool call that started it, counted from
    /// the start of the session, once the call is known.
    pub entry: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TaskState {
    #[default]
    Running,
    Paused,
    Completed,
    Failed,
    Stopped,
    /// A state from a newer node.
    #[serde(other)]
    Unknown,
}

impl TaskState {
    /// The state ACP names `name`, such as `running`.
    pub fn from_acp(name: &str) -> Self {
        match name {
            "running" => Self::Running,
            "paused" => Self::Paused,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "stopped" => Self::Stopped,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
            Self::Unknown => "unknown",
        }
    }

    pub fn ended(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Stopped)
    }
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

/// One entry of a transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub kind: EntryKind,
    pub text: String,
    /// How long a thought took, in milliseconds, once it ended. Only on
    /// `EntryKind::Thought` entries.
    #[serde(default)]
    pub ms: Option<u64>,
}

impl Entry {
    pub fn new(kind: EntryKind, text: String) -> Self {
        Self {
            kind,
            text,
            ms: None,
        }
    }
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
    /// What the agent thought, as its `agent_thought_chunk`s said it.
    Thought,
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
            Self::Thought => "thought",
            Self::Other => "other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_start_with_a_model_reads_on_an_older_node_and_an_older_start_here() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        enum Older {
            Start { cwd: Option<String>, prompt: String },
        }
        let newer = Request::Start {
            cwd: Some("/srv/project".into()),
            prompt: "go".into(),
            model: Some("m".into()),
            effort: Some(Effort::High),
            checkout: Some(CheckoutPlan::NewBranch { base: None }),
        };
        let body = rmp_serde::to_vec_named(&newer).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Older>(&body).unwrap(),
            Older::Start {
                cwd: Some("/srv/project".into()),
                prompt: "go".into()
            }
        );
        let body = rmp_serde::to_vec_named(&Older::Start {
            cwd: None,
            prompt: "go".into(),
        })
        .unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Request>(&body).unwrap(),
            Request::Start {
                cwd: None,
                prompt: "go".into(),
                model: None,
                effort: None,
                checkout: None,
            }
        );
    }

    #[test]
    fn a_session_with_a_parent_reads_on_an_older_reader_and_back() {
        #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        struct Older {
            id: String,
            agent: String,
            turns: u32,
        }
        let newer = SessionInfo {
            id: "c1".into(),
            agent: "codex".into(),
            turns: 2,
            parent: Some(ParentLink {
                machine: None,
                session: "p1".into(),
                key: "p1".into(),
                root_machine: None,
                root: "p1".into(),
                depth: 1,
                others: 0,
                agent: "claude".into(),
                mode: None,
            }),
            ..SessionInfo::default()
        };
        let named = rmp_serde::to_vec_named(&newer).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<Older>(&named).unwrap(),
            Older {
                id: "c1".into(),
                agent: "codex".into(),
                turns: 2
            }
        );
        // A session from a node before the field has no parent.
        let old = rmp_serde::to_vec_named(&Older::default()).unwrap();
        let read = rmp_serde::from_slice::<SessionInfo>(&old).unwrap();
        assert_eq!(read.parent, None);
        assert_eq!(rmp_serde::from_slice::<SessionInfo>(&named).unwrap(), newer);
    }

    #[test]
    fn a_session_with_a_parent_reads_compact_on_a_lenient_older_reader() {
        // What a node from before `parent` reads: the fields up to `place`.
        #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        struct Older {
            id: String,
            agent: String,
            cwd: String,
            title: Option<String>,
            started_at: u64,
            state: State,
            question: Option<Question>,
            last_message: String,
            turns: u32,
            context: Option<AgentContext>,
            tokens: u64,
            cost_usd: Option<f64>,
            modes: Vec<ModeOption>,
            mode: Option<String>,
            plan_ready: Option<String>,
            place: Option<WorkPlace>,
        }
        let newer = SessionInfo {
            id: "c1".into(),
            agent: "codex".into(),
            turns: 3,
            parent: Some(ParentLink {
                machine: None,
                session: "p1".into(),
                key: "p1".into(),
                root_machine: None,
                root: "p1".into(),
                depth: 2,
                others: 1,
                agent: "claude".into(),
                mode: Some("plan".into()),
            }),
            ..SessionInfo::default()
        };
        let compact = rmp_serde::to_vec(&newer).unwrap();
        let read: Older = crate::lenient::from_slice(&compact).unwrap();
        assert_eq!((read.id.as_str(), read.turns), ("c1", 3));
        // Plain rmp refuses the longer array, which is why readers are lenient.
        assert!(rmp_serde::from_slice::<Older>(&compact).is_err());
        // A node from before the field sends the short array.
        let short = rmp_serde::to_vec(&Older::default()).unwrap();
        let read: SessionInfo = crate::lenient::from_slice(&short).unwrap();
        assert_eq!(read.parent, None);
        assert_eq!(
            crate::lenient::from_slice::<SessionInfo>(&compact).unwrap(),
            newer
        );
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
