//! One agent session: the adapter process and the task that talks ACP to
//! it through the official crate. The task owns the process; streams send
//! it orders and follow its state through a watch channel.

use crate::known::{CODEX_CONFIG, OPENCODE_PERMISSION, known};
use super::{
    AgentKind,
    acp::Prompt,
    alerts::AlertHub,
    checkouts::Checkouts,
    compaction::{Compactor, Policy, Route},
    images::Images,
    live::{Live, Mark, Sent},
    processes::Processes,
    route, unix_now,
    usage::UsageHub,
};
use crate::{
    process::spawn_env::{self, Scope},
    process::command,
};
use agent_client_protocol::v1::schema::McpServer;
use anyhow::Result;
use beep_proto::{AccountName, ModelId};
use beep_proto::SessionId;
use beep_proto::node::agent::{
    AgentCommand, AnswerOption, Effort, EntryKind, SessionDevice, SessionInfo,
    Status,
};
use beep_proto::watched;
use futures::stream::FuturesUnordered;
use std::{
    borrow::Cow,
    collections::{HashSet, VecDeque},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};
use tokio::sync::{oneshot, watch};
use tokio_util::task::AbortOnDropHandle;

mod actor;
mod begin;
mod cargo;
mod orders;
mod pi;
mod turns;
mod v2;

use actor::{Accepting, Actor, Node, Offers, Opening};

pub(super) use actor::joined as standing;

/// The most prompts waiting behind a running turn.
const MAX_QUEUED: usize = 8;
/// The most bytes, text and images, waiting behind a running turn.
const MAX_QUEUED_BYTES: usize = 16 * 1024 * 1024;
/// The stderr lines kept to say why an agent stopped.
const STDERR_LINES: usize = 5;
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
/// What a resumed session is told when the node restarted while it worked.
pub(in crate::agents) const CONTINUE: &str = "The node restarted while you were working. Continue the interrupted task, checking what already completed before repeating any actions. If you were waiting for an answer, ask again.";
/// How long an agent has to end its turn after Stop before the node stops
/// the agent itself. Agents answer `session/cancel` within a second or two;
/// one that has not after this is not going to.
const STOP_WITHIN: Duration = Duration::from_secs(10);
/// The most tool calls followed at once, to know when none runs.
const MAX_RUNNING_TOOLS: usize = 256;

pub(crate) use begin::Begin;
pub(in crate::agents) use begin::History;

/// Whether the parent ended for good: not just with the node, which
/// resumes it, and its children, after the restart.
pub(super) fn ended_for_good(live: &Live) -> bool {
    live.status() == Status::Ended
        && live.info.state.error.as_deref() != Some(NODE_STOPPED)
}

/// Waits until the parent session ends for good; false when it ends with
/// the node.
pub(super) async fn parent_gone(parent: &mut watch::Receiver<Live>) -> bool {
    match watched::wait_for(parent, |live| live.status() == Status::Ended).await {
        Ok(()) => watched::read(parent, ended_for_good),
        // The session was dropped.
        Err(_) => true,
    }
}

/// What a stream asks of a session.
#[derive(Debug, strum::IntoStaticStr)]
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

struct Order {
    command: Command,
    done: oneshot::Sender<Result<(), String>>,
}

impl From<&Order> for &'static str {
    fn from(order: &Order) -> Self {
        (&order.command).into()
    }
}

/// Where a session's task leaves its state sender when it ends.
pub(super) type Kept = Arc<Mutex<Option<watch::Sender<Live>>>>;

pub(crate) struct Session {
    /// Private handle for the process and its MCP connection, never a public session id.
    pub(crate) id: SessionId,
    pub(crate) agent: String,
    orders: crate::actor::Sender<Order>,
    pub(crate) live: watch::Receiver<Live>,
    /// Where the retained live replay begins in Claude's saved transcript.
    pub(super) history: Option<crate::AsyncMutex<Option<History>>>,
    /// The session's state sender once its task ended, so a refile can
    /// write the new folder into the one copy. Held from the moment the
    /// last state is published, never before, so a task that dies without
    /// that still drops it and its watchers see the session gone.
    kept: Kept,
    /// A temporary reviewer (`agents::reviews`): never listed, never kept
    /// over a restart, offered `submit_review`.
    pub(crate) reviewer: bool,
    /// The home assistant: offered the assistant's brief and tools.
    pub(super) assistant: bool,
    /// The named account it runs signed in as; `None` for the default one.
    pub(crate) account: Option<AccountName>,
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
    pub(super) id: SessionId,
    pub(super) kind: &'a AgentKind,
    pub(super) cwd: &'a Path,
    /// What the caller asked for. Its `brief` is the final one, with its
    /// `instructions` already in it.
    pub(super) begin: Begin,
    /// Saved user instructions, delivered through a system prompt when supported.
    pub(super) instructions: Option<String>,
    /// Beep's checkout rule for the whole session.
    pub(super) rule: Option<String>,
    /// The skills Beep keeps in its own folder, listed for the agent.
    pub(super) skills: Option<String>,
    /// The Beep MCP server for the agent, passed in `session/new` and
    /// `session/load`. With it the standing prompt also frames Beep and its tools.
    pub(super) mcp: Option<McpServer>,
    /// Runtime context woven into Beep's standing prompt when `mcp` is set.
    pub(super) context: super::brief::Context,
    /// Where its transcript's pictures are kept.
    pub(super) images: Arc<Images>,
    /// Where what its turns spend goes.
    pub(super) usage: Arc<UsageHub>,
    /// Where what the user would want to hear about goes.
    pub(super) alerts: Arc<AlertHub>,
    /// What git says of the folder it works in.
    pub(super) checkouts: Arc<Checkouts>,
    /// Set once the node is stopping: an agent that ends then ended with
    /// the node, whatever its exit says.
    pub(super) stopping: Arc<AtomicBool>,
    /// The folder it runs in, marked to Git sync until it ends.
    pub(super) running: Option<crate::files::holds::Running>,
    /// The agent's own Docker socket, set as its `DOCKER_HOST`.
    pub(super) docker: Option<super::processes::Docker>,
}

/// Closes a child session once its parent ends for good.
async fn follow_parent(mut parent: watch::Receiver<Live>, orders: crate::actor::Sender<Order>) {
    if parent_gone(&mut parent).await {
        let (done, _) = oneshot::channel();
        // Refused when the child ended first.
        let _ = orders
            .send(Order {
                command: Command::ParentGone,
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
            begin,
            mut instructions,
            rule,
            skills,
            mcp,
            context,
            images,
            usage,
            alerts,
            checkouts,
            stopping,
            running,
            docker,
        } = start;
        let Begin {
            prompt,
            images: attached,
            resume,
            earlier,
            timestamps,
            title,
            named,
            model,
            effort,
            fast,
            mode,
            note,
            brief,
            worktrees,
            parent,
            follows,
            assistant,
            account,
            persona,
            continues,
            issue,
            ..
        } = begin;
        let prompt = prompt.map(|text| Prompt {
            text,
            images: attached,
            context: None,
        });
        let protocol = super::acp::Protocol::from_env(&kind.env)?;

        // Only a new session takes the default model. A resumed one takes the
        // model and thinking level it ran on, when the caller kept them.
        let model = model.or_else(|| kind.default_model.clone().filter(|_| resume.is_none()));
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
        let image_titling = resume.is_none() && title.is_none() && !named;
        let environment = mcp.is_some().then(|| super::brief::environment(&context));
        let chat = mcp.is_some().then(|| super::brief::chat(image_titling));
        let own_folder = (assistant && mcp.is_some())
            .then(dirs::home_dir)
            .flatten()
            .and_then(|home| super::conversation::brief(&home, cwd));
        let mut standing = [
            environment.as_deref(),
            rule.as_deref(),
            (assistant && mcp.is_some()).then_some(super::brief::ASSISTANT),
            own_folder,
            chat.as_deref(),
            skills.as_deref(),
        ]
        .into_iter()
        .flatten()
        .fold(None, |text, part| actor::joined(text, Some(part)));
        let mut brief = brief.map(Cow::into_owned);

        let pi_acp = pi::is_pi_acp(kind);
        let program = if pi_acp {
            pi::node_program(kind)
        } else {
            kind.program.clone()
        };
        let mut process = command(&program);
        // The credentials this agent may have, such as Claude Code's token.
        spawn_env::apply(&mut process, Scope::for_agent(&kind.name, account.as_ref()));
        let native_flsh = kind.name == "flsh"
            && Path::new(&kind.program).file_stem().is_some_and(|stem| stem == "flsh")
            && kind.args.first().is_some_and(|arg| arg == "acp");
        if native_flsh {
            let mut system = actor::joined(instructions.take(), standing.take().as_deref());
            if resume.is_none() {
                system = actor::joined(brief.take(), system.as_deref());
            }

            // flsh supports one --system at the root and another after acp.
            // Preserve an existing subcommand flag when adding ours.
            let flag = system.map(|text| format!("--system={text}"));
            if kind.args.iter().any(|arg| arg == "--system" || arg.starts_with("--system=")) {
                process.args(flag).args(&kind.args);
            } else {
                process.args(&kind.args).args(flag);
            }
        } else if !pi_acp {
            process.args(&kind.args);
        }

        if !pi_acp {
            process.args(flag.as_deref());
        }
        process
            .envs(&kind.env)
            .envs(agent_env)
            .current_dir(cwd);
        if let Some(name) = cleared {
            process.env_remove(name);
        }

        if kind.name == "codex" {
            let mut config = serde_json::Map::new();

            // Codex checks the model during resume, before ACP can switch it.
            if resume.is_some()
                && let Some(model) = &model
            {
                config.insert("model".to_owned(), serde_json::json!(model.to_string()));
            }

            // The home assistant starts threads in place of Codex's own agents.
            if assistant {
                config.insert("features.multi_agent".to_owned(), serde_json::json!(false));
                config.insert("features.multi_agent_v2".to_owned(), serde_json::json!(false));
            }

            add_json_env(&mut process, CODEX_CONFIG, config)?;
        }

        // And in place of OpenCode's subagents, whose tool's permission is `task`.
        if kind.name == "opencode" && assistant {
            let deny = serde_json::Map::from_iter([("task".to_owned(), serde_json::json!("deny"))]);
            add_json_env(&mut process, OPENCODE_PERMISSION, deny)?;
        }

        if let Some(wrapper) = cargo::wrapper(cwd, &kind.env) {
            process.env("RUSTC_WRAPPER", wrapper);
        }

        let in_wsl = super::kinds::in_wsl(kind);
        let pi_files = pi::setup(
            kind,
            flag.as_deref(),
            &mut process,
            cwd,
            &mut instructions,
            mcp.as_ref(),
        )?;
        if instructions.is_some() {
            standing = actor::joined(instructions, standing.as_deref());
        }

        // A WSL agent cannot use a Windows path as its Cargo target.
        if !in_wsl && let Some(target) = cargo::target_dir(cwd, &kind.env) {
            process.env("CARGO_TARGET_DIR", target);
        }

        if let Some(docker) = &docker {
            process.env("DOCKER_HOST", docker.host());
        }

        // An agent in WSL reads only the variables `WSLENV` names.
        if in_wsl {
            let names: Vec<String> = process
                .as_std()
                .get_envs()
                .filter(|(name, value)| value.is_some() && *name != "PATH")
                .filter_map(|(name, _)| name.to_str().map(str::to_owned))
                .collect();
            process.env("WSLENV", names.join(":"));
        }

        // Its own process group, so closing it stops what it started; a
        // bridge starts its CLI the same way, once the session starts.
        let agent = super::start(process, kind.bridge.as_ref())?;
        let processes = Processes::new(agent.running.pid(), docker);
        // The agent names a session that came back with a title only on load.
        let kept_title = resume.is_some() && title.is_some();
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
            worktrees,
            assistant,
            account: account.clone(),
            persona,
            continues,
            issue,
            ..SessionInfo::default()
        };
        let mut first = Live::new(info, images, usage);
        first.named = named;
        if let Some(note) = note {
            first.push(EntryKind::Other, &note);
        }
        // Known from the start, so a second takeover of it is refused.
        first.agent_session = resume.clone();
        first.root_pid = agent.running.pid();
        let (live_tx, live) = watch::channel(first);
        let (orders, orders_rx) = crate::actor::channel("session", ORDERS);
        let history = (kind.name == "claude").then(Default::default);
        let actor = Actor {
            live: live_tx,
            node: Node {
                alerts,
                checkouts,
                stopping,
                running,
            },
            moving: false,
            agent: kind.name.clone(),
            acp_session: None,
            banner: None,
            replays: false,
            reading_cost: false,
            offers: Offers::new(protocol),
            foreground: false,
            ready_for_prompts: false,
            initial_v2_state: None,
            compacting_v2: false,
            accepting_v2: Accepting::Done,
            steering: None,
            queued: prompt.into_iter().collect(),
            steers: VecDeque::new(),
            sent: 0,
            context_turn: false,
            kept_title,
            turn: None,
            held_end: None,
            by_user: false,
            stop_within,
            running: HashSet::new(),
            brief,
            standing,
            left_plan_for: None,
            returning: None,
            compactor: Compactor::new(route, policy).with_models(kind.compact_models.clone()),
            asked: VecDeque::new(),
            next_question: 1,
            stderr: VecDeque::new(),
            replies: FuturesUnordered::new(),
        };
        let opening = Opening {
            cwd: cwd.to_path_buf(),
            resume,
            earlier,
            model,
            effort,
            fast,
            first_mode: mode,
            mcp_servers: mcp.into_iter().collect(),
            subagents: !assistant,
        };
        let guard = follows.map(|parent| {
            AbortOnDropHandle::new(tokio::spawn(follow_parent(parent, orders.clone())))
        });
        let actor_processes = processes.clone();
        let kept = Kept::default();
        let handed = kept.clone();
        let task = tokio::spawn(async move {
            let _pi_files = pi_files;
            actor
                .run(agent, orders_rx, actor_processes, opening, handed)
                .await;
        });
        Ok(Self {
            id,
            agent: kind.name.clone(),
            orders,
            live,
            history,
            kept,
            reviewer: false,
            assistant,
            account,
            processes,
            _task: AbortOnDropHandle::new(task),
            guards: Mutex::new(guard.into_iter().collect()),
            peer_messages: AtomicU32::new(0),
        })
    }

    /// Closes with `parent` from now on, as a child started while its
    /// parent ran does: one that came to this machine before its parent.
    pub(super) fn follow(&self, parent: watch::Receiver<Live>) {
        self.keep(tokio::spawn(follow_parent(parent, self.orders.clone())));
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

    /// Notes what it now saw of relatives, once the session's task has
    /// taken it.
    pub(super) async fn see(&self, marks: Vec<(SessionId, Mark)>) {
        // Refused when the session ended, which keeps no more of it.
        let _ = self.order(Command::Saw(marks)).await;
    }

    /// `see` for a caller that cannot wait. A full queue drops the notes, so
    /// the relatives' news is told once more.
    pub(super) fn see_soon(&self, marks: Vec<(SessionId, Mark)>) {
        let (done, _) = oneshot::channel();
        let _ = self.orders.try_send(Order {
            command: Command::Saw(marks),
            done,
        });
    }

    /// What it last saw of the relative `id`.
    pub(super) fn saw(&self, id: &SessionId) -> Option<Mark> {
        watched::read(&self.live, |live| live.seen.get(id).copied())
    }

    /// Marks whether the user's message began the turn that runs or ran
    /// last, once the session's task has taken it.
    pub(super) async fn set_user_turn(&self, user: bool) {
        // Refused when the session ended, whose turns no longer end.
        let _ = self.order(Command::UserTurn(user)).await;
    }

    /// Whether the user's message began the turn that runs or ran last.
    pub(super) fn user_turn(&self) -> bool {
        watched::read(&self.live, |live| live.user_turn)
    }

    /// Whether it did, and clears it for the next turn. A session that ended
    /// takes no order: what it last published answers.
    pub(super) async fn take_user_turn(&self) -> bool {
        let (reply, taken) = oneshot::channel();

        match self.order(Command::TakeUserTurn(reply)).await {
            Ok(()) => taken.await.unwrap_or_else(|_| self.user_turn()),
            Err(_) => self.user_turn(),
        }
    }

    /// The user wrote: other agents may message the session again.
    pub(super) fn user_wrote(&self) {
        self.peer_messages.store(0, Ordering::Relaxed);
    }

    pub(crate) fn info(&self) -> SessionInfo {
        watched::read(&self.live, |live| live.info.clone())
    }

    /// `info`, read from the session's state already, as its list shows it.
    pub(super) fn listed(&self, info: SessionInfo) -> SessionInfo {
        SessionInfo {
            reviewer: self.reviewer,
            ..info
        }
    }

    /// Shows the folder its conversation was refiled in. Only a closed
    /// session is refiled (`relocate_session`): its task handed its state
    /// sender back as it ended, so the folder is written where every reader
    /// looks. A session still running has nothing to write.
    pub(super) fn moved_to(&self, folder: &str) {
        if let Some(live) = crate::lock(&self.kept).as_ref() {
            live.send_modify(|live| live.info.cwd = folder.to_owned());
        }
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
    pub(crate) async fn close(&self) {
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


/// Adds `entries` to the JSON object in `process`'s variable `name`, or the
/// one this node inherited, leaving the rest of it. Nothing to add changes
/// nothing. A variable that is not a JSON object is an error.
fn add_json_env(
    process: &mut tokio::process::Command,
    name: &str,
    entries: serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    let current = process
        .as_std()
        .get_envs()
        .find(|(var, _)| *var == name)
        .and_then(|(_, value)| value.map(std::ffi::OsStr::to_os_string))
        .or_else(|| std::env::var_os(name));
    let mut object = match current.filter(|value| !value.is_empty()) {
        Some(value) => serde_json::from_str::<Option<serde_json::Map<String, serde_json::Value>>>(
            &value.to_string_lossy(),
        )
        .map_err(|_| anyhow::anyhow!("{name} must be a JSON object"))?
        .unwrap_or_default(),
        None => serde_json::Map::new(),
    };
    object.extend(entries);
    process.env(name, serde_json::Value::Object(object).to_string());
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/agents_session.rs"]
mod tests;
