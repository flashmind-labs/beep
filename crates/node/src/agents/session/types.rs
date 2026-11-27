//! What streams ask of a session, and the limits on how it takes them.

use crate::agents::{acp::Prompt, live::{Live, Mark, Sent}};
use beep_proto::node::agent::{AnswerOption, Effort, SessionDevice};
use beep_proto::{ModelId, SessionId};
use std::sync::{Arc, Mutex};
use tokio::sync::{oneshot, watch};

/// The most prompts waiting behind a running turn.
pub(super) const MAX_QUEUED: usize = 8;
/// The most bytes, text and images, waiting behind a running turn.
pub(super) const MAX_QUEUED_BYTES: usize = 16 * 1024 * 1024;
/// The stderr lines kept to say why an agent stopped.
pub(super) const STDERR_LINES: usize = 5;
/// Updates and questions from the agent waiting for the session's task.
pub(super) const EVENTS: usize = 64;
/// Why a session ended with the node.
pub(in crate::agents) const NODE_STOPPED: &str = "beep-node restarted; send a message to resume the session";
/// What a resumed session is told when the node restarted while it worked.
pub(in crate::agents) const CONTINUE: &str = "The node restarted while you were working. Continue the interrupted task, checking what already completed before repeating any actions. If you were waiting for an answer, ask again.";
/// The most tool calls followed at once, to know when none runs.
pub(super) const MAX_RUNNING_TOOLS: usize = 256;

/// The longest text of a question the node asks.
pub(super) const MAX_ASKED: usize = 2000;
/// The longest plan shown for approval.
pub(super) const MAX_PLAN: usize = 32 * 1024;
/// What a stream asks of a session.
#[derive(Debug, strum::IntoStaticStr)]
pub(in crate::agents) enum Command {
    RecoverCodexChildren(Vec<crate::logs::subagent::Run>),
    Prompt(Prompt),
    /// A message for the running turn: it goes once the agent's current
    /// tool call finishes, ahead of the prompts waiting.
    Steer(Prompt),
    /// Takes back the steers not yet sent.
    DropSteers,
    Answer {
        question: u32,
        reply: String,
    },
    /// Stops the main agent's running turn and drops the prompts queued
    /// behind it; its subagents and background commands keep running. When
    /// the user stopped it, the steers not yet sent go as the next prompt
    /// once the turn stops: the app that steered sends them nowhere else
    /// (#378). Otherwise its parent or the node stopped it.
    Cancel {
        by_user: bool,
    },
    /// `Cancel`, and stops the agent's background commands too. While its
    /// subagents run, which the agent stops only with itself, the agent is
    /// stopped and the session ends; the next message resumes it.
    StopAll {
        by_user: bool,
    },
    /// Ends the session; `by_user` as for `Cancel`.
    Close {
        by_user: bool,
    },
    /// Switches the model or thinking level from the next turn.
    Configure {
        model: Option<ModelId>,
        effort: Option<Effort>,
        fast: Option<bool>,
    },
    /// Switches the session mode, at once.
    SetMode(String),
    /// Stops one of the agent's background tasks, by the agent's id, without
    /// ending the turn.
    StopTask(String),
    /// Keeps the device the session uses, or forgets it.
    SetDevice(Option<SessionDevice>),
    /// How far the running `send_files` call has sent its files.
    Sending(Sent),
    /// Puts a question of the node's own to the user, as the agent's are:
    /// `ask_user` or an approval for the Beep MCP server. The answer goes
    /// to `reply`; it is dropped when the session ends or the turn is
    /// cancelled.
    Ask {
        question: Asking,
        reply: oneshot::Sender<Answered>,
    },
    /// Takes back the node's questions whose caller stopped waiting, so their cards go and a
    /// late answer is refused.
    Withdraw,
    /// The session is about to move to another machine: it takes no new
    /// prompt, runs the ones already queued and ends once they are done.
    /// The running turn and the queue are stopped when `stop` says so.
    Moving {
        stop: bool,
    },
    /// The move is off: prompts are taken again.
    NotMoving,
    /// Its parent ended for good, so it closes: unless it is moving, when
    /// its parent may have left first for the machine it goes to.
    ParentGone,
    /// Notes what it now saw of relatives, by each one's `id` here.
    Saw(Vec<(SessionId, Mark)>),
    /// Sets whether the user's message began the turn that runs or ran last.
    UserTurn(bool),
    /// Gives that and clears it.
    TakeUserTurn(oneshot::Sender<bool>),
}

/// A question the node asks in a session.
#[derive(Debug)]
pub(crate) struct Asking {
    pub(crate) text: String,
    pub(crate) detail: Option<String>,
    pub(crate) options: Vec<AnswerOption>,
}

/// The user's answer to an `Asking`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Answered {
    /// One of the options.
    Picked(AnswerOption),
    /// Their own words instead.
    Said(String),
}

pub(super) struct Order {
    pub(super) command: Command,
    pub(super) done: oneshot::Sender<Result<(), String>>,
}

impl From<&Order> for &'static str {
    fn from(order: &Order) -> Self {
        (&order.command).into()
    }
}

/// Where a session's task leaves its state sender when it ends.
pub(super) type Kept = Arc<Mutex<Option<watch::Sender<Live>>>>;
