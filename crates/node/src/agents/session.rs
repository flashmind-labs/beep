//! One agent session: the adapter process and the task that talks ACP to
//! it through the official crate. The task owns the process; streams send
//! it orders and follow its state through a watch channel.

use super::{
    AgentKind,
    acp::{self, Line, LineReader, Prompt, ResumeMethod},
    live::{Live, cut},
};
use crate::process::{GroupLeader, STOP_GRACE};
use agent_client_protocol::{
    Agent, Client, ConnectionTo, JsonRpcRequest, Responder, on_receive_notification,
    on_receive_request,
    schema::v1::{
        CancelNotification, InitializeResponse, LoadSessionRequest, NewSessionRequest,
        NewSessionResponse, PromptResponse, RequestPermissionRequest, RequestPermissionResponse,
        ResumeSessionRequest, SessionId, SessionNotification, SessionUpdate, SetSessionModeRequest,
    },
};
use anyhow::{Context, Result};
use beep_proto::node::agent::{
    AnswerOption, EntryKind, OptionKind, Question, SessionInfo, State, Status, StopReason,
};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::BufReader,
    process::{Child, ChildStderr, Command as Process},
    sync::{mpsc, oneshot, watch},
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
    Answer { question: u32, reply: String },
    Cancel,
    Close,
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
}

impl Session {
    pub(super) fn start(start: Start<'_>) -> Result<Self> {
        let Start {
            id,
            kind,
            cwd,
            prompt,
            resume,
        } = start;
        let child = Process::new(&kind.program)
            .args(&kind.args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Its own process group, so closing it stops what it started.
            .process_group(0)
            .kill_on_drop(true)
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
        let mut first = Live::new(info);
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
            skip_permissions: kind.name == "claude",
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
    NewSession(Result<NewSessionResponse, agent_client_protocol::Error>),
    /// The answer to `session/resume` or `session/load` of this session.
    Resumed(SessionId, Result<(), agent_client_protocol::Error>),
    /// The answer to `session/set_mode` to the mode that asks no permission.
    Bypass(Result<(), agent_client_protocol::Error>),
    Prompt(Result<PromptResponse, agent_client_protocol::Error>),
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
    /// The agent runs without permission prompts: Claude Code, as with
    /// `--dangerously-skip-permissions`. A permission request that comes
    /// anyway is allowed rather than asked.
    skip_permissions: bool,
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
            Reply::NewSession(Ok(created)) => self.ready(cx, created.session_id),
            Reply::Resumed(session, Ok(())) => self.ready(cx, session),
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
                let (stop, error) = match result {
                    Ok(done) => (
                        StopReason::from_acp(&acp::wire_name(&done.stop_reason)),
                        None,
                    ),
                    Err(e) => (StopReason::Failed, Some(e.to_string())),
                };
                self.turn_done(cx, stop, error);
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
            let request = NewSessionRequest::new(self.cwd.clone());
            self.send(cx, request, Reply::NewSession);
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

    /// The agent made or resumed its session `session`: prompts may go.
    fn ready(&mut self, cx: &ConnectionTo<Agent>, session: SessionId) {
        self.acp_session = Some(session.clone());
        self.live
            .send_modify(|live| live.agent_session = Some(session.to_string()));
        if self.skip_permissions {
            // The first prompt waits for the mode, so it never asks.
            let request = SetSessionModeRequest::new(session, acp::BYPASS_MODE);
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
        }
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

    /// Allows a permission request in a session that asks none. Gives the
    /// responder back when the request should be asked after all: the
    /// session asks, or the request offers no way to allow.
    fn allow(
        &mut self,
        request: &RequestPermissionRequest,
        responder: Responder<RequestPermissionResponse>,
    ) -> Result<Option<Responder<RequestPermissionResponse>>> {
        if !self.skip_permissions {
            return Ok(Some(responder));
        }
        let question = self.live.borrow().question(0, request);
        let options = &question.options;
        let Some(option) = options
            .iter()
            .find(|option| option.kind == OptionKind::AllowOnce)
            .or_else(|| options.iter().find(|o| o.kind == OptionKind::AllowAlways))
        else {
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
        if let Some(text) = follow_up {
            self.queued.push_front(Prompt::from(text));
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
        // A turn that ends leaves no question open.
        self.asked.clear();
        self.live.send_modify(|live| {
            live.info.turns += 1;
            live.last_stop = Some(stop);
            live.info.question = None;
            let state: &mut State = &mut live.info.state;
            state.status = Status::Idle;
            state.summary.clear();
            state.error = error;
        });
        self.next_prompt(cx);
    }

    /// Sends the next waiting prompt, if any.
    fn next_prompt(&mut self, cx: &ConnectionTo<Agent>) {
        let Some(session) = self.acp_session.clone() else {
            return;
        };
        let Some(prompt) = self.queued.pop_front() else {
            return;
        };
        self.send(cx, acp::prompt(&session, &prompt), Reply::Prompt);
        let queued = u32::try_from(self.queued.len()).unwrap_or(u32::MAX);
        self.live.send_modify(|live| {
            live.prompted(&prompt.shown());
            live.info.state.queued = queued;
        });
    }
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
    use super::*;
    use beep_proto::Upload;
    use serde_json::Value;

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

    /// An adapter that asks permission on its first prompt and waits.
    const ASKING_ADAPTER: &str = r#"
read -r line; log "$1"
reply '{"protocolVersion":1,"agentCapabilities":{}}'
read -r line; log "$1"
reply '{"sessionId":"s"}'
read -r line; log "$1"
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t1","title":"Run cargo test"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]}}'
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
        assert_eq!(sent[1]["method"], "session/resume");
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

    #[tokio::test]
    async fn other_agents_still_ask() {
        let dir = temp("asks");
        let log = dir.join("sent.jsonl");
        let session = Session::start(Start {
            id: "b4e0d2".into(),
            kind: &script("codex", ASKING_ADAPTER, &log),
            cwd: &dir,
            prompt: Some("go".into()),
            resume: None,
        })
        .unwrap();
        settle(&session).await;
        let info = session.info();
        assert_eq!(info.state.status, Status::Asking);
        assert_eq!(
            info.question.map(|q| q.text).as_deref(),
            Some("Run cargo test")
        );
        let sent = std::fs::read_to_string(&log).unwrap();
        assert!(!sent.contains("session/set_mode"));
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
        let (entries, _) = session.live.borrow().transcript(10);
        assert!(
            entries
                .iter()
                .any(|entry| entry.text == "The button overlaps\n[image: shot.png]"),
            "{entries:?}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
}
