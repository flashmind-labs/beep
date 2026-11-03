//! One agent session: the adapter process and the task that talks ACP to
//! it through the official crate. The task owns the process; streams send
//! it orders and follow its state through a watch channel.

use super::{
    AgentKind,
    acp::{self, Line, LineReader, ModelSwitch, Offered, Prompt, ResumeMethod},
    compaction::{COMPACT_PROMPT, COMPACTED_ENTRY, Compactor, Policy, TurnEnd},
    image_files,
    images::{Images, Picture},
    live::{Live, TurnFiles, TurnTokens, cut},
    usage::UsageHub,
};
use crate::{
    process::{GroupLeader, STOP_GRACE, command, own_group},
    provision::spawn_env::{self, Scope},
};
use agent_client_protocol::{
    Agent, Client, ConnectionTo, JsonRpcRequest, Responder, on_receive_notification,
    on_receive_request,
    schema::v1::{
        CancelNotification, InitializeResponse, LoadSessionRequest, PromptResponse,
        RequestPermissionRequest, RequestPermissionResponse, ResumeSessionRequest, SessionConfigId,
        SessionId, SessionNotification, SessionUpdate, SetSessionModeRequest,
    },
};
use anyhow::{Context, Result};
use beep_proto::node::agent::{
    AnswerOption, Effort, EntryKind, OptionKind, Question, SessionInfo, State, Status, StopReason,
};
use beep_proto::node::usage::{MAX_MODEL_NAME, MAX_TURN_MODELS, ModelTokens};
use beep_proto::{AgentAccount, ModelId};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use serde_json::Value;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Stdio,
    str::FromStr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::BufReader,
    process::{Child, ChildStderr},
    sync::{mpsc, oneshot, watch},
    task::spawn_blocking,
};
use tokio_util::task::AbortOnDropHandle;

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

/// What a stream asks of a session.
#[derive(Debug)]
pub(super) enum Command {
    Prompt(Prompt),
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
    pub(super) model: Option<String>,
    /// How hard a new session thinks, rather than the agent's own level.
    pub(super) effort: Option<Effort>,
    /// A first line for its transcript, such as the branch it started on.
    pub(super) note: Option<String>,
    /// Where its transcript's pictures are kept.
    pub(super) images: Arc<Images>,
    /// Where what its turns spend goes.
    pub(super) usage: Arc<UsageHub>,
}

impl Session {
    pub(super) fn start(start: Start<'_>) -> Result<Self> {
        let Start {
            id,
            kind,
            cwd,
            prompt,
            resume,
            model,
            effort,
            note,
            images,
            usage,
        } = start;
        // Only a new session takes a model and thinking level; a resumed
        // one keeps its own.
        let model = model.filter(|_| resume.is_none());
        let effort = effort.filter(|_| resume.is_none());
        // An agent whose command line picks the model starts on it; the
        // others switch once the session exists.
        let flag = acp::model_flag(&kind.name)
            .zip(model.as_deref())
            .map(|(flag, model)| format!("{flag}={model}"));
        let model = model.filter(|_| flag.is_none());
        let policy = Policy::from_env(&kind.env);
        // Claude Code compacts mid-turn on its own trigger; it is set to
        // the same threshold unless the user set it.
        let agent_env = policy
            .agent_env(&kind.name, &kind.env)
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
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
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
            agent: kind.name.clone(),
            cwd: cwd.to_path_buf(),
            acp_session: None,
            takes_images: None,
            queued: prompt.map(Prompt::from).into_iter().collect(),
            resume,
            model,
            effort,
            thinking: None,
            switch: None,
            bypass_mode: acp::bypass_mode(&kind.name),
            compactor: Compactor::new(&kind.name, policy),
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

/// What the agent sent the session's task.
enum Event {
    Update(SessionUpdate),
    Permission(
        RequestPermissionRequest,
        Responder<RequestPermissionResponse>,
    ),
}

/// The answer to a request the session sent, with what it was for.
enum Reply {
    /// Boxed: the answer to `initialize` is far larger than the others.
    Initialized(Result<Box<InitializeResponse>, agent_client_protocol::Error>),
    /// The answer to `session/new`, untyped to keep the models it offers.
    NewSession(Result<serde_json::Value, agent_client_protocol::Error>),
    /// The answer to switching the new session to `model`.
    ModelSet {
        model: String,
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
    /// The answer to `session/resume` or `session/load` of this session.
    Resumed(SessionId, Result<(), agent_client_protocol::Error>),
    /// The answer to `session/set_mode` to the mode that asks no permission.
    Bypass(Result<(), agent_client_protocol::Error>),
    Prompt(Result<PromptResponse, agent_client_protocol::Error>),
    /// The answer to the `/compact` the node sent.
    Compacted(Result<PromptResponse, agent_client_protocol::Error>),
    /// The image files a finished turn named, read, for the agent message
    /// `entry`.
    FilesRead {
        entry: Option<u64>,
        pictures: Vec<Picture>,
    },
}

/// A permission question waiting for its answer.
struct Asked {
    question: Question,
    responder: Responder<RequestPermissionResponse>,
}

struct Actor {
    live: watch::Sender<Live>,
    /// The agent, such as `claude`.
    agent: String,
    cwd: PathBuf,
    /// The agent's id for the session, once it made one.
    acp_session: Option<SessionId>,
    /// Whether the agent takes image blocks, once `initialize` said.
    takes_images: Option<bool>,
    /// The agent's id of an earlier session to resume.
    resume: Option<String>,
    /// The model a new session switches to before its first prompt.
    model: Option<String>,
    /// How hard a new session thinks, set before its first prompt.
    effort: Option<Effort>,
    /// The session's option that sets how hard it thinks, once it said.
    thinking: Option<SessionConfigId>,
    /// How the session switches models, once it said; `None` when it
    /// cannot.
    switch: Option<ModelSwitch>,
    /// The session mode that turns the agent's permission prompts off, set
    /// before the first prompt: Claude Code's `bypassPermissions`. Every
    /// agent runs without prompts, and a permission request that comes
    /// anyway is allowed rather than asked.
    bypass_mode: Option<&'static str>,
    /// When the node compacts the conversation.
    compactor: Compactor,
    /// Prompts waiting for the agent to be free, the first prompt among them.
    queued: VecDeque<Prompt>,
    /// Questions waiting, the first one shown.
    asked: VecDeque<Asked>,
    next_question: u32,
    /// The last lines the agent wrote to stderr.
    stderr: VecDeque<String>,
    /// Answers still to come. At most a few: one per request in flight, and
    /// a session has at most one prompt in flight.
    replies: FuturesUnordered<BoxFuture<'static, Reply>>,
}

/// Why the session's task stopped.
enum End {
    Closed,
    Exited,
    Failed(String),
}

/// The pipes the session reads besides ACP.
struct Inputs {
    events: mpsc::Receiver<Event>,
    /// The length of each message dropped for being too long.
    oversized: mpsc::Receiver<usize>,
    stderr: LineReader<BufReader<ChildStderr>>,
}

impl Actor {
    async fn run(mut self, child: Child, mut orders: mpsc::Receiver<Order>) {
        // Kills the agent's whole process group however the task ends,
        // aborted with its session included; `stop` below is the gentle way.
        let mut child = GroupLeader::new(child);
        let held = child.hold_output();
        let end = self.serve(child.child_mut(), &mut orders).await;
        let closed = matches!(end, Ok(End::Closed));
        let reason = match end {
            Ok(End::Closed) => "closed".to_owned(),
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
        self.live.send_modify(|live| {
            // A cost the agent reported after its last turn still counts.
            live.spent(TurnTokens::default());
            live.info.question = None;
            live.info.state.status = Status::Ended;
            live.info.state.queued = 0;
            if !closed {
                live.info.state.error = Some(reason);
            }
        });
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
                    Some(event) => self.event(event)?,
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
    fn send<Req>(
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

    fn event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Update(update) => {
                self.live.send_if_modified(|live| live.apply(&update));
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
            Reply::Resumed(session, Ok(())) => {
                return Ok(self.ready(cx, session, Offered::default()));
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
            match acp::new_session(&self.cwd) {
                Ok(request) => self.send(cx, request, Reply::NewSession),
                Err(e) => return Some(End::Failed(format!("the agent did not start: {e}"))),
            }
            return None;
        };
        let session = SessionId::from(session);
        let cwd = self.cwd.clone();
        let id = session.clone();
        match acp::resume_method(initialized) {
            Some(ResumeMethod::Resume) => {
                self.send(cx, ResumeSessionRequest::new(id, cwd), |result| {
                    Reply::Resumed(session, result.map(drop))
                });
            }
            Some(ResumeMethod::Load) => {
                self.send(cx, LoadSessionRequest::new(id, cwd), |result| {
                    Reply::Resumed(session, result.map(drop))
                });
            }
            None => return Some(End::Failed("the agent cannot resume a session".into())),
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
        self.live
            .send_modify(|live| live.agent_session = Some(session.to_string()));
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
        match acp::switch_model(&session, &switch, &model) {
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

    fn command(
        &mut self,
        cx: &ConnectionTo<Agent>,
        command: Command,
    ) -> Result<Result<(), String>> {
        match command {
            Command::Prompt(prompt) => {
                if let Err(refused) = self.can_take(&prompt) {
                    return Ok(Err(refused));
                }
                if let Err(full) = queue_room(&self.queued, &prompt) {
                    return Ok(Err(full));
                }
                self.queued.push_back(prompt);
                self.show_queued();
                if self.live.borrow().status() == Status::Idle {
                    self.next_prompt(cx);
                }
                Ok(Ok(()))
            }
            Command::Answer { question, reply } => self.answer(question, &reply),
            Command::Cancel => {
                self.queued.clear();
                self.show_queued();
                while let Some(asked) = self.asked.pop_front() {
                    asked.responder.respond(acp::cancelled())?;
                }
                if let Some(session) = &self.acp_session
                    && self.live.borrow().status() != Status::Idle
                {
                    cx.send_notification(CancelNotification::new(session.clone()))?;
                }
                self.show_question();
                Ok(Ok(()))
            }
            Command::Close => Ok(Ok(())),
            Command::Configure { model, effort } => Ok(self.configure(cx, model, effort)),
        }
    }

    /// Switches the running session to `model` and `effort`, which the
    /// agent applies from its next turn. Refused, sending nothing, when it
    /// cannot switch what is asked.
    fn configure(
        &mut self,
        cx: &ConnectionTo<Agent>,
        model: Option<ModelId>,
        effort: Option<Effort>,
    ) -> Result<(), String> {
        let Some(session) = self.acp_session.clone() else {
            return Err("the agent is still starting; switch once it is ready".into());
        };
        let title = agent_title(&self.agent);
        let mut requests = Vec::new();
        if let Some(model) = model {
            let Some(switch) = &self.switch else {
                return Err(format!(
                    "{title} here cannot switch models in a running session"
                ));
            };
            let request = acp::switch_model(&session, switch, model.as_str())
                .map_err(|e| format!("{title} could not switch to {model}: {e}"))?;
            requests.push((request, model.to_string()));
        }
        if let Some(effort) = effort {
            let Some(id) = &self.thinking else {
                return Err(format!("{title} here cannot change how hard it thinks"));
            };
            let request = acp::set_effort(&session, id, effort)
                .map_err(|e| format!("{title} could not think at {}: {e}", effort.as_str()))?;
            requests.push((request, format!("thinking {}", effort.as_str())));
        }
        if requests.is_empty() {
            return Err("nothing to switch".into());
        }
        for (request, what) in requests {
            self.send(cx, request, move |result| Reply::Reconfigured {
                what,
                result: result.map(drop),
            });
        }
        Ok(())
    }

    /// Refuses a prompt with images the agent does not take, rather than
    /// send it without them.
    fn can_take(&self, prompt: &Prompt) -> Result<(), String> {
        if prompt.images.is_empty() {
            return Ok(());
        }
        match self.takes_images {
            Some(true) => Ok(()),
            Some(false) => Err(format!(
                "{} here does not take images",
                agent_title(&self.agent)
            )),
            None => {
                Err("the agent is still starting; send the images again once it is ready".into())
            }
        }
    }

    /// Allows a permission request the agent sent despite running without
    /// prompts, as `acp::allow_option` picks. Gives the responder back when
    /// the request is to be asked after all: a choice for the user, or a
    /// request that offers no way to allow.
    fn allow(
        &mut self,
        request: &RequestPermissionRequest,
        responder: Responder<RequestPermissionResponse>,
    ) -> Result<Option<Responder<RequestPermissionResponse>>> {
        let question = self.live.borrow().question(0, request);
        let Some(option) = acp::allow_option(&question.options) else {
            return Ok(Some(responder));
        };
        responder.respond(acp::selected(&option.id))?;
        self.live.send_modify(|live| {
            live.push(EntryKind::Question, &format!("Allowed: {}", question.text));
        });
        Ok(None)
    }

    fn ask(
        &mut self,
        request: &RequestPermissionRequest,
        responder: Responder<RequestPermissionResponse>,
    ) {
        let question = self.live.borrow().question(self.next_question, request);
        self.next_question += 1;
        self.live.send_modify(|live| {
            let detail = question
                .detail
                .as_deref()
                .map(|detail| format!(" ({detail})"))
                .unwrap_or_default();
            live.push(
                EntryKind::Question,
                &format!("Asked: {}{detail}", question.text),
            );
        });
        self.asked.push_back(Asked {
            question,
            responder,
        });
        self.show_question();
    }

    fn answer(&mut self, question: u32, reply: &str) -> Result<Result<(), String>> {
        let Some(asked) = self.asked.front() else {
            return Ok(Err("there is no open question".into()));
        };
        if asked.question.id != question {
            return Ok(Err(format!(
                "question {question} is not open; question {} is",
                asked.question.id
            )));
        }
        let options = &asked.question.options;
        let (chosen, follow_up) = match choose(options, reply) {
            Some(option) => (Some(option), None),
            // Anything else declines and goes to the agent as its next prompt.
            None => (
                options
                    .iter()
                    .find(|option| option.kind == OptionKind::RejectOnce)
                    .or_else(|| options.iter().find(|option| option.kind.rejects())),
                Some(reply.to_owned()),
            ),
        };
        let follow_up = follow_up.map(Prompt::from);
        if let Some(prompt) = &follow_up
            && let Err(full) = queue_room(&self.queued, prompt)
        {
            return Ok(Err(full));
        }
        let answer = match chosen {
            Some(option) => acp::selected(&option.id),
            None => acp::cancelled(),
        };
        let name = chosen
            .map_or("declined", |option| option.name.as_str())
            .to_owned();
        if let Some(asked) = self.asked.pop_front() {
            asked.responder.respond(answer)?;
        }
        if let Some(prompt) = follow_up {
            self.queued.push_front(prompt);
            self.show_queued();
        }
        self.live.send_modify(|live| {
            live.push(EntryKind::Question, &format!("Answered: {name}"));
        });
        self.show_question();
        Ok(Ok(()))
    }

    /// Shows the first waiting question, or none, in the state.
    fn show_question(&mut self) {
        let question = self.asked.front().map(|asked| asked.question.clone());
        self.live.send_if_modified(|live| {
            let status = match (&question, live.status()) {
                (Some(_), _) => Status::Asking,
                (None, Status::Asking) => Status::Working,
                (None, status) => status,
            };
            let changed = live.info.question != question || live.info.state.status != status;
            live.info.question = question;
            live.info.state.status = status;
            changed
        });
    }

    fn show_queued(&mut self) {
        let queued = u32::try_from(self.queued.len()).unwrap_or(u32::MAX);
        self.live.send_if_modified(|live| {
            let changed = live.info.state.queued != queued;
            live.info.state.queued = queued;
            changed
        });
    }

    fn turn_done(&mut self, cx: &ConnectionTo<Agent>, stop: StopReason, error: Option<String>) {
        // A turn that ends leaves no question open. A dropped responder
        // sends nothing, so each is answered cancelled.
        while let Some(asked) = self.asked.pop_front() {
            if let Err(e) = asked.responder.respond(acp::cancelled()) {
                log::debug!("an open question was not cancelled: {e}");
            }
        }
        let mut files = None;
        self.live.send_modify(|live| {
            live.info.turns += 1;
            live.last_stop = Some(stop);
            live.info.question = None;
            let state: &mut State = &mut live.info.state;
            state.status = Status::Idle;
            state.summary.clear();
            state.error = error;
            files = live.turn_files();
        });
        if let Some(files) = files {
            self.read_files(files);
        }
        let live = self.live.borrow();
        self.compactor.turn_ended(TurnEnd {
            usage: live.context,
            commands: &live.commands,
        });
        drop(live);
        self.next_prompt(cx);
    }

    /// Reads the image files a turn named off the async workers; they show
    /// once read.
    fn read_files(&mut self, files: TurnFiles) {
        let TurnFiles { cwd, paths, entry } = files;
        let read = spawn_blocking(move || image_files::read(&cwd, &paths));
        self.replies.push(
            async move {
                let pictures = read.await.unwrap_or_else(|e| {
                    log::warn!("reading a turn's image files failed: {e}");
                    Vec::new()
                });
                Reply::FilesRead { entry, pictures }
            }
            .boxed(),
        );
    }

    /// Sends `/compact` ahead of the waiting prompts.
    fn compact(&mut self, cx: &ConnectionTo<Agent>, session: &SessionId) {
        let prompt = Prompt::from(COMPACT_PROMPT.to_owned());
        self.send(cx, acp::prompt(session, &prompt), Reply::Compacted);
        self.live.send_modify(|live| {
            // What it said before no longer holds once compacted.
            live.context = None;
            live.take_compacted();
            let state = &mut live.info.state;
            state.status = Status::Working;
            state.summary = "Compacting the conversation".into();
            state.error = None;
        });
    }

    /// The node's `/compact` ended: the transcript says so, unless the
    /// agent's own update did, then the waiting prompt goes.
    fn compacted(
        &mut self,
        cx: &ConnectionTo<Agent>,
        result: Result<PromptResponse, agent_client_protocol::Error>,
    ) {
        self.compactor.finished();
        let tokens = result.as_ref().map(turn_tokens).unwrap_or_default();
        self.live.send_modify(|live| live.spent(tokens));
        let line = match result {
            Ok(done) => {
                let stop = StopReason::from_acp(&acp::wire_name(&done.stop_reason));
                (stop == StopReason::EndTurn).then_some(COMPACTED_ENTRY.to_owned())
            }
            Err(e) => {
                log::warn!("the agent did not compact: {e}");
                Some(format!("[compacting the conversation failed: {e}]"))
            }
        };
        self.live.send_modify(|live| {
            if let Some(line) = line
                && !live.take_compacted()
            {
                live.push(EntryKind::Other, &line);
            }
            let state = &mut live.info.state;
            state.status = Status::Idle;
            state.summary.clear();
        });
        self.next_prompt(cx);
    }

    /// Sends the next waiting prompt, if any.
    fn next_prompt(&mut self, cx: &ConnectionTo<Agent>) {
        let Some(session) = self.acp_session.clone() else {
            return;
        };
        if self.queued.is_empty() {
            return;
        }
        if self.compactor.start(!self.asked.is_empty()) {
            self.compact(cx, &session);
            return;
        }
        let Some(prompt) = self.queued.pop_front() else {
            return;
        };
        self.send(cx, acp::prompt(&session, &prompt), Reply::Prompt);
        let queued = u32::try_from(self.queued.len()).unwrap_or(u32::MAX);
        self.live.send_modify(|live| {
            for picture in prompt.pictures() {
                live.keep(picture);
            }
            live.prompted(&prompt.shown());
            live.info.state.queued = queued;
        });
    }
}

/// The tokens a turn used: `_meta.quota` as claude-agent-acp and
/// codex-acp give it, `token_count.totalTokens` with `model_usage` rows by
/// model, else the response's own `usage`, which any agent may send.
/// Nothing from an agent that says neither.
fn turn_tokens(done: &PromptResponse) -> TurnTokens {
    let total = |count: &Value| count.get("totalTokens").and_then(Value::as_u64);
    let quota = done.meta.as_ref().and_then(|meta| meta.get("quota"));
    let models = quota
        .and_then(|quota| quota.get("model_usage"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| {
            Some(ModelTokens {
                model: ModelId::new(cut(row.get("model")?.as_str()?, MAX_MODEL_NAME)),
                tokens: total(row.get("token_count")?)?,
            })
        })
        .filter(|row| row.tokens > 0)
        .take(MAX_TURN_MODELS)
        .collect();
    let total = quota
        .and_then(|quota| quota.get("token_count"))
        .and_then(total)
        .or_else(|| done.usage.as_ref().map(|usage| usage.total_tokens))
        .unwrap_or(0);
    TurnTokens { total, models }
}

/// Refuses a prompt the queue has no room for: too many waiting already,
/// or too many bytes with this one.
fn queue_room(queued: &VecDeque<Prompt>, prompt: &Prompt) -> Result<(), String> {
    if queued.len() >= MAX_QUEUED {
        return Err(format!("{MAX_QUEUED} prompts already wait for this turn"));
    }
    let waiting = queued
        .iter()
        .fold(prompt.size(), |n, queued| n.saturating_add(queued.size()));
    if waiting > MAX_QUEUED_BYTES {
        return Err(format!(
            "Too much already waits for this turn (over {} MB); send this once it is done",
            MAX_QUEUED_BYTES >> 20
        ));
    }
    Ok(())
}

/// What people call the agent `name`, such as Claude Code for `claude`.
fn agent_title(name: &str) -> &str {
    match name {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "gemini" => "Gemini CLI",
        "opencode" => "OpenCode",
        other => other,
    }
}

/// The option `reply` names: by id, number, name or kind, or plain yes/no.
pub(super) fn choose<'a>(options: &'a [AnswerOption], reply: &str) -> Option<&'a AnswerOption> {
    let reply = reply.trim().to_lowercase();
    let by_kind = |kind: OptionKind| options.iter().find(|option| option.kind == kind);
    options
        .iter()
        .find(|option| option.id.to_lowercase() == reply)
        .or_else(|| {
            let n: usize = reply.parse().ok()?;
            options.get(n.checked_sub(1)?)
        })
        .or_else(|| {
            options
                .iter()
                .find(|option| option.name.to_lowercase() == reply)
        })
        .or_else(|| options.iter().find(|option| option.kind.as_str() == reply))
        .or_else(|| match reply.as_str() {
            "allow" | "yes" | "y" | "approve" | "ok" => by_kind(OptionKind::AllowOnce),
            "always" => by_kind(OptionKind::AllowAlways),
            "reject" | "deny" | "no" | "n" => by_kind(OptionKind::RejectOnce),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    // The fake agents are sh scripts, so most tests run on Unix only.
    #![cfg_attr(windows, allow(dead_code, unused_imports))]

    use super::*;
    use beep_proto::Upload;

    #[test]
    fn turn_tokens_read_the_quota_then_the_usage() {
        let response = |value: Value| -> PromptResponse { serde_json::from_value(value).unwrap() };
        let quota = response(serde_json::json!({
            "stopReason": "end_turn",
            "_meta": {"quota": {
                "token_count": {"totalTokens": 1200, "inputTokens": 1000},
                "model_usage": [
                    {"model": "gpt-5-codex", "token_count": {"totalTokens": 1200}},
                    {"model": "idle", "token_count": {"totalTokens": 0}},
                ],
            }},
        }));
        let read = turn_tokens(&quota);
        assert_eq!(read.total, 1200);
        assert_eq!(
            read.models,
            [ModelTokens {
                model: "gpt-5-codex".into(),
                tokens: 1200
            }]
        );
        let usage = response(serde_json::json!({
            "stopReason": "end_turn",
            "usage": {"totalTokens": 77, "inputTokens": 70, "outputTokens": 7},
        }));
        assert_eq!(turn_tokens(&usage).total, 77);
        let none = response(serde_json::json!({"stopReason": "end_turn"}));
        assert_eq!(turn_tokens(&none), TurnTokens::default());
    }

    fn options() -> Vec<AnswerOption> {
        let option = |id: &str, name: &str, kind| AnswerOption {
            id: id.into(),
            name: name.into(),
            kind,
        };
        vec![
            option("allow", "Allow", OptionKind::AllowOnce),
            option("always", "Always Allow", OptionKind::AllowAlways),
            option("reject", "Reject", OptionKind::RejectOnce),
        ]
    }

    /// The shell helpers fake adapters share: `reply` answers the request
    /// last read into `$line` with the id it carried.
    const REPLY: &str = r#"
log() { printf '%s\n' "$line" >> "$1"; }
id() { printf '%s\n' "$line" | sed -nE 's/.*"id":("[^"]*"|[0-9]+).*/\1/p'; }
reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$(id)" "$1"; }
"#;

    /// An ACP adapter in a few lines of shell: it can resume sessions and
    /// appends each line it reads to the file in `$1`. On the prompt it asks
    /// permission once, then ends the turn.
    const FAKE_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"resume":{}}}}'
read -r line; log "$1"
reply '{}'
read -r line; log "$1"
reply '{}'
read -r line; log "$1"
prompt=$(id)
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t1","title":"Run cargo test"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"},{"optionId":"reject","name":"Reject","kind":"reject_once"}]}}'
read -r line; log "$1"
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Picked it up."}}}}'
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
cat > /dev/null
"#;

    /// An adapter that puts a choice to the user on its first prompt, as a
    /// pi extension's select does, and waits.
    const ASKING_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s"}'
read -r line; log "$1"
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"pi-ui-1","title":"Which file?"},"options":[{"optionId":"choice-0","name":"notes.txt","kind":"allow_once"},{"optionId":"choice-1","name":"other.txt","kind":"allow_once"}]}}'
cat > /dev/null
"#;

    fn fake(name: &str, log: &Path) -> AgentKind {
        script(name, FAKE_ADAPTER, log)
    }

    fn script(name: &str, script: &str, log: &Path) -> AgentKind {
        AgentKind {
            name: name.into(),
            program: "sh".into(),
            args: vec![
                "-c".into(),
                format!("{REPLY}{script}"),
                "fake-adapter".into(),
                log.display().to_string(),
            ],
            env: Default::default(),
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("beep-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    async fn settle(session: &Session) {
        let mut live = session.live.clone();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            live.wait_for(|live| {
                live.info.turns >= 1 || matches!(live.status(), Status::Ended | Status::Asking)
            }),
        )
        .await
        .expect("the session settles")
        .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_takeover_resumes_without_permission_prompts() {
        let dir = temp("takeover");
        let log = dir.join("sent.jsonl");
        let id = "0f3c2a9e-1111-4222-8333-944455556666";
        let session = Session::start(Start {
            id: "a3f9c1".into(),
            kind: &fake("claude", &log),
            cwd: &dir,
            prompt: Some("carry on".into()),
            resume: Some(id.into()),
            model: None,
            effort: None,
            note: None,
            images: Arc::default(),
            usage: Arc::default(),
        })
        .unwrap();
        settle(&session).await;
        let info = session.info();
        assert_eq!(info.turns, 1, "{:?}", info.state.error);
        assert_eq!(info.last_message, "Picked it up.");
        assert!(info.question.is_none());
        assert_eq!(session.live.borrow().agent_session.as_deref(), Some(id));
        let sent: Vec<Value> = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(sent[1]["method"], "session/load");
        assert_eq!(sent[1]["params"]["sessionId"], id);
        assert_eq!(sent[1]["params"]["cwd"], dir.display().to_string());
        assert_eq!(sent[2]["method"], "session/set_mode");
        assert_eq!(sent[2]["params"]["modeId"], "bypassPermissions");
        assert_eq!(
            sent[3]["method"], "session/prompt",
            "the prompt waits for the mode"
        );
        // The stray permission request was allowed, not asked.
        assert_eq!(sent[4]["id"], "p1");
        assert_eq!(sent[4]["result"]["outcome"]["optionId"], "allow");
        let (entries, _) = session.live.borrow().transcript(10);
        assert!(
            entries
                .iter()
                .any(|entry| entry.text == "Allowed: Run cargo test")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An adapter that writes `$BEEP_FAKE_MODE` to `$1.env`, asks
    /// permission on its first prompt, then ends the turn once answered.
    const ALLOWING_ADAPTER: &str = r#"
printf '%s\n' "$BEEP_FAKE_MODE" > "$1.env"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s"}'
read -r line; log "$1"
prompt=$(id)
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t1","title":"Run cargo test"},"options":[{"optionId":"once","name":"Allow","kind":"allow_once"},{"optionId":"always","name":"Always Allow","kind":"allow_always"},{"optionId":"reject","name":"Reject","kind":"reject_once"}]}}'
read -r line; log "$1"
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
cat > /dev/null
"#;

    #[cfg(unix)]
    #[tokio::test]
    async fn every_agent_has_its_permission_requests_allowed() {
        let dir = temp("allows");
        let log = dir.join("sent.jsonl");
        let mut kind = script("codex", ALLOWING_ADAPTER, &log);
        kind.env.insert("BEEP_FAKE_MODE".into(), "full".into());
        let session = Session::start(Start {
            id: "b4e0d2".into(),
            kind: &kind,
            cwd: &dir,
            prompt: Some("go".into()),
            resume: None,
            model: None,
            effort: None,
            note: None,
            images: Arc::default(),
            usage: Arc::default(),
        })
        .unwrap();
        settle(&session).await;
        let info = session.info();
        assert_eq!(info.turns, 1, "{:?}", info.state.error);
        assert!(info.question.is_none());
        let sent: Vec<Value> = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        // Codex starts in full access, so no mode is set.
        assert_eq!(sent[2]["method"], "session/prompt");
        assert_eq!(sent[3]["id"], "p1");
        assert_eq!(sent[3]["result"]["outcome"]["optionId"], "always");
        let (entries, _) = session.live.borrow().transcript(10);
        assert!(
            entries
                .iter()
                .any(|entry| entry.text == "Allowed: Run cargo test"),
            "{entries:?}"
        );
        // The kind's variables reach the agent.
        let env = std::fs::read_to_string(dir.join("sent.jsonl.env")).unwrap();
        assert_eq!(env, "full\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_choice_is_still_asked() {
        let dir = temp("asks");
        let log = dir.join("sent.jsonl");
        let session = Session::start(Start {
            id: "b4e0d3".into(),
            kind: &script("pi", ASKING_ADAPTER, &log),
            cwd: &dir,
            prompt: Some("go".into()),
            resume: None,
            model: None,
            effort: None,
            note: None,
            images: Arc::default(),
            usage: Arc::default(),
        })
        .unwrap();
        settle(&session).await;
        let info = session.info();
        assert_eq!(info.state.status, Status::Asking);
        let question = info.question.expect("a question");
        assert_eq!(question.text, "Which file?");
        assert_eq!(question.options.len(), 2);
        let sent = std::fs::read_to_string(&log).unwrap();
        assert!(!sent.contains("session/set_mode"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_declining_answer_waits_only_where_there_is_room() {
        let dir = temp("asks-full");
        let log = dir.join("sent.jsonl");
        let session = Session::start(Start {
            id: "b4e0d5".into(),
            kind: &script("pi", ASKING_ADAPTER, &log),
            cwd: &dir,
            prompt: Some("go".into()),
            resume: None,
            model: None,
            effort: None,
            note: None,
            images: Arc::default(),
            usage: Arc::default(),
        })
        .unwrap();
        settle(&session).await;
        let question = session.info().question.expect("a question").id;
        for n in 0..MAX_QUEUED {
            let prompt = Command::Prompt(format!("later {n}").into());
            session.order(prompt).await.unwrap();
        }
        let answer = Command::Answer {
            question,
            reply: "neither, look in docs".into(),
        };
        assert!(session.order(answer).await.is_err());
        assert_eq!(session.info().state.queued as usize, MAX_QUEUED);
        assert!(session.info().question.is_some(), "still open");
        session.close().await;
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An adapter that puts a choice to the user, then ends its turn without
    /// waiting for it, logs the answer the node gives and exits.
    const ABANDONING_ADAPTER: &str = r#"
read -r line; reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; reply '{"sessionId":"s"}'
read -r line
prompt=$(id)
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"pi-ui-1","title":"Which file?"},"options":[{"optionId":"choice-0","name":"notes.txt","kind":"allow_once"}]}}'
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt"
read -r line; log "$1"
"#;

    #[cfg(unix)]
    #[tokio::test]
    async fn a_question_left_open_by_a_turn_is_answered_cancelled() {
        let dir = temp("abandons");
        let log = dir.join("sent.jsonl");
        let session = Session::start(Start {
            id: "b4e0d4".into(),
            kind: &script("pi", ABANDONING_ADAPTER, &log),
            cwd: &dir,
            prompt: Some("go".into()),
            resume: None,
            model: None,
            effort: None,
            note: None,
            images: Arc::default(),
            usage: Arc::default(),
        })
        .unwrap();
        let mut live = session.live.clone();
        // The adapter exits once it hears the answer; without one it waits.
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            live.wait_for(|live| live.status() == Status::Ended),
        )
        .await
        .expect("the question was never answered")
        .unwrap();
        let sent = std::fs::read_to_string(&log).unwrap();
        let answer: Value = serde_json::from_str(sent.trim()).unwrap();
        assert_eq!(answer["id"], "p1");
        assert_eq!(answer["result"]["outcome"]["outcome"], "cancelled");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An adapter that answers with whatever id each request carries, then
    /// writes a burst far larger than a pipe holds before it reads the
    /// prompt. A node that stops reading while it writes deadlocks on it.
    const BURSTING_ADAPTER: &str = r#"
read -r line; reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; reply '{"sessionId":"s"}'
chunk=$(head -c 2000 /dev/zero | tr '\0' x)
i=0
while [ $i -lt 2000 ]; do
  printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$chunk"
  i=$((i+1))
done
line=$(head -n 1)
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;

    #[cfg(unix)]
    #[tokio::test]
    async fn a_large_prompt_and_a_large_burst_do_not_deadlock() {
        let dir = temp("burst");
        let log = dir.join("unused");
        let session = Session::start(Start {
            id: "c5f1e3".into(),
            kind: &script("codex", BURSTING_ADAPTER, &log),
            cwd: &dir,
            prompt: Some("y".repeat(4 * 1024 * 1024)),
            resume: None,
            model: None,
            effort: None,
            note: None,
            images: Arc::default(),
            usage: Arc::default(),
        })
        .unwrap();
        let mut live = session.live.clone();
        // The timeout only bounds a failure; the turn ending is the signal.
        tokio::time::timeout(
            std::time::Duration::from_secs(60),
            live.wait_for(|live| live.info.turns >= 1 || live.status() == Status::Ended),
        )
        .await
        .expect("the session hung: reading waited on writing")
        .unwrap();
        let info = session.info();
        assert_eq!(info.turns, 1, "{:?}", info.state.error);
        assert_eq!(info.last_message.len(), 4 * 1024, "the burst was read");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Starts the adapter `body` (after the handshake, with `$line` holding
    /// the first prompt) as a session that asks permission.
    fn started(dir: &Path, body: &str) -> Session {
        let handshake = r#"
read -r line; reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; reply '{"sessionId":"s"}'
read -r line
"#;
        Session::start(Start {
            id: "d6a2f4".into(),
            kind: &script("codex", &format!("{handshake}{body}"), &dir.join("log")),
            cwd: dir,
            prompt: Some("go".into()),
            resume: None,
            model: None,
            effort: None,
            note: None,
            images: Arc::default(),
            usage: Arc::default(),
        })
        .unwrap()
    }

    /// Waits until `done` holds. The timeout only bounds a failure.
    async fn until(session: &Session, done: impl FnMut(&Live) -> bool) {
        let mut live = session.live.clone();
        tokio::time::timeout(std::time::Duration::from_secs(60), live.wait_for(done))
            .await
            .expect("the session got there")
            .unwrap();
    }

    const UPDATE: &str =
        r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":"#;

    #[cfg(unix)]
    #[tokio::test]
    async fn an_oversized_message_is_dropped_and_the_session_goes_on() {
        let dir = temp("oversized");
        let body = format!(
            r#"
printf '%s{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"' '{UPDATE}'
head -c {} /dev/zero | tr '\0' x
printf '"}}}}}}}}\n'
echo '{UPDATE}{{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Still here"}}}}}}'
cat > /dev/null
"#,
            acp::MAX_MESSAGE + 1
        );
        let session = started(&dir, &body);
        until(&session, |live| {
            live.info.state.summary == "Still here" || live.status() == Status::Ended
        })
        .await;
        let info = session.info();
        assert_eq!(info.state.status, Status::Working);
        let error = info.state.error.unwrap_or_default();
        assert!(error.contains("dropped a 16 MiB message"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_update_of_an_unknown_kind_is_skipped() {
        let dir = temp("unknown");
        let body = format!(
            r#"
echo '{UPDATE}{{"sessionUpdate":"something_new","detail":1}}}}}}'
echo '{UPDATE}{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"done"}}}}}}}}'
reply '{{"stopReason":"end_turn"}}'
cat > /dev/null
"#
        );
        let session = started(&dir, &body);
        until(&session, |live| {
            live.info.turns >= 1 || live.status() == Status::Ended
        })
        .await;
        let info = session.info();
        assert_eq!(info.turns, 1, "{:?}", info.state.error);
        assert_eq!(info.last_message, "done");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn closing_stops_the_agent_with_sigterm() {
        let dir = temp("close");
        let marker = dir.join("terminated");
        let body = format!(
            r#"
trap 'echo bye > "{}"; exit 0' TERM
echo '{UPDATE}{{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Listening"}}}}}}'
while :; do sleep 1; done
"#,
            marker.display()
        );
        let session = started(&dir, &body);
        until(&session, |live| live.info.state.summary == "Listening").await;
        session.close().await;
        assert_eq!(session.info().state.status, Status::Ended);
        assert_eq!(session.info().state.error, None, "closed, not failed");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "bye\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_image_file_the_agent_names_shows_once_the_turn_ends() {
        let dir = temp("named-image");
        let png = b"\x89PNG\r\n\x1a\nchart".to_vec();
        std::fs::write(dir.join("chart.png"), &png).unwrap();
        let body = format!(
            r#"
echo '{UPDATE}{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"Saved chart.png"}}}}}}}}'
reply '{{"stopReason":"end_turn"}}'
cat > /dev/null
"#
        );
        let session = started(&dir, &body);
        let shown = format!(
            "Saved chart.png\n[image: image-{}]",
            beep_proto::image_key(&png)
        );
        until(&session, |live| {
            live.transcript(10)
                .0
                .iter()
                .any(|entry| entry.text == shown)
                || live.status() == Status::Ended
        })
        .await;
        assert_eq!(session.info().state.error, None);
        let kept = session.live.borrow().picture(&beep_proto::image_key(&png));
        assert_eq!(kept.map(|picture| picture.data.clone()), Some(png));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An adapter that says whether it takes images (`$2`), then ends each
    /// prompt's turn at once.
    const IMAGES_ADAPTER: &str = r#"
read -r line; log "$1"
reply "{\"protocolVersion\":1,\"agentCapabilities\":{\"promptCapabilities\":{\"image\":$2}}}"
read -r line; log "$1"
reply '{"sessionId":"s"}'
while read -r line; do log "$1"; reply '{"stopReason":"end_turn"}'; done
"#;

    /// A session of an agent that takes images or not, once it is idle.
    async fn image_session(dir: &Path, images: bool) -> Session {
        let mut kind = script("codex", IMAGES_ADAPTER, &dir.join("sent.jsonl"));
        kind.args.push(images.to_string());
        let session = Session::start(Start {
            id: "e7b3a5".into(),
            kind: &kind,
            cwd: dir,
            prompt: None,
            resume: None,
            model: None,
            effort: None,
            note: None,
            images: Arc::default(),
            usage: Arc::default(),
        })
        .unwrap();
        until(&session, |live| {
            matches!(live.status(), Status::Idle | Status::Ended)
        })
        .await;
        assert_eq!(session.info().state.status, Status::Idle);
        session
    }

    fn shot() -> Prompt {
        Prompt {
            text: "The button overlaps".into(),
            images: vec![Upload {
                name: "shot.png".into(),
                mime: "image/png".into(),
                data: b"\x89PNG\r\n\x1a\n".to_vec(),
            }],
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn images_go_to_an_agent_that_takes_them() {
        let dir = temp("images");
        let session = image_session(&dir, true).await;
        session.order(Command::Prompt(shot())).await.unwrap();
        until(&session, |live| {
            live.info.turns >= 1 || live.status() == Status::Ended
        })
        .await;
        let sent: Vec<Value> = std::fs::read_to_string(dir.join("sent.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let prompt = &sent[2];
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"]["prompt"][0]["text"], "The button overlaps");
        assert_eq!(prompt["params"]["prompt"][1]["type"], "image");
        assert_eq!(prompt["params"]["prompt"][1]["mimeType"], "image/png");
        let data = b"\x89PNG\r\n\x1a\n";
        let key = beep_proto::image_key(data);
        let (entries, _) = session.live.borrow().transcript(10);
        let shown = format!("The button overlaps\n[image: image-{key}]");
        assert!(
            entries.iter().any(|entry| entry.text == shown),
            "{entries:?}"
        );
        let kept = session.live.borrow().picture(&key);
        assert_eq!(
            kept.map(|picture| picture.data.clone()),
            Some(data.to_vec())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_agent_without_images_refuses_them() {
        let dir = temp("no-images");
        let session = image_session(&dir, false).await;
        let refused = session.order(Command::Prompt(shot())).await;
        assert_eq!(refused, Err("Codex here does not take images".to_owned()));
        let info = session.info();
        assert_eq!(info.state.status, Status::Idle, "nothing was sent");
        assert_eq!(info.state.queued, 0);
        // Text alone still goes.
        session
            .order(Command::Prompt("just text".to_owned().into()))
            .await
            .unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_queue_is_bounded_in_prompts_and_bytes() {
        let text = |len: usize| Prompt::from("x".repeat(len));
        let mut queued = VecDeque::new();
        assert_eq!(queue_room(&queued, &text(10)), Ok(()));

        let image = |len: usize| Prompt {
            text: String::new(),
            images: vec![Upload {
                name: "shot.png".into(),
                mime: "image/png".into(),
                data: vec![0; len],
            }],
        };
        queued.push_back(image(MAX_QUEUED_BYTES / 2));
        queued.push_back(image(MAX_QUEUED_BYTES / 2 - 10));
        assert_eq!(queue_room(&queued, &text(10)), Ok(()), "exactly the limit");
        let full = queue_room(&queued, &text(11)).unwrap_err();
        assert!(full.starts_with("Too much already waits"), "{full}");

        let many: VecDeque<_> = (0..MAX_QUEUED).map(|_| text(1)).collect();
        let full = queue_room(&many, &text(1)).unwrap_err();
        assert_eq!(
            full,
            format!("{MAX_QUEUED} prompts already wait for this turn")
        );
    }

    #[test]
    fn a_reply_picks_an_option_by_id_number_name_or_word() {
        let options = options();
        let pick = |reply| choose(&options, reply).map(|option| option.id.as_str());
        assert_eq!(pick("reject"), Some("reject"));
        assert_eq!(pick("2"), Some("always"));
        assert_eq!(pick("Always Allow"), Some("always"));
        assert_eq!(pick("allow_once"), Some("allow"));
        assert_eq!(pick("yes"), Some("allow"));
        assert_eq!(pick("no"), Some("reject"));
        assert_eq!(pick("4"), None);
        assert_eq!(pick("use the other folder instead"), None);
    }

    /// An adapter that writes its arguments after `$1` to `$1.args`, offers
    /// two models on `session/new`, answers a switch with
    /// `$BEEP_FAKE_SWITCH` as its error (an empty result when unset), then
    /// ends a turn.
    const MODEL_ADAPTER: &str = r#"
out="$1"; shift; printf '%s\n' "$@" > "$out.args"; set -- "$out"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s","models":{"availableModels":[{"modelId":"fast"},{"modelId":"deep"}],"currentModelId":"fast"}}'
read -r line; log "$1"
case "$line" in
  *session/prompt*) ;;
  *) if [ -n "$BEEP_FAKE_SWITCH" ]; then printf '{"jsonrpc":"2.0","id":%s,"error":%s}\n' "$(id)" "$BEEP_FAKE_SWITCH"; else reply '{}'; fi
     read -r line; log "$1";;
esac
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(id)"
cat > /dev/null
"#;

    fn sent(log: &Path) -> Vec<Value> {
        std::fs::read_to_string(log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn start_on<'a>(kind: &'a AgentKind, dir: &'a Path, model: Option<&str>) -> Start<'a> {
        Start {
            id: "c0ffee".into(),
            kind,
            cwd: dir,
            prompt: Some("go".into()),
            resume: None,
            model: model.map(str::to_owned),
            effort: None,
            note: None,
            images: Arc::default(),
            usage: Arc::default(),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_new_session_switches_to_its_model_before_the_first_prompt() {
        let dir = temp("model-switch");
        let log = dir.join("sent.jsonl");
        let kind = script("codex", MODEL_ADAPTER, &log);
        let session = Session::start(start_on(&kind, &dir, Some("deep"))).unwrap();
        settle(&session).await;
        let info = session.info();
        assert_eq!(info.turns, 1, "{:?}", info.state.error);
        let sent = sent(&log);
        assert_eq!(sent[1]["method"], "session/new");
        assert_eq!(sent[2]["method"], "session/set_model");
        assert_eq!(sent[2]["params"]["sessionId"], "s");
        assert_eq!(sent[2]["params"]["modelId"], "deep");
        assert_eq!(sent[3]["method"], "session/prompt", "the prompt waits");
        let args = std::fs::read_to_string(dir.join("sent.jsonl.args")).unwrap();
        assert_eq!(args.trim(), "", "no flag on the command line");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_session_without_a_model_asked_for_keeps_the_agents_own() {
        let dir = temp("model-none");
        let log = dir.join("sent.jsonl");
        let kind = script("codex", MODEL_ADAPTER, &log);
        let session = Session::start(start_on(&kind, &dir, None)).unwrap();
        settle(&session).await;
        assert_eq!(session.info().turns, 1);
        assert_eq!(sent(&log)[2]["method"], "session/prompt");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_model_the_agent_refuses_ends_the_session() {
        let dir = temp("model-refused");
        let log = dir.join("sent.jsonl");
        let mut kind = script("codex", MODEL_ADAPTER, &log);
        kind.env.insert(
            "BEEP_FAKE_SWITCH".into(),
            r#"{"code":-32602,"message":"unknown model"}"#.into(),
        );
        let session = Session::start(start_on(&kind, &dir, Some("huge"))).unwrap();
        settle(&session).await;
        let info = session.info();
        assert_eq!(info.state.status, Status::Ended);
        let error = info.state.error.unwrap_or_default();
        assert!(error.contains("could not switch to huge"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn flsh_starts_on_its_model_from_the_command_line() {
        let dir = temp("model-flag");
        let log = dir.join("sent.jsonl");
        let kind = script("flsh", MODEL_ADAPTER, &log);
        let session = Session::start(start_on(&kind, &dir, Some("openai:gpt-test"))).unwrap();
        settle(&session).await;
        assert_eq!(session.info().turns, 1);
        let args = std::fs::read_to_string(dir.join("sent.jsonl.args")).unwrap();
        assert_eq!(args.trim(), "--model=openai:gpt-test");
        assert_eq!(sent(&log)[2]["method"], "session/prompt", "no switch");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An adapter that offers two models, ends its first turn, then
    /// answers the switch that comes after.
    const SWITCH_LATER_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s","models":{"availableModels":[{"modelId":"fast"},{"modelId":"deep"}],"currentModelId":"fast"}}'
read -r line; log "$1"
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(id)"
read -r line; log "$1"
reply '{}'
cat > /dev/null
"#;

    #[cfg(unix)]
    #[tokio::test]
    async fn a_running_session_switches_its_model_for_the_next_turn() {
        let dir = temp("model-later");
        let log = dir.join("sent.jsonl");
        let kind = script("codex", SWITCH_LATER_ADAPTER, &log);
        let session = Session::start(start_on(&kind, &dir, None)).unwrap();
        settle(&session).await;
        // It cannot think at another level: refused, nothing sent.
        let refused = session
            .order(Command::Configure {
                model: None,
                effort: Some(Effort::High),
            })
            .await;
        assert!(
            refused
                .unwrap_err()
                .contains("cannot change how hard it thinks")
        );
        session
            .order(Command::Configure {
                model: Some("deep".into()),
                effort: None,
            })
            .await
            .unwrap();
        until(&session, |live| {
            live.transcript(10)
                .0
                .iter()
                .any(|entry| entry.text.contains("Switched to deep"))
        })
        .await;
        let sent = sent(&log);
        assert_eq!(sent[3]["method"], "session/set_model");
        assert_eq!(sent[3]["params"]["modelId"], "deep");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_agent_that_cannot_switch_runs_its_own_and_says_so() {
        let dir = temp("model-fixed");
        let log = dir.join("sent.jsonl");
        let kind = script("pi", ALLOWING_ADAPTER, &log);
        let session = Session::start(start_on(&kind, &dir, Some("deep"))).unwrap();
        settle(&session).await;
        assert_eq!(session.info().turns, 1);
        assert_eq!(sent(&log)[2]["method"], "session/prompt");
        let (entries, _) = session.live.borrow().transcript(10);
        assert!(
            entries
                .iter()
                .any(|entry| entry.text.contains("runs its own rather than deep")),
            "{entries:?}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An adapter whose `session/new` offers a thinking option, answers
    /// setting it with `$BEEP_FAKE_SET` as its error (an empty result when
    /// unset), then ends a turn.
    const THINKING_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s","configOptions":[{"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"medium","options":[{"value":"medium","name":"Medium"},{"value":"max","name":"Max"}]}]}'
read -r line; log "$1"
case "$line" in
  *session/prompt*) ;;
  *) if [ -n "$BEEP_FAKE_SET" ]; then printf '{"jsonrpc":"2.0","id":%s,"error":%s}\n' "$(id)" "$BEEP_FAKE_SET"; else reply '{"configOptions":[]}'; fi
     read -r line; log "$1";;
esac
printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$(id)"
cat > /dev/null
"#;

    fn thinking_at<'a>(kind: &'a AgentKind, dir: &'a Path, effort: Effort) -> Start<'a> {
        Start {
            effort: Some(effort),
            ..start_on(kind, dir, None)
        }
    }

    fn noted(session: &Session, text: &str) -> bool {
        let (entries, _) = session.live.borrow().transcript(10);
        entries.iter().any(|entry| entry.text.contains(text))
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_new_session_sets_its_thinking_before_the_first_prompt() {
        let dir = temp("effort-set");
        let log = dir.join("sent.jsonl");
        let kind = script("codex", THINKING_ADAPTER, &log);
        let session = Session::start(thinking_at(&kind, &dir, Effort::Max)).unwrap();
        settle(&session).await;
        assert_eq!(session.info().turns, 1);
        let sent = sent(&log);
        assert_eq!(sent[2]["method"], "session/set_config_option");
        assert_eq!(sent[2]["params"]["configId"], "effort");
        assert_eq!(sent[2]["params"]["value"], "max");
        assert_eq!(sent[3]["method"], "session/prompt", "the prompt waits");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_thinking_level_the_agent_refuses_runs_at_its_own_and_says_so() {
        let dir = temp("effort-refused");
        let log = dir.join("sent.jsonl");
        let mut kind = script("codex", THINKING_ADAPTER, &log);
        kind.env.insert(
            "BEEP_FAKE_SET".into(),
            r#"{"code":-32602,"message":"unknown value"}"#.into(),
        );
        let session = Session::start(thinking_at(&kind, &dir, Effort::Max)).unwrap();
        settle(&session).await;
        assert_eq!(session.info().turns, 1, "the session goes on");
        assert!(noted(&session, "could not be set to think at max"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_agent_without_a_thinking_option_runs_at_its_own_and_says_so() {
        let dir = temp("effort-none");
        let log = dir.join("sent.jsonl");
        let kind = script("pi", ALLOWING_ADAPTER, &log);
        let session = Session::start(thinking_at(&kind, &dir, Effort::High)).unwrap();
        settle(&session).await;
        assert_eq!(session.info().turns, 1);
        assert_eq!(sent(&log)[2]["method"], "session/prompt");
        assert!(noted(&session, "cannot be set to think at high"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_session_opens_its_transcript_with_its_note() {
        let dir = temp("note");
        let log = dir.join("sent.jsonl");
        let kind = script("pi", ALLOWING_ADAPTER, &log);
        let start = Start {
            note: Some("Started on a new branch beep/go from HEAD.".into()),
            ..start_on(&kind, &dir, None)
        };
        let session = Session::start(start).unwrap();
        let (entries, _) = session.live.borrow().transcript(100);
        assert_eq!(
            entries[0].text,
            "Started on a new branch beep/go from HEAD."
        );
        settle(&session).await;
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An adapter that ends its first turn with `/compact` advertised and
    /// the window at 85%, reports a finished compaction for the second
    /// prompt, then ends the third turn.
    const COMPACTING_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s"}'
update() { printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":%s}}\n' "$1"; }
read -r line; log "$1"
update '{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"compact","description":"Compact"}]}'
update '{"sessionUpdate":"usage_update","used":85000,"size":100000}'
reply '{"stopReason":"end_turn"}'
read -r line; log "$1"
update '{"sessionUpdate":"compaction_update","compactionId":"c1","status":"in_progress"}'
update '{"sessionUpdate":"compaction_update","compactionId":"c1","status":"completed"}'
update '{"sessionUpdate":"usage_update","used":12000,"size":100000}'
reply '{"stopReason":"end_turn"}'
read -r line; log "$1"
update '{"sessionUpdate":"usage_update","used":13000,"size":100000}'
reply '{"stopReason":"end_turn"}'
cat > /dev/null
"#;

    /// The text of each prompt the adapter got.
    fn prompts(log: &Path) -> Vec<String> {
        sent(log)
            .iter()
            .filter(|line| line["method"] == "session/prompt")
            .map(|line| {
                line["params"]["prompt"][0]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_full_window_is_compacted_before_the_next_prompt() {
        let dir = temp("compact");
        let log = dir.join("sent.jsonl");
        let kind = script("codex", COMPACTING_ADAPTER, &log);
        let session = Session::start(start_on(&kind, &dir, None)).unwrap();
        settle(&session).await;
        assert_eq!(prompts(&log), ["go"], "nothing is compacted mid-turn");
        session
            .order(Command::Prompt(Prompt::from("next".to_owned())))
            .await
            .unwrap();
        until(&session, |live| {
            live.info.turns >= 2 || live.status() == Status::Ended
        })
        .await;
        let info = session.info();
        assert_eq!(info.turns, 2, "{:?}", info.state.error);
        assert_eq!(prompts(&log), ["go", COMPACT_PROMPT, "next"]);
        let initialize = &sent(&log)[0];
        assert_eq!(
            initialize["params"]["clientCapabilities"]["session"]["compaction"],
            serde_json::json!({})
        );
        let (entries, _) = session.live.borrow().transcript(100);
        let compacted: Vec<_> = entries
            .iter()
            .filter(|entry| entry.text == COMPACTED_ENTRY)
            .collect();
        assert_eq!(compacted.len(), 1, "{entries:?}");
        assert_eq!(compacted[0].kind, EntryKind::Other);
        assert!(
            !entries.iter().any(|entry| entry.text == COMPACT_PROMPT),
            "the node's /compact is not shown as the user's"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An adapter that writes Claude Code's threshold variable to `$1.env`
    /// and refuses to start.
    const ENV_ADAPTER: &str = r#"
printf '%s\n' "${CLAUDE_AUTOCOMPACT_PCT_OVERRIDE-unset}" > "$1.env"
read -r line
printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32603,"message":"no"}}\n' "$(id)"
cat > /dev/null
"#;

    /// What Claude Code's threshold variable held when `agent` started
    /// with `env` set, or `unset`.
    async fn threshold_seen(dir: &Path, agent: &str, env: &[(&str, &str)]) -> String {
        let log = dir.join("sent");
        let mut kind = script(agent, ENV_ADAPTER, &log);
        for (name, value) in env {
            kind.env.insert((*name).into(), (*value).into());
        }
        let session = Session::start(start_on(&kind, dir, None)).unwrap();
        until(&session, |live| live.status() == Status::Ended).await;
        let seen = std::fs::read_to_string(log.with_extension("env")).unwrap();
        seen.trim().to_owned()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_starts_with_the_threshold_unless_the_user_set_its_own() {
        let dir = temp("compact-env");
        assert_eq!(threshold_seen(&dir, "claude", &[]).await, "80");
        let at = ("BEEP_COMPACT_AT", "70");
        assert_eq!(threshold_seen(&dir, "claude", &[at]).await, "70");
        let off = ("BEEP_COMPACT_AT", "off");
        assert_eq!(threshold_seen(&dir, "claude", &[off]).await, "unset");
        let own = ("CLAUDE_AUTOCOMPACT_PCT_OVERRIDE", "60");
        assert_eq!(threshold_seen(&dir, "claude", &[own, at]).await, "60");
        assert_eq!(threshold_seen(&dir, "codex", &[]).await, "unset");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
