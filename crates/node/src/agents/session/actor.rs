//! The session's task: the ACP connection, its replies and the agent's events.

use super::{
    Answered, Command, EVENTS, MAX_STDERR_LINE, NODE_STOPPED, Order, STDERR_LINES,
    turns::turn_tokens,
};
use crate::agents::{
    acp::{self, Line, LineReader, ModelSwitch, Offered, Prompt},
    alerts::AlertHub,
    compaction::Compactor,
    images::Picture,
    known,
    live::{Live, TurnTokens, cut},
};
use crate::run::process::{GroupLeader, STOP_GRACE};
use agent_client_protocol::{
    Agent, Client, ConnectionTo, JsonRpcRequest, Responder, on_receive_notification,
    on_receive_request,
    schema::v1::{
        InitializeResponse, McpServer, PromptResponse, RequestPermissionRequest,
        RequestPermissionResponse, SessionConfigId, SessionId, SessionNotification, SessionUpdate,
        SetSessionModeRequest, ToolCallId,
    },
};
use anyhow::{Context, Result};
use beep_proto::ModelId;
use beep_proto::node::agent::{AlertKind, Effort, EntryKind, Question, Status, StopReason};
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
    io::BufReader,
    process::{Child, ChildStderr},
    sync::{mpsc, oneshot, watch},
};

/// What the agent sent the session's task.
pub(super) enum Event {
    Update(SessionUpdate),
    Permission(
        RequestPermissionRequest,
        Responder<RequestPermissionResponse>,
    ),
}

/// The answer to a request the session sent, with what it was for.
pub(super) enum Reply {
    /// Boxed: the answer to `initialize` is far larger than the others.
    Initialized(Result<Box<InitializeResponse>, agent_client_protocol::Error>),
    /// The answer to `session/new`, untyped to keep the models it offers.
    NewSession(Result<serde_json::Value, agent_client_protocol::Error>),
    /// The answer to switching the new session to `model`.
    ModelSet {
        model: ModelId,
        result: Result<(), agent_client_protocol::Error>,
    },
    /// The answer to switching the running session's model or thinking
    /// level, `what` saying which, as the transcript notes it.
    Reconfigured {
        what: String,
        result: Result<(), agent_client_protocol::Error>,
    },
    /// The answer to setting how hard the new session thinks.
    EffortSet {
        effort: Effort,
        result: Result<(), agent_client_protocol::Error>,
    },
    /// The answer to `session/resume` or `session/load` of this session,
    /// untyped to keep the models it offers.
    Resumed(
        SessionId,
        Result<serde_json::Value, agent_client_protocol::Error>,
    ),
    /// The answer to `session/set_mode` to the mode that asks no permission.
    Bypass(Result<(), agent_client_protocol::Error>),
    Prompt(Result<PromptResponse, agent_client_protocol::Error>),
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
}

/// A permission question waiting for its answer.
pub(super) struct Asked {
    pub(super) question: Question,
    pub(super) waiting: Waiting,
}

/// Who waits for a question's answer.
pub(super) enum Waiting {
    /// The agent, on its `session/request_permission`.
    Agent(Responder<RequestPermissionResponse>),
    /// The node, for a question of its own (`Command::Ask`).
    Node(oneshot::Sender<Answered>),
}

impl Waiting {
    /// Answers that the question was dropped. The node hears it as its
    /// sender going away.
    pub(super) fn cancel(self) -> Result<(), agent_client_protocol::Error> {
        match self {
            Self::Agent(responder) => responder.respond(acp::cancelled()),
            Self::Node(_) => Ok(()),
        }
    }
}

pub(super) struct Actor {
    pub(super) live: watch::Sender<Live>,
    pub(super) alerts: Arc<AlertHub>,
    /// Set once the node is stopping.
    pub(super) node_stopping: Arc<AtomicBool>,
    /// While true, prompts wait instead of starting a turn.
    pub(super) holding: watch::Receiver<bool>,
    /// The session's id on the node.
    pub(super) session: String,
    /// The agent, such as `claude`.
    pub(super) agent: String,
    pub(super) cwd: PathBuf,
    /// The agent's id for the session, once it made one.
    pub(super) acp_session: Option<SessionId>,
    /// Whether the agent takes image blocks, once `initialize` said.
    pub(super) takes_images: Option<bool>,
    /// The agent's id of an earlier session to resume.
    pub(super) resume: Option<String>,
    /// The model a new session switches to before its first prompt.
    pub(super) model: Option<ModelId>,
    /// How hard a new session thinks, set before its first prompt.
    pub(super) effort: Option<Effort>,
    /// The session's option that sets how hard it thinks, once it said.
    pub(super) thinking: Option<SessionConfigId>,
    /// How the session switches models, once it said; `None` when it
    /// cannot.
    pub(super) switch: Option<ModelSwitch>,
    /// The session mode that turns the agent's permission prompts off, set
    /// before the first prompt: Claude Code's `bypassPermissions`. Every
    /// agent runs without prompts, and a permission request that comes
    /// anyway is allowed rather than asked.
    pub(super) bypass_mode: Option<&'static str>,
    /// When the node compacts the conversation.
    pub(super) compactor: Compactor,
    /// Prompts waiting for the agent to be free, the first prompt among them.
    pub(super) queued: VecDeque<Prompt>,
    /// Messages for the running turn, sent once its tool call finishes.
    pub(super) steers: VecDeque<Prompt>,
    /// The prompts sent so far, which numbers each turn.
    pub(super) sent: u64,
    /// The turn running, by its number, while its prompt is unanswered.
    pub(super) turn: Option<u64>,
    /// Why the running turn was asked to stop, once it was.
    pub(super) stopping: Option<Stopping>,
    /// How long the agent has to end its turn after Stop.
    pub(super) stop_within: Duration,
    /// The tool calls of the running turn that have not finished.
    pub(super) running: HashSet<ToolCallId>,
    /// Beep's word to the agent, until the first prompt carries it.
    pub(super) brief: Option<String>,
    /// The MCP servers the agent's session gets.
    pub(super) mcp_servers: Vec<McpServer>,
    /// Questions waiting, the first one shown.
    pub(super) asked: VecDeque<Asked>,
    pub(super) next_question: u32,
    /// The last lines the agent wrote to stderr.
    pub(super) stderr: VecDeque<String>,
    /// Answers still to come. At most a few: one per request in flight, and
    /// a session has at most one prompt in flight.
    pub(super) replies: FuturesUnordered<BoxFuture<'static, Reply>>,
}

/// Why the session's task stopped.
pub(super) enum End {
    Closed,
    Exited,
    Failed(String),
    /// The agent did not end its turn within `stop_within` of Stop.
    Unstoppable(Duration),
    /// The node stopped or restarted.
    NodeStopped,
}

/// Why the node asked the agent to end its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stopping {
    /// The user stopped it.
    User,
    /// A tool call finished with a steer waiting: the turn ends there and
    /// the steer goes as the next prompt.
    Steer,
}

impl Stopping {
    /// What the session says it does meanwhile.
    pub(super) fn summary(self) -> &'static str {
        match self {
            Self::User => "Stopping",
            Self::Steer => "Taking your message",
        }
    }
}

/// The pipes the session reads besides ACP.
pub(super) struct Inputs {
    pub(super) events: mpsc::Receiver<Event>,
    /// The length of each message dropped for being too long.
    pub(super) oversized: mpsc::Receiver<usize>,
    pub(super) stderr: LineReader<BufReader<ChildStderr>>,
}

impl Actor {
    pub(super) async fn run(mut self, child: Child, mut orders: mpsc::Receiver<Order>) {
        // Kills the agent's whole process group however the task ends,
        // aborted with its session included; `stop` below is the gentle way.
        let mut child = GroupLeader::new(child);
        let held = child.hold_output();
        let end = self.serve(child.child_mut(), &mut orders).await;
        // The node stopping ends its sessions, whether it closed them or the
        // service manager stopped the agents first.
        let end = match end {
            Ok(End::Closed | End::Exited) if self.node_stopping.load(Ordering::Relaxed) => {
                Ok(End::NodeStopped)
            }
            end => end,
        };
        let closed = matches!(end, Ok(End::Closed));
        // Stopped as the user asked, or with the node, so not a failure to
        // alert about.
        let asked = closed || matches!(end, Ok(End::Unstoppable(_) | End::NodeStopped));
        // What still waits goes with the session when the node resumes it.
        let waiting: Vec<Prompt> = if matches!(end, Ok(End::NodeStopped)) {
            self.steers.drain(..).chain(self.queued.drain(..)).collect()
        } else {
            Vec::new()
        };
        let reason = match end {
            Ok(End::NodeStopped) => NODE_STOPPED.to_owned(),
            Ok(End::Closed) => "closed".to_owned(),
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
        child.stop(STOP_GRACE).await;
        drop(held);
        if !asked {
            self.alert(AlertKind::Failed {
                reason: reason.clone(),
            });
        }
        self.live.send_modify(|live| {
            // A cost the agent reported after its last turn still counts.
            live.spent(TurnTokens::default());
            live.info.question = None;
            live.info.state.status = Status::Ended;
            live.info.state.queued = 0;
            live.held = waiting;
            if !closed {
                live.info.state.error = Some(reason);
            }
        });
        crate::link::restart::changed();
    }

    async fn serve(
        &mut self,
        child: &mut Child,
        orders: &mut mpsc::Receiver<Order>,
    ) -> Result<End> {
        let stdin = child.stdin.take().context("the agent has no stdin")?;
        let stdout = child.stdout.take().context("the agent has no stdout")?;
        let stderr = child.stderr.take().context("the agent has no stderr")?;
        let (events_tx, events) = mpsc::channel(EVENTS);
        let (oversized_tx, oversized) = mpsc::channel(1);
        let mut inputs = Inputs {
            events,
            oversized,
            stderr: LineReader::new(BufReader::new(stderr), MAX_STDERR_LINE),
        };
        let updates = events_tx.clone();
        let questions = events_tx;
        // The handlers run in the crate's dispatch loop, which reads apart
        // from writing; each only hands the message to this task.
        let ended = Client
            .builder()
            .name("beep-node")
            .on_receive_notification(
                async move |notification: SessionNotification, _cx| {
                    // Fails only once the session ended, when it no longer matters.
                    let _ = updates.send(Event::Update(notification.update)).await;
                    Ok(())
                },
                on_receive_notification!(),
            )
            .on_receive_request(
                async move |request: RequestPermissionRequest, responder, _cx| {
                    let event = Event::Permission(request, responder);
                    if let Err(mpsc::error::SendError(Event::Permission(_, responder))) =
                        questions.send(event).await
                    {
                        responder.respond(acp::cancelled())?;
                    }
                    Ok(())
                },
                on_receive_request!(),
            )
            .connect_with(
                acp::transport(stdin, stdout, oversized_tx),
                async |cx: ConnectionTo<Agent>| Ok(self.drive(&cx, &mut inputs, orders).await),
            )
            .await;
        ended.context("the ACP connection failed")?
    }

    async fn drive(
        &mut self,
        cx: &ConnectionTo<Agent>,
        inputs: &mut Inputs,
        orders: &mut mpsc::Receiver<Order>,
    ) -> Result<End> {
        self.send(cx, acp::initialize(), |result| {
            Reply::Initialized(result.map(Box::new))
        });
        let mut stderr_open = true;
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
                Ok(()) = self.holding.changed() => {
                    if !*self.holding.borrow() && self.live.borrow().status() == Status::Idle {
                        self.next_prompt(cx);
                    }
                }
                () = cx.incoming_closed() => return Ok(End::Exited),
                Some(reply) = self.replies.next() => {
                    if let Some(end) = self.reply(cx, reply)? {
                        return Ok(end);
                    }
                }
                line = inputs.stderr.next(), if stderr_open => match line {
                    Ok(Some(Line::Text(line))) if !line.trim().is_empty() => {
                        log::debug!("agent stderr: {line}");
                        if self.stderr.len() >= STDERR_LINES {
                            self.stderr.pop_front();
                        }
                        self.stderr.push_back(cut(line.trim(), 300));
                    }
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => stderr_open = false,
                },
                order = orders.recv() => {
                    let Some(Order { command, done }) = order else {
                        return Ok(End::Closed);
                    };
                    if matches!(command, Command::Close) {
                        let _ = done.send(Ok(()));
                        return Ok(End::Closed);
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
        let answer = cx.send_request(request).block_task();
        self.replies
            .push(async move { reply(answer.await) }.boxed());
    }

    fn event(&mut self, cx: &ConnectionTo<Agent>, event: Event) -> Result<()> {
        match event {
            Event::Update(update) => {
                let finished = self.tool_finished(&update);
                self.live.send_if_modified(|live| live.apply(&update));
                if let Some(stopping) = self.stopping {
                    self.show_stopping(stopping);
                }
                if finished {
                    self.steer_now(cx)?;
                }
            }
            Event::Permission(request, responder) => {
                if let Some(responder) = self.allow(&request, responder)? {
                    self.ask(&request, responder);
                }
            }
        }
        Ok(())
    }

    /// The agent sent a message too long to keep: it was dropped, and the
    /// session says so.
    fn oversized(&mut self, length: usize) {
        let error = format!(
            "dropped a {} MiB message from the agent; at most {} MiB are read",
            length / (1024 * 1024),
            acp::MAX_MESSAGE / (1024 * 1024)
        );
        self.live
            .send_modify(|live| live.info.state.error = Some(error));
    }

    fn reply(&mut self, cx: &ConnectionTo<Agent>, reply: Reply) -> Result<Option<End>> {
        match reply {
            Reply::Initialized(Ok(initialized)) => {
                self.takes_images = Some(acp::takes_images(&initialized));
                if let Some(end) = self.initialized(cx, &initialized) {
                    return Ok(Some(end));
                }
            }
            Reply::NewSession(Ok(answer)) => match acp::session_created(answer) {
                Ok((created, offered)) => return Ok(self.ready(cx, created.session_id, offered)),
                Err(e) => {
                    return Ok(Some(End::Failed(format!(
                        "the agent did not start: it answered session/new with {e}"
                    ))));
                }
            },
            Reply::Resumed(session, Ok(answer)) => {
                return Ok(self.ready(cx, session, acp::offers(&answer)));
            }
            Reply::ModelSet { result: Ok(()), .. } => self.think(cx),
            Reply::Reconfigured { what, result } => match result {
                Ok(()) => self.note(&format!("Switched to {what} from the next turn")),
                Err(e) => self.note(&format!("Could not switch to {what}: {e}")),
            },
            Reply::EffortSet { effort, result } => {
                if let Err(e) = result {
                    self.note(&format!(
                        "{} could not be set to think at {}, so it thinks at its own level: {e}",
                        agent_title(&self.agent),
                        effort.as_str()
                    ));
                }
                self.unprompted(cx);
            }
            Reply::ModelSet {
                model,
                result: Err(e),
            } => {
                return Ok(Some(End::Failed(format!(
                    "the agent could not switch to {model}: {e}"
                ))));
            }
            Reply::Bypass(result) => {
                if let Err(e) = result {
                    // Requests that still come are allowed in `ask`.
                    log::warn!("the agent kept asking permission: {e}");
                }
                self.idle(cx);
            }
            Reply::Resumed(_, Err(e)) => {
                return Ok(Some(End::Failed(format!(
                    "the agent could not resume the session: {e}"
                ))));
            }
            Reply::Initialized(Err(e)) | Reply::NewSession(Err(e)) => {
                return Ok(Some(End::Failed(format!("the agent did not start: {e}"))));
            }
            Reply::Prompt(result) => {
                self.turn = None;
                self.stopping = None;
                self.running.clear();
                let tokens = result.as_ref().map(turn_tokens).unwrap_or_default();
                self.live.send_modify(|live| live.spent(tokens));
                let (stop, error) = match result {
                    Ok(done) => (
                        StopReason::from_acp(&acp::wire_name(&done.stop_reason)),
                        None,
                    ),
                    Err(e) => (StopReason::Failed, Some(e.to_string())),
                };
                self.turn_done(cx, stop, error);
            }
            Reply::Compacted(result) => self.compacted(cx, result),
            Reply::StopDue { turn } => {
                if self.turn == Some(turn) && self.stopping == Some(Stopping::User) {
                    log::warn!("the agent ignored session/cancel; stopping it");
                    return Ok(Some(End::Unstoppable(self.stop_within)));
                }
            }
            Reply::FilesRead { entry, pictures } => {
                if !pictures.is_empty() {
                    self.live
                        .send_modify(|live| live.show_files(entry, pictures));
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
        initialized: &InitializeResponse,
    ) -> Option<End> {
        let Some(session) = self.resume.take() else {
            match acp::new_session(&self.cwd, self.mcp_servers.clone()) {
                Ok(request) => self.send(cx, request, Reply::NewSession),
                Err(e) => return Some(End::Failed(format!("the agent did not start: {e}"))),
            }
            return None;
        };
        let session = SessionId::from(session);
        let Some(method) = acp::resume_method(initialized) else {
            return Some(End::Failed("the agent cannot resume a session".into()));
        };
        match acp::resume_session(method, session.clone(), &self.cwd, self.mcp_servers.clone()) {
            Ok(request) => self.send(cx, request, |result| Reply::Resumed(session, result)),
            Err(e) => {
                return Some(End::Failed(format!(
                    "the agent could not resume the session: {e}"
                )));
            }
        }
        None
    }

    /// The agent made or resumed its session `session`, which offers the
    /// models in `offered`: switches it to the model asked for, then lets
    /// prompts go. Gives why the session ends when it cannot.
    fn ready(
        &mut self,
        cx: &ConnectionTo<Agent>,
        session: SessionId,
        offered: Offered,
    ) -> Option<End> {
        self.acp_session = Some(session.clone());
        self.thinking = offered.thinking;
        self.switch = offered.switch.clone();
        // From now on the session goes by the agent's own id, which
        // still finds it once the node restarts.
        self.live.send_modify(|live| {
            live.agent_session = Some(session.to_string());
            live.info.id = session.to_string();
        });
        crate::link::restart::changed();
        let Some(model) = self.model.take() else {
            self.think(cx);
            return None;
        };
        let Some(switch) = offered.switch else {
            self.note(&format!(
                "{} here cannot switch models, so it runs its own rather than {model}",
                agent_title(&self.agent)
            ));
            self.think(cx);
            return None;
        };
        match acp::switch_model(&session, &switch, model.as_str()) {
            Ok(request) => {
                // The first prompt waits for the model.
                self.send(cx, request, move |result| Reply::ModelSet {
                    model,
                    result: result.map(drop),
                });
                None
            }
            Err(e) => Some(End::Failed(format!(
                "the agent could not switch to {model}: {e}"
            ))),
        }
    }

    /// Shows `note` in the transcript, for what the session does other
    /// than asked.
    fn note(&mut self, note: &str) {
        log::info!("{note}");
        self.live
            .send_modify(|live| live.push(EntryKind::Other, note));
    }

    /// The session runs on its model: sets how hard it thinks when asked,
    /// then turns permission prompts off. An agent that cannot think at
    /// that level runs at its own and says so.
    fn think(&mut self, cx: &ConnectionTo<Agent>) {
        let (Some(effort), Some(session)) = (self.effort.take(), self.acp_session.clone()) else {
            self.unprompted(cx);
            return;
        };
        let cannot = |agent: &str| {
            format!(
                "{} here cannot be set to think at {}, so it thinks at its own level",
                agent_title(agent),
                effort.as_str()
            )
        };
        let Some(id) = self.thinking.clone() else {
            self.note(&cannot(&self.agent));
            self.unprompted(cx);
            return;
        };
        match acp::set_effort(&session, &id, effort) {
            // The first prompt waits for the level.
            Ok(request) => self.send(cx, request, move |result| Reply::EffortSet {
                effort,
                result: result.map(drop),
            }),
            Err(e) => {
                self.note(&format!("{}: {e}", cannot(&self.agent)));
                self.unprompted(cx);
            }
        }
    }

    /// The session runs on its model and thinking level: turns permission
    /// prompts off, then lets prompts go.
    fn unprompted(&mut self, cx: &ConnectionTo<Agent>) {
        let Some(session) = self.acp_session.clone() else {
            return;
        };
        if let Some(mode) = self.bypass_mode {
            // The first prompt waits for the mode, so it never asks.
            let request = SetSessionModeRequest::new(session, mode);
            self.send(cx, request, |result| Reply::Bypass(result.map(drop)));
            return;
        }
        self.idle(cx);
    }

    /// The session takes prompts: sends the first one waiting.
    fn idle(&mut self, cx: &ConnectionTo<Agent>) {
        self.live
            .send_modify(|live| live.info.state.status = Status::Idle);
        self.next_prompt(cx);
    }
}

/// What people call the agent `name`, such as Claude Code for `claude`.
pub(super) fn agent_title(name: &str) -> &str {
    known(name).map_or(name, |known| known.title)
}
