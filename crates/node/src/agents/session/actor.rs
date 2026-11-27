//! The session's task: the ACP connection, its replies and the agent's events.

use super::{
    Answered, Command, EVENTS, NODE_STOPPED, Order, STDERR_LINES,
    turns::{HeldEnd, turn_tokens},
};
use crate::agents::{
    acp::{self, AsyncTask, ModelSwitch, Offered, Prompt},
    alerts::AlertHub,
    checkouts::Checkouts,
    compaction::{Compactor, TurnEnd},
    picture::Picture,
    known,
    launch::{AgentIn, AgentOut, Running, Started},
    live::{Live, TurnTokens},
    text::cut,
    made_files,
    found::folder_place::of_folder,
};
use crate::process::STOP_GRACE;
use agent_client_protocol::{
    Agent, Builder, Client, ConnectionTo, Dispatch, Handled, JsonRpcMessage, JsonRpcRequest,
    JsonRpcResponse, Responder, UntypedMessage, on_receive_dispatch,
    v1::schema::{
        CLIENT_METHOD_NAMES, ContentBlock, CreateElicitationRequest, CreateElicitationResponse, McpServer,
        PromptResponse, RequestPermissionRequest, RequestPermissionResponse, SessionConfigId,
        SessionId, SessionNotification, SessionUpdate, ToolCallId,
    },
};
use anyhow::{Context, Result};
use beep_proto::ModelId;
use beep_proto::node::agent::{
    AlertKind, Effort, Entry, EntryKind, LineChanges, ModeOption, Question, Status, StopReason, WorkPlace,
};
use beep_proto::watched;
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::spawn_blocking,
};

/// What the agent sent the session's task.
pub(super) enum Event {
    Update(Box<SessionUpdate>),
    Notification(UntypedMessage),
    PermissionRequest(UntypedMessage, Box<Responder<RequestPermissionResponse>>),
    /// An `elicitation/create`: a form for the user to fill in.
    Elicitation(UntypedMessage, Box<Responder<CreateElicitationResponse>>),
    /// A command the agent runs in the background started, moved or ended.
    Task(AsyncTask),
}

/// The answer to a request the session sent, with what it was for.
pub(super) enum Reply {
    /// What pi's log says the session spent so far, when it could be read.
    PiCost(Option<f64>),
    /// Capabilities for the protocol selected by the agent.
    Initialized(
        Result<acp::Capabilities, agent_client_protocol::Error>,
        Opening,
    ),
    Accepted {
        turn: u64,
        compact: bool,
        result:
            Result<agent_client_protocol::v2::schema::PromptResponse, agent_client_protocol::Error>,
    },
    Configured {
        answer: serde_json::Value,
        reply: Box<Reply>,
    },
    /// The answer to `session/new`, untyped to keep the models it offers.
    NewSession(
        Result<serde_json::Value, agent_client_protocol::Error>,
        Opening,
    ),
    /// The answer to switching the new session to `model`.
    ModelSet {
        model: ModelId,
        result: Result<(), agent_client_protocol::Error>,
        opening: Opening,
    },
    /// The answer to switching the running session's model or thinking
    /// level, `what` saying which, as the transcript notes it.
    Reconfigured {
        what: String,
        model: Option<ModelId>,
        effort: Option<Effort>,
        fast: Option<bool>,
        result: Result<(), agent_client_protocol::Error>,
    },
    /// The answer to setting how hard the new session thinks.
    FastSet {
        fast: bool,
        result: Result<(), agent_client_protocol::Error>,
        opening: Opening,
    },
    EffortSet {
        effort: Effort,
        result: Result<(), agent_client_protocol::Error>,
        opening: Opening,
    },
    /// The answer to `session/resume` or `session/load` of this session,
    /// untyped to keep the models it offers.
    Resumed(
        SessionId,
        Result<serde_json::Value, agent_client_protocol::Error>,
        Opening,
    ),
    /// The answer to `session/set_mode` to the `mode` it starts in.
    FirstMode {
        mode: String,
        result: Result<(), agent_client_protocol::Error>,
    },
    /// The answer to switching the session's mode, which shows already,
    /// from `was`.
    ModeSet {
        was: Option<String>,
        result: Result<(), agent_client_protocol::Error>,
    },
    /// The answer to stopping the background task `task`: whether it
    /// stopped.
    TaskStopped {
        task: String,
        result: Result<serde_json::Value, agent_client_protocol::Error>,
    },
    Prompt(Result<PromptResponse, agent_client_protocol::Error>),
    Steered(Result<serde_json::Value, agent_client_protocol::Error>),
    /// The answer to the `/compact` the node sent.
    Compacted(Result<PromptResponse, agent_client_protocol::Error>),
    /// The agent was given `stop_within` to end turn `turn` after Stop.
    StopDue {
        turn: u64,
    },
    /// The image files a finished turn named, read, for the agent message
    /// `entry`.
    FilesRead {
        entry: Option<u64>,
        pictures: Vec<Picture>,
    },
    /// The checkout the session's working folder is in.
    Placed(Option<WorkPlace>),
    /// The files a finished turn's commands made, relative to its folder.
    Made(Vec<String>),
    /// The linked worktrees a finished turn's commands added.
    Worktrees(Vec<String>),
    /// The lines changed in the session's checkout as a turn ended.
    Changed(Option<LineChanges>),
}

/// A permission question waiting for its answer.
pub(super) struct Asked {
    pub(super) question: Question,
    pub(super) waiting: Waiting,
    /// The plan it asks to approve, for a plan's approval.
    pub(super) plan: Option<String>,
}

pub(super) struct PendingSteer {
    pub(super) prompt: Prompt,
    pub(super) shown: bool,
    /// The user took it back while the adapter still held it: it is not
    /// waiting, and goes nowhere if the adapter hands it back.
    pub(super) dropped: bool,
}

/// Who waits for a question's answer.
pub(super) enum Waiting {
    /// The agent, on its `session/request_permission`.
    Agent(Box<Responder<RequestPermissionResponse>>),
    /// The node, for a question of its own (`Command::Ask`).
    Node(oneshot::Sender<Answered>),
    /// The agent, on its `elicitation/create`, for this field of the form.
    Form(Box<acp::Form>),
}

impl Waiting {
    /// Answers that the question was dropped. The node hears it as its
    /// sender going away.
    pub(super) fn cancel(self) -> Result<(), agent_client_protocol::Error> {
        match self {
            Self::Agent(responder) => responder.respond(acp::cancelled()),
            Self::Node(_) => Ok(()),
            Self::Form(form) => form.cancel(),
        }
    }
}

/// What the session starts with, which the steps from `initialize` to the
/// first prompt each take their part of. The answer to each step carries
/// what is left to the next.
pub(super) struct Opening {
    pub(super) cwd: PathBuf,
    /// The agent's id of an earlier session to resume.
    pub(super) resume: Option<beep_proto::SessionId>,
    /// The transcript the resumed session had before the node restarted,
    /// shown again when the agent resumes without replaying it.
    pub(super) earlier: Vec<Entry>,
    /// The model a new session switches to before its first prompt.
    pub(super) model: Option<ModelId>,
    /// How hard a new session thinks, set before its first prompt.
    pub(super) effort: Option<Effort>,
    pub(super) fast: Option<bool>,
    /// The mode to start in instead, for a session the node resumes.
    pub(super) first_mode: Option<String>,
    /// The MCP servers the agent's session gets.
    pub(super) mcp_servers: Vec<McpServer>,
    /// Whether the agent may start subagents of its own harness; the home
    /// assistant starts threads instead.
    pub(super) subagents: bool,
}

/// What the session reaches of the node it runs on.
pub(super) struct Node {
    /// Where what the user would want to hear about goes.
    pub(super) alerts: Arc<AlertHub>,
    /// What git says of the folder it works in.
    pub(super) checkouts: Arc<Checkouts>,
    /// Set once the node is stopping.
    pub(super) stopping: Arc<AtomicBool>,
    /// The folder the session runs in, marked to Git sync until it ends.
    pub(super) running: Option<crate::files::holds::Running>,
}

pub(super) struct Actor {
    pub(super) live: watch::Sender<Live>,
    pub(super) node: Node,
    /// A move to another machine waits for the turn: queued prompts wait
    /// too.
    pub(super) moving: bool,
    /// The agent, such as `claude`.
    pub(super) agent: String,
    /// The agent's id for the session, once it made one.
    pub(super) acp_session: Option<SessionId>,
    /// What the adapter said it sends as the session's first message, which
    /// is left out.
    pub(super) banner: Option<String>,
    /// The agent replays the conversation when it loads the session, so
    /// the node need not keep the transcript across a restart.
    pub(super) replays: bool,
    /// pi's log is being read for what the session spent.
    pub(super) reading_cost: bool,
    pub(super) offers: Offers,
    pub(super) foreground: bool,
    pub(super) ready_for_prompts: bool,
    pub(super) initial_v2_state: Option<agent_client_protocol::v2::schema::StateUpdate>,
    pub(super) compacting_v2: bool,
    pub(super) accepting_v2: Accepting,
    /// A steer sent to the adapter, awaiting its delivery outcome.
    pub(super) steering: Option<PendingSteer>,
    /// The last mode the session was in other than plan mode.
    pub(super) left_plan_for: Option<String>,
    /// The mode to go back to once the agent left plan mode for an approved
    /// plan: it picks its own among those the approval offers.
    pub(super) returning: Option<String>,
    /// When the node compacts the conversation.
    pub(super) compactor: Compactor,
    /// Prompts waiting for the agent to be free, the first prompt among them.
    pub(super) queued: VecDeque<Prompt>,
    /// Messages for the running turn, sent once its tool call finishes.
    pub(super) steers: VecDeque<Prompt>,
    /// The prompts sent so far, which numbers each turn.
    pub(super) sent: u64,
    pub(super) context_turn: bool,
    /// It came back with a title, which a title the agent writes from a
    /// prompt sent here does not replace: flsh writes one from the first
    /// prompt it gets, Beep's brief included.
    pub(super) kept_title: bool,
    /// The turn running, while its prompt is unanswered.
    pub(super) turn: Option<Turn>,
    /// The end of the last turn, held while its subagents run.
    pub(super) held_end: Option<HeldEnd>,
    /// The user stopped the running turn, or closed the session.
    pub(super) by_user: bool,
    /// How long the agent has to end its turn after Stop.
    pub(super) stop_within: Duration,
    /// The tool calls of the running turn that have not finished.
    pub(super) running: HashSet<ToolCallId>,
    /// Beep's word to the agent from whoever started the session; once
    /// `initialize` is answered, what the first prompt still carries.
    pub(super) brief: Option<String>,
    /// Saved instructions and Beep guidance for the whole session. Goes in
    /// the system prompt when supported, or with the first prompt and again
    /// after the agent compacts.
    pub(super) standing: Option<String>,
    /// Questions waiting, the first one shown.
    pub(super) asked: VecDeque<Asked>,
    pub(super) next_question: u32,
    /// The last lines the agent wrote to stderr.
    pub(super) stderr: VecDeque<String>,
    /// Answers still to come. At most a few: one per request in flight, and
    /// a session has at most one prompt in flight.
    pub(super) replies: FuturesUnordered<BoxFuture<'static, Reply>>,
}

/// The turn running: what only has a meaning while its prompt is
/// unanswered.
pub(super) struct Turn {
    pub(super) number: u64,
    /// Why it was asked to stop, once it was.
    pub(super) stopping: Option<Stopping>,
    /// The adapter found no running turn for a steer; wait for our
    /// prompt's reply.
    pub(super) steer_after: bool,
}

impl Turn {
    pub(super) fn new(number: u64) -> Self {
        Self {
            number,
            stopping: None,
            steer_after: false,
        }
    }
}

/// Why the session's task stopped.
pub(super) enum End {
    Closed,
    Exited,
    Failed(String),
    /// The agent did not end its turn within `stop_within` of Stop.
    Unstoppable(Duration),
    /// Stop all, while subagents ran that only stopping the agent ends.
    StoppedAll,
    /// The node stopped or restarted.
    NodeStopped,
}

/// Why the node asked the agent to end its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
pub(super) enum Stopping {
    /// The user stopped it.
    #[strum(serialize = "Stopping")]
    User,
    /// A tool call finished with a steer waiting: the turn ends there and
    /// the steer goes as the next prompt.
    #[strum(serialize = "Taking your message")]
    Steer,
}

/// What the agent said it offers: its protocol and abilities once
/// `initialize` is answered, and its options once the session exists and
/// again when it announces new ones.
pub(super) struct Offers {
    /// The version the agent speaks; the one asked for until `initialize`
    /// is answered.
    pub(super) protocol: acp::Protocol,
    /// Whether the agent takes image blocks, once `initialize` said.
    pub(super) takes_images: Option<bool>,
    pub(super) takes_steering: bool,
    pub(super) fast_option: Option<SessionConfigId>,
    /// The session's option that sets how hard it thinks, once it said.
    pub(super) thinking: Option<SessionConfigId>,
    pub(super) mode_config: Option<SessionConfigId>,
    /// How the session switches models, once it said; `None` when it
    /// cannot.
    pub(super) switch: Option<ModelSwitch>,
}

impl Offers {
    pub(super) fn new(protocol: acp::Protocol) -> Self {
        Self {
            protocol,
            takes_images: None,
            takes_steering: false,
            fast_option: None,
            thinking: None,
            mode_config: None,
            switch: None,
        }
    }

    /// `initialize` was answered.
    fn initialized(&mut self, initialized: &acp::Capabilities) {
        self.protocol = initialized.protocol;
        self.takes_images = Some(initialized.images);
        self.takes_steering = initialized.steering;
    }

    /// The session was made: its answer replaces every option.
    fn made(&mut self, offered: &Offered) {
        self.fast_option = offered.fast.clone();
        self.thinking = offered.thinking.clone();
        self.mode_config = offered.mode_config.clone();
        self.switch = offered.switch.clone();
    }

    /// A v1 agent announced its options: they replace the two it lists.
    fn announced(&mut self, offered: &Offered) {
        self.fast_option = offered.fast.clone();
        self.thinking = offered.thinking.clone();
    }

    /// A v2 agent announced its options: they replace the fast option and
    /// add what else they list.
    pub(super) fn added(&mut self, offered: &Offered) {
        self.fast_option = offered.fast.clone();
        self.thinking = offered.thinking.clone().or(self.thinking.take());
        self.switch = offered.switch.clone().or(self.switch.take());
        self.mode_config = offered.mode_config.clone().or(self.mode_config.take());
    }
}

/// Whether the agent has answered the v2 prompt just sent.
pub(super) enum Accepting {
    /// It answered, or no prompt was sent.
    Done,
    Waiting,
    /// The agent already went idle: applied once it answers.
    Idle(agent_client_protocol::v2::schema::IdleStateUpdate),
}

/// What the session reads besides ACP.
pub(super) struct Inputs<'a> {
    pub(super) events: mpsc::Receiver<Event>,
    /// The length of each message dropped for being too long.
    pub(super) oversized: mpsc::Receiver<usize>,
    /// The agent's stderr, a line each.
    pub(super) stderr: &'a mut mpsc::Receiver<String>,
    /// The agent, whose bridge says when its CLI started.
    pub(super) running: &'a mut Running,
    pub(super) processes: &'a super::Processes,
}

impl Actor {
    pub(super) async fn run(
        mut self,
        agent: Started,
        mut orders: crate::actor::Receiver<Order>,
        processes: Arc<super::Processes>,
        opening: Opening,
        kept: super::Kept,
    ) {
        let _cleanup = super::super::processes::Cleanup(processes.clone());
        let _watch = processes.watch(self.live.subscribe());
        // Kills the agent's whole process group however the task ends,
        // aborted with its session included; `stop` below is the gentle way.
        let Started {
            stdin,
            stdout,
            mut stderr,
            mut running,
        } = agent;
        // A session the node resumed shows its lines before its next turn.
        self.count_changes();
        let end = self
            .serve(
                (stdin, stdout),
                &mut stderr,
                &mut running,
                &processes,
                &mut orders,
                opening,
            )
            .await;

        // A bridge that failed says why as it ends.
        if matches!(end, Ok(End::Exited)) {
            running.settle(STOP_GRACE).await;

            while let Ok(line) = stderr.try_recv() {
                self.heard(&line);
            }
        }

        // The node stopping ends its sessions, whether it closed them or the
        // service manager stopped the agents first.
        let end = match end {
            Ok(End::Closed | End::Exited) if self.node.stopping.load(Ordering::Relaxed) => {
                Ok(End::NodeStopped)
            }
            end => end,
        };
        let closed = matches!(end, Ok(End::Closed));
        let node_stopped = matches!(end, Ok(End::NodeStopped));
        let by_user = self.by_user;
        // Stopped as the user asked, or with the node, so not a failure to
        // alert about.
        let asked = closed
            || matches!(
                end,
                Ok(End::Unstoppable(_) | End::StoppedAll | End::NodeStopped)
            );
        // What still waits goes with the session when the node resumes it.
        let waiting: Vec<Prompt> = match &end {
            Ok(End::NodeStopped) => {
                let interrupted = matches!(
                    watched::read_sent(&self.live, Live::status),
                    Status::Working | Status::Asking
                );
                let continuation = interrupted.then(|| {
                    Prompt::from(crate::agents::tags::context(
                        super::CONTINUE,
                    ))
                });
                continuation
                    .into_iter()
                    .chain(self.steers.drain(..))
                    .chain(self.queued.drain(..))
                    .collect()
            }
            Ok(End::Unstoppable(_)) => self
                .steers
                .drain(..)
                .chain(self.queued.drain(..))
                .collect(),
            _ => Vec::new(),
        };
        let reason = match end {
            Ok(End::NodeStopped) => NODE_STOPPED.to_owned(),
            Ok(End::Closed) => "closed".to_owned(),
            Ok(End::StoppedAll) => {
                "stopped with its subagents; send a message to continue".to_owned()
            }
            Ok(End::Unstoppable(within)) => format!(
                "the agent did not stop within {} s, so Beep stopped it; send a message to continue",
                within.as_secs()
            ),
            Ok(End::Exited) => match self.stderr.back() {
                Some(line) => format!("the agent exited: {line}"),
                None => "the agent exited".to_owned(),
            },
            Ok(End::Failed(reason)) => reason,
            Err(e) => format!("lost the agent: {e:#}"),
        };
        log::info!("agent session ended: {reason}");
        // Orders that come in too late hear the session ended.
        orders.close();
        // Questions still open are dropped with the connection.
        self.asked.clear();
        processes.close().await;
        running.stop(STOP_GRACE).await;

        if closed && by_user {
            let info = watched::read_sent(&self.live, |live| live.info.clone());
            let removed =
                spawn_blocking(move || crate::agents::uploads::remove(&info.agent, &info.id)).await;
            if let Err(e) = removed.unwrap_or_else(|e| Err(e.into())) {
                log::warn!("could not remove closed session attachments: {e:#}");
            }
        } else if !node_stopped {
            let info = watched::read_sent(&self.live, |live| live.info.clone());
            let marked =
                spawn_blocking(move || crate::agents::uploads::ended(&info.agent, &info.id)).await;
            if let Err(e) = marked.unwrap_or_else(|e| Err(e.into())) {
                log::warn!("could not mark ended session attachments: {e:#}");
            }
        }

        if !asked {
            self.alert(AlertKind::Failed {
                reason: reason.clone(),
            });
        }

        // Locked across the last publish, so a refile that sees the session
        // ended finds the sender.
        let mut kept = crate::lock(&kept);
        self.live.send_modify(|live| {
            // A cost the agent reported after its last turn still counts.
            live.spent(TurnTokens::default());
            live.info.question = None;
            live.info.plan_ready = None;
            live.info.state.status = Status::Ended;
            live.info.state.queued = 0;
            live.info.state.queued_messages.clear();
            live.end_tasks();
            live.info.state.subagents = 0;
            live.orphaned_subagents = false;
            live.held = waiting;
            live.stopped(closed && by_user);
            if !closed {
                live.info.state.error = Some(reason);
            }
        });
        // Released with the end, so a hold taken as `close` returns finds the folder free.
        drop(self.node.running.take());
        *kept = Some(self.live);
        drop(kept);
        crate::events::open_changed();
    }

    /// Keeps `line` of the agent's stderr, to say why it stopped.
    fn heard(&mut self, line: &str) {
        let line = line.trim();

        if line.is_empty() {
            return;
        }

        log::debug!("agent stderr: {line}");

        if self.stderr.len() >= STDERR_LINES {
            self.stderr.pop_front();
        }

        self.stderr.push_back(cut(line, 300));
    }

    async fn serve(
        &mut self,
        (stdin, stdout): (AgentIn, AgentOut),
        stderr: &mut mpsc::Receiver<String>,
        running: &mut Running,
        processes: &super::Processes,
        orders: &mut crate::actor::Receiver<Order>,
        opening: Opening,
    ) -> Result<End> {
        let (events_tx, events) = mpsc::channel(EVENTS);
        let (oversized_tx, oversized) = mpsc::channel(1);
        let mut inputs = Inputs {
            events,
            oversized,
            stderr,
            running,
            processes,
        };
        // Keep dispatch version-neutral: a draft v2 opt-in can negotiate
        // down to v1 before any session traffic. Each payload is validated
        // against the selected version in the session's ordered event loop.
        let ended = Builder::new(Client)
            .name("beep-node")
            .on_receive_dispatch(
                async move |dispatch: Dispatch, _cx| {
                    let event = match dispatch {
                        Dispatch::Notification(message) if message.method() == CLIENT_METHOD_NAMES.session_update => {
                            Event::Notification(message)
                        }
                        Dispatch::Request(message, responder)
                            if message.method() == CLIENT_METHOD_NAMES.session_request_permission =>
                        {
                            Event::PermissionRequest(message, Box::new(responder.cast()))
                        }
                        Dispatch::Request(message, responder)
                            if message.method() == CLIENT_METHOD_NAMES.elicitation_create =>
                        {
                            Event::Elicitation(message, Box::new(responder.cast()))
                        }
                        dispatch => {
                            return Ok(Handled::No {
                                message: dispatch,
                                retry: false,
                            });
                        }
                    };
                    let _ = events_tx.send(event).await;
                    Ok(Handled::Yes)
                },
                on_receive_dispatch!(),
            )
            .connect_with(
                acp::transport(stdin, stdout, oversized_tx),
                async |cx: ConnectionTo<Agent>| Ok(self.drive(&cx, &mut inputs, orders, opening).await),
            )
            .await;
        ended.context("the ACP connection failed")?
    }

    async fn drive(
        &mut self,
        cx: &ConnectionTo<Agent>,
        inputs: &mut Inputs<'_>,
        orders: &mut crate::actor::Receiver<Order>,
        opening: Opening,
    ) -> Result<End> {
        let async_tasks = known(&self.agent).is_some_and(|known| known.async_tasks);
        let protocol = self.offers.protocol;
        let request = acp::initialize(async_tasks).to_untyped_message()?;
        self.send(cx, request, move |result| {
            Reply::Initialized(result.and_then(|answer| protocol.initialized(answer)), opening)
        });
        let mut stderr_open = true;
        self.look_for_place();

        loop {
            // Biased so an update the agent sent before an answer is
            // applied before it, as the dispatch loop handed them over.
            tokio::select! {
                biased;
                event = inputs.events.recv() => match event {
                    Some(event) => self.event(cx, event)?,
                    None => return Ok(End::Exited),
                },
                Some(length) = inputs.oversized.recv() => self.oversized(length),
                () = cx.incoming_closed() => return Ok(End::Exited),
                Some(reply) = self.replies.next() => {
                    if let Some(end) = self.reply(cx, reply)? {
                        return Ok(end);
                    }
                }
                line = inputs.stderr.recv(), if stderr_open => match line {
                    Some(line) => self.heard(&line),
                    None => stderr_open = false,
                },
                pid = inputs.running.started() => {
                    self.live.send_modify(|live| live.root_pid = Some(pid));
                    inputs.processes.set_root(pid);
                }
                () = tokio::time::sleep_until(
                    self.held_end.as_ref().map_or_else(tokio::time::Instant::now, |held| held.due)
                ), if self.held_end.is_some() => self.subagents_quiet(),
                order = orders.recv() => {
                    let Some((Order { command, done }, _timing)) = order else {
                        return Ok(End::Closed);
                    };

                    let command = match command {
                        Command::ParentGone if self.moving => {
                            let _ = done.send(Ok(()));
                            continue;
                        }
                        Command::ParentGone => Command::Close { by_user: false },
                        other => other,
                    };

                    if let Command::Close { by_user } = command {
                        self.by_user = by_user;
                        self.live.send_modify(|live| live.closing = by_user);
                        let _ = done.send(Ok(()));
                        return Ok(End::Closed);
                    }

                    if let Command::StopAll { by_user } = command
                        && watched::read_sent(&self.live, |live| {
                            live.info.state.subagents > 0 || live.orphaned_subagents
                        })
                    {
                        self.by_user = by_user;
                        let _ = done.send(Ok(()));
                        return Ok(End::StoppedAll);
                    }

                    let result = self.command(cx, command)?;
                    // The stream may have gone; the order still happened.
                    let _ = done.send(result);
                }
            }
        }
    }

    /// Sends `request`; its answer comes back through `replies` as `reply`.
    /// Sending only queues it for the crate's writer, so it never waits.
    pub(super) fn send<Req>(
        &mut self,
        cx: &ConnectionTo<Agent>,
        request: Req,
        reply: impl FnOnce(Result<Req::Response, agent_client_protocol::Error>) -> Reply
        + Send
        + 'static,
    ) where
        Req: JsonRpcRequest,
        Req::Response: Send + 'static,
    {
        let mut trace = beep_proto::logging::RequestTrace::new(std::any::type_name::<Req>());
        trace.phase("waiting for agent");
        let protocol = self.offers.protocol;
        let method = request.method().to_owned();
        let request = request
            .to_untyped_message()
            .and_then(|request| protocol.request(request));
        let answer = request.map(|request| cx.send_request(request).block_task());
        self.replies.push(
            async move {
                let answer = match answer {
                    Ok(answer) => answer.await,
                    Err(error) => Err(error),
                };
                trace.finish(answer.is_ok());
                let configured = (method == "session/set_config_option")
                    .then(|| answer.as_ref().ok().cloned())
                    .flatten()
                    .filter(|answer| answer.get("configOptions").is_some());
                let reply =
                    reply(answer.and_then(|value| Req::Response::from_json(&method, value)));
                match configured {
                    Some(answer) => Reply::Configured {
                        answer,
                        reply: Box::new(reply),
                    },
                    None => reply,
                }
            }
            .boxed(),
        );
    }

    fn event(&mut self, cx: &ConnectionTo<Agent>, event: Event) -> Result<()> {
        match event {
            Event::Notification(message) => {
                if let Some(task) = acp::async_task(&message) {
                    return self.event(cx, Event::Task(task));
                }
                if self.offers.protocol == acp::Protocol::V2 {
                    let notification = agent_client_protocol::v2::schema::UpdateSessionNotification::parse_message(message.method(), message.params())?;
                    if self
                        .acp_session
                        .as_ref()
                        .is_some_and(|session| session.0 != notification.session_id.0)
                    {
                        return Ok(());
                    }
                    self.v2_update(cx, notification.update)?;
                } else if let Ok(notification) =
                    SessionNotification::parse_message(message.method(), message.params())
                {
                    self.event(cx, Event::Update(Box::new(notification.update)))?;
                }
            }
            Event::PermissionRequest(message, responder) => {
                if self.offers.protocol == acp::Protocol::V2
                    && *responder.raw_id() == agent_client_protocol::RawJsonRpcId::Null
                {
                    responder.respond_with_error(agent_client_protocol::Error::invalid_request())?;
                    return Ok(());
                }
                let request = if self.offers.protocol == acp::Protocol::V2 {
                    acp::permission_v2(message.params())
                } else {
                    RequestPermissionRequest::parse_message(message.method(), message.params()).map(
                        |request| acp::Permission {
                            question: watched::read_sent(&self.live, |live| {
                                live.question(&request)
                            }),
                            plan: acp::plan_of(&request),
                        },
                    )
                };
                let permission = match request {
                    Ok(permission) => permission,
                    Err(error) => {
                        responder.respond_with_error(error)?;
                        return Ok(());
                    }
                };
                let Some(responder) =
                    self.plan_ready(permission.plan, &permission.question, *responder)
                else {
                    return Ok(());
                };
                if let Some(responder) = self.allow(&permission.question, responder)? {
                    self.ask(permission.question, responder);
                }
            }
            Event::Elicitation(message, responder) => {
                let fields =
                    CreateElicitationRequest::parse_message(message.method(), message.params())
                        .and_then(acp::Fields::try_from);
                match fields {
                    Ok(fields) => self.ask_form(acp::Form::new(fields, *responder)),
                    Err(error) => responder.respond_with_error(error)?,
                }
            }
            Event::Update(update) => {
                let update = *update;
                if self.drops_title() && matches!(update, SessionUpdate::SessionInfoUpdate(_)) {
                    return Ok(());
                }

                if let SessionUpdate::AgentMessageChunk(chunk) = &update
                    && let ContentBlock::Text(text) = &chunk.content
                    && (self.is_banner(&text.text) || self.compactor.heard(&text.text))
                {
                    return Ok(());
                }

                // The adapter echoes an injected message before continuing.
                // Show it there so its answer follows it in the transcript.
                if let SessionUpdate::UserMessageChunk(chunk) = &update
                    && let ContentBlock::Text(text) = &chunk.content
                    && self
                        .steering
                        .as_ref()
                        .is_some_and(|steer| steer.prompt.text == text.text)
                {
                    self.show_steer();
                }

                if let SessionUpdate::ConfigOptionUpdate(options) = &update {
                    let offered = acp::offers(
                        &serde_json::json!({ "configOptions": options.config_options }),
                    );

                    self.offers.announced(&offered);
                    if let Some(model) = offered.current {
                        self.model_is(Some(&model));
                    }
                }

                if let SessionUpdate::CurrentModeUpdate(now) = &update {
                    self.mode_is(cx, now.current_mode_id.to_string());
                }

                let finished = self.tool_finished(&update);
                // Followers also need transcript and metadata changes that
                // leave the compact state unchanged.
                self.live.send_modify(|live| {
                    live.apply(&update);
                });
                self.look_for_place();

                if let Some(stopping) = self.stopping() {
                    self.show_stopping(stopping);
                }

                if finished
                    || matches!(&update, SessionUpdate::UsageUpdate(usage) if usage.cost.is_none())
                {
                    self.read_pi_cost();
                }

                if finished {
                    self.steer_now(cx)?;
                }

                self.subagents_moved();
            }
            Event::Task(task) => {
                self.live.send_if_modified(|live| live.apply_task(task));
                self.subagents_moved();
                if self.has_waiting_prompts() {
                    self.next_prompt(cx);
                }
            }
        }

        Ok(())
    }

    /// Asks the checkout the session works in now when its folder changed
    /// since the last time, off the async workers. The answer comes back as
    /// `Reply::Placed`.
    pub(super) fn look_for_place(&mut self) {
        let mut wanted = None;
        self.live.send_if_modified(|live| {
            wanted = live.place_wanted.take();
            false
        });
        let Some(folder) = wanted else {
            return;
        };
        let read = spawn_blocking(move || of_folder(&folder));
        self.replies.push(
            async move {
                Reply::Placed(read.await.unwrap_or_else(|e| {
                    log::warn!("a session's checkout was not read: {e}");
                    None
                }))
            }
            .boxed(),
        );
    }

    /// The agent sent a message too long to keep: it was dropped, and the
    /// session says so.
    fn oversized(&mut self, length: usize) {
        let error = format!(
            "dropped a {} MiB message from the agent; at most {} MiB are read",
            length / (1024 * 1024),
            crate::process::line_reader::MAX_MESSAGE / (1024 * 1024)
        );
        self.live
            .send_modify(|live| live.info.state.error = Some(error));
    }

    fn reply(&mut self, cx: &ConnectionTo<Agent>, reply: Reply) -> Result<Option<End>> {
        match reply {
            Reply::PiCost(cost) => {
                self.reading_cost = false;

                if let Some(cost) = cost {
                    self.live
                        .send_modify(|live| live.usage_update(Some((cost, "USD")), None));
                }
            }
            Reply::Configured { answer, reply } => {
                self.v2_options(cx, &answer);
                return self.reply(cx, *reply);
            }
            Reply::Accepted {
                turn,
                compact,
                result,
            } => {
                if self.sent != turn {
                    return Ok(None);
                }
                let idle = match std::mem::replace(&mut self.accepting_v2, Accepting::Done) {
                    Accepting::Idle(idle) => Some(idle),
                    Accepting::Waiting | Accepting::Done => None,
                };
                match result {
                    Ok(accepted) => {
                        if !compact {
                            self.live
                                .send_modify(|live| live.v2_accepted(accepted.message_id));
                        }
                        if let Some(idle) = idle {
                            self.v2_idle(cx, idle);
                        }
                    }
                    Err(error) => {
                        self.live.send_modify(|live| live.v2_rejected());
                        if compact {
                            self.turn = None;
                            self.compacting_v2 = false;
                            self.compacted(cx, Err(error));
                        } else {
                            return self.reply(cx, Reply::Prompt(Err(error)));
                        }
                    }
                }
            }
            Reply::Initialized(Ok(initialized), opening) => {
                self.offers.initialized(&initialized);

                if let Some(end) = self.initialized(cx, &initialized, opening) {
                    return Ok(Some(end));
                }
            }
            Reply::NewSession(Ok(answer), opening) => match acp::session_created(answer.clone()) {
                Ok((created, _)) => {
                    self.banner = acp::startup_banner(&answer);
                    let offered = self.offers.protocol.offers(&answer);
                    return Ok(self.ready(cx, created.session_id, offered, opening));
                }
                Err(e) => {
                    return Ok(Some(End::Failed(format!(
                        "the agent did not start: it answered session/new with {e}"
                    ))));
                }
            },
            Reply::Resumed(session, Ok(answer), opening) => {
                let offered = self.offers.protocol.offers(&answer);
                return Ok(self.ready(cx, session, offered, opening));
            }
            Reply::ModelSet {
                model,
                result: Ok(()),
                opening,
            } => {
                self.model_is(Some(model.as_ref()));
                self.think(cx, opening);
            }
            Reply::Reconfigured {
                what,
                model,
                effort,
                fast,
                result,
            } => match result {
                Ok(()) => {
                    if let Some(model) = model {
                        self.model_is(Some(model.as_ref()));
                    }

                    if let Some(effort) = effort {
                        self.thinks_at(effort);
                    }

                    if let Some(fast) = fast {
                        self.live.send_modify(|live| live.fast = Some(fast));
                    }
                    self.note(&format!("Switched to {what} from the next turn"));
                }
                Err(e) => self.note(&format!("Could not switch to {what}: {e}")),
            },
            Reply::FastSet {
                fast,
                result,
                opening,
            } => {
                match result {
                    Ok(()) => self.live.send_modify(|live| live.fast = Some(fast)),
                    Err(e) => self.note(&format!("Could not set fast mode: {e}")),
                }
                self.unprompted(cx, opening);
            }
            Reply::EffortSet {
                effort,
                result,
                opening,
            } => {
                if result.is_ok() {
                    self.thinks_at(effort);
                }

                if let Err(e) = result {
                    self.note(&format!(
                        "{} could not be set to think at {}, so it thinks at its own level: {e}",
                        agent_title(&self.agent),
                        effort.as_ref()
                    ));
                }

                self.set_fast(cx, opening);
            }
            Reply::ModelSet {
                model,
                result: Err(e),
                ..
            } => {
                return Ok(Some(End::Failed(format!(
                    "the agent could not switch to {model}: {e}"
                ))));
            }
            Reply::FirstMode { mode, result } => {
                match result {
                    Ok(()) => self.mode_is(cx, mode),
                    // Requests that still come are allowed in `ask`.
                    Err(e) => log::warn!("the agent did not switch to {mode}: {e}"),
                }

                self.idle(cx);
            }
            Reply::ModeSet { was, result } => {
                if let Err(e) = result {
                    self.note(&format!("Could not switch mode: {e}"));

                    if let Some(was) = was {
                        self.mode_is(cx, was);
                    }
                }
            }
            Reply::TaskStopped { task, result } => match result {
                Ok(done) if done["stopped"] == false => {
                    self.note(&format!("The agent did not stop the task {task}"));
                }
                Ok(_) => {}
                Err(e) => self.note(&format!("Could not stop the task {task}: {e}")),
            },
            Reply::Resumed(_, Err(e), _) => {
                return Ok(Some(End::Failed(format!(
                    "the agent could not resume the session: {e}"
                ))));
            }
            Reply::Initialized(Err(e), _) | Reply::NewSession(Err(e), _) => {
                return Ok(Some(End::Failed(format!("the agent did not start: {e}"))));
            }
            Reply::Prompt(result) => {
                self.turn = None;
                self.running.clear();
                let tokens = result.as_ref().map(turn_tokens).unwrap_or_default();
                self.live.send_modify(|live| live.spent(tokens));
                let (stop, error) = match result {
                    Ok(done) => (stop_reason(&done), None),
                    Err(e) if lost_server_tools(&e) => {
                        log::warn!("the turn ended with server tool calls that never ran: {e}");
                        (StopReason::EndTurn, None)
                    }
                    Err(e) => (StopReason::Failed, Some(e.to_string())),
                };
                self.turn_done(cx, stop, error);
            }
            Reply::Steered(result) => self.steered(cx, result)?,
            Reply::Compacted(result) => self.compacted(cx, result),
            Reply::StopDue { turn } => {
                if self.stopping().is_some_and(|stopping| stopping == Stopping::User)
                    && self.turn.as_ref().is_some_and(|running| running.number == turn)
                {
                    log::warn!("the agent ignored session/cancel; stopping it");
                    return Ok(Some(End::Unstoppable(self.stop_within)));
                }
            }
            Reply::Placed(place) => {
                self.live.send_if_modified(|live| {
                    let changed = live.info.place != place;
                    live.info.place = place;
                    changed
                });
            }
            Reply::FilesRead { entry, pictures } => {
                if !pictures.is_empty() {
                    self.live
                        .send_modify(|live| live.show_files(entry, pictures));
                }
            }
            Reply::Worktrees(trees) => {
                if !trees.is_empty() {
                    self.live
                        .send_if_modified(|live| live.created_worktrees(trees));
                }
            }
            Reply::Changed(changes) => {
                self.live.send_if_modified(|live| {
                    let changed = live.info.changes != changes;
                    live.info.changes = changes;
                    changed
                });
            }
            Reply::Made(paths) => {
                if !paths.is_empty() {
                    let note = made_files::note(&paths);
                    self.live
                        .send_modify(|live| live.push(EntryKind::Other, &note));
                }
            }
        }

        Ok(None)
    }

    /// The agent answered `initialize`: makes the session, or resumes the
    /// earlier one. Gives why the session ends when it cannot.
    fn initialized(
        &mut self,
        cx: &ConnectionTo<Agent>,
        initialized: &acp::Capabilities,
        mut opening: Opening,
    ) -> Option<End> {
        let system_format = initialized.system_prompt;
        let takes_system = system_format.is_some();
        let append = |text| system_format.map(|format| format.append(text));
        let standing = if takes_system {
            self.standing.take()
        } else {
            self.standing.clone()
        };
        let method = initialized.resume;
        self.replays = method == Some(acp::ResumeMethod::Load);
        let Some(session) = opening.resume.take() else {
            let brief = joined(self.brief.take(), standing.as_deref());
            let system = if takes_system {
                brief
            } else {
                self.brief = brief;
                None
            };

            match acp::new_session(
                &opening.cwd,
                std::mem::take(&mut opening.mcp_servers),
                system.as_deref().and_then(append),
                opening.subagents,
            ) {
                Ok(request) => self.send(cx, request, |result| Reply::NewSession(result, opening)),
                Err(e) => return Some(End::Failed(format!("the agent did not start: {e}"))),
            }

            return None;
        };
        // A resumed session keeps the caller's brief in its first prompt: it
        // says what changed at this point of the conversation, and Claude
        // Code takes a changed system prompt only at its next compaction.
        let system = if takes_system {
            standing
        } else {
            self.brief = joined(self.brief.take(), standing.as_deref());
            None
        };
        let session = SessionId::from(String::from(session));
        let Some(method) = method else {
            return Some(End::Failed("the agent cannot resume a session".into()));
        };
        if method == acp::ResumeMethod::Resume && !opening.earlier.is_empty() {
            let earlier = std::mem::take(&mut opening.earlier);
            self.live.send_modify(|live| live.restore(earlier));
        }

        match acp::resume_session(
            method,
            session.clone(),
            &opening.cwd,
            std::mem::take(&mut opening.mcp_servers),
            system.as_deref().and_then(append),
            opening.subagents,
        ) {
            Ok(request) => self.send(cx, request, |result| {
                Reply::Resumed(session, result, opening)
            }),
            Err(e) => {
                return Some(End::Failed(format!(
                    "the agent could not resume the session: {e}"
                )));
            }
        }
        None
    }

    /// pi's adapter reports no cost: reads what the session spent from pi's
    /// own log, one read at a time.
    fn read_pi_cost(&mut self) {
        if self.agent != "pi" || self.reading_cost {
            return;
        }

        let Some(session) = &self.acp_session else {
            return;
        };
        let id = Into::<beep_proto::SessionId>::into(&*session.0);
        self.reading_cost = true;
        self.replies.push(
            async move {
                let read = tokio::task::spawn_blocking(move || {
                    let sessions = crate::agents::found::pi_sessions()?;
                    crate::agents::spend::pi_session_cost(&sessions, &id)
                });
                let cost = read.await.ok().flatten();
                Reply::PiCost(cost)
            }
            .boxed(),
        );
    }

    /// Whether a title the agent writes now is dropped: one written from
    /// the recovery context alone, or one written from a prompt sent here
    /// to a session that came back with its title.
    pub(super) fn drops_title(&self) -> bool {
        self.context_turn || (self.kept_title && self.sent > 0)
    }

    /// Whether `text` is what the adapter says before the session's first
    /// prompt: an agent message before the session exists, which only a
    /// loaded session's replay has, or the banner it named.
    fn is_banner(&mut self, text: &str) -> bool {
        if self.acp_session.is_none() {
            return !self.replays;
        }

        self.banner.take_if(|banner| banner == text).is_some()
    }

    /// The agent made or resumed its session `session`, which offers the
    /// models in `offered`: switches it to the model asked for, then lets
    /// prompts go. Gives why the session ends when it cannot.
    fn ready(
        &mut self,
        cx: &ConnectionTo<Agent>,
        session: SessionId,
        offered: Offered,
        mut opening: Opening,
    ) -> Option<End> {
        let earlier = std::mem::take(&mut opening.earlier);
        self.live.send_modify(|live| {
            live.finish_replay();
            live.restore_times(&earlier);
        });
        self.acp_session = Some(session.clone());
        if let Some(state) = self.initial_v2_state.take() {
            self.v2_state(cx, state);
        }
        self.model_is(offered.current.as_deref());
        self.offers.made(&offered);

        if let Some(modes) = offered.modes {
            let full = self.full_mode();
            self.live.send_modify(|live| {
                live.info.modes = cycle(modes.available, full);
            });
            self.mode_is(cx, modes.current);
        }
        // From now on the session goes by the agent's own id, which
        // still finds it once the node restarts.
        self.live.send_modify(|live| {
            let id = Into::<beep_proto::SessionId>::into(&*session.0);
            live.agent_session = Some(id.clone());
            live.info.id = id;
        });
        crate::events::open_changed();
        let Some(model) = opening.model.take() else {
            self.think(cx, opening);
            return None;
        };
        let Some(switch) = offered.switch else {
            self.note(&format!(
                "{} here cannot switch models, so it runs its own rather than {model}",
                agent_title(&self.agent)
            ));
            self.think(cx, opening);
            return None;
        };

        match acp::switch_model(&session, &switch, model.as_ref()) {
            Ok(request) => {
                // The first prompt waits for the model.
                self.send(cx, request, move |result| Reply::ModelSet {
                    model,
                    result: result.map(drop),
                    opening,
                });
                None
            }
            Err(e) => Some(End::Failed(format!(
                "the agent could not switch to {model}: {e}"
            ))),
        }
    }

    /// The agent took thinking level `effort`, which the saved session keeps.
    fn thinks_at(&mut self, effort: Effort) {
        self.live.send_modify(|live| live.effort = Some(effort));
    }

    /// A model switch while idle can make the existing context due for compaction.
    pub(super) fn model_is(&mut self, model: Option<&str>) {
        if let Some(model) = model {
            let model = ModelId::new(model);
            // The session's watch notes the change, so a crash after a
            // switch resumes on the new model.
            self.live.send_if_modified(|live| {
                if live.info.model.as_ref() == Some(&model) {
                    return false;
                }
                live.info.model = Some(model);
                true
            });
        }

        if self.compactor.set_model(model) {
            let idle = watched::read_sent(&self.live, |live| {
                (live.info.state.status == Status::Idle)
                    .then(|| (live.info.context, live.commands.clone()))
            });

            if let Some((usage, commands)) = idle {
                self.compactor.turn_ended(TurnEnd {
                    usage,
                    commands: &commands,
                });
            }
        }
    }

    /// The session mode that turns the agent's permission prompts off, set
    /// before the first prompt: Claude Code's `bypassPermissions`. Every
    /// agent runs without prompts, and a permission request that comes
    /// anyway is allowed rather than asked.
    pub(super) fn bypass_mode(&self) -> Option<&'static str> {
        known(&self.agent).and_then(|known| known.bypass_mode)
    }

    /// The session mode that asks no permission, however launch reached it.
    pub(super) fn full_mode(&self) -> Option<&'static str> {
        known(&self.agent).and_then(|known| known.full_mode)
    }

    /// Why the running turn was asked to stop, once it was.
    pub(super) fn stopping(&self) -> Option<Stopping> {
        self.turn.as_ref().and_then(|turn| turn.stopping)
    }

    /// Shows `note` in the transcript, for what the session does other
    /// than asked.
    pub(super) fn note(&mut self, note: &str) {
        log::info!("{note}");
        self.live
            .send_modify(|live| live.push(EntryKind::Other, note));
    }

    /// The session runs on its model: sets how hard it thinks when asked,
    /// then turns permission prompts off. An agent that cannot think at
    /// that level runs at its own and says so.
    fn think(&mut self, cx: &ConnectionTo<Agent>, mut opening: Opening) {
        let (Some(effort), Some(session)) = (opening.effort.take(), self.acp_session.clone())
        else {
            self.set_fast(cx, opening);
            return;
        };
        let cannot = |agent: &str| {
            format!(
                "{} here cannot be set to think at {}, so it thinks at its own level",
                agent_title(agent),
                effort.as_ref()
            )
        };
        let Some(id) = self.offers.thinking.clone() else {
            self.note(&cannot(&self.agent));
            self.set_fast(cx, opening);
            return;
        };

        match acp::set_effort(&session, &id, effort) {
            // The first prompt waits for the level.
            Ok(request) => self.send(cx, request, move |result| Reply::EffortSet {
                effort,
                result: result.map(drop),
                opening,
            }),
            Err(e) => {
                self.note(&format!("{}: {e}", cannot(&self.agent)));
                self.set_fast(cx, opening);
            }
        }
    }

    fn set_fast(&mut self, cx: &ConnectionTo<Agent>, mut opening: Opening) {
        if let Some(fast) = opening.fast.take() {
            if let (Some(session), Some(id)) = (&self.acp_session, &self.offers.fast_option) {
                let request = acp::set_fast(session, id, fast);
                match request {
                    Ok(request) => {
                        self.send(cx, request, move |result| Reply::FastSet {
                            fast,
                            result: result.map(drop),
                            opening,
                        });
                        return;
                    }
                    Err(e) => self.note(&format!("Could not set fast mode: {e}")),
                }
            } else {
                self.note("This model or adapter does not offer fast mode");
            }
        }
        self.unprompted(cx, opening);
    }

    /// The session runs on its model and thinking level: turns permission
    /// prompts off, then lets prompts go.
    fn unprompted(&mut self, cx: &ConnectionTo<Agent>, mut opening: Opening) {
        let Some(session) = self.acp_session.clone() else {
            return;
        };
        let first = opening.first_mode.take();

        if let Some(mode) = first.or_else(|| self.bypass_mode().map(str::to_owned)) {
            // The first prompt waits for the mode, so it never asks.
            let request = match self.mode_request(session, &mode) {
                Ok(request) => request,
                Err(error) => {
                    self.note(&error.to_string());
                    self.idle(cx);
                    return;
                }
            };
            self.send(cx, request, |result| Reply::FirstMode {
                mode,
                result: result.map(drop),
            });
            return;
        }

        self.idle(cx);
    }

    /// The session takes prompts: sends the first one waiting.
    fn idle(&mut self, cx: &ConnectionTo<Agent>) {
        self.ready_for_prompts = true;
        if self.foreground {
            self.show_queued();
            return;
        }

        // A prompt that goes out at once leaves no moment of Idle to read:
        // only a session with nothing to do waits for a prompt.
        let goes_now = self.has_waiting_prompts();
        if !goes_now {
            self.live
                .send_modify(|live| live.info.state.status = Status::Idle);
        }
        self.next_prompt(cx);
    }
}

/// The modes an agent offers in the order a switch goes through them: the one
/// that asks no permission, shown as Full access, then plan mode, then the
/// others as the agent lists them.
fn cycle(mut modes: Vec<ModeOption>, full: Option<&str>) -> Vec<ModeOption> {
    let is_full = |mode: &ModeOption| Some(mode.id.as_str()) == full;
    modes.sort_by_key(|mode| match mode.id.as_str() {
        _ if is_full(mode) => 0,
        acp::PLAN_MODE => 1,
        _ => 2,
    });

    for mode in modes.iter_mut().filter(|mode| is_full(mode)) {
        mode.name = "Full access".to_owned();
    }

    modes
}

/// What people call the agent `name`, such as Claude Code for `claude`.
pub(super) fn agent_title(name: &str) -> &str {
    known(name).map_or(name, |known| known.title)
}

/// How a turn ended, with a stop reason this node does not know as `Other`.
pub(super) fn stop_reason(done: &PromptResponse) -> StopReason {
    acp::wire_name(&done.stop_reason)
        .parse()
        .unwrap_or(StopReason::Other)
}

/// Whether a prompt failed only because claude-agent-acp found server tool
/// calls (`srvtoolu_` ids, run by the Anthropic API) without results. A
/// server tool in the same message as a client tool never runs; Claude Code
/// drops it and the turn goes on to end normally.
pub(super) fn lost_server_tools(e: &agent_client_protocol::Error) -> bool {
    let incomplete = e
        .data
        .as_ref()
        .is_some_and(|data| data["errorKind"] == "incomplete_tool_call");
    let ids = e.message.rsplit_once(": ").map_or("", |(_, ids)| ids);

    incomplete && !ids.is_empty() && ids.split(", ").all(|id| id.starts_with("srvtoolu_"))
}

/// `first`, then `then`.
pub(in crate::agents) fn joined(first: Option<String>, then: Option<&str>) -> Option<String> {
    match (first, then) {
        (Some(first), Some(then)) => Some(format!("{first}\n\n{then}")),
        (first, then) => first.or(then.map(str::to_owned)),
    }
}

#[cfg(test)]
#[path = "../../tests/agents_session_actor_v2.rs"]
mod tests;
