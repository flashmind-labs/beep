//! One agent session: the adapter process and the task that talks ACP to
//! it through the official crate. The task owns the process; streams send
//! it orders and follow its state through a watch channel.

use super::{
    AgentKind,
    acp::Prompt,
    alerts::AlertHub,
    compaction::{Compactor, Policy},
    images::Images,
    known,
    live::Live,
    mcp::Upstream,
    route, unix_now,
    usage::UsageHub,
};
use crate::{
    machine::provision::spawn_env::{self, Scope},
    run::process::{command, own_group},
};
use agent_client_protocol::schema::v1::McpServer;
use anyhow::{Context, Result};
use beep_proto::node::agent::{AnswerOption, Effort, EntryKind, SessionInfo, Status};
use beep_proto::{AgentAccount, ModelId};
use futures::stream::FuturesUnordered;
use std::{
    borrow::Cow,
    collections::{HashSet, VecDeque},
    path::Path,
    process::Stdio,
    str::FromStr,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::task::AbortOnDropHandle;

mod actor;
mod orders;
mod turns;

use actor::Actor;

/// The most prompts waiting behind a running turn.
const MAX_QUEUED: usize = 8;
/// The most bytes, text and images, waiting behind a running turn.
const MAX_QUEUED_BYTES: usize = 16 * 1024 * 1024;
/// The stderr lines kept to say why an agent stopped.
const STDERR_LINES: usize = 5;
/// The longest stderr line read whole; a longer one is skipped.
const MAX_STDERR_LINE: usize = 64 * 1024;
const ORDERS: usize = 16;
/// Updates and questions from the agent waiting for the session's task.
const EVENTS: usize = 64;
/// The longest text of a question the node asks.
const MAX_ASKED: usize = 2000;
/// The longest plan shown for approval.
const MAX_PLAN: usize = 32 * 1024;
/// How long an agent has to end its turn after Stop before the node stops
/// the agent itself. Agents answer `session/cancel` within a second or two;
/// one that has not after this is not going to.
/// Why a session ended with the node.
pub(super) const NODE_STOPPED: &str = "beep-node restarted; send a message to resume the session";
const STOP_WITHIN: Duration = Duration::from_secs(10);
/// The most tool calls followed at once, to know when none runs.
const MAX_RUNNING_TOOLS: usize = 256;

/// What a stream asks of a session.
#[derive(Debug)]
pub(super) enum Command {
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
    Cancel,
    Close,
    /// Switches the model or thinking level from the next turn.
    Configure {
        model: Option<ModelId>,
        effort: Option<Effort>,
    },
    /// Switches the session mode, at once.
    SetMode(String),
    /// Stops one of the agent's background tasks, by the agent's id, without
    /// ending the turn.
    StopTask(String),
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
    /// The session is about to move to another machine: no queued prompt
    /// starts a turn from here on, and the running turn is stopped when
    /// `stop` says so. The queue is kept.
    Moving {
        stop: bool,
    },
    /// The move is off: queued prompts start turns again.
    NotMoving,
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

struct Order {
    command: Command,
    done: oneshot::Sender<Result<(), String>>,
}

pub(super) struct Session {
    pub(super) id: String,
    pub(super) agent: String,
    orders: mpsc::Sender<Order>,
    pub(super) live: watch::Receiver<Live>,
    /// Dropping the session stops its task and with it the agent, at once.
    _task: AbortOnDropHandle<()>,
}

/// What a new session starts with.
pub(super) struct Start<'a> {
    pub(super) id: String,
    pub(super) kind: &'a AgentKind,
    pub(super) cwd: &'a Path,
    pub(super) prompt: Option<String>,
    /// The agent's id of an earlier session to resume instead of a new one.
    pub(super) resume: Option<String>,
    /// The model to run a new session on, rather than the agent's own.
    pub(super) model: Option<ModelId>,
    /// How hard a new session thinks, rather than the agent's own level.
    pub(super) effort: Option<Effort>,
    /// The session mode it was in before the node restarted, rather than
    /// the one that asks no permission.
    pub(super) mode: Option<String>,
    /// A first line for its transcript, such as the branch it started on.
    pub(super) note: Option<String>,
    /// Beep's word to the agent, sent ahead of the first prompt and kept
    /// out of the transcript.
    pub(super) brief: Option<Cow<'static, str>>,
    /// The Beep MCP server for the agent, passed in `session/new` and
    /// `session/load`. With it the brief also says what its tools are.
    pub(super) mcp: Option<McpServer>,
    /// Where to read the user's standing rules, which join the brief.
    pub(super) rules: Option<Upstream>,
    /// Where its transcript's pictures are kept.
    pub(super) images: Arc<Images>,
    /// Where what its turns spend goes.
    pub(super) usage: Arc<UsageHub>,
    /// Where what the user would want to hear about goes.
    pub(super) alerts: Arc<AlertHub>,
    /// What the node as a whole asks of its sessions.
    pub(super) node: NodeMode,
}

/// What the node as a whole asks of its sessions.
#[derive(Clone)]
pub(super) struct NodeMode {
    /// Set once the node is stopping: an agent that ends then ended with
    /// the node, whatever its exit says.
    pub(super) stopping: Arc<AtomicBool>,
    /// While true, prompts wait instead of starting a turn: the node is
    /// about to restart for an update.
    pub(super) holding: watch::Receiver<bool>,
}

impl Default for NodeMode {
    /// A node that never holds prompts and is not stopping.
    fn default() -> Self {
        Self {
            stopping: Arc::default(),
            holding: watch::channel(false).1,
        }
    }
}

impl Session {
    pub(super) fn start(start: Start<'_>) -> Result<Self> {
        Self::start_within(start, STOP_WITHIN)
    }

    /// `start`, giving the agent `stop_within` to end a turn after Stop.
    fn start_within(start: Start<'_>, stop_within: Duration) -> Result<Self> {
        let Start {
            id,
            kind,
            cwd,
            prompt,
            resume,
            model,
            effort,
            mode,
            note,
            brief,
            mcp,
            rules,
            images,
            usage,
            alerts,
            node,
        } = start;
        let memory = rules.as_ref().is_none_or(Upstream::has_server);
        let brief = match (brief, &mcp) {
            (Some(brief), Some(_)) => {
                Some(format!("{brief}\n\n{}", crate::agents::mcp::brief(memory)))
            }
            (None, Some(_)) => Some(crate::agents::mcp::brief(memory)),
            (brief, None) => brief.map(Cow::into_owned),
        };
        // Only a new session takes a model and thinking level; a resumed
        // one keeps its own.
        let model = model.filter(|_| resume.is_none());
        let effort = effort.filter(|_| resume.is_none());
        // An agent whose command line picks the model starts on it; the
        // others switch once the session exists.
        let known = known(&kind.name);
        let flag = model
            .as_ref()
            .filter(|_| known.is_some_and(|known| known.model_flag))
            .map(|model| format!("--model={model}"));
        let model = model.filter(|_| flag.is_none());
        let policy = Policy::from_setting(kind.compact);
        // An agent with its own auto-compaction is set to the same
        // threshold unless the user set it.
        let route = route(&kind.name);
        let agent_env = policy
            .env(route, &kind.env)
            .map(|var| (var.name, var.value));
        let mut process = command(&kind.program);
        // The token Beep keeps for this agent, such as Claude Code's, when
        // there is one.
        let account = AgentAccount::from_str(&kind.name).ok();
        spawn_env::apply(&mut process, Scope::Agent(account));
        process
            .args(&kind.args)
            .args(flag)
            .envs(&kind.env)
            .envs(agent_env)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // Its own process group, so closing it stops what it started.
        let child = own_group(&mut process)
            .spawn()
            .with_context(|| format!("starting {}", kind.program))?;
        let info = SessionInfo {
            id: id.clone(),
            agent: kind.name.clone(),
            cwd: cwd.display().to_string(),
            started_at: unix_now(),
            ..SessionInfo::default()
        };
        let mut first = Live::new(info, images, usage);
        if let Some(note) = note {
            first.push(EntryKind::Other, &note);
        }
        // Known from the start, so a second takeover of it is refused.
        first.agent_session = resume.clone();
        let (live_tx, live) = watch::channel(first);
        let (orders, orders_rx) = mpsc::channel(ORDERS);
        let actor = Actor {
            live: live_tx,
            alerts,
            node_stopping: node.stopping,
            holding: node.holding,
            moving: false,
            session: id.clone(),
            agent: kind.name.clone(),
            cwd: cwd.to_path_buf(),
            acp_session: None,
            takes_images: None,
            queued: prompt.map(Prompt::from).into_iter().collect(),
            steers: VecDeque::new(),
            sent: 0,
            turn: None,
            stopping: None,
            stop_within,
            running: HashSet::new(),
            brief,
            rules,
            mcp_servers: mcp.into_iter().collect(),
            resume,
            model,
            effort,
            thinking: None,
            switch: None,
            bypass_mode: known.and_then(|known| known.bypass_mode),
            first_mode: mode,
            left_plan_for: None,
            returning: None,
            compactor: Compactor::new(route, policy),
            asked: VecDeque::new(),
            next_question: 1,
            stderr: VecDeque::new(),
            replies: FuturesUnordered::new(),
        };
        let task = tokio::spawn(actor.run(child, orders_rx));
        Ok(Self {
            id,
            agent: kind.name.clone(),
            orders,
            live,
            _task: AbortOnDropHandle::new(task),
        })
    }

    pub(super) fn info(&self) -> SessionInfo {
        self.live.borrow().info.clone()
    }

    /// Hands `command` to the session and waits until it is done or refused.
    pub(super) async fn order(&self, command: Command) -> Result<(), String> {
        let (done, answer) = oneshot::channel();
        self.orders
            .send(Order { command, done })
            .await
            .map_err(|_| "the session has ended".to_owned())?;
        answer
            .await
            .map_err(|_| "the session has ended".to_owned())?
    }

    /// Closes the session and waits until its agent has stopped.
    pub(super) async fn close(&self) {
        // Refused when it already ends; either way it is waited for.
        let _ = self.order(Command::Close).await;
        let mut live = self.live.clone();
        // An error means the task is gone, which also ends the wait.
        let _ = live.wait_for(|live| live.status() == Status::Ended).await;
    }
}

#[cfg(test)]
mod tests;
