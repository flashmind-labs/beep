//! One agent session: the adapter process and the task that talks ACP to
//! it through the official crate. The task owns the process; streams send
//! it orders and follow its state through a watch channel.

use super::{
    AgentKind,
    acp::Prompt,
    alerts::AlertHub,
    compaction::{Compactor, Policy, Route},
    images::Images,
    known,
    live::Live,
    processes::Processes,
    route, unix_now,
    usage::UsageHub,
};
use crate::{
    machine::provision::spawn_env::{self, Scope},
    run::process::{command, own_group},
};
use agent_client_protocol::v1::schema::McpServer;
use anyhow::{Context, Result};
use beep_proto::ModelId;
use beep_proto::node::agent::{
    AgentCommand, AnswerOption, Effort, Entry, EntryKind, ParentLink, SessionDevice, SessionInfo, Status,
};
use beep_proto::watched;
use futures::stream::FuturesUnordered;
use std::{
    borrow::Cow,
    collections::{HashSet, VecDeque},
    path::Path,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::task::AbortOnDropHandle;

mod actor;
mod orders;
mod sccache;
mod turns;
mod v2;

use actor::Actor;

pub(super) use actor::joined as standing;

/// The most prompts waiting behind a running turn.
const MAX_QUEUED: usize = 8;
/// The most bytes, text and images, waiting behind a running turn.
const MAX_QUEUED_BYTES: usize = 16 * 1024 * 1024;
/// The stderr lines kept to say why an agent stopped.
const STDERR_LINES: usize = 5;
/// The longest stderr line read whole; a longer one is skipped.
const MAX_STDERR_LINE: usize = 64 * 1024;
const ORDERS: usize = 16;
/// How long an order may wait for the session's actor to take it.
const ORDER_WAIT: Duration = Duration::from_secs(20);
/// Updates and questions from the agent waiting for the session's task.
const EVENTS: usize = 64;
/// The longest text of a question the node asks.
const MAX_ASKED: usize = 2000;
/// The longest plan shown for approval.
const MAX_PLAN: usize = 32 * 1024;
/// Why a session ended with the node.
pub(super) const NODE_STOPPED: &str = "beep-node restarted; send a message to resume the session";
/// How long an agent has to end its turn after Stop before the node stops
/// the agent itself. Agents answer `session/cancel` within a second or two;
/// one that has not after this is not going to.
const STOP_WITHIN: Duration = Duration::from_secs(10);
/// The most tool calls followed at once, to know when none runs.
const MAX_RUNNING_TOOLS: usize = 256;

/// What a stream asks of a session.
#[derive(Debug)]
pub(super) enum Command {
    RecoverCodexChildren(Vec<crate::agents::SubagentRun>),
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
    },
    /// Switches the session mode, at once.
    SetMode(String),
    /// Stops one of the agent's background tasks, by the agent's id, without
    /// ending the turn.
    StopTask(String),
    /// Keeps the device the session uses, or forgets it.
    SetDevice(Option<SessionDevice>),
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
    /// Private handle for the process and its MCP connection, never a public session id.
    pub(super) id: String,
    pub(super) agent: String,
    orders: mpsc::Sender<Order>,
    pub(super) live: watch::Receiver<Live>,
    moved_cwd: Mutex<Option<String>>,
    /// A temporary reviewer (`agents::reviews`): never listed, never kept
    /// over a restart, offered `submit_review`.
    pub(super) reviewer: bool,
    pub(super) processes: Arc<Processes>,
    /// Dropping the session stops its task and with it the agent, at once.
    _task: AbortOnDropHandle<()>,
    /// What watches over its children, which stops with it.
    guards: Mutex<Vec<AbortOnDropHandle<()>>>,
    /// Messages from agents that are not its parent since the user last
    /// wrote, so two agents cannot keep each other talking.
    peer_messages: AtomicU32,
}

/// The most messages a session takes from other agents between two of the
/// user's.
pub(super) const PEER_MESSAGES: u32 = 5;

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .field("agent", &self.agent)
            .finish_non_exhaustive()
    }
}

/// What a new session starts with.
pub(super) struct Start<'a> {
    pub(super) id: String,
    pub(super) kind: &'a AgentKind,
    pub(super) cwd: &'a Path,
    pub(super) prompt: Option<Prompt>,
    /// The agent's id of an earlier session to resume instead of a new one.
    pub(super) resume: Option<String>,
    /// The resumed session's transcript from before the node restarted.
    pub(super) earlier: Vec<Entry>,
    pub(super) timestamps: Option<(u64, u64)>,
    pub(super) title: Option<String>,
    /// The model to run a new session on, rather than the agent's own.
    pub(super) model: Option<ModelId>,
    /// How hard a new session thinks, rather than the agent's own level.
    pub(super) effort: Option<Effort>,
    /// The session mode it was in before the node restarted, rather than
    /// the one that asks no permission.
    pub(super) mode: Option<String>,
    /// The session that started it with `start_agent`.
    pub(super) parent: Option<ParentLink>,
    /// The parent's state when it runs on this node: the session ends when
    /// the parent does.
    pub(super) follows: Option<watch::Receiver<Live>>,
    /// A first line for its transcript, such as the branch it started on.
    pub(super) note: Option<String>,
    /// Beep's word to the agent, kept out of the transcript: in its system
    /// prompt when it takes one, else ahead of the first prompt.
    pub(super) brief: Option<Cow<'static, str>>,
    /// Saved user instructions and Beep's checkout rule for the whole session.
    pub(super) rule: Option<String>,
    /// The Beep MCP server for the agent, passed in `session/new` and
    /// `session/load`. With it the brief also says what its tools are.
    pub(super) mcp: Option<McpServer>,
    /// Where its transcript's pictures are kept.
    pub(super) images: Arc<Images>,
    /// Where what its turns spend goes.
    pub(super) usage: Arc<UsageHub>,
    /// Where what the user would want to hear about goes.
    pub(super) alerts: Arc<AlertHub>,
    /// Set once the node is stopping: an agent that ends then ended with
    /// the node, whatever its exit says.
    pub(super) stopping: Arc<AtomicBool>,
}

/// Closes a child session once its parent ends for good.
async fn follow_parent(mut parent: watch::Receiver<Live>, orders: mpsc::Sender<Order>) {
    if super::family::parent_gone(&mut parent).await {
        let (done, _) = oneshot::channel();
        // Refused when the child ended first.
        let _ = orders
            .send(Order {
                command: Command::Close { by_user: false },
                done,
            })
            .await;
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
            earlier,
            timestamps,
            title,
            model,
            effort,
            mode,
            parent,
            follows,
            note,
            brief,
            rule,
            mcp,
            images,
            usage,
            alerts,
            stopping,
        } = start;
        let protocol = super::acp::Protocol::from_env(&kind.env)?;

        // Only a new session takes the default model and a thinking level. A
        // resumed one takes the model it ran on, when the caller kept it.
        let model = model.or_else(|| kind.default_model.clone().filter(|_| resume.is_none()));
        let effort = effort.filter(|_| resume.is_none());
        // An agent whose command line picks the model starts on it; the
        // others switch once the session exists.
        let known = known(&kind.name);
        let flag = model
            .as_ref()
            .filter(|_| known.is_some_and(|known| known.model_flag))
            .map(|model| format!("--model={model}"));
        let initial_model = model.clone().filter(|_| flag.is_some());
        let model = model.filter(|_| flag.is_none());
        let policy = Policy::from(kind.compact);
        // An agent with its own auto-compaction is set to the same
        // threshold unless the user set it.
        let route = route(&kind.name);
        // Model rules move the triggers to the node, including the fallback.
        // Turn off Claude's configurable trigger, as for a harness token limit.
        // Codex keeps its own default safety compaction.
        let native_policy = match (kind.compact_models.is_empty(), route) {
            (true, _) => Some(policy),
            (false, Route::Percent { .. }) => Some(Policy::Off),
            _ => None,
        };
        let agent_env = native_policy
            .and_then(|policy| policy.env(route, &kind.env))
            .map(|var| (var.name, var.value));
        // An off switch the node inherited, such as `DISABLE_AUTO_COMPACT`
        // from the shell that started it, would beat the setting.
        let cleared = native_policy.and_then(|policy| policy.cleared(route, &kind.env));
        let mut process = command(&kind.program);
        // The credentials this agent may have, such as Claude Code's token.
        spawn_env::apply(&mut process, Scope::for_agent(&kind.name));
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
        if let Some(name) = cleared {
            process.env_remove(name);
        }

        if let Some(wrapper) = sccache::wrapper(cwd, &kind.env) {
            process.env("RUSTC_WRAPPER", wrapper);
        }
        // Its own process group, so closing it stops what it started.
        let child = own_group(&mut process)
            .spawn()
            .with_context(|| format!("starting {}", kind.program))?;
        let processes = Processes::new(child.id().context("the agent has no pid")?);
        let info = SessionInfo {
            title,
            id: resume.clone().unwrap_or_default(),
            agent: kind.name.clone(),
            cwd: cwd.display().to_string(),
            started_at: timestamps
                .map(|(start, _)| start)
                .filter(|start| *start != 0)
                .unwrap_or_else(unix_now),
            last_activity: timestamps.map_or(0, |(_, activity)| activity),
            parent,
            model: initial_model,
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
            node_stopping: stopping,
            moving: false,
            agent: kind.name.clone(),
            cwd: cwd.to_path_buf(),
            acp_session: None,
            protocol,
            foreground: false,
            ready_for_prompts: false,
            initial_v2_state: None,
            compacting_v2: false,
            accepting_v2: false,
            idle_v2: None,
            takes_images: None,
            takes_steering: false,
            steering: None,
            steer_after_turn: false,
            queued: prompt.into_iter().collect(),
            steers: VecDeque::new(),
            sent: 0,
            context_turn: false,
            turn: None,
            held_end: None,
            stopping: None,
            by_user: false,
            stop_within,
            running: HashSet::new(),
            brief: brief.map(Cow::into_owned),
            standing: actor::joined(
                actor::joined(rule, mcp.is_some().then_some(super::mcp::BRIEF)),
                mcp.is_some().then_some(super::mcp::CHAT),
            ),
            mcp_servers: mcp.into_iter().collect(),
            resume,
            earlier,
            model,
            effort,
            thinking: None,
            mode_config: None,
            switch: None,
            bypass_mode: known.and_then(|known| known.bypass_mode),
            first_mode: mode,
            left_plan_for: None,
            returning: None,
            compactor: Compactor::new(route, policy).with_models(kind.compact_models.clone()),
            asked: VecDeque::new(),
            next_question: 1,
            stderr: VecDeque::new(),
            replies: FuturesUnordered::new(),
        };
        let guard = follows.map(|parent| {
            AbortOnDropHandle::new(tokio::spawn(follow_parent(parent, orders.clone())))
        });
        let task = tokio::spawn(actor.run(child, orders_rx, processes.clone()));
        Ok(Self {
            id,
            agent: kind.name.clone(),
            orders,
            live,
            moved_cwd: Mutex::new(None),
            reviewer: false,
            processes,
            _task: AbortOnDropHandle::new(task),
            guards: Mutex::new(guard.into_iter().collect()),
            peer_messages: AtomicU32::new(0),
        })
    }

    /// Keeps `task`, which watches over a child, for as long as the session
    /// lives.
    pub(super) fn keep(&self, task: tokio::task::JoinHandle<()>) {
        crate::lock(&self.guards).push(AbortOnDropHandle::new(task));
    }

    /// Counts one more message from another agent, or refuses it once
    /// `PEER_MESSAGES` came since the user last wrote.
    pub(super) fn take_peer_message(&self) -> Result<(), String> {
        self.peer_messages
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                (count < PEER_MESSAGES).then_some(count + 1)
            })
            .map(drop)
            .map_err(|_| {
                format!(
                    "It took {PEER_MESSAGES} messages from agents since the user last wrote. \
                     Stop and let the user decide."
                )
            })
    }

    /// The user wrote: other agents may message the session again.
    pub(super) fn user_wrote(&self) {
        self.peer_messages.store(0, Ordering::Relaxed);
    }

    pub(super) fn info(&self) -> SessionInfo {
        let mut info = watched::read(&self.live, |live| live.info.clone());
        if let Some(folder) = crate::lock(&self.moved_cwd).as_ref() {
            info.cwd.clone_from(folder);
        }
        info
    }

    pub(super) fn moved_to(&self, folder: &str) {
        *crate::lock(&self.moved_cwd) = Some(folder.to_owned());
    }

    /// Waits for ACP to name the session. A failed startup has no session to publish.
    pub(super) async fn named_info(&self) -> Result<SessionInfo, String> {
        named_info(&mut self.live.clone()).await
    }

    /// The commands and skills the agent last reported.
    pub(super) fn commands(&self) -> Vec<AgentCommand> {
        watched::read(&self.live, |live| live.commands.clone())
    }

    /// Hands `command` to the session and waits until it is done or refused.
    /// The actor answers an order at once; one stuck past `ORDER_WAIT` is
    /// an error and a line in the log, not a caller held for good (#427).
    pub(super) async fn order(&self, command: Command) -> Result<(), String> {
        let (done, answer) = oneshot::channel();
        let ordered = async {
            self.orders
                .send(Order { command, done })
                .await
                .map_err(|_| "the session has ended".to_owned())?;
            answer
                .await
                .map_err(|_| "the session has ended".to_owned())?
        };
        tokio::time::timeout(ORDER_WAIT, ordered)
            .await
            .unwrap_or_else(|_| {
                log::warn!(
                    "session {}: no answer to an order in {ORDER_WAIT:?}",
                    self.id
                );
                Err(format!(
                    "the session did not answer in {} s",
                    ORDER_WAIT.as_secs()
                ))
            })
    }

    /// Closes the session and waits until its agent has stopped.
    pub(super) async fn close(&self) {
        // Refused when it already ends; either way it is waited for.
        let _ = self.order(Command::Close { by_user: false }).await;
        let mut live = self.live.clone();
        // An error means the task is gone, which also ends the wait.
        let _ = watched::wait_for(&mut live, |live| live.status() == Status::Ended).await;
    }
}

async fn named_info(live: &mut watch::Receiver<Live>) -> Result<SessionInfo, String> {
    watched::wait_for(live, |live| {
        live.agent_session.is_some() || live.status() == Status::Ended
    })
    .await
    .map_err(|_| "the agent stopped before giving a session id".to_owned())?;

    watched::read(live, |named| {
        if named.agent_session.is_some() {
            Ok(named.info.clone())
        } else {
            Err(named
                .info
                .state
                .error
                .clone()
                .unwrap_or_else(|| "the agent stopped before giving a session id".to_owned()))
        }
    })
}

#[cfg(test)]
#[path = "../../tests/agents_session.rs"]
mod tests;
