//! The session's task: the ACP connection, its replies and the agent's events.

use super::{
    Answered, Command, EVENTS, MAX_STDERR_LINE, NODE_STOPPED, Order, STDERR_LINES,
    turns::{HeldEnd, turn_tokens},
};
use crate::agents::{
    acp::{self, AsyncTask, Line, LineReader, ModelSwitch, Offered, Prompt},
    alerts::AlertHub,
    compaction::{Compactor, TurnEnd},
    images::Picture,
    known,
    live::{Live, TurnTokens, cut},
    made_files,
    place::of_folder,
};
use crate::run::process::{GroupLeader, STOP_GRACE};
use agent_client_protocol::{
    Agent, Client, Handled, JsonRpcRequest, on_receive_dispatch, on_receive_notification,
    on_receive_request,
    v1::schema::{
        ContentBlock, InitializeResponse, McpServer, PromptResponse, RequestPermissionRequest,
        RequestPermissionResponse, SessionConfigId, SessionId, SessionNotification, SessionUpdate,
        SetSessionModeRequest, ToolCallId,
    },
    v1::{ConnectionTo, Dispatch, Responder},
};
use anyhow::{Context, Result};
use beep_proto::ModelId;
use beep_proto::node::agent::{
    AlertKind, Effort, Entry, EntryKind, ModeOption, Question, Status, StopReason, WorkPlace,
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
    io::BufReader,
    process::{Child, ChildStderr},
    sync::{mpsc, oneshot, watch},
    task::spawn_blocking,
};

/// What the agent sent the session's task.
pub(super) enum Event {
    Update(SessionUpdate),
    /// A command the agent runs in the background started, moved or ended.
    Task(AsyncTask),
    Permission(
        RequestPermissionRequest,
        Box<Responder<RequestPermissionResponse>>,
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
        model: Option<ModelId>,
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
}

/// Who waits for a question's answer.
pub(super) enum Waiting {
    /// The agent, on its `session/request_permission`.
    Agent(Box<Responder<RequestPermissionResponse>>),
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
    /// A move to another machine waits for the turn: queued prompts wait
    /// too.
    pub(super) moving: bool,
    /// The agent, such as `claude`.
    pub(super) agent: String,
    pub(super) cwd: PathBuf,
    /// The agent's id for the session, once it made one.
    pub(super) acp_session: Option<SessionId>,
    /// Whether the agent takes image blocks, once `initialize` said.
    pub(super) takes_images: Option<bool>,
    pub(super) takes_steering: bool,
    /// A steer sent to the adapter, awaiting its delivery outcome.
    pub(super) steering: Option<PendingSteer>,
    /// The adapter found no running turn; wait for our prompt's reply.
    pub(super) steer_after_turn: bool,
    /// The agent's id of an earlier session to resume.
    pub(super) resume: Option<String>,
    /// The transcript the resumed session had before the node restarted,
    /// shown again when the agent resumes without replaying it.
    pub(super) earlier: Vec<Entry>,
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
    /// The mode to start in instead, for a session the node resumes.
    pub(super) first_mode: Option<String>,
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
    /// The turn running, by its number, while its prompt is unanswered.
    pub(super) turn: Option<u64>,
    /// The end of the last turn, held while its subagents run.
    pub(super) held_end: Option<HeldEnd>,
    /// Why the running turn was asked to stop, once it was.
    pub(super) stopping: Option<Stopping>,
    /// The user stopped the running turn, or closed the session.
    pub(super) by_user: bool,
    /// How long the agent has to end its turn after Stop.
    pub(super) stop_within: Duration,
    /// The tool calls of the running turn that have not finished.
    pub(super) running: HashSet<ToolCallId>,
    /// Beep's word to the agent from whoever started the session; once
    /// `initialize` is answered, what the first prompt still carries.
    pub(super) brief: Option<String>,
    /// What holds for the whole session: the git rule for its folder and
    /// what the Beep tools are. In the system prompt of an agent that takes
    /// one; for the others, said again after the agent compacts.
    pub(super) standing: Option<String>,
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

/// The pipes the session reads besides ACP.
pub(super) struct Inputs {
    pub(super) events: mpsc::Receiver<Event>,
    /// The length of each message dropped for being too long.
    pub(super) oversized: mpsc::Receiver<usize>,
    pub(super) stderr: LineReader<BufReader<ChildStderr>>,
}

impl Actor {
    pub(super) async fn run(
        mut self,
        child: Child,
        mut orders: mpsc::Receiver<Order>,
        processes: Arc<super::Processes>,
    ) {
        let _cleanup = super::super::processes::Cleanup(processes.clone());
        let _watch = processes.watch(self.live.subscribe());
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
        let by_user = self.by_user;
        // Stopped as the user asked, or with the node, so not a failure to
        // alert about.
        let asked = closed
            || matches!(
                end,
                Ok(End::Unstoppable(_) | End::StoppedAll | End::NodeStopped)
            );
        // What still waits goes with the session when the node resumes it.
        let waiting: Vec<Prompt> = if matches!(end, Ok(End::NodeStopped)) {
            let interrupted = matches!(
                watched::read_sent(&self.live, Live::status),
                Status::Working | Status::Asking
            );
            let continuation =
                interrupted.then(|| Prompt::from(super::super::restart::CONTINUE.to_owned()));
            continuation
                .into_iter()
                .chain(self.steers.drain(..))
                .chain(self.queued.drain(..))
                .collect()
        } else {
            Vec::new()
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
        child.stop(STOP_GRACE).await;
        drop(held);

        if closed && by_user {
            let info = watched::read_sent(&self.live, |live| live.info.clone());
            let removed =
                spawn_blocking(move || crate::agents::uploads::remove(&info.agent, &info.id)).await;
            if let Err(e) = removed.unwrap_or_else(|e| Err(e.into())) {
                log::warn!("could not remove closed session attachments: {e:#}");
            }
        }

        if !asked {
            self.alert(AlertKind::Failed {
                reason: reason.clone(),
            });
        }

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
            live.held = waiting;
            live.stopped(closed && by_user);
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
        let tasks = events_tx.clone();
        let questions = events_tx;
        // The handlers run in the crate's dispatch loop, which reads apart
        // from writing; each only hands the message to this task.
        let ended = Client
            .v1()
            .name("beep-node")
            // Before the typed handler below, which refuses the updates the
            // crate does not know.
            .on_receive_dispatch(
                async move |dispatch: Dispatch, _cx| {
                    let Dispatch::Notification(message) = &dispatch else {
                        return Ok(Handled::No {
                            message: dispatch,
                            retry: false,
                        });
                    };
                    let Some(task) = acp::async_task(message) else {
                        return Ok(Handled::No {
                            message: dispatch,
                            retry: false,
                        });
                    };
                    // Fails only once the session ended, when it no longer matters.
                    let _ = tasks.send(Event::Task(task)).await;
                    Ok(Handled::Yes)
                },
                on_receive_dispatch!(),
            )
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
                    let event = Event::Permission(request, Box::new(responder));

                    if let Err(mpsc::error::SendError(Event::Permission(_, responder))) =
                        questions.send(event).await
                    {
                        (*responder).respond(acp::cancelled())?;
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
        let async_tasks = known(&self.agent).is_some_and(|known| known.async_tasks);
        self.send(cx, acp::initialize(async_tasks), |result| {
            Reply::Initialized(result.map(Box::new))
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
                () = tokio::time::sleep_until(
                    self.held_end.as_ref().map_or_else(tokio::time::Instant::now, |held| held.due)
                ), if self.held_end.is_some() => self.subagents_quiet(),
                order = orders.recv() => {
                    let Some(Order { command, done }) = order else {
                        return Ok(End::Closed);
                    };

                    if let Command::Close { by_user } = command {
                        self.by_user = by_user;
                        self.live.send_modify(|live| live.closing = by_user);
                        let _ = done.send(Ok(()));
                        return Ok(End::Closed);
                    }

                    if let Command::StopAll { by_user } = command
                        && watched::read_sent(&self.live, |live| live.info.state.subagents) > 0
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
        let answer = cx.send_request(request).block_task();
        self.replies.push(
            async move {
                let answer = answer.await;
                trace.finish(answer.is_ok());
                reply(answer)
            }
            .boxed(),
        );
    }

    fn event(&mut self, cx: &ConnectionTo<Agent>, event: Event) -> Result<()> {
        match event {
            Event::Update(update) => {
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
                    let offered = acp::offered_models(
                        &serde_json::json!({ "configOptions": options.config_options }),
                    );

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

                if let Some(stopping) = self.stopping {
                    self.show_stopping(stopping);
                }

                if finished {
                    self.steer_now(cx)?;
                }

                self.subagents_moved();
            }
            Event::Task(task) => {
                self.live.send_if_modified(|live| live.apply_task(task));
                self.subagents_moved();
            }
            Event::Permission(request, responder) => {
                let question = watched::read_sent(&self.live, |live| live.question(&request));
                let Some(responder) = self.plan_ready(&request, &question, *responder) else {
                    return Ok(());
                };

                if let Some(responder) = self.allow(&question, responder)? {
                    self.ask(question, responder);
                }
            }
        }

        Ok(())
    }

    /// Asks the checkout the session works in now when its folder changed
    /// since the last time, off the async workers. The answer comes back as
    /// `Reply::Placed`.
    fn look_for_place(&mut self) {
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
            acp::MAX_MESSAGE / (1024 * 1024)
        );
        self.live
            .send_modify(|live| live.info.state.error = Some(error));
    }

    fn reply(&mut self, cx: &ConnectionTo<Agent>, reply: Reply) -> Result<Option<End>> {
        match reply {
            Reply::Initialized(Ok(initialized)) => {
                self.takes_images = Some(acp::takes_images(&initialized));
                self.takes_steering = acp::takes_steering(&initialized);

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
            Reply::ModelSet {
                model,
                result: Ok(()),
            } => {
                self.model_is(Some(model.as_ref()));
                self.think(cx);
            }
            Reply::Reconfigured {
                what,
                model,
                result,
            } => match result {
                Ok(()) => {
                    if let Some(model) = model {
                        self.model_is(Some(model.as_ref()));
                    }

                    self.note(&format!("Switched to {what} from the next turn"));
                }
                Err(e) => self.note(&format!("Could not switch to {what}: {e}")),
            },
            Reply::EffortSet { effort, result } => {
                if let Err(e) = result {
                    self.note(&format!(
                        "{} could not be set to think at {}, so it thinks at its own level: {e}",
                        agent_title(&self.agent),
                        effort.as_ref()
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
                self.steer_after_turn = false;
                self.stopping = None;
                self.running.clear();
                let tokens = result.as_ref().map(turn_tokens).unwrap_or_default();
                self.live.send_modify(|live| live.spent(tokens));
                let (stop, error) = match result {
                    Ok(done) => (stop_reason(&done), None),
                    Err(e) => (StopReason::Failed, Some(e.to_string())),
                };
                self.turn_done(cx, stop, error);
            }
            Reply::Steered(result) => self.steered(cx, result)?,
            Reply::Compacted(result) => self.compacted(cx, result),
            Reply::StopDue { turn } => {
                if self.turn == Some(turn) && self.stopping == Some(Stopping::User) {
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
        initialized: &InitializeResponse,
    ) -> Option<End> {
        // The standing rules hold for the whole session; the caller's brief
        // is said once, where the session starts or resumes. An agent with
        // no system prompt gets both in its first prompt and the rules again
        // after each compaction, which can summarize them away.
        let takes_system = acp::takes_system_prompt(initialized);
        let standing = if takes_system {
            self.standing.take()
        } else {
            self.standing.clone()
        };
        let method = acp::resume_method(initialized);
        self.live.send_modify(|live| {
            live.replays = method == Some(acp::ResumeMethod::Load);
        });
        let Some(session) = self.resume.take() else {
            let brief = joined(self.brief.take(), standing.as_deref());
            let system = if takes_system {
                brief
            } else {
                self.brief = brief;
                None
            };

            match acp::new_session(&self.cwd, self.mcp_servers.clone(), system.as_deref()) {
                Ok(request) => self.send(cx, request, Reply::NewSession),
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
        let session = SessionId::from(session);
        let Some(method) = method else {
            return Some(End::Failed("the agent cannot resume a session".into()));
        };
        let earlier = std::mem::take(&mut self.earlier);

        if method == acp::ResumeMethod::Resume && !earlier.is_empty() {
            self.live.send_modify(|live| live.restore(earlier));
        }

        match acp::resume_session(
            method,
            session.clone(),
            &self.cwd,
            self.mcp_servers.clone(),
            system.as_deref(),
        ) {
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
        self.model_is(offered.current.as_deref());
        self.thinking = offered.thinking;
        self.switch = offered.switch.clone();

        if let Some(modes) = offered.modes {
            let bypass = self.bypass_mode;
            self.live.send_modify(|live| {
                live.info.modes = cycle(modes.available, bypass);
            });
            self.mode_is(cx, modes.current);
        }
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

        match acp::switch_model(&session, &switch, model.as_ref()) {
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

    /// A model switch while idle can make the existing context due for compaction.
    fn model_is(&mut self, model: Option<&str>) {
        if let Some(model) = model {
            let model = ModelId::new(model);
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
    fn think(&mut self, cx: &ConnectionTo<Agent>) {
        let (Some(effort), Some(session)) = (self.effort.take(), self.acp_session.clone()) else {
            self.unprompted(cx);
            return;
        };
        let cannot = |agent: &str| {
            format!(
                "{} here cannot be set to think at {}, so it thinks at its own level",
                agent_title(agent),
                effort.as_ref()
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
        let first = self.first_mode.take();

        if let Some(mode) = first.or_else(|| self.bypass_mode.map(str::to_owned)) {
            // The first prompt waits for the mode, so it never asks.
            let request = SetSessionModeRequest::new(session, mode.clone());
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
        // A prompt that goes out at once leaves no moment of Idle to read:
        // only a session with nothing to do waits for a prompt.
        let goes_now = !(self.queued.is_empty() && self.steers.is_empty());
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
fn cycle(mut modes: Vec<ModeOption>, bypass: Option<&str>) -> Vec<ModeOption> {
    let is_bypass = |mode: &ModeOption| Some(mode.id.as_str()) == bypass;
    modes.sort_by_key(|mode| match mode.id.as_str() {
        _ if is_bypass(mode) => 0,
        acp::PLAN_MODE => 1,
        _ => 2,
    });

    for mode in modes.iter_mut().filter(|mode| is_bypass(mode)) {
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

/// `first`, then `then`.
pub(in crate::agents) fn joined(first: Option<String>, then: Option<&str>) -> Option<String> {
    match (first, then) {
        (Some(first), Some(then)) => Some(format!("{first}\n\n{then}")),
        (first, then) => first.or(then.map(str::to_owned)),
    }
}
